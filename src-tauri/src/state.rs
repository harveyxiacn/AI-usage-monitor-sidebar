//! Shared application state. [BACKEND owns this file]
//!
//! The platform layer only needs `AppState` to exist and be managed via
//! `app.manage(AppState::new(...))` in `lib.rs`; everything else is internal
//! to the backend.
//!
//! `AppState::new` is deliberately **synchronous and infallible**: settings and
//! the pricing table are read from disk before it returns, so the platform
//! layer sees real settings the moment `app.manage` runs.

use crate::commands::pricing;
use crate::commands::providers::{self, ProviderCtx};
use crate::commands::settings;
use crate::commands::store::Db;
use crate::model::{AppSnapshot, PricingTable, Settings};
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// Per-provider error backoff bookkeeping (exponential, capped at 5 min).
#[derive(Clone, Copy, Debug, Default)]
pub struct Backoff {
    pub consecutive_errors: u32,
    /// unix ms before which the scheduler should not retry
    pub next_attempt_ms: i64,
}

pub const BACKOFF_MAX_MS: i64 = 5 * 60 * 1000;

impl Backoff {
    pub fn on_error(&mut self, now_ms: i64) {
        self.consecutive_errors = self.consecutive_errors.saturating_add(1);
        let step = 15_000i64
            .saturating_mul(1i64 << self.consecutive_errors.min(6))
            .min(BACKOFF_MAX_MS);
        self.next_attempt_ms = now_ms + step;
    }
    pub fn on_success(&mut self) {
        self.consecutive_errors = 0;
        self.next_attempt_ms = 0;
    }
}

/// Adaptive polling bookkeeping, shared between the refresh loop and the log
/// watcher thread. All timestamps are unix ms; a missing entry means "never".
#[derive(Clone, Debug, Default)]
pub struct PollClocks {
    /// When each provider was last polled (or deliberately skipped as idle).
    pub last_poll_ms: HashMap<String, i64>,
    /// When each provider's session logs last changed.
    pub last_activity_ms: HashMap<String, i64>,
}

impl PollClocks {
    /// Seconds since `provider` was last seen working, `None` if never.
    pub fn idle_secs(&self, provider: &str, now_ms: i64) -> Option<u64> {
        let last = *self.last_activity_ms.get(provider)?;
        Some((now_ms.saturating_sub(last).max(0) / 1000) as u64)
    }

    /// Treat every provider as active again — used when the user explicitly
    /// asks for fresh numbers (tray refresh, dashboard opening).
    pub fn mark_interaction(&mut self, now_ms: i64) {
        for id in [
            crate::commands::providers::CLAUDE_ID,
            crate::commands::providers::CODEX_ID,
        ] {
            self.last_activity_ms.insert(id.to_string(), now_ms);
        }
    }
}

pub struct AppState {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub settings: RwLock<Settings>,
    pub snapshot: RwLock<AppSnapshot>,
    /// Complete user pricing table, or built-in defaults before the first save.
    pub pricing: RwLock<PricingTable>,
    /// `None` only when SQLite could not be opened at all; every DB-backed
    /// command then reports a friendly error instead of panicking.
    pub db: Option<Arc<Db>>,
    /// The single shared HTTP client (15 s timeout).
    pub http: reqwest::Client,
    pub provider_ctx: ProviderCtx,
    /// Guards against two ingestion runs at the same time.
    pub ingest_running: Arc<AtomicBool>,
    /// Serializes scheduled/manual refreshes so stale network responses cannot
    /// overwrite a newer provider snapshot or race the disk cache writer.
    pub refresh_lock: tokio::sync::Mutex<()>,
    pub backoff: Mutex<HashMap<String, Backoff>>,
    /// Adaptive polling clocks; the log watcher thread holds a clone.
    pub poll_clocks: Arc<Mutex<PollClocks>>,
}

impl AppState {
    pub fn new(config_dir: PathBuf, data_dir: PathBuf) -> Self {
        std::fs::create_dir_all(&config_dir).ok();
        std::fs::create_dir_all(&data_dir).ok();

        let settings = settings::load(&config_dir);
        let pricing = pricing::load(&config_dir);
        let provider_ctx = ProviderCtx::with_data_dir(&data_dir);

        let db = match Db::open(&data_dir.join("usage.db")) {
            Ok(db) => Some(Arc::new(db)),
            Err(e) => {
                log::error!("cannot open usage.db: {e:#}");
                None
            }
        };

        // Seed the snapshot from the on-disk cache so the very first
        // `get_snapshot` (before the scheduler's first fetch) is not empty.
        let snapshot = providers::snapshot_from_cache(&provider_ctx, &settings);

        Self {
            config_dir,
            data_dir,
            settings: RwLock::new(settings),
            snapshot: RwLock::new(snapshot),
            pricing: RwLock::new(pricing),
            db,
            http: providers::http_client(&provider_ctx.user_agent),
            provider_ctx,
            ingest_running: Arc::new(AtomicBool::new(false)),
            refresh_lock: tokio::sync::Mutex::new(()),
            backoff: Mutex::new(HashMap::new()),
            poll_clocks: Arc::new(Mutex::new(PollClocks::default())),
        }
    }

    /// The DB handle, or a command-boundary error string.
    pub fn db(&self) -> Result<Arc<Db>, String> {
        self.db
            .clone()
            .ok_or_else(|| "usage database is unavailable".to_string())
    }
}
