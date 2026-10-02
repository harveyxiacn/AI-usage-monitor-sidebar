//! Alerts: everything that can interrupt the user. [BACKEND]
//!
//! ```text
//! scheduler::refresh ──► alerts::on_snapshot ─┬─ threshold  (warn / critical crossings)
//!                                             ├─ predictive (on pace to run out)
//!                                             └─ advisor    (another provider has room; opt-in)
//! alerts::start loop ──► budget  (80 % / 100 % of monthlyBudgetUsd)
//!                    └─► summary (Monday ~09:00, last week)
//!                                  │
//!                                  ▼
//!                  notifier::deliver ─► native notification
//!                                    └► webhook (generic | ntfy | slack)
//! ```
//!
//! Gates, applied in one place (`notifier::channels_for`): the `notifications`
//! master switch, then focus mode, which silences **every** channel. Each
//! alert type has its own toggle on top (`thresholdNotifications`,
//! `forecastNotifications`, `advisorNotifications`, `budgetNotifications`,
//! `weeklySummary`).
//!
//! The scheduler only calls [`on_snapshot`]; [`start`] runs the two
//! time-driven checks (budget, weekly summary) on their own slow timer.

pub mod budget;
pub mod notifier;
pub mod predictive;
pub mod summary;
pub mod threshold;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use crate::commands::store;
use crate::model::{AppSnapshot, ProviderStatus, QuotaWindow, Settings, WeeklySummary, WindowKind};
use crate::state::AppState;

use self::notifier::Alert;

/// First time-driven check, a while after start-up.
const FIRST_TICK: Duration = Duration::from_secs(90);
/// Period of the time-driven checks (budget, weekly summary).
const TICK: Duration = Duration::from_secs(5 * 60);
const STATE_FILE: &str = "alerts-state.json";

pub fn now_ms() -> i64 {
    store::now_ms()
}

/// Short name of a quota window for notification text, in the user's language.
pub fn window_name(w: &QuotaWindow, chinese: bool) -> String {
    let kind = match (w.kind, chinese) {
        (WindowKind::FiveHour, false) => "5-hour".to_string(),
        (WindowKind::FiveHour, true) => "5 小时".to_string(),
        (WindowKind::SevenDay, false) => "weekly".to_string(),
        (WindowKind::SevenDay, true) => "每周".to_string(),
        (WindowKind::Other, _) => w.label.clone(),
    };
    match w.scope.as_deref() {
        Some(scope) => format!("{kind} · {scope}"),
        None => kind,
    }
}

// ---------- persisted once-only bookkeeping ----------

/// What has already been announced, so a restart does not repeat it.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
struct Announced {
    /// `YYYY-MM` the budget level below belongs to
    budget_month: String,
    /// highest budget level announced that month (1 = 80 %, 2 = 100 %)
    budget_level: u8,
    /// Monday (`YYYY-MM-DD`) of the last week whose summary was sent
    summary_week: String,
}

fn state_path(data_dir: &Path) -> PathBuf {
    data_dir.join(STATE_FILE)
}

