//! Weekly summary: last Monday–Sunday in one notification and one card.
//!
//! The same [`WeeklySummary`] feeds both the opt-in Monday ~09:00
//! notification (`weeklySummary` setting) and the Overview card, through the
//! `get_weekly_summary` command. Everything is computed from the local
//! database; cost is the usual API-equivalent *estimate*.

use anyhow::Result;
use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, Timelike, Weekday};

use super::notifier::{Alert, Level};
use crate::commands::store::{self, Db};
use crate::model::{
    Bucket, CalendarQuery, HistoryQuery, PricingTable, WeeklyAccount, WeeklySummary,
};

/// Local hour on Monday from which the summary is due.
pub const DUE_HOUR: u32 = 9;

fn monday_of(date: NaiveDate) -> NaiveDate {
    date - Duration::days(date.weekday().num_days_from_monday() as i64)
}

/// Monday of the most recent *completed* week as seen from `now`.
pub fn last_full_week_start(now: NaiveDate) -> NaiveDate {
    monday_of(now) - Duration::days(7)
}

/// `Some(monday of the week to summarise)` when it is Monday, past
/// [`DUE_HOUR`], and that week was not announced yet.
pub fn due(now: NaiveDateTime, last_sent_week: &str) -> Option<NaiveDate> {
    if now.date().weekday() != Weekday::Mon || now.hour() < DUE_HOUR {
        return None;
    }
    let week = last_full_week_start(now.date());
    (week.to_string() != last_sent_week).then_some(week)
}

fn ymd(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

/// Summarise the week starting on `monday` (local time).
pub fn compute(db: &Db, pricing: &PricingTable, monday: NaiveDate) -> Result<WeeklySummary> {
    let next_monday = monday + Duration::days(7);
    let calendar = store::query_calendar(
        db,
        &CalendarQuery {
            from: ymd(monday),
            to: ymd(next_monday),
            provider: None,
            project: None,
            account: None,
        },
        pricing,
    )?;
    let busiest = calendar
        .days
        .iter()
        .filter(|d| d.totals.total_tokens > 0)
        .max_by(|a, b| {
            a.totals
                .total_tokens
                .cmp(&b.totals.total_tokens)
                // on a tie the earlier day wins
                .then_with(|| b.date.cmp(&a.date))
        });

    let from_ms = store::usage::parse_time_ms(&ymd(monday)).unwrap_or(0);
    let to_ms = store::usage::parse_time_ms(&ymd(next_monday)).unwrap_or(i64::MAX);
    let limits_hit = limits_hit(db, from_ms, to_ms)?;
    // Per account: only filled when an extra account had usage that week.
    let per_account = store::query_history(
        db,
        &HistoryQuery {
            from: ymd(monday),
            to: ymd(next_monday),
            bucket: Bucket::Week,
            group_by_model: false,
            provider: None,
            project: None,
            group_by_project: false,
            account: None,
        },
        pricing,
    )?;
    let accounts = per_account
        .by_account
        .into_iter()
        .map(|(key, t)| WeeklyAccount {
            key,
            total_tokens: t.total_tokens,
            requests: t.requests,
            estimated_cost_usd: t.estimated_cost_usd.or(t.known_cost_usd),
        })
        .collect();

    Ok(WeeklySummary {
        week_start: ymd(monday),
        week_end: ymd(next_monday - Duration::days(1)),
        total_tokens: calendar.totals.total_tokens,
        requests: calendar.totals.requests,
        estimated_cost_usd: calendar
            .totals
            .estimated_cost_usd
            .or(calendar.totals.known_cost_usd),
        busiest_day: busiest.map(|d| d.date.clone()),
        busiest_day_tokens: busiest.map(|d| d.totals.total_tokens).unwrap_or(0),
        limits_hit,
        accounts,
    })
}

/// Quota windows (one per provider, account, kind, scope and cycle) whose samples
/// reached 100 % inside `[from_ms, to_ms)`.
fn limits_hit(db: &Db, from_ms: i64, to_ms: i64) -> Result<u32> {
    let conn = db.lock();
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM (
             SELECT 1 FROM quota_samples
             WHERE ts >= ?1 AND ts < ?2
             GROUP BY provider, account, kind, COALESCE(scope, ''), COALESCE(resets_at, 0)
             HAVING MAX(used_percent) >= 100
         )",
        rusqlite::params![from_ms, to_ms],
        |r| r.get(0),
    )?;
    Ok(n.max(0) as u32)
}

fn tokens_short(n: i64) -> String {
    let n = n as f64;
    if n >= 1e9 {
        format!("{:.1}B", n / 1e9)
    } else if n >= 1e6 {
        format!("{:.1}M", n / 1e6)
    } else if n >= 1e3 {
        format!("{:.1}K", n / 1e3)
    } else {
        format!("{n:.0}")
    }
}

