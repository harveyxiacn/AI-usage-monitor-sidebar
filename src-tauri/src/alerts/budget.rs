//! Budget alerts: month-to-date *estimated* cost against `monthlyBudgetUsd`.
//!
//! Two levels, 80 % and 100 %, each announced once per calendar month (local
//! time). The estimate is the same API-equivalent figure the History tab
//! shows — never a bill. The month-to-date sum comes from
//! `store::query_history` with one month bucket, which is served from the
//! query cache between ingests, so evaluating every few minutes is cheap.

use chrono::{Datelike, Local, TimeZone};

use super::notifier::{Alert, Level};
use crate::commands::store::{self, Db};
use crate::model::{Bucket, HistoryQuery, PricingTable};

/// Fractions of the budget that raise an alert, lowest first.
pub const LEVELS: [f64; 2] = [0.8, 1.0];

/// Highest level (1 = 80 %, 2 = 100 %) that `spent` has reached; 0 = none.
/// A budget of 0 (or less, or NaN) is "off".
pub fn level_reached(spent: f64, budget: f64) -> u8 {
    if !(budget.is_finite() && budget > 0.0) || !spent.is_finite() {
        return 0;
    }
    let ratio = spent / budget;
    LEVELS.iter().filter(|l| ratio >= **l).count() as u8
}

/// `Some(level)` when `reached` is higher than what was already announced for
/// this month. A new month starts from nothing.
pub fn due(reached: u8, announced_month: &str, announced_level: u8, month: &str) -> Option<u8> {
    let already = if announced_month == month {
        announced_level
    } else {
        0
    };
    (reached > already).then_some(reached)
}

/// Local `YYYY-MM` of `now_ms`.
pub fn month_key(now_ms: i64) -> String {
    match Local.timestamp_millis_opt(now_ms).single() {
        Some(t) => format!("{:04}-{:02}", t.year(), t.month()),
        None => String::new(),
    }
}

/// Month-to-date estimated cost in USD (unpriced requests count as 0).
pub fn month_to_date_usd(db: &Db, pricing: &PricingTable, now_ms: i64) -> anyhow::Result<f64> {
    let start = store::usage::bucket_start_ms(now_ms, Bucket::Month);
    let result = store::query_history(
        db,
        &HistoryQuery {
            from: store::usage::local_rfc3339(start),
            to: store::usage::local_rfc3339(now_ms + 1_000),
            bucket: Bucket::Month,
            group_by_model: false,
            provider: None,
            project: None,
            group_by_project: false,
        },
        pricing,
    )?;
    Ok(result
        .totals
        .estimated_cost_usd
        .or(result.totals.known_cost_usd)
        .unwrap_or(0.0))
}

/// Notification text.
pub fn alert_for(level: u8, spent: f64, budget: f64, chinese: bool) -> Alert {
    let full = level >= 2;
    let (title, body) = if chinese {
        (
            if full {
                "本月预估费用已达预算".to_string()
            } else {
                "本月预估费用已达预算的 80%".to_string()
            },
            format!("预估 ${spent:.2} / 预算 ${budget:.2}（按 API 价格估算，非账单）"),
        )
    } else {
        (
            if full {
                "Monthly estimated cost reached the budget".to_string()
            } else {
                "Monthly estimated cost at 80% of the budget".to_string()
            },
            format!("Estimated ${spent:.2} of ${budget:.2} (API-equivalent estimate, not a bill)"),
        )
    };
    Alert::new(Level::Budget, title, body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_are_80_and_100_percent() {
        assert_eq!(level_reached(0.0, 100.0), 0);
        assert_eq!(level_reached(79.99, 100.0), 0);
        assert_eq!(level_reached(80.0, 100.0), 1);
        assert_eq!(level_reached(99.0, 100.0), 1);
        assert_eq!(level_reached(100.0, 100.0), 2);
        assert_eq!(level_reached(250.0, 100.0), 2);
    }

    #[test]
    fn no_budget_means_no_alerts() {
        assert_eq!(level_reached(500.0, 0.0), 0);
        assert_eq!(level_reached(500.0, -5.0), 0);
        assert_eq!(level_reached(500.0, f64::NAN), 0);
        assert_eq!(level_reached(f64::NAN, 100.0), 0);
    }

    #[test]
    fn each_level_is_announced_once_per_month() {
        assert_eq!(due(1, "", 0, "2026-09"), Some(1));
        assert_eq!(due(1, "2026-09", 1, "2026-09"), None, "already announced");
        assert_eq!(due(2, "2026-09", 1, "2026-09"), Some(2), "100 % after 80 %");
        assert_eq!(due(2, "2026-09", 2, "2026-09"), None);
        assert_eq!(
            due(1, "2026-09", 2, "2026-10"),
            Some(1),
            "a new month starts over"
        );
        assert_eq!(due(0, "2026-09", 0, "2026-09"), None);
    }

    #[test]
    fn jumping_straight_past_100_percent_announces_once() {
        assert_eq!(due(level_reached(130.0, 100.0), "", 0, "2026-09"), Some(2));
    }

    #[test]
    fn month_keys_are_zero_padded_local_months() {
        let ms = store::usage::parse_time_ms("2026-03-05T12:00:00").unwrap();
        assert_eq!(month_key(ms), "2026-03");
    }

    #[test]
    fn the_text_says_estimate_in_both_languages() {
        let a = alert_for(1, 41.0, 50.0, false);
        assert!(a.title.contains("80%") && a.body.contains("$41.00 of $50.00"));
        assert!(a.body.contains("not a bill"));
        let a = alert_for(2, 55.5, 50.0, true);
        assert!(a.title.contains("已达预算") && a.body.contains("非账单"));
        assert_eq!(a.level, Level::Budget);
    }

    fn event(ts: i64, id: &str) -> store::UsageEvent {
        store::UsageEvent {
            provider: "claude".into(),
            model: "claude-sonnet-4-5".into(),
            reasoning_effort: None,
            ts,
            input_tokens: 1_000_000,
            cache_write_tokens: 0,
            cache_read_tokens: 0,
            output_tokens: 0,
            reasoning_tokens: 0,
            total_tokens: 1_000_000,
            session_id: Some("s".into()),
            request_id: id.into(),
            cwd: None,
            source_file: None,
        }
    }

    #[test]
    fn month_to_date_sums_only_this_month() {
        let db = Db::open_in_memory().unwrap();
        let pricing = crate::commands::pricing::default_table();
        let now = store::usage::parse_time_ms("2026-09-15T12:00:00").unwrap();
        assert_eq!(
            month_to_date_usd(&db, &pricing, now).unwrap(),
            0.0,
            "empty is zero"
        );

        let events = [
            event(
                store::usage::parse_time_ms("2026-08-31T23:00:00").unwrap(),
                "last-month",
            ),
            event(
                store::usage::parse_time_ms("2026-09-01T00:30:00").unwrap(),
                "first",
            ),
            event(
                store::usage::parse_time_ms("2026-09-14T08:00:00").unwrap(),
                "second",
            ),
        ];
        store::insert_usage_events(&db, &events).unwrap();
        // 2 events × 1M input tokens × $3 / M
        let spent = month_to_date_usd(&db, &pricing, now).unwrap();
        assert!((spent - 6.0).abs() < 1e-6, "{spent}");
    }
}
