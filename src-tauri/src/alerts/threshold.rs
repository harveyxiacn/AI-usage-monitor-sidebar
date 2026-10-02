//! Threshold-crossing alerts: "Claude 5-hour is at 72 %".
//!
//! Contract (README / AGENTS.md): with `notifications` on, a window that
//! crosses `thresholds.warn` or `thresholds.critical` produces **one**
//! notification per `(provider, window, level, cycle)`.
//!
//! * a *cycle* is identified by the window's `resets_at`; a new cycle re-arms
//!   both levels;
//! * only **upward** crossings notify. Falling back below a level inside one
//!   cycle (a plan change, a corrected reading) re-arms it silently;
//! * jumping from 40 % straight to 95 % sends the critical alert only, not a
//!   warn + critical pair;
//! * the first reading of a window after the app starts only *seeds* the
//!   state: restarting the app at 85 % must not announce a crossing that
//!   happened hours ago. Because of this the state needs no persistence.

use std::collections::BTreeMap;

use super::notifier::{Alert, Level};
use crate::model::{QuotaWindow, Thresholds};

/// `resets_at` values of one cycle differ by a few milliseconds between polls;
/// anything further apart than this is a different cycle.
const SAME_CYCLE_TOLERANCE_MS: i64 = 10 * 60 * 1000;

/// How far over the thresholds a window is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Below,
    Warn,
    Critical,
}

pub fn severity(used_percent: f64, t: &Thresholds) -> Severity {
    if used_percent >= t.critical {
        Severity::Critical
    } else if used_percent >= t.warn {
        Severity::Warn
    } else {
        Severity::Below
    }
}

#[derive(Clone, Copy, Debug)]
struct Armed {
    resets_at_ms: Option<i64>,
    /// highest level already announced (or seeded) in this cycle
    sent: Severity,
}

/// What the scheduler remembers between two snapshots.
#[derive(Default)]
pub struct ThresholdState {
    windows: BTreeMap<String, Armed>,
}

fn same_cycle(a: Option<i64>, b: Option<i64>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => (a - b).abs() <= SAME_CYCLE_TOLERANCE_MS,
        (None, None) => true,
        _ => false,
    }
}

impl ThresholdState {
    /// Observe one window; `Some(level)` means "announce this now".
    /// `key` identifies the window (`provider|kind|scope`).
    pub fn observe(
        &mut self,
        key: &str,
        resets_at_ms: Option<i64>,
        used_percent: f64,
        thresholds: &Thresholds,
    ) -> Option<Severity> {
        let now_level = severity(used_percent, thresholds);
        let Some(armed) = self.windows.get_mut(key) else {
            // First sight: remember, never announce.
            self.windows.insert(
                key.to_string(),
                Armed {
                    resets_at_ms,
                    sent: now_level,
                },
            );
            return None;
        };
        if !same_cycle(armed.resets_at_ms, resets_at_ms) {
            // A new cycle: everything is armed again.
            *armed = Armed {
                resets_at_ms,
                sent: Severity::Below,
            };
        }
        if now_level > armed.sent {
            armed.sent = now_level;
            return Some(now_level);
        }
        if now_level < armed.sent {
            // Fell back inside the cycle: re-arm the levels we are now under.
            armed.sent = now_level;
        }
        None
    }

    /// Forget windows that vanished (a provider was switched off).
    pub fn retain_keys(&mut self, live: &std::collections::BTreeSet<String>) {
        self.windows.retain(|k, _| live.contains(k));
    }
}

pub fn window_key(provider: &str, w: &QuotaWindow) -> String {
    format!(
        "{provider}|{}|{}",
        w.kind.as_str(),
        w.scope.as_deref().unwrap_or("")
    )
}