fn load_announced(data_dir: &Path) -> Announced {
    std::fs::read(state_path(data_dir))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_announced(data_dir: &Path, announced: &Announced) {
    let result = serde_json::to_vec_pretty(announced)
        .map_err(anyhow::Error::from)
        .and_then(|b| crate::commands::settings::write_atomic(&state_path(data_dir), &b));
    if let Err(e) = result {
        log::warn!("could not save {STATE_FILE}: {e:#}");
    }
}

// ---------- snapshot-driven alerts ----------

static THRESHOLDS: Mutex<Option<threshold::ThresholdState>> = Mutex::new(None);

/// Called by the scheduler after every refresh. Cheap and synchronous.
pub fn on_snapshot(app: &AppHandle, snapshot: &AppSnapshot) {
    let settings = crate::window::settings_of(app);
    let now = now_ms();
    // Focus mode holds *everything* back; because nothing is recorded as
    // announced meanwhile, a crossing that happened during focus is reported
    // right after it ends.
    if settings.notifications && !crate::focus::notifications_allowed(&settings, now) {
        return;
    }
    let chinese = crate::window::tray::prefers_chinese(&settings);
    let alerts = {
        let mut guard = THRESHOLDS.lock();
        snapshot_alerts(
            guard.get_or_insert_with(Default::default),
            snapshot,
            &settings,
            now,
            chinese,
        )
    };
    for alert in alerts {
        notifier::deliver(app, &settings, alert);
    }
}

/// The alerts one snapshot raises. Threshold state is advanced even while the
/// toggles are off, so switching notifications on later does not announce
/// crossings that already happened. `state` is the process-wide `THRESHOLDS`
/// in the app; tests pass their own so they cannot prune each other's keys.
fn snapshot_alerts(
    state: &mut threshold::ThresholdState,
    snapshot: &AppSnapshot,
    settings: &Settings,
    now_ms: i64,
    chinese: bool,
) -> Vec<Alert> {
    let mut out = Vec::new();
    let mut live = BTreeSet::new();
    for q in &snapshot.providers {
        if q.status != ProviderStatus::Ok {
            continue;
        }
        // `claude` or `claude@work`: each account has its own dedupe state
        let provider_key = q.key();
        for w in &q.windows {
            if !w.used_percent.is_finite() {
                continue;
            }
            let key = threshold::window_key(&provider_key, w);
            live.insert(key.clone());
            let resets = w
                .resets_at
                .as_deref()
                .and_then(crate::commands::forecast::parse_ms);
            let crossed = state.observe(&key, resets, w.used_percent, &settings.thresholds);
            if !settings.notifications {
                continue;
            }
            if settings.threshold_notifications {
                if let Some(level) = crossed {
                    out.push(threshold::alert_for(
                        &q.display_name,
                        &provider_key,
                        w,
                        level,
                        chinese,
                    ));
                }
            }
            if settings.forecast_notifications {
                if let Some(f) = predictive::forecast_alert(&provider_key, w, now_ms) {
                    if predictive::claim_alert(&f, now_ms) {
                        let (title, body) =
                            predictive::alert_text(&q.display_name, w, f.early_by_ms, chinese);
                        out.push(
                            Alert::new(notifier::Level::Forecast, title, body)
                                .for_window(&provider_key, &window_name(w, chinese)),
                        );
                    }
                }
            }
        }
    }
    if settings.notifications && settings.advisor_notifications {
        if let Some(advice) = crate::commands::advisor::routing::advise(snapshot, settings, now_ms)
        {
            use crate::commands::advisor::routing;
            if advice.kind == routing::RoutingKind::Switch
                && routing::claim_notification(&advice, now_ms)
            {
                if let (Some((title, body)), Some(from)) =
                    (routing::alert_text(&advice, chinese), advice.from.as_ref())
                {
                    out.push(
                        Alert::new(notifier::Level::Advice, title, body)
                            .for_window(&from.key, &from.label),
                    );
                }
            }
        }
    }
    state.retain_keys(&live);
    out
}

// ---------- time-driven alerts ----------

/// Spawn the budget / weekly-summary timer. Called once from the scheduler.
pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_TICK).await;
        loop {
            tick(&app).await;
            tokio::time::sleep(TICK).await;
        }
    });
}

async fn tick(app: &AppHandle) {
    let settings = crate::window::settings_of(app);
    if !settings.notifications
        || !crate::focus::notifications_allowed(&settings, now_ms())
        || !(settings.weekly_summary
            || (settings.budget_notifications && settings.monthly_budget_usd > 0.0))
    {
        return;
    }
    let (db, pricing, data_dir) = {
        let state = app.state::<AppState>();
        let pricing = state.pricing.read().clone();
        (state.db.clone(), pricing, state.data_dir.clone())
    };
    let Some(db) = db else { return };
    let chinese = crate::window::tray::prefers_chinese(&settings);
    let mut announced = load_announced(&data_dir);
    let before = announced.clone();

    if settings.budget_notifications && settings.monthly_budget_usd > 0.0 {
        let now = now_ms();
        let (db, pricing) = (db.clone(), pricing.clone());
        let spent = tauri::async_runtime::spawn_blocking(move || {
            budget::month_to_date_usd(&db, &pricing, now)
        })
        .await;
        match spent {
            Ok(Ok(spent)) => {
                let budget_usd = settings.monthly_budget_usd;
                let month = budget::month_key(now);
                let reached = budget::level_reached(spent, budget_usd);
                if let Some(level) = budget::due(
                    reached,
                    &announced.budget_month,
                    announced.budget_level,
                    &month,
                ) {
                    let alert = budget::alert_for(level, spent, budget_usd, chinese);
                    if notifier::deliver(app, &settings, alert) {
                        announced.budget_month = month;
                        announced.budget_level = level;
                    }
                }
            }
            Ok(Err(e)) => log::warn!("budget check failed: {e:#}"),
            Err(e) => log::warn!("budget check task failed: {e}"),
        }
    }

    if settings.weekly_summary {
        let now = chrono::Local::now().naive_local();
        if let Some(monday) = summary::due(now, &announced.summary_week) {
            let computed = tauri::async_runtime::spawn_blocking(move || {
                summary::compute(&db, &pricing, monday)
            })
            .await;
            match computed {
                Ok(Ok(s)) => {
                    if notifier::deliver(app, &settings, summary::alert_for(&s, chinese)) {
                        announced.summary_week = s.week_start;
                    }
                }
                Ok(Err(e)) => log::warn!("weekly summary failed: {e:#}"),
                Err(e) => log::warn!("weekly summary task failed: {e}"),
            }
        }
    }

    if announced != before {
        save_announced(&data_dir, &announced);
    }
}

