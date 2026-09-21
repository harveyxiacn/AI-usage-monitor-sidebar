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

use crate::commands::{forecast, ingest, providers, store};
use crate::model::{
    events, AppSnapshot, DataSource, ForecastConfidence, IngestStats, ProviderStatus, QuotaWindow,
    WindowKind,
};
use crate::state::AppState;
use std::collections::{BTreeMap, HashSet};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Listener, Manager};
use tauri_plugin_notification::NotificationExt;

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

    let mut snapshot =
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
        notify_forecasts(app, &snapshot);
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

// ---------- predictive notifications ----------

/// A "you will run out early" warning has to buy the user at least this much
/// time to be worth an interruption.
const FORECAST_NOTIFY_LEAD_MS: i64 = 5 * 60 * 1000;

/// `provider|kind|scope` → the reset (unix ms) the window was already warned
/// about, so each window is announced at most once per reset period. Entries
/// are forgotten once their reset has passed. It lives here rather than in
/// `AppState` so the whole predictive-notification path is in one place.
static FORECAST_NOTIFIED: Mutex<BTreeMap<String, i64>> = Mutex::new(BTreeMap::new());

/// A window that is on pace to run out before it resets.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ForecastAlert {
    /// deduplication key, stable across refreshes
    key: String,
    /// the reset this alert belongs to (unix ms)
    resets_at_ms: i64,
    /// how long before that reset the window is projected to hit 100 %
    early_by_ms: i64,
}

/// Decide whether `w` deserves a predictive notification. Pure: the settings
/// gate and the once-per-period bookkeeping are applied by the caller.
fn forecast_alert(provider: &str, w: &QuotaWindow, now_ms: i64) -> Option<ForecastAlert> {
    let f = w.forecast.as_ref()?;
    // A guess we ourselves call "low" must never wake the user.
    if f.confidence == ForecastConfidence::Low {
        return None;
    }
    let exhausts_at = forecast::parse_ms(f.exhausts_at.as_deref()?)?;
    let resets_at = forecast::parse_ms(w.resets_at.as_deref()?)?;
    let early_by_ms = resets_at - exhausts_at;
    if early_by_ms < FORECAST_NOTIFY_LEAD_MS || exhausts_at <= now_ms {
        return None;
    }
    Some(ForecastAlert {
        key: format!(
            "{provider}|{}|{}",
            w.kind.as_str(),
            w.scope.as_deref().unwrap_or("")
        ),
        resets_at_ms: resets_at,
        early_by_ms,
    })
}

/// `true` the first time this alert is seen for its reset period.
fn claim_alert(alert: &ForecastAlert, now_ms: i64) -> bool {
    let mut seen = FORECAST_NOTIFIED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    seen.retain(|_, resets_at| *resets_at > now_ms);
    if seen.get(&alert.key) == Some(&alert.resets_at_ms) {
        return false;
    }
    seen.insert(alert.key.clone(), alert.resets_at_ms);
    true
}

/// Notification title + body. The native side carries the same two catalogues
/// as the tray menu (see `window::tray::prefers_chinese`).
fn alert_text(
    display_name: &str,
    w: &QuotaWindow,
    early_by_ms: i64,
    chinese: bool,
) -> (String, String) {
    let kind = match (w.kind, chinese) {
        (WindowKind::FiveHour, false) => "5-hour".to_string(),
        (WindowKind::FiveHour, true) => "5 小时".to_string(),
        (WindowKind::SevenDay, false) => "weekly".to_string(),
        (WindowKind::SevenDay, true) => "每周".to_string(),
        (WindowKind::Other, _) => w.label.clone(),
    };
    let window = match w.scope.as_deref() {
        Some(scope) => format!("{kind} · {scope}"),
        None => kind,
    };
    let minutes = (early_by_ms / 60_000).max(1);
    let (h, m) = (minutes / 60, minutes % 60);
    if chinese {
        let lead = if h > 0 {
            format!("{h} 小时 {m} 分钟")
        } else {
            format!("{m} 分钟")
        };
        (
            format!("{display_name} {window}限额"),
            format!("按当前速度，将在重置前约 {lead}用尽"),
        )
    } else {
        let lead = if h > 0 {
            format!("{h} h {m} min")
        } else {
            format!("{m} min")
        };
        (
            format!("{display_name} {window} limit"),
            format!("On pace to run out ~{lead} before it resets"),
        )
    }
}