/// Notification text for a crossing.
pub fn alert_for(
    display_name: &str,
    provider: &str,
    w: &QuotaWindow,
    level: Severity,
    chinese: bool,
) -> Alert {
    let window = super::window_name(w, chinese);
    let pct = w.used_percent.round() as i64;
    let exhausted = w.used_percent >= 100.0;
    let (title, body) = if chinese {
        let body = match (level, exhausted) {
            (_, true) => format!("已用 {pct}%，限额已用尽"),
            (Severity::Critical, _) => format!("已用 {pct}%，接近上限"),
            _ => format!("已用 {pct}%，已超过预警线"),
        };
        (format!("{display_name} {window}限额"), body)
    } else {
        let body = match (level, exhausted) {
            (_, true) => format!("{pct}% used, the limit is reached"),
            (Severity::Critical, _) => format!("{pct}% used, close to the limit"),
            _ => format!("{pct}% used, past the warning level"),
        };
        (format!("{display_name} {window} limit"), body)
    };
    let level = if level == Severity::Critical {
        Level::Critical
    } else {
        Level::Warn
    };
    Alert::new(level, title, body).for_window(provider, &window)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::WindowKind;

    const T: Thresholds = Thresholds {
        warn: 70.0,
        critical: 90.0,
    };
    const KEY: &str = "claude|five_hour|";
    const R1: Option<i64> = Some(1_000_000_000_000);
    const R2: Option<i64> = Some(1_000_000_000_000 + 5 * 3_600_000);

    fn run(state: &mut ThresholdState, resets: Option<i64>, pct: f64) -> Option<Severity> {
        state.observe(KEY, resets, pct, &T)
    }

    #[test]
    fn severity_follows_the_thresholds() {
        assert_eq!(severity(69.9, &T), Severity::Below);
        assert_eq!(severity(70.0, &T), Severity::Warn);
        assert_eq!(severity(89.9, &T), Severity::Warn);
        assert_eq!(severity(90.0, &T), Severity::Critical);
        assert_eq!(severity(100.0, &T), Severity::Critical);
    }

    #[test]
    fn each_level_fires_once_per_cycle_going_up() {
        let mut s = ThresholdState::default();
        assert_eq!(run(&mut s, R1, 10.0), None, "seed");
        assert_eq!(run(&mut s, R1, 50.0), None);
        assert_eq!(run(&mut s, R1, 71.0), Some(Severity::Warn));
        assert_eq!(run(&mut s, R1, 75.0), None, "same level stays quiet");
        assert_eq!(run(&mut s, R1, 91.0), Some(Severity::Critical));
        assert_eq!(run(&mut s, R1, 100.0), None);
    }

    #[test]
    fn a_jump_past_both_levels_sends_only_the_critical_one() {
        let mut s = ThresholdState::default();
        run(&mut s, R1, 10.0);
        assert_eq!(run(&mut s, R1, 95.0), Some(Severity::Critical));
        assert_eq!(run(&mut s, R1, 96.0), None);
    }

    #[test]
    fn the_first_reading_only_seeds() {
        let mut s = ThresholdState::default();
        assert_eq!(
            run(&mut s, R1, 85.0),
            None,
            "restarting at 85 % is not a crossing"
        );
        assert_eq!(run(&mut s, R1, 86.0), None);
        assert_eq!(run(&mut s, R1, 92.0), Some(Severity::Critical));
    }

    #[test]
    fn a_new_cycle_re_arms_both_levels() {
        let mut s = ThresholdState::default();
        run(&mut s, R1, 10.0);
        assert_eq!(run(&mut s, R1, 95.0), Some(Severity::Critical));
        assert_eq!(run(&mut s, R2, 5.0), None, "reset: low again");
        assert_eq!(run(&mut s, R2, 72.0), Some(Severity::Warn));
        assert_eq!(run(&mut s, R2, 91.0), Some(Severity::Critical));
    }

    #[test]
    fn a_new_cycle_that_starts_high_announces_immediately() {
        let mut s = ThresholdState::default();
        run(&mut s, R1, 10.0);
        assert_eq!(run(&mut s, R2, 80.0), Some(Severity::Warn));
    }

    #[test]
    fn jitter_in_resets_at_is_still_the_same_cycle() {
        let mut s = ThresholdState::default();
        run(&mut s, R1, 10.0);
        assert_eq!(run(&mut s, R1.map(|r| r + 800), 75.0), Some(Severity::Warn));
        assert_eq!(run(&mut s, R1.map(|r| r - 300), 76.0), None);
    }

    #[test]
    fn falling_back_re_arms_silently_and_only_upward_crossings_fire() {
        let mut s = ThresholdState::default();
        run(&mut s, R1, 10.0);
        assert_eq!(run(&mut s, R1, 95.0), Some(Severity::Critical));
        assert_eq!(run(&mut s, R1, 75.0), None, "down is silent");
        assert_eq!(
            run(&mut s, R1, 93.0),
            Some(Severity::Critical),
            "back up fires again"
        );
        assert_eq!(run(&mut s, R1, 20.0), None);
        assert_eq!(run(&mut s, R1, 71.0), Some(Severity::Warn));
    }

    #[test]
    fn changed_thresholds_apply_to_the_next_reading() {
        let mut s = ThresholdState::default();
        run(&mut s, R1, 60.0);
        let low = Thresholds {
            warn: 50.0,
            critical: 80.0,
        };
        assert_eq!(s.observe(KEY, R1, 61.0, &low), Some(Severity::Warn));
    }

    #[test]
    fn windows_are_tracked_independently() {
        let mut s = ThresholdState::default();
        s.observe("claude|five_hour|", R1, 10.0, &T);
        s.observe("claude|seven_day|", R1, 10.0, &T);
        assert_eq!(
            s.observe("claude|seven_day|", R1, 80.0, &T),
            Some(Severity::Warn)
        );
        assert_eq!(s.observe("claude|five_hour|", R1, 20.0, &T), None);
    }

    fn window(pct: f64) -> QuotaWindow {
        QuotaWindow {
            kind: WindowKind::FiveHour,
            label: "5-hour".into(),
            window_seconds: Some(18_000),
            used_percent: pct,
            resets_at: None,
            scope: None,
            is_primary: true,
            forecast: None,
        }
    }

    #[test]
    fn the_text_is_bilingual_and_names_provider_window_and_percent() {
        let a = alert_for("Claude", "claude", &window(72.4), Severity::Warn, false);
        assert_eq!(a.title, "Claude 5-hour limit");
        assert_eq!(a.body, "72% used, past the warning level");
        assert_eq!(a.level, Level::Warn);
        assert_eq!(a.provider.as_deref(), Some("claude"));
        let a = alert_for("Claude", "claude", &window(93.0), Severity::Critical, true);
        assert_eq!(a.title, "Claude 5 小时限额");
        assert_eq!(a.body, "已用 93%，接近上限");
        assert_eq!(a.level, Level::Critical);
        let a = alert_for("Codex", "codex", &window(100.0), Severity::Critical, false);
        assert_eq!(a.body, "100% used, the limit is reached");
    }
}
