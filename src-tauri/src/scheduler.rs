//! Periodic refresh + log ingestion. [BACKEND owns this file]
//! `start()` is called once from `lib.rs` after `AppState` is managed.
//!
//! Three background tasks:
//! * **refresh loop** — fetch every provider immediately, then whenever that
//!   provider is due again: `settings.refreshIntervalSec` (re-read each round,
//!   minimum 15 s) while the provider's session logs are busy, stretched by
//!   `poll_interval_secs` while they are quiet (`settings.adaptiveRefresh`),
//!   and never before a per-provider exponential backoff (capped at 5 min)
//!   after failures has elapsed.
//! * **ingest loop** — first scan 2 s after start, then every 60 s, plus a
//!   `notify` file watcher on the log roots debounced by 3 s. The same watcher
//!   feeds the per-provider "last activity" clock the refresh loop reads.
//! * **event listener** — the tray emits `refresh-requested`; opening the
//!   dashboard cancels the adaptive stretch.
//!
//! Nothing here ever blocks the UI thread: HTTP happens on the async runtime,
//! SQLite inside `spawn_blocking`.

use crate::commands::{ingest, providers, store};
use crate::model::{events, AppSnapshot, DataSource, IngestStats, ProviderStatus, Settings};
use crate::state::{AppState, Backoff, PollClocks};
use parking_lot::Mutex;
use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Listener, Manager};

/// App-wide event the tray (platform layer) emits to force a refresh.
pub const REFRESH_REQUESTED: &str = "refresh-requested";

const INITIAL_INGEST_DELAY: Duration = Duration::from_secs(2);
const INGEST_INTERVAL_MS: i64 = 60_000;
/// Quiet period after the last file-system event before ingesting.
const WATCH_DEBOUNCE_MS: i64 = 3_000;
const TICK: Duration = Duration::from_secs(1);

pub fn start(app: AppHandle) {
    // Timestamp (unix ms) of the newest file-system event, 0 = nothing pending.
    let dirty = Arc::new(AtomicI64::new(0));
    let clocks = app.state::<AppState>().poll_clocks.clone();
    spawn_log_watcher(dirty.clone(), clocks);

    let refresh_app = app.clone();
    tauri::async_runtime::spawn(async move { refresh_loop(refresh_app).await });

    let ingest_app = app.clone();
    tauri::async_runtime::spawn(async move { ingest_loop(ingest_app, dirty).await });

    let listen_app = app.clone();
    app.listen_any(REFRESH_REQUESTED, move |_event| {
        let app = listen_app.clone();
        tauri::async_runtime::spawn(async move {
            refresh_now(&app, None).await;
        });
    });

    // Opening the dashboard means "show me the current numbers": end the
    // adaptive stretch so the next tick polls (the error backoff still holds).
    let dashboard_app = app.clone();
    app.listen_any(events::DASHBOARD_NAVIGATE, move |_event| {
        let state = dashboard_app.state::<AppState>();
        state.poll_clocks.lock().mark_interaction(store::now_ms());
    });
}

// ---------- adaptive polling ----------

/// Lower bound on `refreshIntervalSec` (also enforced by `settings::clamp`).
pub const MIN_INTERVAL_SEC: u64 = 15;
/// `(idle seconds, multiplier)` — the first matching row wins, so keep this
/// ordered from the longest idle period down.
const ADAPTIVE_STEPS: &[(u64, u64)] = &[(1_800, 5), (600, 2)];
/// However long a provider stays quiet, never poll it less often than this.
const ADAPTIVE_MAX_SEC: u64 = 600;

/// Everything the scheduler knows about one provider when it decides whether
/// to poll it this round. Deliberately plain data so the decision below is a
/// pure function.
#[derive(Clone, Copy, Debug)]
pub struct PollInput {
    /// `settings.refreshIntervalSec`.
    pub configured_secs: u64,
    /// `settings.adaptiveRefresh`.
    pub adaptive: bool,
    /// Seconds since this provider's logs last changed; `None` = never seen.
    pub idle_secs: Option<u64>,
    /// unix ms of the last poll; 0 = never polled.
    pub last_poll_ms: i64,
    /// unix ms before which the error backoff forbids a retry; 0 = none.
    pub backoff_until_ms: i64,
}

