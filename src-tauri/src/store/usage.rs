//! `usage_events` writes and the history aggregation. [BACKEND]

use super::Db;
use crate::commands::pricing;
use crate::model::{Bucket, HistoryQuery, HistoryResult, HistoryRow, PricingTable, TokenTotals};
use anyhow::Result;
use chrono::{Datelike, Local, LocalResult, NaiveDate, NaiveDateTime, TimeZone, Timelike};
use std::collections::BTreeMap;

/// One parsed request from a provider session log.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UsageEvent {
    pub provider: String,
    pub model: String,
    /// unix milliseconds
    pub ts: i64,
    pub input_tokens: i64,
    pub cache_write_tokens: i64,
    pub cache_read_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_tokens: i64,
    pub total_tokens: i64,
    pub session_id: Option<String>,
    /// dedupe key, unique per provider
    pub request_id: String,
    pub cwd: Option<String>,
    pub source_file: Option<String>,
}

/// Insert a batch of events.
///
/// `INSERT OR IGNORE` on `UNIQUE(provider, request_id)` makes re-ingestion
/// idempotent. Because Claude writes several streaming lines for one message,
/// an already-stored row is *upgraded* when the new row carries more tokens
/// (the final streaming line) — otherwise the partial first line would win.
/// Returns the number of genuinely new rows.
pub fn insert_usage_events(db: &Db, events: &[UsageEvent]) -> Result<u64> {
    if events.is_empty() {
        return Ok(0);
    }
    let mut conn = db.lock();
    let tx = conn.transaction()?;
    let mut added = 0u64;
    {
        let mut insert = tx.prepare(
            "INSERT OR IGNORE INTO usage_events
               (provider, model, ts, input_tokens, cache_write_tokens, cache_read_tokens,
                output_tokens, reasoning_tokens, total_tokens, session_id, request_id, cwd, source_file)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
        )?;
        let mut upgrade = tx.prepare(
            "UPDATE usage_events SET model=?2, ts=?3, input_tokens=?4, cache_write_tokens=?5,
               cache_read_tokens=?6, output_tokens=?7, reasoning_tokens=?8, total_tokens=?9,
               session_id=?10, cwd=?12, source_file=?13
             WHERE provider=?1 AND request_id=?11 AND
               (total_tokens < ?9 OR (total_tokens = ?9 AND reasoning_tokens < ?8))",
        )?;
        for e in events {
            let params = rusqlite::params![
                e.provider,
                e.model,
                e.ts,
                e.input_tokens,
                e.cache_write_tokens,
                e.cache_read_tokens,
                e.output_tokens,
                e.reasoning_tokens,
                e.total_tokens,
                e.session_id,
                e.request_id,
                e.cwd,
                e.source_file,
            ];
            let n = insert.execute(params)?;
            if n > 0 {
                added += 1;
            } else {
                upgrade.execute(params)?;
            }
        }
    }
    tx.commit()?;
    Ok(added)
}

/// Total number of stored events (used by the probe / diagnostics).
pub fn count_events(db: &Db) -> Result<i64> {
    let conn = db.lock();
    Ok(conn.query_row("SELECT COUNT(*) FROM usage_events", [], |r| r.get(0))?)
}