fn weekday_name(date: &str, chinese: bool) -> Option<&'static str> {
    let d = NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    const EN: [&str; 7] = [
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
        "Sunday",
    ];
    const ZH: [&str; 7] = ["周一", "周二", "周三", "周四", "周五", "周六", "周日"];
    let i = d.weekday().num_days_from_monday() as usize;
    Some(if chinese { ZH[i] } else { EN[i] })
}

/// Notification text.
pub fn alert_for(s: &WeeklySummary, chinese: bool) -> Alert {
    let mut parts: Vec<String> = Vec::new();
    if chinese {
        parts.push(format!("{} 个 Token", tokens_short(s.total_tokens)));
        if let Some(cost) = s.estimated_cost_usd {
            parts.push(format!("预估 ${cost:.2}"));
        }
        if let Some(day) = &s.busiest_day {
            parts.push(format!(
                "最忙 {}（{}）",
                weekday_name(day, true).unwrap_or(day),
                tokens_short(s.busiest_day_tokens)
            ));
        }
        parts.push(format!("触及限额 {} 次", s.limits_hit));
        Alert::new(
            Level::Summary,
            format!("上周用量（{} 至 {}）", s.week_start, s.week_end),
            parts.join("，"),
        )
    } else {
        parts.push(format!("{} tokens", tokens_short(s.total_tokens)));
        if let Some(cost) = s.estimated_cost_usd {
            parts.push(format!("~${cost:.2} estimated"));
        }
        if let Some(day) = &s.busiest_day {
            parts.push(format!(
                "busiest {} ({})",
                weekday_name(day, false).unwrap_or(day),
                tokens_short(s.busiest_day_tokens)
            ));
        }
        parts.push(match s.limits_hit {
            1 => "1 limit hit".to_string(),
            n => format!("{n} limits hit"),
        });
        Alert::new(
            Level::Summary,
            format!("Last week's usage ({} to {})", s.week_start, s.week_end),
            parts.join(", "),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(y: i32, m: u32, d: u32, h: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(h, 0, 0)
            .unwrap()
    }

    #[test]
    fn the_summary_is_due_on_monday_from_nine_until_it_is_sent() {
        // 2026-09-21 is a Monday
        assert_eq!(due(at(2026, 9, 21, 8), ""), None, "too early");
        let week = due(at(2026, 9, 21, 9), "").expect("Monday 09:00");
        assert_eq!(week.to_string(), "2026-09-14");
        assert_eq!(
            due(at(2026, 9, 21, 17), "").map(|d| d.to_string()),
            Some("2026-09-14".into()),
            "an app started in the afternoon still sends it"
        );
        assert_eq!(due(at(2026, 9, 21, 17), "2026-09-14"), None, "once only");
        assert_eq!(
            due(at(2026, 9, 28, 9), "2026-09-14").map(|d| d.to_string()),
            Some("2026-09-21".into()),
            "next Monday is a new week"
        );
        assert_eq!(due(at(2026, 9, 22, 12), ""), None, "only on Mondays");
        assert_eq!(due(at(2026, 9, 27, 12), ""), None);
    }

    #[test]
    fn the_last_full_week_is_the_previous_monday_to_sunday() {
        let sun = NaiveDate::from_ymd_opt(2026, 9, 20).unwrap();
        assert_eq!(last_full_week_start(sun).to_string(), "2026-09-07");
        let mon = NaiveDate::from_ymd_opt(2026, 9, 21).unwrap();
        assert_eq!(last_full_week_start(mon).to_string(), "2026-09-14");
    }

    fn event(ts: i64, id: &str, tokens: i64) -> store::UsageEvent {
        store::UsageEvent {
            provider: "claude".into(),
            model: "claude-sonnet-4-5".into(),
            reasoning_effort: None,
            ts,
            input_tokens: tokens,
            cache_write_tokens: 0,
            cache_read_tokens: 0,
            output_tokens: 0,
            reasoning_tokens: 0,
            total_tokens: tokens,
            session_id: Some("s".into()),
            request_id: id.into(),
            cwd: None,
            source_file: None,
            account: String::new(),
        }
    }

    fn sample(db: &Db, provider: &str, kind: &str, pct: f64, resets: i64, ts: i64) {
        db.lock()
            .execute(
                "INSERT INTO quota_samples(provider, kind, scope, used_percent, resets_at, plan, ts)
                 VALUES (?1,?2,NULL,?3,?4,NULL,?5)",
                rusqlite::params![provider, kind, pct, resets, ts],
            )
            .unwrap();
    }

    #[test]
    fn the_week_is_split_by_account_only_when_an_extra_account_was_used() {
        let db = Db::open_in_memory().unwrap();
        let pricing = crate::commands::pricing::default_table();
        let t = |s: &str| store::usage::parse_time_ms(s).unwrap();
        let monday = NaiveDate::from_ymd_opt(2026, 9, 14).unwrap();
        store::insert_usage_events(&db, &[event(t("2026-09-14T10:00:00"), "p", 1_000_000)])
            .unwrap();
        assert!(compute(&db, &pricing, monday).unwrap().accounts.is_empty());

        let mut work = event(t("2026-09-15T10:00:00"), "w", 2_000_000);
        work.request_id = store::usage::scoped_request_id("work", "w");
        work.account = "work".into();
        store::insert_usage_events(&db, &[work]).unwrap();
        let s = compute(&db, &pricing, monday).unwrap();
        assert_eq!(
            s.total_tokens, 3_000_000,
            "the headline covers every account"
        );
        let by: Vec<(&str, i64)> = s
            .accounts
            .iter()
            .map(|a| (a.key.as_str(), a.total_tokens))
            .collect();
        assert_eq!(by, vec![("claude", 1_000_000), ("claude@work", 2_000_000)]);
    }

    #[test]
    fn compute_totals_the_week_finds_the_busiest_day_and_counts_limits() {
        let db = Db::open_in_memory().unwrap();
        let pricing = crate::commands::pricing::default_table();
        let t = |s: &str| store::usage::parse_time_ms(s).unwrap();
        store::insert_usage_events(
            &db,
            &[
                event(t("2026-09-13T23:00:00"), "before", 5_000_000), // Sunday before
                event(t("2026-09-14T10:00:00"), "mon", 1_000_000),
                event(t("2026-09-16T10:00:00"), "wed-a", 2_000_000),
                event(t("2026-09-16T15:00:00"), "wed-b", 2_000_000),
                event(t("2026-09-20T23:00:00"), "sun", 500_000),
                event(t("2026-09-21T00:30:00"), "after", 9_000_000), // next Monday
            ],
        )
        .unwrap();
        // a 5-hour window that hit 100 %, one that peaked at 99 %, and one
        // outside the week
        sample(
            &db,
            "claude",
            "five_hour",
            80.0,
            111,
            t("2026-09-15T10:00:00"),
        );
        sample(
            &db,
            "claude",
            "five_hour",
            100.0,
            111,
            t("2026-09-15T11:00:00"),
        );
        sample(
            &db,
            "claude",
            "seven_day",
            99.0,
            222,
            t("2026-09-15T11:00:00"),
        );
        sample(
            &db,
            "codex",
            "seven_day",
            100.0,
            333,
            t("2026-09-22T11:00:00"),
        );

        let monday = NaiveDate::from_ymd_opt(2026, 9, 14).unwrap();
        let s = compute(&db, &pricing, monday).unwrap();
        assert_eq!(
            (s.week_start.as_str(), s.week_end.as_str()),
            ("2026-09-14", "2026-09-20")
        );
        assert_eq!(s.total_tokens, 5_500_000);
        assert_eq!(s.requests, 4);
        assert_eq!(s.busiest_day.as_deref(), Some("2026-09-16"));
        assert_eq!(s.busiest_day_tokens, 4_000_000);
        assert_eq!(s.limits_hit, 1);
        // 5.5M input tokens at $3 / M
        assert!((s.estimated_cost_usd.unwrap() - 16.5).abs() < 1e-6);
    }

    #[test]
    fn an_empty_week_is_zeroes_without_a_busiest_day() {
        let db = Db::open_in_memory().unwrap();
        let pricing = crate::commands::pricing::default_table();
        let monday = NaiveDate::from_ymd_opt(2026, 9, 14).unwrap();
        let s = compute(&db, &pricing, monday).unwrap();
        assert_eq!((s.total_tokens, s.requests, s.limits_hit), (0, 0, 0));
        assert_eq!(s.busiest_day, None);
    }

    fn summary() -> WeeklySummary {
        WeeklySummary {
            week_start: "2026-09-14".into(),
            week_end: "2026-09-20".into(),
            total_tokens: 5_500_000,
            requests: 4,
            estimated_cost_usd: Some(16.5),
            busiest_day: Some("2026-09-16".into()),
            busiest_day_tokens: 4_000_000,
            limits_hit: 2,
            accounts: Vec::new(),
        }
    }

    #[test]
    fn the_text_is_bilingual() {
        let a = alert_for(&summary(), false);
        assert_eq!(a.title, "Last week's usage (2026-09-14 to 2026-09-20)");
        assert_eq!(
            a.body,
            "5.5M tokens, ~$16.50 estimated, busiest Wednesday (4.0M), 2 limits hit"
        );
        let a = alert_for(&summary(), true);
        assert_eq!(a.title, "上周用量（2026-09-14 至 2026-09-20）");
        assert_eq!(
            a.body,
            "5.5M 个 Token，预估 $16.50，最忙 周三（4.0M），触及限额 2 次"
        );
        assert_eq!(a.level, Level::Summary);

        let mut quiet = summary();
        quiet.estimated_cost_usd = None;
        quiet.busiest_day = None;
        quiet.limits_hit = 1;
        assert_eq!(alert_for(&quiet, false).body, "5.5M tokens, 1 limit hit");
    }
}
