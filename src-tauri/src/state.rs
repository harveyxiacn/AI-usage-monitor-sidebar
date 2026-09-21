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
use crate::model::{AppSnapshot, DataSource, PricingTable, Settings};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// Per-provider error backoff bookkeeping (exponential, capped at 5 min) plus
/// the AIMD multiplier learned from `HTTP 429` answers. Persisted across
/// restarts so restarting the app cannot walk straight back into a limit.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Backoff {
    pub consecutive_errors: u32,
    /// unix ms before which the scheduler should not retry
    pub next_attempt_ms: i64,
    /// unix ms the *server* asked us to wait until (`Retry-After`). Honoured
    /// even by an explicit refresh — hammering a 429 only makes it worse.
    pub retry_after_ms: i64,
    /// Multiplier on this provider's steady-state interval: doubled by every
    /// `429`, halved only after a run of good polls (0 reads as 1).
    pub rate_limit_factor: u32,
    pub consecutive_successes: u32,
}

pub const BACKOFF_MAX_MS: i64 = 5 * 60 * 1000;
/// A `429` at most doubles the interval each time, up to this multiplier.
pub const RATE_LIMIT_FACTOR_MAX: u32 = 8;
/// Consecutive good polls before the learned multiplier halves again — one
/// lucky success must not put us back on the cadence that caused the 429.
pub const RATE_LIMIT_DECAY_AFTER: u32 = 5;

impl Backoff {
    /// The learned multiplier, never below 1 (also repairs an older file).
    pub fn factor(&self) -> u32 {
        self.rate_limit_factor.max(1)
    }

    pub fn on_error(&mut self, now_ms: i64) {
        self.consecutive_errors = self.consecutive_errors.saturating_add(1);
        self.consecutive_successes = 0;
        let step = 15_000i64
            .saturating_mul(1i64 << self.consecutive_errors.min(6))
            .min(BACKOFF_MAX_MS);
        self.next_attempt_ms = now_ms + step;
    }

    /// `HTTP 429`: wait at least `retry_after_secs` and remember that this
    /// provider dislikes the current cadence.
    pub fn on_rate_limited(&mut self, now_ms: i64, retry_after_secs: u64) {
        self.consecutive_successes = 0;
        self.rate_limit_factor = self.factor().saturating_mul(2).min(RATE_LIMIT_FACTOR_MAX);
        self.retry_after_ms = now_ms.saturating_add(retry_after_secs as i64 * 1_000);
        self.next_attempt_ms = self.next_attempt_ms.max(self.retry_after_ms);
    }

    pub fn on_success(&mut self) {
        self.consecutive_errors = 0;
        self.next_attempt_ms = 0;
        self.retry_after_ms = 0;
        self.consecutive_successes = self.consecutive_successes.saturating_add(1);
        if self.consecutive_successes >= RATE_LIMIT_DECAY_AFTER {
            self.consecutive_successes = 0;
            self.rate_limit_factor = (self.factor() / 2).max(1);
        }
    }
}

pub const POLL_STATE_FILE: &str = "poll-state.json";

fn poll_state_path(data_dir: &Path) -> PathBuf {
    data_dir.join("cache").join(POLL_STATE_FILE)
}

/// Backoff state remembered from the last run; anything unreadable is simply
/// forgotten (the app then behaves like a first start).
pub fn load_backoff(data_dir: &Path) -> HashMap<String, Backoff> {
    std::fs::read_to_string(poll_state_path(data_dir))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save_backoff(data_dir: &Path, map: &HashMap<String, Backoff>) {
    let Ok(bytes) = serde_json::to_vec(map) else {
        return;
    };
    if let Err(e) = settings::write_atomic(&poll_state_path(data_dir), &bytes) {
        log::debug!("could not persist the poll state: {e:#}");
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
        // The user's own table wins; below it sits the cached remote list
        // (only when they configured one) and then the bundled defaults.
        let pricing = pricing::load_with_base(
            &config_dir,
            pricing::base_table(&data_dir, &settings.pricing_url),
        );
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

        // …and pretend those cached values were polled when they were
        // fetched, so restarting the app does not fire a burst of requests at
        // providers whose numbers are still fresh.
        let backoff = load_backoff(&data_dir);
        let mut poll_clocks = PollClocks::default();
        for q in &snapshot.providers {
            if q.source != DataSource::Cache {
                continue;
            }
            if let Some(ms) = providers::rfc3339_to_ms(&q.fetched_at) {
                poll_clocks.last_poll_ms.insert(q.provider.clone(), ms);
            }
        }

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
            backoff: Mutex::new(backoff),
            poll_clocks: Arc::new(Mutex::new(poll_clocks)),
        }
    }

    /// The DB handle, or a command-boundary error string.
    pub fn db(&self) -> Result<Arc<Db>, String> {
        self.db
            .clone()
            .ok_or_else(|| "usage database is unavailable".to_string())
    }
}
