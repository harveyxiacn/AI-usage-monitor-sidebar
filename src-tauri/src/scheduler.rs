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
//! * **maintenance loop** — daily quota-sample retention/thinning, WAL
//!   checkpoint and `PRAGMA optimize` (`settings.quotaRetentionDays`).
//! * **event listener** — the tray emits `refresh-requested`; opening the
//!   dashboard cancels the adaptive stretch.
//!
//! Nothing here ever blocks the UI thread: HTTP happens on the async runtime,
//! SQLite inside `spawn_blocking`.

use crate::commands::{forecast, ingest, providers, store};
use crate::model::{events, AppSnapshot, DataSource, IngestStats, ProviderStatus, Settings};
use crate::state::{AppState, Backoff, PollClocks};
use parking_lot::Mutex;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
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
const MAINTENANCE_FIRST_DELAY: Duration = Duration::from_secs(300);
const MAINTENANCE_INTERVAL: Duration = Duration::from_secs(24 * 3600);

pub fn start(app: AppHandle) {
    let dirty = Arc::new(Mutex::new(DirtyFiles::default()));
    let clocks = app.state::<AppState>().poll_clocks.clone();
    spawn_log_watcher(dirty.clone(), clocks);
    // Budget alerts and the weekly summary run on their own slow timer.
    crate::commands::alerts::start(app.clone());

    let refresh_app = app.clone();
    tauri::async_runtime::spawn(async move { refresh_loop(refresh_app).await });

    let maintenance_app = app.clone();
    tauri::async_runtime::spawn(async move { maintenance_loop(maintenance_app).await });

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

/// Daily database housekeeping (`store::maintain_quota_samples`). The first
/// pass runs a few minutes after start so it never competes with start-up.
async fn maintenance_loop(app: AppHandle) {
    tokio::time::sleep(MAINTENANCE_FIRST_DELAY).await;
    loop {
        let (db, retention_days) = {
            let state = app.state::<AppState>();
            let days = state.settings.read().quota_retention_days;
            (state.db.clone(), days)
        };
        if let Some(db) = db {
            let result = tauri::async_runtime::spawn_blocking(move || {
                store::maintain_quota_samples(&db, store::now_ms(), retention_days)
            })
            .await;
            match result {
                Ok(Ok(stats)) => log::info!(
                    "maintenance: {} expired and {} thinned quota samples removed",
                    stats.expired,
                    stats.thinned
                ),
                Ok(Err(e)) => log::warn!("database maintenance failed: {e:#}"),
                Err(e) => log::warn!("database maintenance task failed: {e}"),
            }
        }
        tokio::time::sleep(MAINTENANCE_INTERVAL).await;
    }
}

// ---------- adaptive polling ----------

/// Lower bound on `refreshIntervalSec` (also enforced by `settings::clamp`).
pub const MIN_INTERVAL_SEC: u64 = 15;
/// Anthropic's `/api/oauth/usage` shares its budget with Claude Code's own
/// calls and answers `HTTP 429` when polled too eagerly. A 5-hour window moves
/// about 1 % per 3 min, so polling faster than this buys nothing (§7).
pub const CLAUDE_MIN_INTERVAL_SEC: u64 = 120;
/// `(idle seconds, multiplier)` — the first matching row wins, so keep this
/// ordered from the longest idle period down.
const ADAPTIVE_STEPS: &[(u64, u64)] = &[(1_800, 5), (600, 2)];
/// However long a provider stays quiet, never poll it less often than this.
const ADAPTIVE_MAX_SEC: u64 = 600;
/// Ceiling for the interval learned from `HTTP 429` answers.
const LEARNED_MAX_SEC: u64 = 900;

/// The floor below which a provider is never polled, whatever the user
/// configured. Only Claude needs one today.
pub fn provider_min_interval_secs(provider: &str) -> u64 {
    match provider {
        providers::CLAUDE_ID => CLAUDE_MIN_INTERVAL_SEC,
        _ => MIN_INTERVAL_SEC,
    }
}

/// Everything the scheduler knows about one provider when it decides whether
/// to poll it this round. Deliberately plain data so the decisions below are
/// pure functions.
#[derive(Clone, Copy, Debug)]
pub struct PollInput {
    /// `settings.refreshIntervalSec`.
    pub configured_secs: u64,
    /// `settings.adaptiveRefresh`.
    pub adaptive: bool,
    /// Hard floor for this provider (`provider_min_interval_secs`).
    pub min_interval_secs: u64,
    /// Seconds since this provider's logs last changed; `None` = never seen.
    pub idle_secs: Option<u64>,
    /// unix ms of the last poll; 0 = never polled.
    pub last_poll_ms: i64,
    /// unix ms before which the error backoff forbids a retry; 0 = none.
    pub backoff_until_ms: i64,
    /// unix ms the server asked us to wait until (`Retry-After`); 0 = none.
    /// Unlike the other waits this one also holds for an explicit refresh.
    pub retry_after_ms: i64,
    /// AIMD multiplier learned from `HTTP 429` answers (1 = none).
    pub rate_limit_factor: u32,
}

/// The steady-state interval before any adaptive or learned stretching.
fn base_interval_secs(input: &PollInput) -> u64 {
    input
        .configured_secs
        .max(MIN_INTERVAL_SEC)
        .max(input.min_interval_secs)
}

/// Effective polling period for one provider: the longest of the applicable
/// waits.
///
/// * **idle stretch** — a provider whose session logs changed recently is
///   polled at the configured interval; after ~10 min of quiet the period
///   doubles and after ~30 min it is five times as long (cap
///   `ADAPTIVE_MAX_SEC`).
/// * **learned stretch** — every `HTTP 429` doubles a per-provider multiplier
///   (cap `LEARNED_MAX_SEC`) which only decays after a run of good polls, so
///   the scheduler cannot oscillate straight back into the limit.
///
/// Neither stretch ever shortens what the user configured.
pub fn poll_interval_secs(input: &PollInput) -> u64 {
    let base = base_interval_secs(input);
    // An unknown activity time means "assume the user is working".
    let idle_factor = match (input.adaptive, input.idle_secs) {
        (true, Some(idle)) => ADAPTIVE_STEPS
            .iter()
            .find(|(after, _)| idle >= *after)
            .map_or(1, |(_, factor)| *factor),
        _ => 1,
    };
    let idle = base
        .saturating_mul(idle_factor)
        .min(base.max(ADAPTIVE_MAX_SEC));
    let learned = base
        .saturating_mul(input.rate_limit_factor.max(1) as u64)
        .min(base.max(LEARNED_MAX_SEC));
    idle.max(learned)
}

/// unix ms at which `input`'s provider may be polled again: the effective
/// period after the last poll, and never before the error backoff or a
/// server-supplied `Retry-After` has elapsed.
pub fn next_poll_due_ms(input: &PollInput) -> i64 {
    let gate = input.backoff_until_ms.max(input.retry_after_ms);
    if input.last_poll_ms == 0 {
        // Never polled (start-up, or a provider the user just enabled).
        return gate;
    }
    input
        .last_poll_ms
        .saturating_add((poll_interval_secs(input) as i64).saturating_mul(1_000))
        .max(gate)
}

pub fn should_poll(input: &PollInput, now_ms: i64) -> bool {
    now_ms >= next_poll_due_ms(input)
}

/// An explicit refresh (tray, dashboard button, `refresh_now`) ignores the
/// schedule and the error backoff, but not a `Retry-After`: the server told us
/// in so many words to stop asking.
pub fn should_force_poll(input: &PollInput, now_ms: i64) -> bool {
    now_ms >= input.retry_after_ms
}

fn poll_input(
    settings: &Settings,
    clocks: &PollClocks,
    backoff: &std::collections::HashMap<String, Backoff>,
    provider: &str,
    now_ms: i64,
) -> PollInput {
    let entry = backoff.get(provider).copied().unwrap_or_default();
    PollInput {
        configured_secs: settings.refresh_interval_sec,
        adaptive: settings.adaptive_refresh,
        min_interval_secs: provider_min_interval_secs(provider),
        idle_secs: clocks.idle_secs(provider, now_ms),
        last_poll_ms: clocks.last_poll_ms.get(provider).copied().unwrap_or(0),
        backoff_until_ms: entry.next_attempt_ms,
        retry_after_ms: entry.retry_after_ms,
        rate_limit_factor: entry.factor(),
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

    // Providers that are not due yet — still inside their error backoff or a
    // `Retry-After`, or quiet enough for the adaptive interval — keep their
    // previous value. An explicit refresh (`respect_backoff == false`) polls
    // everything except providers the server explicitly told us to leave alone.
    let skipped: HashSet<String> = {
        let state = app.state::<AppState>();
        let clocks = state.poll_clocks.lock();
        let backoff = state.backoff.lock();
        pollable_providers(&settings)
            .into_iter()
            .filter(|id| {
                let input = poll_input(&settings, &clocks, &backoff, id, now);
                if respect_backoff {
                    !should_poll(&input, now)
                } else {
                    !should_force_poll(&input, now)
                }
            })
            .collect()
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

    let mut snapshot =
        providers::fetch_snapshot(&ctx, &http, &settings, only.as_deref(), &previous, |id| {
            skipped.contains(id)
        })
        .await;

    {
        let state = app.state::<AppState>();
        let clocks = state.poll_clocks.lock();
        let mut backoff = state.backoff.lock();
        let before = backoff.clone();
        let done = store::now_ms();
        for q in &mut snapshot.providers {
            let fetched =
                !skipped.contains(&q.provider) && only.as_deref().is_none_or(|id| id == q.provider);
            if fetched {
                let entry = backoff.entry(q.provider.clone()).or_default();
                match q.status {
                    ProviderStatus::RateLimited => {
                        let wait = retry_after_secs(q, done);
                        entry.on_rate_limited(done, wait);
                        // Once per 429, so the log can answer "how often?".
                        log::warn!(
                            "{}: rate limited (HTTP 429), waiting {}s; learned interval ×{}",
                            q.provider,
                            wait,
                            entry.factor()
                        );
                    }
                    ProviderStatus::Error => entry.on_error(done),
                    // Nothing to retry faster for: these need the user to act.
                    _ => entry.on_success(),
                }
            }
            if q.status == ProviderStatus::RateLimited {
                // Replace the server's answer with the time we will really try.
                let input = poll_input(&settings, &clocks, &backoff, &q.provider, done);
                q.next_attempt_at = providers::rfc3339_from_unix_ms(next_poll_due_ms(&input));
            }
        }
        if *backoff != before {
            crate::state::save_backoff(&state.data_dir, &backoff);
        }
        *state.snapshot.write() = snapshot.clone();
    }

    if let Some(db) = db {
        let mut to_store = snapshot.clone();
        let sampled_provider = only.clone();
        // Sample first, then forecast: the projection must see the value the UI
        // is about to show, not the one from the previous round.
        let enriched = tauri::async_runtime::spawn_blocking(move || {
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
            forecast::attach(&db, &mut to_store, store::now_ms());
            to_store
        })
        .await;
        if let Ok(enriched) = enriched {
            snapshot = enriched;
            *app.state::<AppState>().snapshot.write() = snapshot.clone();
        }
    }

    crate::commands::alerts::on_snapshot(app, &snapshot);

    if let Err(e) = app.emit(events::SNAPSHOT_UPDATED, &snapshot) {
        log::warn!("could not emit {}: {e}", events::SNAPSHOT_UPDATED);
    }
    crate::window::tray::sync_usage(app, &snapshot);
    snapshot
}

/// How long the provider asked us to wait, read back from the `next_attempt_at`
/// it derived from `Retry-After`. Falls back to the default wait.
fn retry_after_secs(q: &crate::model::ProviderQuota, now_ms: i64) -> u64 {
    q.next_attempt_at
        .as_deref()
        .and_then(providers::rfc3339_to_ms)
        .map(|ms| (ms.saturating_sub(now_ms).max(0) / 1_000) as u64)
        .filter(|secs| *secs > 0)
        .unwrap_or(providers::RETRY_AFTER_DEFAULT_SECS)
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

/// Bounded path queue; overflow falls back to a full directory reconciliation.
#[derive(Default)]
struct DirtyFiles {
    paths: BTreeMap<PathBuf, String>,
    first_change: i64,
    last_change: i64,
    overflow: bool,
}

impl DirtyFiles {
    fn enqueue(&mut self, provider: &str, path: PathBuf, now: i64) {
        if self.first_change == 0 {
            self.first_change = now;
        }
        self.last_change = now;
        if self.paths.len() < 4096 {
            self.paths.insert(path, provider.to_owned());
        } else {
            self.overflow = true;
        }
    }
    /// Request a full reconciliation (events may have been lost).
    fn mark_overflow(&mut self, now: i64) {
        if self.first_change == 0 {
            self.first_change = now;
        }
        self.last_change = now;
        self.overflow = true;
    }
    fn ready(&self, now: i64) -> bool {
        self.first_change > 0
            && (now - self.last_change >= WATCH_DEBOUNCE_MS || now - self.first_change >= 10_000)
    }
    fn take(&mut self) -> (Vec<(String, PathBuf)>, bool) {
        let state = std::mem::take(self);
        (
            state
                .paths
                .into_iter()
                .map(|(path, provider)| (provider, path))
                .collect(),
            state.overflow,
        )
    }
}

async fn ingest_loop(app: AppHandle, dirty: Arc<Mutex<DirtyFiles>>) {
    tokio::time::sleep(INITIAL_INGEST_DELAY).await;
    let mut last_reconcile_ms = 0i64;
    loop {
        let enabled = app.state::<AppState>().settings.read().ingest_enabled;
        if enabled {
            let now = store::now_ms();
            let periodic = last_reconcile_ms == 0 || now - last_reconcile_ms >= INGEST_INTERVAL_MS;
            let batch = {
                let mut queue = dirty.lock();
                (periodic || queue.ready(now)).then(|| queue.take())
            };
            if let Some((paths, overflow)) = batch {
                let reconcile = periodic || overflow;
                let stats = run_ingest_selected(
                    &app,
                    false,
                    if reconcile { None } else { Some(paths.clone()) },
                )
                .await;
                if stats.running {
                    // Manual reindex is already running: retry this exact batch.
                    let mut queue = dirty.lock();
                    for (provider, path) in paths {
                        queue.enqueue(&provider, path, now);
                    }
                    queue.overflow |= overflow;
                } else if reconcile {
                    last_reconcile_ms = store::now_ms();
                }
            }
        }
        tokio::time::sleep(TICK).await;
    }
}

/// Run one ingestion pass (`full` = forget byte offsets first) and emit
/// `ingest-progress` at the start and the end.
pub async fn run_ingest(app: &AppHandle, full: bool) -> IngestStats {
    run_ingest_selected(app, full, None).await
}

async fn run_ingest_selected(
    app: &AppHandle,
    full: bool,
    paths: Option<Vec<(String, PathBuf)>>,
) -> IngestStats {
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
    let stats = tauri::async_runtime::spawn_blocking(move || match paths {
        Some(paths) => ingest::run_paths(&db, &paths),
        None => ingest::run(&db, full),
    })
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

/// How often the watcher thread re-checks which roots exist.
const WATCH_RETRY: Duration = Duration::from_secs(60);
/// Minimum gap between two logged watcher errors.
const WATCH_ERROR_LOG_MS: i64 = 300_000;

/// A watched root: its path and a cheap identity (creation time in ms, when
/// the file system reports one) so a deleted-and-recreated directory is
/// noticed even though the path is unchanged.
type RootIdentity = (PathBuf, Option<i64>);

fn root_identity(path: &Path) -> Option<i64> {
    std::fs::metadata(path)
        .ok()?
        .created()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis().min(i64::MAX as u128) as i64)
}

/// Compare the roots that exist now with the ones currently watched.
/// Returns `(to_unwatch, to_watch)`: watches whose root vanished or was
/// recreated are dropped, and every existing root that is not (or no longer)
/// watched is added.
fn plan_watches(
    existing: &[RootIdentity],
    watched: &[RootIdentity],
) -> (Vec<PathBuf>, Vec<RootIdentity>) {
    let stale = watched
        .iter()
        .filter(|w| !existing.contains(w))
        .map(|(path, _)| path.clone())
        .collect();
    let missing = existing
        .iter()
        .filter(|e| !watched.contains(e))
        .cloned()
        .collect();
    (stale, missing)
}

/// Watch the log roots and record the time of the newest change, globally
/// (for the ingest debounce) and per provider (for the adaptive interval).
/// Runs on its own thread which owns the watcher for the lifetime of the app.
/// Every `WATCH_RETRY` the set of existing roots is re-checked, so a root that
/// appears later (CLI installed after the app) or is deleted and recreated is
/// picked up without a restart.
fn spawn_log_watcher(dirty: Arc<Mutex<DirtyFiles>>, clocks: Arc<Mutex<PollClocks>>) {
    std::thread::spawn(move || {
        use notify::{RecursiveMode, Watcher};
        use std::sync::mpsc::RecvTimeoutError;
        let mut roots = ingest::roots();
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
        let mut watched: Vec<RootIdentity> = Vec::new();
        let mut last_error_log_ms = 0i64;
        let mut next_check = std::time::Instant::now();
        loop {
            if std::time::Instant::now() >= next_check {
                next_check = std::time::Instant::now() + WATCH_RETRY;
                let fresh = ingest::roots();
                let existing: Vec<RootIdentity> = fresh
                    .iter()
                    .map(|r| (r.path.clone(), root_identity(&r.path)))
                    .collect();
                let (stale, missing) = plan_watches(&existing, &watched);
                for path in &stale {
                    let _ = watcher.unwatch(path);
                }
                watched.retain(|w| !stale.contains(&w.0));
                let mut changed = !stale.is_empty();
                for root in missing {
                    match watcher.watch(&root.0, RecursiveMode::Recursive) {
                        Ok(()) => {
                            log::debug!("watching {}", root.0.display());
                            watched.push(root);
                            changed = true;
                        }
                        Err(e) => log::debug!("cannot watch {}: {e}", root.0.display()),
                    }
                }
                roots = fresh;
                if changed {
                    // Anything written while a root was unwatched is unseen.
                    seed_activity(&roots, &clocks);
                    dirty.lock().mark_overflow(store::now_ms());
                }
            }
            let wait = next_check.saturating_duration_since(std::time::Instant::now());
            let event = match rx.recv_timeout(wait) {
                Ok(event) => event,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => return,
            };
            match event {
                Ok(ev) => {
                    if ev.kind.is_modify() || ev.kind.is_create() {
                        let now = store::now_ms();
                        let mut guard = clocks.lock();
                        for path in &ev.paths {
                            if let Some(provider) = provider_for_path(&roots, path) {
                                guard.last_activity_ms.insert(provider.to_string(), now);
                                if path.extension().and_then(|s| s.to_str()) == Some("jsonl") {
                                    dirty.lock().enqueue(provider, path.clone(), now);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    // The backend lost events (overflow, deleted root, ...):
                    // rescan everything and re-check the roots right away.
                    let now = store::now_ms();
                    if now - last_error_log_ms >= WATCH_ERROR_LOG_MS {
                        last_error_log_ms = now;
                        log::warn!("log watcher error ({e}); scheduling a full rescan");
                    }
                    dirty.lock().mark_overflow(now);
                    next_check = std::time::Instant::now();
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
            forecast: None,
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
            min_interval_secs: MIN_INTERVAL_SEC,
            idle_secs,
            last_poll_ms,
            backoff_until_ms: 0,
            retry_after_ms: 0,
            rate_limit_factor: 1,
        }
    }

    fn interval(configured: u64, adaptive: bool, idle: Option<u64>) -> u64 {
        poll_interval_secs(&PollInput {
            configured_secs: configured,
            adaptive,
            idle_secs: idle,
            ..input(idle, 0)
        })
    }

    #[test]
    fn the_interval_stretches_while_a_provider_is_quiet() {
        // Busy, or activity not known yet → exactly what the user configured.
        assert_eq!(interval(60, true, Some(0)), 60);
        assert_eq!(interval(60, true, Some(599)), 60);
        assert_eq!(interval(60, true, None), 60);
        // 10 min quiet → ×2, 30 min quiet → ×5.
        assert_eq!(interval(60, true, Some(600)), 120);
        assert_eq!(interval(60, true, Some(1_799)), 120);
        assert_eq!(interval(60, true, Some(1_800)), 300);
        assert_eq!(interval(60, true, Some(86_400)), 300);
        // The cap applies to the stretch, never to the configured value.
        assert_eq!(interval(300, true, Some(86_400)), 600);
        assert_eq!(interval(900, true, Some(86_400)), 900);
        // Opting out, and the hard minimum, still hold.
        assert_eq!(interval(60, false, Some(86_400)), 60);
        assert_eq!(interval(0, true, Some(86_400)), 75);
    }

    #[test]
    fn claude_keeps_a_two_minute_floor_and_codex_does_not() {
        assert_eq!(provider_min_interval_secs("claude"), 120);
        assert_eq!(provider_min_interval_secs("codex"), MIN_INTERVAL_SEC);
        let floored = PollInput {
            configured_secs: 15,
            min_interval_secs: provider_min_interval_secs("claude"),
            ..input(Some(0), 0)
        };
        assert_eq!(poll_interval_secs(&floored), 120);
        // A longer configured interval still wins over the floor.
        assert_eq!(
            poll_interval_secs(&PollInput {
                configured_secs: 300,
                ..floored
            }),
            300
        );
        // Codex follows the user's setting all the way down to the clamp.
        assert_eq!(interval(15, true, Some(0)), 15);
    }

    #[test]
    fn a_learned_rate_limit_stretches_the_interval_and_composes_with_idling() {
        let busy = |factor| PollInput {
            rate_limit_factor: factor,
            ..input(Some(0), 0)
        };
        assert_eq!(poll_interval_secs(&busy(1)), 60);
        assert_eq!(poll_interval_secs(&busy(2)), 120);
        assert_eq!(poll_interval_secs(&busy(8)), 480);
        // The longest applicable wait wins: ×5 idle beats ×2 learned…
        assert_eq!(
            poll_interval_secs(&PollInput {
                rate_limit_factor: 2,
                ..input(Some(3_600), 0)
            }),
            300
        );
        // …and ×8 learned beats the idle stretch.
        assert_eq!(
            poll_interval_secs(&PollInput {
                rate_limit_factor: 8,
                ..input(Some(3_600), 0)
            }),
            480
        );
        // The learned stretch is capped too.
        assert_eq!(
            poll_interval_secs(&PollInput {
                configured_secs: 600,
                rate_limit_factor: 8,
                ..input(Some(0), 0)
            }),
            900
        );
    }

    #[test]
    fn aimd_grows_fast_and_decays_only_after_a_run_of_successes() {
        let now = 1_700_000_000_000i64;
        let mut b = Backoff::default();
        assert_eq!(b.factor(), 1);
        b.on_rate_limited(now, 300);
        assert_eq!(b.factor(), 2);
        assert_eq!(b.retry_after_ms, now + 300_000);
        assert_eq!(b.next_attempt_ms, now + 300_000);
        b.on_rate_limited(now, 60);
        assert_eq!(b.factor(), 4);
        // One success is not enough to go back to the old cadence.
        b.on_success();
        assert_eq!(b.factor(), 4);
        assert_eq!(b.retry_after_ms, 0, "the server gate is released");
        for _ in 1..crate::state::RATE_LIMIT_DECAY_AFTER {
            b.on_success();
        }
        assert_eq!(b.factor(), 2, "decays after a run of good polls");
        // Growth is capped.
        for _ in 0..10 {
            b.on_rate_limited(now, 30);
        }
        assert_eq!(b.factor(), crate::state::RATE_LIMIT_FACTOR_MAX);
    }

    #[test]
    fn an_explicit_refresh_obeys_retry_after_but_not_the_error_backoff() {
        let now = 1_700_000_000_000i64;
        let backed_off = PollInput {
            backoff_until_ms: now + 60_000,
            ..input(Some(0), now)
        };
        assert!(!should_poll(&backed_off, now));
        assert!(should_force_poll(&backed_off, now), "the user asked");

        let told_to_wait = PollInput {
            retry_after_ms: now + 60_000,
            ..input(Some(0), now - 600_000)
        };
        assert!(!should_poll(&told_to_wait, now));
        assert!(
            !should_force_poll(&told_to_wait, now),
            "hammering a 429 only makes it worse"
        );
        assert_eq!(next_poll_due_ms(&told_to_wait), now + 60_000);
        assert!(should_force_poll(&told_to_wait, now + 60_000));
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

    #[test]
    fn dirty_queue_deduplicates_files_without_losing_events_during_a_scan() {
        let mut dirty = DirtyFiles::default();
        dirty.enqueue("codex", "first.jsonl".into(), 1000);
        dirty.enqueue("codex", "first.jsonl".into(), 2000);
        assert!(!dirty.ready(4999));
        assert!(dirty.ready(5000));
        let (paths, overflow) = dirty.take();
        assert_eq!(paths.len(), 1);
        assert!(!overflow);
        dirty.enqueue("claude", "second.jsonl".into(), 5500);
        assert!(!dirty.ready(6000));
        assert!(dirty.ready(8500));
        assert_eq!(dirty.take().0[0].0, "claude");
    }

    #[test]
    fn busy_dirty_queue_flushes_and_overflow_requests_reconciliation() {
        let mut dirty = DirtyFiles::default();
        for now in 1000..12000 {
            dirty.enqueue("codex", format!("{now}.jsonl").into(), now);
        }
        assert!(
            dirty.ready(12000),
            "continuous writing cannot postpone ingestion indefinitely"
        );
        assert!(dirty.take().1, "a bounded queue reconciles after overflow");
        assert!(dirty.paths.is_empty());
    }

    #[test]
    fn watch_plan_adds_late_roots_and_rewatches_recreated_ones() {
        let a = (PathBuf::from("/a"), Some(1));
        let b = (PathBuf::from("/b"), Some(2));
        let (stale, missing) = plan_watches(&[a.clone(), b.clone()], &[]);
        assert!(stale.is_empty());
        assert_eq!(missing, vec![a.clone(), b.clone()]);
        let (stale, missing) = plan_watches(std::slice::from_ref(&a), std::slice::from_ref(&a));
        assert!(stale.is_empty() && missing.is_empty());
        let (stale, missing) = plan_watches(std::slice::from_ref(&a), &[a.clone(), b.clone()]);
        assert_eq!(stale, vec![b.0.clone()]);
        assert!(missing.is_empty());
        // Same path, new identity (deleted and recreated): unwatch and re-add.
        let a2 = (PathBuf::from("/a"), Some(9));
        let (stale, missing) = plan_watches(std::slice::from_ref(&a2), std::slice::from_ref(&a));
        assert_eq!(stale, vec![a.0.clone()]);
        assert_eq!(missing, vec![a2]);
    }

    #[test]
    fn a_watcher_error_requests_a_full_reconciliation() {
        let mut dirty = DirtyFiles::default();
        dirty.mark_overflow(1_000);
        assert!(dirty.ready(1_000 + WATCH_DEBOUNCE_MS));
        let (paths, overflow) = dirty.take();
        assert!(paths.is_empty() && overflow);
    }
}
