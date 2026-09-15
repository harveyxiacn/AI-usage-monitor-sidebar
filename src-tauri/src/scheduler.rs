//! Periodic refresh + log ingestion. [BACKEND owns this file]
//! `start()` is called once from `lib.rs` after `AppState` is managed.

use tauri::AppHandle;

pub fn start(app: AppHandle) {
    let _ = app;
    // TODO(backend): spawn tokio task: fetch all providers every
    // settings.refresh_interval_sec, store quota samples, emit
    // events::SNAPSHOT_UPDATED; run incremental ingestion; watch log dirs.
}