/// Warn about every window that is on pace to run out before its reset.
/// Gated by `notifications` **and** `forecastNotifications`.
fn notify_forecasts(app: &AppHandle, snapshot: &AppSnapshot) {
    let settings = {
        let state = app.state::<AppState>();
        let settings = state.settings.read().clone();
        settings
    };
    if !(settings.notifications && settings.forecast_notifications) {
        return;
    }
    let now = store::now_ms();
    let chinese = crate::window::tray::prefers_chinese(&settings);
    for q in &snapshot.providers {
        if q.status != ProviderStatus::Ok {
            continue;
        }
        for w in &q.windows {
            let Some(alert) = forecast_alert(&q.provider, w, now) else {
                continue;
            };
            if !claim_alert(&alert, now) {
                continue;
            }
            let (title, body) = alert_text(&q.display_name, w, alert.early_by_ms, chinese);
            log::info!("forecast notification: {title} — {body}");
            if let Err(e) = app.notification().builder().title(title).body(body).show() {
                log::warn!("could not show the forecast notification: {e}");
            }
        }
    }
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

    // ---------- predictive notifications ----------

    use crate::model::QuotaForecast;

    const NOW: i64 = 1_789_430_400_000; // 2026-09-15T00:00:00Z
    const RESETS_AT: &str = "2026-09-15T02:00:00Z"; // NOW + 2 h

    /// A 5-hour window projected to run out `early_by` minutes before it resets.
    fn alerting_window(early_by_min: i64, confidence: ForecastConfidence) -> QuotaWindow {
        let exhausts = NOW + (120 - early_by_min) * 60_000;
        QuotaWindow {
            kind: WindowKind::FiveHour,
            label: "5-hour".into(),
            window_seconds: Some(18_000),
            used_percent: 82.0,
            resets_at: Some(RESETS_AT.into()),
            scope: None,
            is_primary: true,
            forecast: Some(QuotaForecast {
                projected_percent_at_reset: 140.0,
                exhausts_at: crate::commands::providers::rfc3339_from_unix_ms(exhausts),
                rate_percent_per_hour: 29.0,
                confidence,
            }),
        }
    }

    #[test]
    fn only_a_confident_early_exhaustion_is_worth_a_notification() {
        let w = alerting_window(25, ForecastConfidence::High);
        let alert = forecast_alert("claude", &w, NOW).expect("high confidence, 25 min early");
        assert_eq!(alert.key, "claude|five_hour|");
        assert_eq!(alert.early_by_ms, 25 * 60_000);
        assert!(forecast_alert(
            "claude",
            &alerting_window(25, ForecastConfidence::Medium),
            NOW
        )
        .is_some());

        // low confidence, too little lead time, and no forecast at all
        assert!(
            forecast_alert("claude", &alerting_window(25, ForecastConfidence::Low), NOW).is_none()
        );
        assert!(
            forecast_alert("claude", &alerting_window(4, ForecastConfidence::High), NOW).is_none()
        );
        let mut w = alerting_window(25, ForecastConfidence::High);
        w.forecast = None;
        assert!(forecast_alert("claude", &w, NOW).is_none());

        // a projection that only reaches 100 % at the reset has no exhausts_at
        let mut w = alerting_window(25, ForecastConfidence::High);
        w.forecast.as_mut().unwrap().exhausts_at = None;
        assert!(forecast_alert("claude", &w, NOW).is_none());

        // the scope is part of the key, so per-model limits warn separately
        let mut w = alerting_window(25, ForecastConfidence::High);
        w.scope = Some("Fable".into());
        assert_eq!(
            forecast_alert("claude", &w, NOW).unwrap().key,
            "claude|five_hour|Fable"
        );
    }

    #[test]
    fn a_window_is_announced_once_per_reset_period() {
        let base = ForecastAlert {
            key: "test-provider|five_hour|".into(),
            resets_at_ms: NOW + 2 * 3_600_000,
            early_by_ms: 25 * 60_000,
        };
        assert!(claim_alert(&base, NOW), "first time wins");
        assert!(!claim_alert(&base, NOW), "same reset period stays quiet");

        // the next period is a new warning
        let next = ForecastAlert {
            resets_at_ms: base.resets_at_ms + 5 * 3_600_000,
            ..base.clone()
        };
        assert!(claim_alert(&next, NOW + 3 * 3_600_000));
        // …and the elapsed period is forgotten rather than accumulated
        assert!(!FORECAST_NOTIFIED
            .lock()
            .unwrap()
            .values()
            .any(|resets_at| *resets_at <= NOW + 3 * 3_600_000));
    }

    #[test]
    fn the_notification_text_names_the_provider_window_and_lead_time() {
        let w = alerting_window(25, ForecastConfidence::High);
        assert_eq!(
            alert_text("Claude", &w, 25 * 60_000, false),
            (
                "Claude 5-hour limit".to_string(),
                "On pace to run out ~25 min before it resets".to_string()
            )
        );
        assert_eq!(
            alert_text("Claude", &w, 25 * 60_000, true),
            (
                "Claude 5 小时限额".to_string(),
                "按当前速度，将在重置前约 25 分钟用尽".to_string()
            )
        );

        let mut weekly = w.clone();
        weekly.kind = WindowKind::SevenDay;
        weekly.scope = Some("Fable".into());
        assert_eq!(
            alert_text("Claude", &weekly, 150 * 60_000, false).0,
            "Claude weekly · Fable limit"
        );
        assert_eq!(
            alert_text("Claude", &weekly, 150 * 60_000, false).1,
            "On pace to run out ~2 h 30 min before it resets"
        );
    }
}