/// Aggregate `usage_events` into local-time buckets.
///
/// Bucketing happens in Rust (not SQL) so it can use the machine's local time
/// zone including DST, and so weeks can start on Monday.
pub fn query_history(db: &Db, q: &HistoryQuery, pricing: &PricingTable) -> Result<HistoryResult> {
    let (from, to) = query_range(&q.from, &q.to)?;

    // key: (bucket_start_ms, provider, model)
    let mut buckets: BTreeMap<(i64, String, Option<String>), Acc> = BTreeMap::new();
    let mut totals = Acc::default();
    let mut by_provider: BTreeMap<String, Acc> = BTreeMap::new();

    {
        let conn = db.lock();
        let (sql, provider_filter) = match q.provider.as_deref() {
            Some(p) => (
                "SELECT provider, model, ts, input_tokens, cache_write_tokens, cache_read_tokens,
                        output_tokens, reasoning_tokens, total_tokens
                 FROM usage_events WHERE ts >= ?1 AND ts <= ?2 AND provider = ?3 ORDER BY ts",
                Some(p.to_string()),
            ),
            None => (
                "SELECT provider, model, ts, input_tokens, cache_write_tokens, cache_read_tokens,
                        output_tokens, reasoning_tokens, total_tokens
                 FROM usage_events WHERE ts >= ?1 AND ts <= ?2 ORDER BY ts",
                None,
            ),
        };
        let mut stmt = conn.prepare(sql)?;
        let mut rows = match &provider_filter {
            Some(p) => stmt.query(rusqlite::params![from, to, p])?,
            None => stmt.query(rusqlite::params![from, to])?,
        };
        while let Some(row) = rows.next()? {
            let provider: String = row.get(0)?;
            let model: String = row.get(1)?;
            let ts: i64 = row.get(2)?;
            let t = TokenTotals {
                input_tokens: row.get(3)?,
                cache_write_tokens: row.get(4)?,
                cache_read_tokens: row.get(5)?,
                output_tokens: row.get(6)?,
                reasoning_tokens: row.get(7)?,
                total_tokens: row.get(8)?,
                requests: 1,
                estimated_cost_usd: None,
            };
            let cost = pricing::estimate_cost(pricing, &model, &t);
            let key_model = if q.group_by_model {
                Some(model.clone())
            } else {
                None
            };
            let bucket_ms = bucket_start_ms(ts, q.bucket);
            buckets
                .entry((bucket_ms, provider.clone(), key_model))
                .or_default()
                .add(&t, cost);
            by_provider.entry(provider).or_default().add(&t, cost);
            totals.add(&t, cost);
        }
    }

    let rows = buckets
        .into_iter()
        .map(|((ms, provider, model), acc)| HistoryRow {
            bucket_start: local_rfc3339(ms),
            provider,
            model,
            totals: acc.finish(),
        })
        .collect();

    Ok(HistoryResult {
        rows,
        totals: totals.finish(),
        by_provider: by_provider
            .into_iter()
            .map(|(k, v)| (k, v.finish()))
            .collect(),
    })
}

/// Accumulator for one bucket / provider / grand total.
#[derive(Default)]
struct Acc {
    totals: TokenTotals,
    /// A total is only priced if every contributing event has a known price.
    cost_known: bool,
    cost_missing: bool,
}

impl Acc {
    fn add(&mut self, t: &TokenTotals, cost: Option<f64>) {
        self.totals.input_tokens += t.input_tokens;
        self.totals.cache_write_tokens += t.cache_write_tokens;
        self.totals.cache_read_tokens += t.cache_read_tokens;
        self.totals.output_tokens += t.output_tokens;
        self.totals.reasoning_tokens += t.reasoning_tokens;
        self.totals.total_tokens += t.total_tokens;
        self.totals.requests += t.requests;
        if let Some(c) = cost {
            self.cost_known = true;
            self.totals.estimated_cost_usd =
                Some(self.totals.estimated_cost_usd.unwrap_or(0.0) + c);
        } else {
            self.cost_missing = true;
        }
    }

    /// Unknown-price buckets report `null` rather than an understated number.
    fn finish(self) -> TokenTotals {
        let mut t = self.totals;
        if !self.cost_known || self.cost_missing {
            t.estimated_cost_usd = None;
        }
        t
    }
}

/// Truncate a unix-ms timestamp to the start of its bucket **in local time**.
pub fn bucket_start_ms(ts_ms: i64, bucket: Bucket) -> i64 {
    let dt = match Local.timestamp_millis_opt(ts_ms) {
        LocalResult::Single(d) => d,
        LocalResult::Ambiguous(d, _) => d,
        LocalResult::None => return ts_ms,
    };
    let naive = dt.naive_local();
    let date = naive.date();
    let truncated: NaiveDateTime = match bucket {
        Bucket::Hour => date.and_hms_opt(naive.hour(), 0, 0),
        Bucket::Day => date.and_hms_opt(0, 0, 0),
        Bucket::Week => {
            let monday =
                date - chrono::Duration::days(date.weekday().num_days_from_monday() as i64);
            monday.and_hms_opt(0, 0, 0)
        }
        Bucket::Month => NaiveDate::from_ymd_opt(date.year(), date.month(), 1)
            .and_then(|d| d.and_hms_opt(0, 0, 0)),
    }
    .unwrap_or(naive);
    local_ms(truncated).unwrap_or(ts_ms)
}

/// Resolve a local naive datetime to unix ms, stepping over DST gaps.
fn local_ms(mut naive: NaiveDateTime) -> Option<i64> {
    for _ in 0..4 {
        match Local.from_local_datetime(&naive) {
            LocalResult::Single(d) => return Some(d.timestamp_millis()),
            // On a DST fall-back pick the earlier of the two instants.
            LocalResult::Ambiguous(d, _) => return Some(d.timestamp_millis()),
            // Spring-forward gap: nudge forward until the time exists.
            LocalResult::None => naive += chrono::Duration::minutes(30),
        }
    }
    None
}

