//! Periodic refresh + log ingestion. [BACKEND owns this file]
//! `start()` is called once from `lib.rs` after `AppState` is managed.
//!
//! Three background tasks:
//! * **refresh loop** — fetch every provider immediately, then every
//!   `settings.refreshIntervalSec` (re-read each round, minimum 15 s), honouring
//!   a per-provider exponential backoff (capped at 5 min) after failures.
//! * **ingest loop** — first scan 2 s after start, then every 60 s, plus a
//!   `notify` file watcher on the log roots debounced by 3 s.
//! * **event listener** — the tray emits `refresh-requested`.
//!
//! Nothing here ever blocks the UI thread: HTTP happens on the async runtime,
//! SQLite inside `spawn_blocking`.

use crate::commands::{ingest, providers, store};
use crate::model::{events, AppSnapshot, IngestStats, ProviderStatus};
use crate::state::AppState;
use std::collections::HashSet;
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
    spawn_log_watcher(dirty.clone());

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
}

// ---------- quota refresh ----------

async fn refresh_loop(app: AppHandle) {
    loop {
        refresh(&app, None, true).await;
        let secs = {
            let state = app.state::<AppState>();
            let s = state.settings.read();
            s.refresh_interval_sec.max(15)
        };
        tokio::time::sleep(Duration::from_secs(secs)).await;
    }
}

/// Force a refresh (ignores the backoff) — used by `refresh_now` and the tray.
pub async fn refresh_now(app: &AppHandle, provider: Option<String>) -> AppSnapshot {
    refresh(app, provider, false).await
}

/// Fetch quotas, update the cached snapshot, store samples and emit
/// `snapshot-updated`. Never fails: failures surface as provider statuses.
pub async fn refresh(app: &AppHandle, only: Option<String>, respect_backoff: bool) -> AppSnapshot {
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

    // Providers whose backoff window has not elapsed keep their previous value.
    let skipped: HashSet<String> = if respect_backoff {
        let state = app.state::<AppState>();
        let guard = state.backoff.lock();
        guard
            .iter()
            .filter(|(_, b)| b.next_attempt_ms > now)
            .map(|(id, _)| id.clone())
            .collect()
    } else {
        HashSet::new()
    };
    if !skipped.is_empty() {
        log::debug!("refresh: backing off {:?}", skipped);
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
            if skipped.contains(&q.provider) {
                continue;
            }
            let entry = backoff.entry(q.provider.clone()).or_default();
            match q.status {
                ProviderStatus::Error => entry.on_error(now),
                // Nothing to retry faster for: these need the user to act.
                _ => entry.on_success(),
            }
        }
        *state.snapshot.write() = snapshot.clone();
    }

    if let Some(db) = db {
        let to_store = snapshot.clone();
        let _ = tauri::async_runtime::spawn_blocking(move || {
            for q in &to_store.providers {
                if q.status == ProviderStatus::Disabled || q.windows.is_empty() {
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

/// Watch the log roots and record the time of the newest change. Runs on its
/// own thread which owns the watcher for the lifetime of the app.
fn spawn_log_watcher(dirty: Arc<AtomicI64>) {
    std::thread::spawn(move || {
        use notify::{RecursiveMode, Watcher};
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
        for root in ingest::roots() {
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
                    dirty.store(store::now_ms(), Ordering::Relaxed);
                }
            }
        }
    });
}