/// Effective polling period for one provider.
///
/// A provider whose session logs changed recently is polled at the configured
/// interval; after ~10 min of quiet the period doubles and after ~30 min it is
/// five times as long, capped at `ADAPTIVE_MAX_SEC` (but never shorter than
/// what the user configured). Anthropic answers `HTTP 429` when polled too
/// often, so an idle sidebar should not keep knocking.
pub fn poll_interval_secs(configured_secs: u64, adaptive: bool, idle_secs: Option<u64>) -> u64 {
    let base = configured_secs.max(MIN_INTERVAL_SEC);
    if !adaptive {
        return base;
    }
    // An unknown activity time means "assume the user is working".
    let Some(idle) = idle_secs else {
        return base;
    };
    let factor = ADAPTIVE_STEPS
        .iter()
        .find(|(after, _)| idle >= *after)
        .map_or(1, |(_, factor)| *factor);
    base.saturating_mul(factor).min(base.max(ADAPTIVE_MAX_SEC))
}

/// unix ms at which `input`'s provider may be polled again: the adaptive
/// period after the last poll, and never before the error backoff expires.
pub fn next_poll_due_ms(input: &PollInput) -> i64 {
    if input.last_poll_ms == 0 {
        // Never polled (start-up, or a provider the user just enabled).
        return input.backoff_until_ms;
    }
    let period = poll_interval_secs(input.configured_secs, input.adaptive, input.idle_secs) as i64;
    input
        .last_poll_ms
        .saturating_add(period.saturating_mul(1_000))
        .max(input.backoff_until_ms)
}

pub fn should_poll(input: &PollInput, now_ms: i64) -> bool {
    now_ms >= next_poll_due_ms(input)
}

fn poll_input(
    settings: &Settings,
    clocks: &PollClocks,
    backoff: &std::collections::HashMap<String, Backoff>,
    provider: &str,
    now_ms: i64,
) -> PollInput {
    PollInput {
        configured_secs: settings.refresh_interval_sec,
        adaptive: settings.adaptive_refresh,
        idle_secs: clocks.idle_secs(provider, now_ms),
        last_poll_ms: clocks.last_poll_ms.get(provider).copied().unwrap_or(0),
        backoff_until_ms: backoff.get(provider).map_or(0, |b| b.next_attempt_ms),
    }
}

/// The enabled providers, in settings order. Disabled ones never need a poll.
fn pollable_providers(settings: &Settings) -> Vec<String> {
    settings
        .providers
        .iter()
        .filter(|(id, _)| providers::is_enabled(settings, id))
        .map(|(id, _)| id.clone())
        .collect()
}

/// True when at least one provider's next poll time has arrived.
fn any_provider_due(app: &AppHandle, now_ms: i64) -> bool {
    let state = app.state::<AppState>();
    let settings = state.settings.read();
    let clocks = state.poll_clocks.lock();
    let backoff = state.backoff.lock();
    pollable_providers(&settings).iter().any(|id| {
        should_poll(
            &poll_input(&settings, &clocks, &backoff, id, now_ms),
            now_ms,
        )
    })
}

// ---------- quota refresh ----------

async fn refresh_loop(app: AppHandle) {
    loop {
        if any_provider_due(&app, store::now_ms()) {
            refresh(&app, None, true).await;
        }
        tokio::time::sleep(TICK).await;
    }
}

/// Force a refresh (ignores the backoff) — used by `refresh_now` and the tray.
pub async fn refresh_now(app: &AppHandle, provider: Option<String>) -> AppSnapshot {
    refresh(app, provider, false).await
}