// ---------- commands ----------

/// "Send test" button. `channel` is `"native"` or `"webhook"`; the error text
/// is shown to the user as is (it never contains the webhook URL).
#[tauri::command]
pub async fn send_test_notification(app: AppHandle, channel: String) -> Result<(), String> {
    let channel = notifier::Channel::parse(&channel)
        .ok_or_else(|| format!("unknown notification channel `{channel}`"))?;
    let chinese = crate::window::tray::prefers_chinese(&crate::window::settings_of(&app));
    notifier::send_test(&app, channel, chinese)
        .await
        .map_err(|e| format!("{e:#}"))
}

/// OS permission for native notifications: `granted`, `denied`, `prompt` or
/// `unknown`. Desktop platforms without a permission model report `granted`.
#[tauri::command]
pub async fn get_notification_permission(app: AppHandle) -> Result<String, String> {
    use tauri_plugin_notification::{NotificationExt, PermissionState};
    Ok(match app.notification().permission_state() {
        Ok(PermissionState::Granted) => "granted",
        Ok(PermissionState::Denied) => "denied",
        Ok(PermissionState::Prompt) | Ok(PermissionState::PromptWithRationale) => "prompt",
        Err(_) => "unknown",
    }
    .to_string())
}

/// Last completed Monday–Sunday: tokens, estimated cost, busiest day and
/// limits hit. Same data as the Monday notification.
#[tauri::command]
pub async fn get_weekly_summary(state: State<'_, AppState>) -> Result<WeeklySummary, String> {
    let db = state.db()?;
    let pricing = state.pricing.read().clone();
    let monday = summary::last_full_week_start(chrono::Local::now().date_naive());
    match tauri::async_runtime::spawn_blocking(move || summary::compute(&db, &pricing, monday))
        .await
    {
        Ok(Ok(s)) => Ok(s),
        Ok(Err(e)) => Err(format!("{e:#}")),
        Err(e) => Err(format!("background task failed: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ProviderQuota, QuotaWindow};

    fn quota(provider: &str, pct: f64, resets: &str) -> ProviderQuota {
        let mut q = crate::commands::providers::empty_quota(provider, "Claude", ProviderStatus::Ok);
        q.windows = vec![QuotaWindow {
            kind: WindowKind::FiveHour,
            label: "5-hour".into(),
            window_seconds: Some(18_000),
            used_percent: pct,
            resets_at: Some(resets.into()),
            scope: None,
            is_primary: true,
            forecast: None,
        }];
        q
    }

    fn snap(q: ProviderQuota) -> AppSnapshot {
        AppSnapshot {
            providers: vec![q],
            ..AppSnapshot::default()
        }
    }

    #[test]
    fn crossings_follow_the_toggles_and_seed_while_off() {
        let mut st = threshold::ThresholdState::default();
        let p = "alerts-test";
        let mut s = Settings::default();
        let r = "2030-01-01T05:00:00Z";
        // while notifications are off the state still tracks the window …
        assert!(snapshot_alerts(&mut st, &snap(quota(p, 10.0, r)), &s, 0, false).is_empty());
        assert!(snapshot_alerts(&mut st, &snap(quota(p, 80.0, r)), &s, 0, false).is_empty());
        // … so turning them on does not announce the crossing that happened
        s.notifications = true;
        assert!(snapshot_alerts(&mut st, &snap(quota(p, 81.0, r)), &s, 0, false).is_empty());
        let alerts = snapshot_alerts(&mut st, &snap(quota(p, 95.0, r)), &s, 0, false);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].level, notifier::Level::Critical);
        assert!(snapshot_alerts(&mut st, &snap(quota(p, 96.0, r)), &s, 0, false).is_empty());
        // the type toggle silences it without disturbing the bookkeeping
        s.threshold_notifications = false;
        let r2 = "2030-01-01T10:00:00Z";
        snapshot_alerts(&mut st, &snap(quota(p, 1.0, r2)), &s, 0, false);
        assert!(snapshot_alerts(&mut st, &snap(quota(p, 95.0, r2)), &s, 0, false).is_empty());
    }

    #[test]
    fn providers_that_are_not_ok_never_alert() {
        let mut st = threshold::ThresholdState::default();
        let p = "alerts-test-err";
        let s = Settings {
            notifications: true,
            ..Settings::default()
        };
        let r = "2030-01-01T05:00:00Z";
        snapshot_alerts(&mut st, &snap(quota(p, 10.0, r)), &s, 0, false);
        let mut q = quota(p, 99.0, r);
        q.status = ProviderStatus::Error;
        assert!(snapshot_alerts(&mut st, &snap(q), &s, 0, false).is_empty());
    }

    #[test]
    fn announced_state_round_trips_and_tolerates_garbage() {
        let dir = crate::commands::test_support::tempdir();
        assert_eq!(load_announced(&dir), Announced::default());
        let a = Announced {
            budget_month: "2026-09".into(),
            budget_level: 1,
            summary_week: "2026-09-14".into(),
        };
        save_announced(&dir, &a);
        assert_eq!(load_announced(&dir), a);
        std::fs::write(state_path(&dir), b"{not json").unwrap();
        assert_eq!(load_announced(&dir), Announced::default());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn window_names_are_bilingual_and_scoped() {
        let mut w = quota("claude", 1.0, "2030-01-01T05:00:00Z")
            .windows
            .remove(0);
        assert_eq!(window_name(&w, false), "5-hour");
        assert_eq!(window_name(&w, true), "5 小时");
        w.kind = WindowKind::SevenDay;
        w.scope = Some("Fable".into());
        assert_eq!(window_name(&w, false), "weekly · Fable");
    }

    #[test]
    fn two_accounts_of_one_provider_alert_independently() {
        let mut st = threshold::ThresholdState::default();
        let p = "alerts-test-acct";
        let s = Settings {
            notifications: true,
            ..Settings::default()
        };
        let r = "2030-01-01T05:00:00Z";
        let primary = |pct| quota(p, pct, r);
        let work = |pct| {
            let mut q = quota(p, pct, r);
            q.account_id = Some("work".into());
            q.account_label = Some("Work".into());
            q.display_name = "Claude · Work".into();
            q
        };
        let both = |a: ProviderQuota, b: ProviderQuota| AppSnapshot {
            providers: vec![a, b],
            ..AppSnapshot::default()
        };
        assert!(
            snapshot_alerts(&mut st, &both(primary(10.0), work(10.0)), &s, 0, false).is_empty()
        );
        // only the work account crosses: exactly one alert, and it names it
        let alerts = snapshot_alerts(&mut st, &both(primary(10.0), work(95.0)), &s, 0, false);
        assert_eq!(alerts.len(), 1);
        assert!(alerts[0].title.contains("Work"), "{}", alerts[0].title);
        assert_eq!(alerts[0].provider.as_deref(), Some("alerts-test-acct@work"));
        // the primary crossing later is its own event, the work one stays quiet
        let alerts = snapshot_alerts(&mut st, &both(primary(95.0), work(96.0)), &s, 0, false);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].provider.as_deref(), Some("alerts-test-acct"));
    }

    #[test]
    fn advisor_alert_follows_its_toggle_and_fires_once_per_reset() {
        use crate::model::{ForecastConfidence, QuotaForecast};
        let now = 1_789_430_400_000i64;
        let iso = |ms: i64| crate::commands::providers::rfc3339_from_unix_ms(ms).unwrap();
        let mut hot = quota("claude", 92.0, &iso(now + 2 * 3_600_000));
        hot.fetched_at = iso(now - 60_000);
        hot.windows[0].forecast = Some(QuotaForecast {
            projected_percent_at_reset: 150.0,
            exhausts_at: Some(iso(now + 25 * 60_000)),
            rate_percent_per_hour: 30.0,
            confidence: ForecastConfidence::High,
        });
        let mut roomy = quota("codex", 10.0, &iso(now + 4 * 3_600_000));
        roomy.display_name = "Codex".into();
        roomy.fetched_at = iso(now - 60_000);
        let both = AppSnapshot {
            providers: vec![hot, roomy],
            ..AppSnapshot::default()
        };
        let mut s = Settings {
            notifications: true,
            threshold_notifications: false,
            forecast_notifications: false,
            ..Settings::default()
        };
        let mut st = threshold::ThresholdState::default();
        // off by default
        assert!(snapshot_alerts(&mut st, &both, &s, now, false).is_empty());
        s.advisor_notifications = true;
        let alerts = snapshot_alerts(&mut st, &both, &s, now, false);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].level, notifier::Level::Advice);
        assert!(alerts[0].title.contains("Codex"), "{}", alerts[0].title);
        // the next refresh of the same situation stays quiet
        assert!(snapshot_alerts(&mut st, &both, &s, now + 60_000, false).is_empty());
        // and the master switch still wins
        s.notifications = false;
        assert!(snapshot_alerts(&mut st, &both, &s, now + 120_000, false).is_empty());
    }
}
