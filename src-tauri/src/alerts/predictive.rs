//! Predictive alerts: "on pace to run out ~25 min before it resets".
//! (Moved out of `scheduler.rs`; behaviour unchanged.)

use std::collections::BTreeMap;

use parking_lot::Mutex;

use crate::commands::forecast;
use crate::model::{ForecastConfidence, QuotaWindow};

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
pub struct ForecastAlert {
    /// deduplication key, stable across refreshes
    pub key: String,
    /// the reset this alert belongs to (unix ms)
    pub resets_at_ms: i64,
    /// how long before that reset the window is projected to hit 100 %
    pub early_by_ms: i64,
}

/// Decide whether `w` deserves a predictive notification. Pure: the settings
/// gate and the once-per-period bookkeeping are applied by the caller.
pub fn forecast_alert(provider: &str, w: &QuotaWindow, now_ms: i64) -> Option<ForecastAlert> {
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
pub fn claim_alert(alert: &ForecastAlert, now_ms: i64) -> bool {
    let mut seen = FORECAST_NOTIFIED.lock();
    seen.retain(|_, resets_at| *resets_at > now_ms);
    if seen.get(&alert.key) == Some(&alert.resets_at_ms) {
        return false;
    }
    seen.insert(alert.key.clone(), alert.resets_at_ms);
    true
}

/// Notification title + body. The native side carries the same two catalogues
/// as the tray menu (see `window::tray::prefers_chinese`).
pub fn alert_text(
    display_name: &str,
    w: &QuotaWindow,
    early_by_ms: i64,
    chinese: bool,
) -> (String, String) {
    let window = super::window_name(w, chinese);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::QuotaForecast;
    use crate::model::WindowKind;

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