/// Fetch quotas, update the cached snapshot, store samples and emit
/// `snapshot-updated`. Never fails: failures surface as provider statuses.
pub async fn refresh(app: &AppHandle, only: Option<String>, respect_backoff: bool) -> AppSnapshot {
    let state = app.state::<AppState>();
    let _refresh_guard = state.refresh_lock.lock().await;
    let (ctx, http, settings, previous, db) = {
        let state = app.state::<AppState>();
        let parts = (
            state.provider_ctx.clone(),
            state.http.clone(),
            state.settings.read().clone(),
            state.snapshot.read().clone(),
            state.db.clone(),
        );
        parts
    };
    let now = store::now_ms();

    // Providers that are not due yet — either still inside their error backoff
    // window or quiet enough for the adaptive interval — keep their previous
    // value. An explicit refresh (`respect_backoff == false`) polls everything.
    let skipped: HashSet<String> = if respect_backoff {
        let state = app.state::<AppState>();
        let clocks = state.poll_clocks.lock();
        let backoff = state.backoff.lock();
        pollable_providers(&settings)
            .into_iter()
            .filter(|id| !should_poll(&poll_input(&settings, &clocks, &backoff, id, now), now))
            .collect()
    } else {
        HashSet::new()
    };
    if !skipped.is_empty() {
        log::debug!("refresh: not due yet {:?}", skipped);
    }
    {
        // Record the attempt before awaiting so the next tick sees it.
        let state = app.state::<AppState>();
        let mut clocks = state.poll_clocks.lock();
        for id in pollable_providers(&settings) {
            if skipped.contains(&id) || only.as_deref().is_some_and(|wanted| wanted != id) {
                continue;
            }
            clocks.last_poll_ms.insert(id, now);
        }
    }

    let snapshot =
        providers::fetch_snapshot(&ctx, &http, &settings, only.as_deref(), &previous, |id| {
            skipped.contains(id)
        })
        .await;

    {
        let state = app.state::<AppState>();
        let mut backoff = state.backoff.lock();
        for q in &snapshot.providers {
            if skipped.contains(&q.provider) || only.as_deref().is_some_and(|id| id != q.provider) {
                continue;
            }
            let entry = backoff.entry(q.provider.clone()).or_default();
            match q.status {
                ProviderStatus::Error => entry.on_error(store::now_ms()),
                // Nothing to retry faster for: these need the user to act.
                _ => entry.on_success(),
            }
        }
        *state.snapshot.write() = snapshot.clone();
    }

    if let Some(db) = db {
        let to_store = snapshot.clone();
        let sampled_provider = only.clone();
        let _ = tauri::async_runtime::spawn_blocking(move || {
            for q in &to_store.providers {
                if !should_sample(q, sampled_provider.as_deref(), &skipped) {
                    continue;
                }
                let plan = q.plan_label.as_deref().or(q.plan.as_deref());
                if let Err(e) =
                    store::quota::insert_quota_samples(&db, &q.provider, plan, &q.windows)
                {
                    log::warn!("could not store quota samples for {}: {e:#}", q.provider);
                }
            }
        })
        .await;
    }

    if let Err(e) = app.emit(events::SNAPSHOT_UPDATED, &snapshot) {
        log::warn!("could not emit {}: {e}", events::SNAPSHOT_UPDATED);
    }
    snapshot
}

fn should_sample(
    q: &crate::model::ProviderQuota,
    only: Option<&str>,
    skipped: &HashSet<String>,
) -> bool {
    q.status == ProviderStatus::Ok
        && q.source == DataSource::Api
        && !q.windows.is_empty()
        && !skipped.contains(&q.provider)
        && only.is_none_or(|id| id == q.provider)
}

// ---------- log ingestion ----------

async fn ingest_loop(app: AppHandle, dirty: Arc<AtomicI64>) {
    tokio::time::sleep(INITIAL_INGEST_DELAY).await;
    let mut last_run_ms = 0i64;
    loop {
        let enabled = {
            let state = app.state::<AppState>();
            let enabled = state.settings.read().ingest_enabled;
            enabled
        };
        if enabled {
            let now = store::now_ms();
            let pending = dirty.load(Ordering::Relaxed);
            let debounced = pending > 0 && now - pending >= WATCH_DEBOUNCE_MS;
            let periodic = now - last_run_ms >= INGEST_INTERVAL_MS;
            if last_run_ms == 0 || debounced || periodic {
                if debounced {
                    dirty.store(0, Ordering::Relaxed);
                }
                run_ingest(&app, false).await;
                last_run_ms = store::now_ms();
            }
        }
        tokio::time::sleep(TICK).await;
    }
}

/// Run one ingestion pass (`full` = forget byte offsets first) and emit
/// `ingest-progress` at the start and the end.
pub async fn run_ingest(app: &AppHandle, full: bool) -> IngestStats {
    let (db, running) = {
        let state = app.state::<AppState>();
        (state.db.clone(), state.ingest_running.clone())
    };
    let Some(db) = db else {
        let mut stats = IngestStats::default();
        stats.errors.push("usage database is unavailable".into());
        return stats;
    };
    if running.swap(true, Ordering::SeqCst) {
        log::debug!("ingestion already running, skipping this round");
        return IngestStats {
            running: true,
            ..IngestStats::default()
        };
    }

    emit_progress(
        app,
        &IngestStats {
            running: true,
            ..IngestStats::default()
        },
    );
    let stats = tauri::async_runtime::spawn_blocking(move || ingest::run(&db, full))
        .await
        .unwrap_or_else(|e| {
            let mut stats = IngestStats::default();
            stats.errors.push(format!("ingestion task failed: {e}"));
            stats
        });
    running.store(false, Ordering::SeqCst);

    if stats.events_added > 0 || !stats.errors.is_empty() {
        log::info!(
            "ingest: {} files scanned, {} updated, {} events, {} ms",
            stats.files_scanned,
            stats.files_updated,
            stats.events_added,
            stats.duration_ms
        );
    }
    emit_progress(app, &stats);
    stats
}