/// Format a unix-ms timestamp as RFC 3339 **with the local UTC offset**.
pub fn local_rfc3339(ms: i64) -> String {
    match Local.timestamp_millis_opt(ms) {
        LocalResult::Single(d) | LocalResult::Ambiguous(d, _) => d.to_rfc3339(),
        LocalResult::None => chrono::Utc
            .timestamp_millis_opt(ms)
            .single()
            .map(|d| d.to_rfc3339())
            .unwrap_or_default(),
    }
}

/// Parse an RFC 3339 timestamp, a bare `YYYY-MM-DD` date or a local
/// `YYYY-MM-DDTHH:MM:SS` into unix milliseconds.
pub fn parse_time_ms(s: &str) -> Option<i64> {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(dt.timestamp_millis());
    }
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return d.and_hms_opt(0, 0, 0).and_then(local_ms);
    }
    for fmt in ["%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M:%S", "%Y-%m-%dT%H:%M"] {
        if let Ok(dt) = NaiveDateTime::parse_from_str(s, fmt) {
            return local_ms(dt);
        }
    }
    None
}

/// Invalid filters must not silently turn into a query of all history.
pub fn query_range(from: &str, to: &str) -> Result<(i64, i64)> {
    let from = parse_time_ms(from).ok_or_else(|| anyhow::anyhow!("invalid history start time"))?;
    let to = parse_time_ms(to).ok_or_else(|| anyhow::anyhow!("invalid history end time"))?;
    anyhow::ensure!(from <= to, "history start time must precede end time");
    Ok((from, to))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::pricing;
    use crate::model::Bucket;
    use chrono::{Local, TimeZone};

    fn ms(local: &str) -> i64 {
        parse_time_ms(local).expect("parse")
    }

    fn event(
        provider: &str,
        model: &str,
        ts: i64,
        id: &str,
        input: i64,
        output: i64,
    ) -> UsageEvent {
        UsageEvent {
            provider: provider.into(),
            model: model.into(),
            ts,
            input_tokens: input,
            cache_write_tokens: 0,
            cache_read_tokens: 0,
            output_tokens: output,
            reasoning_tokens: 0,
            total_tokens: input + output,
            session_id: Some("s".into()),
            request_id: id.into(),
            cwd: None,
            source_file: None,
        }
    }

    fn query(from: &str, to: &str, bucket: Bucket) -> HistoryQuery {
        HistoryQuery {
            from: from.into(),
            to: to.into(),
            bucket,
            group_by_model: false,
            provider: None,
        }
    }

    #[test]
    fn buckets_are_local_time_and_weeks_start_on_monday() {
        // 2026-09-15 is a Tuesday.
        let t = ms("2026-09-15T13:45:30");
        assert_eq!(bucket_start_ms(t, Bucket::Hour), ms("2026-09-15T13:00:00"));
        assert_eq!(bucket_start_ms(t, Bucket::Day), ms("2026-09-15T00:00:00"));
        assert_eq!(bucket_start_ms(t, Bucket::Week), ms("2026-09-14T00:00:00"));
        assert_eq!(bucket_start_ms(t, Bucket::Month), ms("2026-09-01T00:00:00"));

        // a Monday stays on its own day
        assert_eq!(
            bucket_start_ms(ms("2026-09-14T00:30:00"), Bucket::Week),
            ms("2026-09-14T00:00:00")
        );
        // a Sunday belongs to the week that started the Monday before
        assert_eq!(
            bucket_start_ms(ms("2026-09-20T23:59:59"), Bucket::Week),
            ms("2026-09-14T00:00:00")
        );
    }

    #[test]
    fn bucket_start_is_rendered_with_the_local_offset() {
        let start = bucket_start_ms(ms("2026-09-15T13:45:30"), Bucket::Day);
        let rendered = local_rfc3339(start);
        let expected = Local
            .timestamp_millis_opt(start)
            .single()
            .unwrap()
            .to_rfc3339();
        assert_eq!(rendered, expected);
        assert!(rendered.starts_with("2026-09-15T00:00:00"));
    }

    #[test]
    fn parse_time_accepts_rfc3339_dates_and_local_datetimes() {
        assert_eq!(
            parse_time_ms("2026-09-15T00:00:00Z"),
            Some(1_789_430_400_000)
        );
        assert_eq!(parse_time_ms("2026-09-15"), Some(ms("2026-09-15T00:00:00")));
        assert_eq!(
            parse_time_ms("2026-09-15 08:30:00"),
            Some(ms("2026-09-15T08:30:00"))
        );
        assert!(parse_time_ms("yesterday").is_none());
    }

    #[test]
    fn history_aggregates_per_bucket_provider_and_model() {
        let db = Db::open_in_memory().unwrap();
        let events = vec![
            event(
                "claude",
                "claude-opus-4-5",
                ms("2026-09-15T09:00:00"),
                "a",
                1_000_000,
                0,
            ),
            event(
                "claude",
                "claude-opus-4-5",
                ms("2026-09-15T23:30:00"),
                "b",
                1_000_000,
                0,
            ),
            event(
                "claude",
                "claude-haiku-4-5",
                ms("2026-09-16T00:30:00"),
                "c",
                10,
                10,
            ),
            event(
                "codex",
                "gpt-6-astra",
                ms("2026-09-16T10:00:00"),
                "d",
                100,
                50,
            ),
            event("codex", "who-knows", ms("2026-09-16T11:00:00"), "e", 7, 3),
        ];
        assert_eq!(insert_usage_events(&db, &events).unwrap(), 5);
        assert_eq!(insert_usage_events(&db, &events).unwrap(), 0, "idempotent");

        let table = pricing::default_table();
        let r =
            query_history(&db, &query("2026-09-14", "2026-09-20", Bucket::Day), &table).unwrap();
        assert_eq!(
            r.rows.len(),
            3,
            "2026-09-15/claude, 2026-09-16/claude, 2026-09-16/codex"
        );
        assert_eq!(r.totals.requests, 5);
        assert_eq!(r.totals.total_tokens, 2_000_000 + 20 + 150 + 10);
        assert_eq!(r.by_provider["claude"].requests, 3);
        assert_eq!(r.by_provider["codex"].requests, 2);
        assert!(
            r.by_provider["codex"].estimated_cost_usd.is_none(),
            "unknown model makes the combined estimate incomplete"
        );
        assert!(r.totals.estimated_cost_usd.is_none());
        // two opus-4-5 megatokens of input = 2 × $5
        let claude_cost = r.by_provider["claude"].estimated_cost_usd.unwrap();
        assert!((claude_cost - 10.00001).abs() < 0.001, "got {claude_cost}");

        let first = &r.rows[0];
        assert_eq!(first.provider, "claude");
        assert!(first.bucket_start.starts_with("2026-09-15T00:00:00"));
        assert_eq!(
            first.totals.requests, 2,
            "23:30 local stays in the same day"
        );
        assert!(first.model.is_none(), "not grouped by model");

        let grouped = query_history(
            &db,
            &HistoryQuery {
                group_by_model: true,
                ..query("2026-09-14", "2026-09-20", Bucket::Month)
            },
            &table,
        )
        .unwrap();
        assert_eq!(grouped.rows.len(), 4, "one row per model in one month");
        let unknown = grouped
            .rows
            .iter()
            .find(|r| r.model.as_deref() == Some("who-knows"))
            .unwrap();
        assert!(
            unknown.totals.estimated_cost_usd.is_none(),
            "unknown models cost null"
        );

        let only_codex = query_history(
            &db,
            &HistoryQuery {
                provider: Some("codex".into()),
                ..query("2026-09-14", "2026-09-20", Bucket::Week)
            },
            &table,
        )
        .unwrap();
        assert_eq!(only_codex.rows.len(), 1);
        assert_eq!(only_codex.totals.requests, 2);
        assert!(!only_codex.by_provider.contains_key("claude"));

        let empty =
            query_history(&db, &query("2020-01-01", "2020-01-02", Bucket::Day), &table).unwrap();
        assert!(empty.rows.is_empty());
        assert_eq!(empty.totals.requests, 0);
    }

    #[test]
    fn a_later_streaming_line_upgrades_the_stored_row() {
        let db = Db::open_in_memory().unwrap();
        let partial = event(
            "claude",
            "claude-opus-5",
            ms("2026-09-15T09:00:00"),
            "msg:req",
            2,
            1,
        );
        let complete = event(
            "claude",
            "claude-opus-5",
            ms("2026-09-15T09:00:02"),
            "msg:req",
            2,
            281,
        );
        assert_eq!(insert_usage_events(&db, &[partial]).unwrap(), 1);
        assert_eq!(insert_usage_events(&db, &[complete]).unwrap(), 0);
        assert_eq!(count_events(&db).unwrap(), 1);
        let table = pricing::default_table();
        let r =
            query_history(&db, &query("2026-09-14", "2026-09-20", Bucket::Day), &table).unwrap();
        assert_eq!(r.totals.output_tokens, 281);
    }

    #[test]
    fn malformed_or_reversed_ranges_fail_without_querying_all_history() {
        let db = Db::open_in_memory().unwrap();
        let table = pricing::default_table();
        assert!(query_history(&db, &query("bad", "2026-09-20", Bucket::Day), &table).is_err());
        assert!(
            query_history(&db, &query("2026-09-20", "2026-09-01", Bucket::Day), &table).is_err()
        );
    }
}
