//! Focus / do-not-disturb mode: pure helpers around `Settings.focusUntil`.
//!
//! `focusUntil` is `0` (off), [`UNTIL_OFF`] (`-1`, until the user turns it
//! off) or an epoch-ms deadline after which focus has ended by itself. The
//! tray owns the menu and the expiry timer (`window::tray`); the scheduler
//! asks [`notifications_allowed`] before it shows anything natively.

use chrono::{DateTime, Duration, Local, TimeZone};

use crate::model::Settings;

/// `focusUntil` value meaning "until I turn it off".
pub const UNTIL_OFF: i64 = -1;

/// Hour (local time) at which "until tomorrow" ends.
pub const MORNING_HOUR: u32 = 8;

/// Is focus mode on at `now_ms`?
pub fn is_active(focus_until: i64, now_ms: i64) -> bool {
    focus_until == UNTIL_OFF || focus_until > now_ms
}

/// A timed focus whose deadline has passed and should be reset to `0`.
pub fn is_expired(focus_until: i64, now_ms: i64) -> bool {
    focus_until > 0 && focus_until <= now_ms
}

/// Single gate for every native notification.
pub fn notifications_allowed(settings: &Settings, now_ms: i64) -> bool {
    !is_active(settings.focus_until, now_ms)
}

/// Epoch ms of the next day's 08:00 in `now`'s time zone. Falls back to 24 h
/// later when that wall-clock time does not exist (DST gap).
pub fn tomorrow_morning_ms<Tz: TimeZone>(now: DateTime<Tz>) -> i64 {
    let target = (now.date_naive() + Duration::days(1))
        .and_hms_opt(MORNING_HOUR, 0, 0)
        .and_then(|naive| now.timezone().from_local_datetime(&naive).earliest());
    match target {
        Some(t) => t.timestamp_millis(),
        None => now.timestamp_millis() + 24 * 3_600_000,
    }
}

/// Deadline for "until tomorrow 08:00" from the real clock.
pub fn tomorrow_morning_from_now() -> i64 {
    tomorrow_morning_ms(Local::now())
}

/// `HH:MM` local time of a deadline, for labels.
pub fn clock_label(until_ms: i64) -> String {
    Local
        .timestamp_millis_opt(until_ms)
        .single()
        .map(|t| t.format("%H:%M").to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, Timelike};

    #[test]
    fn focus_is_active_until_its_deadline_or_forever() {
        assert!(!is_active(0, 1_000), "0 is off");
        assert!(is_active(UNTIL_OFF, i64::MAX), "-1 never ends by itself");
        assert!(is_active(2_000, 1_999));
        assert!(!is_active(2_000, 2_000), "the deadline itself is over");
        assert!(!is_active(2_000, 5_000));
    }

    #[test]
    fn only_a_passed_timed_deadline_counts_as_expired() {
        assert!(!is_expired(0, 10));
        assert!(!is_expired(UNTIL_OFF, 10));
        assert!(!is_expired(11, 10));
        assert!(is_expired(10, 10));
    }

    #[test]
    fn notifications_follow_focus() {
        let mut s = Settings::default();
        assert!(notifications_allowed(&s, 5));
        s.focus_until = 100;
        assert!(!notifications_allowed(&s, 5));
        assert!(notifications_allowed(&s, 100));
        s.focus_until = UNTIL_OFF;
        assert!(!notifications_allowed(&s, 5));
    }

    #[test]
    fn tomorrow_morning_is_next_days_eight_local() {
        let tz = FixedOffset::east_opt(8 * 3600).unwrap();
        let late = tz.with_ymd_and_hms(2026, 3, 10, 23, 30, 0).unwrap();
        let ms = tomorrow_morning_ms(late);
        let got = tz.timestamp_millis_opt(ms).unwrap();
        assert_eq!(
            (got.date_naive().to_string(), got.hour(), got.minute()),
            ("2026-03-11".to_string(), 8, 0)
        );
        let early = tz.with_ymd_and_hms(2026, 3, 10, 1, 0, 0).unwrap();
        let got = tz.timestamp_millis_opt(tomorrow_morning_ms(early)).unwrap();
        assert_eq!(
            got.date_naive().to_string(),
            "2026-03-11",
            "always tomorrow"
        );
    }
}