fn emit_progress(app: &AppHandle, stats: &IngestStats) {
    if let Err(e) = app.emit(events::INGEST_PROGRESS, stats) {
        log::warn!("could not emit {}: {e}", events::INGEST_PROGRESS);
    }
}

/// Watch the log roots and record the time of the newest change, globally
/// (for the ingest debounce) and per provider (for the adaptive interval).
/// Runs on its own thread which owns the watcher for the lifetime of the app.
fn spawn_log_watcher(dirty: Arc<AtomicI64>, clocks: Arc<Mutex<PollClocks>>) {
    std::thread::spawn(move || {
        use notify::{RecursiveMode, Watcher};
        let roots = ingest::roots();
        seed_activity(&roots, &clocks);
        let (tx, rx) = std::sync::mpsc::channel();
        let mut watcher = match notify::recommended_watcher(move |res| {
            let _ = tx.send(res);
        }) {
            Ok(w) => w,
            Err(e) => {
                log::warn!("no log watcher ({e}); falling back to polling");
                return;
            }
        };
        let mut watched = 0;
        for root in &roots {
            match watcher.watch(&root.path, RecursiveMode::Recursive) {
                Ok(()) => watched += 1,
                Err(e) => log::debug!("cannot watch {}: {e}", root.path.display()),
            }
        }
        if watched == 0 {
            return;
        }
        while let Ok(event) = rx.recv() {
            if let Ok(ev) = event {
                if ev.kind.is_modify() || ev.kind.is_create() {
                    let now = store::now_ms();
                    dirty.store(now, Ordering::Relaxed);
                    let mut guard = clocks.lock();
                    for path in &ev.paths {
                        if let Some(provider) = provider_for_path(&roots, path) {
                            guard.last_activity_ms.insert(provider.to_string(), now);
                        }
                    }
                }
            }
        }
    });
}

/// The provider a changed file belongs to: the deepest watched root that is a
/// prefix of `path` (Codex has both a live and an archived session root).
fn provider_for_path<'a>(roots: &'a [ingest::Root], path: &Path) -> Option<&'a str> {
    roots
        .iter()
        .filter(|root| path.starts_with(&root.path))
        .max_by_key(|root| root.path.as_os_str().len())
        .map(|root| root.provider)
}

/// Seed the activity clock from the newest session log on disk, so a cold
/// start already knows whether the user has been working recently.
fn seed_activity(roots: &[ingest::Root], clocks: &Mutex<PollClocks>) {
    for root in roots {
        let newest = ingest::session_files(&root.path)
            .iter()
            .filter_map(|path| mtime_ms(path))
            .max();
        if let Some(ms) = newest {
            let mut guard = clocks.lock();
            let entry = guard
                .last_activity_ms
                .entry(root.provider.to_string())
                .or_insert(ms);
            *entry = (*entry).max(ms);
        }
    }
}

fn mtime_ms(path: &Path) -> Option<i64> {
    std::fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis().min(i64::MAX as u128) as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{QuotaWindow, WindowKind};

    #[test]
    fn quota_history_only_samples_successful_live_refreshes() {
        let mut quota = providers::empty_quota("codex", "Codex", ProviderStatus::Ok);
        quota.windows.push(QuotaWindow {
            kind: WindowKind::SevenDay,
            label: "Weekly".into(),
            window_seconds: Some(604800),
            used_percent: 40.0,
            resets_at: None,
            scope: None,
            is_primary: true,
        });
        let empty = HashSet::new();
        assert!(should_sample(&quota, None, &empty));
        assert!(!should_sample(&quota, Some("claude"), &empty));
        assert!(!should_sample(
            &quota,
            None,
            &HashSet::from(["codex".into()])
        ));
        quota.source = DataSource::Cache;
        assert!(!should_sample(&quota, None, &empty));
        quota.source = DataSource::LocalLog;
        assert!(!should_sample(&quota, None, &empty));
        quota.source = DataSource::Api;
        quota.status = ProviderStatus::Error;
        assert!(!should_sample(&quota, None, &empty));
    }

    fn input(idle_secs: Option<u64>, last_poll_ms: i64) -> PollInput {
        PollInput {
            configured_secs: 60,
            adaptive: true,
            idle_secs,
            last_poll_ms,
            backoff_until_ms: 0,
        }
    }

    #[test]
    fn the_interval_stretches_while_a_provider_is_quiet() {
        // Busy, or activity not known yet → exactly what the user configured.
        assert_eq!(poll_interval_secs(60, true, Some(0)), 60);
        assert_eq!(poll_interval_secs(60, true, Some(599)), 60);
        assert_eq!(poll_interval_secs(60, true, None), 60);
        // 10 min quiet → ×2, 30 min quiet → ×5.
        assert_eq!(poll_interval_secs(60, true, Some(600)), 120);
        assert_eq!(poll_interval_secs(60, true, Some(1_799)), 120);
        assert_eq!(poll_interval_secs(60, true, Some(1_800)), 300);
        assert_eq!(poll_interval_secs(60, true, Some(86_400)), 300);
        // The cap applies to the stretch, never to the configured value.
        assert_eq!(poll_interval_secs(300, true, Some(86_400)), 600);
        assert_eq!(poll_interval_secs(900, true, Some(86_400)), 900);
        // Opting out, and the hard minimum, still hold.
        assert_eq!(poll_interval_secs(60, false, Some(86_400)), 60);
        assert_eq!(poll_interval_secs(0, true, Some(86_400)), 75);
    }

    #[test]
    fn a_provider_is_due_after_its_own_interval() {
        let now = 1_700_000_000_000i64;
        // Never polled → due at once.
        assert!(should_poll(&input(Some(0), 0), now));
        // Busy: one configured interval after the last poll.
        assert!(!should_poll(&input(Some(30), now - 59_000), now));
        assert!(should_poll(&input(Some(30), now - 60_000), now));
        // Quiet for an hour: five intervals.
        assert!(!should_poll(&input(Some(3_600), now - 120_000), now));
        assert!(should_poll(&input(Some(3_600), now - 300_000), now));
        // Fresh activity snaps back: the same poll time is due again.
        assert!(should_poll(&input(Some(5), now - 120_000), now));
    }

    #[test]
    fn the_error_backoff_still_wins_over_the_adaptive_interval() {
        let now = 1_700_000_000_000i64;
        let mut busy = input(Some(0), now - 600_000);
        assert!(should_poll(&busy, now), "overdue without a backoff");
        busy.backoff_until_ms = now + 1;
        assert!(!should_poll(&busy, now), "backoff delays an overdue poll");
        assert_eq!(next_poll_due_ms(&busy), now + 1);

        // A backoff that already expired never pulls a poll forward.
        let quiet = PollInput {
            backoff_until_ms: now - 10_000,
            ..input(Some(3_600), now - 60_000)
        };
        assert!(!should_poll(&quiet, now));
        assert_eq!(next_poll_due_ms(&quiet), now - 60_000 + 300_000);

        // A provider that has never been polled waits for the backoff only.
        let fresh = PollInput {
            backoff_until_ms: now + 5_000,
            ..input(None, 0)
        };
        assert_eq!(next_poll_due_ms(&fresh), now + 5_000);
    }

    #[test]
    fn activity_is_attributed_to_the_deepest_matching_root() {
        let roots = vec![
            ingest::Root {
                provider: providers::CLAUDE_ID,
                path: "/home/u/.claude/projects".into(),
            },
            ingest::Root {
                provider: providers::CODEX_ID,
                path: "/home/u/.codex/sessions".into(),
            },
            ingest::Root {
                provider: providers::CODEX_ID,
                path: "/home/u/.codex/sessions/archived".into(),
            },
        ];
        let at = |p: &str| provider_for_path(&roots, Path::new(p));
        assert_eq!(at("/home/u/.claude/projects/app/a.jsonl"), Some("claude"));
        assert_eq!(at("/home/u/.codex/sessions/2026/a.jsonl"), Some("codex"));
        assert_eq!(
            at("/home/u/.codex/sessions/archived/a.jsonl"),
            Some("codex")
        );
        assert_eq!(at("/home/u/notes/a.jsonl"), None);
    }

    #[test]
    fn idle_seconds_come_from_the_activity_clock() {
        let now = 1_700_000_000_000i64;
        let mut clocks = PollClocks::default();
        assert_eq!(clocks.idle_secs("claude", now), None);
        clocks
            .last_activity_ms
            .insert("claude".into(), now - 90_000);
        assert_eq!(clocks.idle_secs("claude", now), Some(90));
        // An explicit interaction makes every provider count as active again.
        clocks.mark_interaction(now);
        assert_eq!(clocks.idle_secs("claude", now), Some(0));
        assert_eq!(clocks.idle_secs("codex", now), Some(0));
    }
}
