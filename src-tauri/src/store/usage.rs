//! `usage_events` writes and the history aggregation. [BACKEND]

use super::Db;
use crate::commands::pricing;
use crate::model::{
    Bucket, CalendarDay, CalendarQuery, CalendarResult, CalendarSlot, HistoryQuery, HistoryResult,
    HistoryRow, PricingTable, SessionQuery, SessionRow, SessionsResult, TokenTotals,
};
use anyhow::Result;
use chrono::{Datelike, Local, LocalResult, NaiveDate, NaiveDateTime, TimeZone, Timelike};
use std::collections::{BTreeMap, BTreeSet};

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

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct HistoryBucket {
    start: i64,
    provider: String,
    project: Option<String>,
    model: Option<String>,
}

/// Aggregate `usage_events` into local-time buckets.
///
/// Bucketing happens in Rust (not SQL) so it can use the machine's local time
/// zone including DST, and so weeks can start on Monday.
pub fn query_history(db: &Db, q: &HistoryQuery, pricing: &PricingTable) -> Result<HistoryResult> {
    let (from, to) = query_range(&q.from, &q.to)?;

    let mut buckets: BTreeMap<HistoryBucket, Acc> = BTreeMap::new();
    let mut totals = Acc::default();
    let mut by_provider: BTreeMap<String, Acc> = BTreeMap::new();
    // Set as soon as one model was priced from its family, not from itself.
    let mut cost_approximate = false;
    let projects;

    {
        let conn = db.lock();
        // Project options deliberately ignore the active project selection.
        // Do not normalize paths: case, separators, spaces and Unicode are
        // part of the provider's original cwd identity on every platform.
        let mut options = conn.prepare(
            "SELECT DISTINCT COALESCE(cwd, '') AS project FROM usage_events
             WHERE ts >= ?1 AND ts < ?2 AND (?3 IS NULL OR provider = ?3)
             ORDER BY project COLLATE BINARY",
        )?;
        projects = options
            .query_map(rusqlite::params![from, to, q.provider], |row| {
                row.get::<_, String>(0)
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let sql = "SELECT provider, model, ts, input_tokens, cache_write_tokens, cache_read_tokens,
                          output_tokens, reasoning_tokens, total_tokens, cwd
                   FROM usage_events
                   WHERE ts >= ?1 AND ts < ?2 AND (?3 IS NULL OR provider = ?3)
                     AND (?4 IS NULL OR COALESCE(cwd, '') = ?4)
                   ORDER BY ts";
        let mut stmt = conn.prepare(sql)?;
        let mut rows = stmt.query(rusqlite::params![from, to, q.provider, q.project])?;
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
                ..TokenTotals::default()
            };
            let cost = match pricing::estimate_cost_kind(pricing, &model, &t) {
                Some((cost, kind)) => {
                    cost_approximate |= kind == pricing::MatchKind::Family;
                    Some(cost)
                }
                None => None,
            };
            let key_model = if q.group_by_model {
                Some(model.clone())
            } else {
                None
            };
            let project = if q.group_by_project {
                Some(row.get::<_, Option<String>>(9)?.unwrap_or_default())
            } else {
                q.project.clone()
            };
            let bucket_ms = bucket_start_ms(ts, q.bucket);
            buckets
                .entry(HistoryBucket {
                    start: bucket_ms,
                    provider: provider.clone(),
                    project,
                    model: key_model,
                })
                .or_default()
                .add(&t, cost);
            by_provider.entry(provider).or_default().add(&t, cost);
            totals.add(&t, cost);
        }
    }

    let rows = buckets
        .into_iter()
        .map(|(bucket, acc)| HistoryRow {
            bucket_start: local_rfc3339(bucket.start),
            provider: bucket.provider,
            model: bucket.model,
            project: bucket.project,
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
        projects,
        cost_approximate,
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
            self.totals.unpriced_requests += t.requests;
        }
    }

    /// Preserve a separately labeled known subtotal; full costs remain null
    /// whenever any contributing record is unpriced.
    fn finish(self) -> TokenTotals {
        let mut t = self.totals;
        t.known_cost_usd = if self.cost_known {
            t.estimated_cost_usd
        } else {
            None
        };
        if !self.cost_known || self.cost_missing {
            t.estimated_cost_usd = None;
        }
        t
    }
}

/// Default / maximum number of session rows handed to the webview.
const SESSION_LIMIT_DEFAULT: u32 = 200;
const SESSION_LIMIT_MAX: u32 = 1000;

/// Aggregate `usage_events` into a local-day calendar **and** a weekday × hour
/// punch card in a single pass over the range.
///
/// Both grids come from the same scan because the UI toggles between them; the
/// webview only ever receives the aggregates (at most ~26 × 7 days + 168 slots),
/// never the individual events.
pub fn query_calendar(
    db: &Db,
    q: &CalendarQuery,
    pricing: &PricingTable,
) -> Result<CalendarResult> {
    let (from, to) = query_range(&q.from, &q.to)?;
    let mut days: BTreeMap<i64, Acc> = BTreeMap::new();
    let mut slots: BTreeMap<(u8, u8), Acc> = BTreeMap::new();
    let mut totals = Acc::default();

    {
        let conn = db.lock();
        let mut stmt = conn.prepare(
            "SELECT provider, model, ts, input_tokens, cache_write_tokens, cache_read_tokens,
                    output_tokens, reasoning_tokens, total_tokens
             FROM usage_events
             WHERE ts >= ?1 AND ts < ?2 AND (?3 IS NULL OR provider = ?3)
               AND (?4 IS NULL OR COALESCE(cwd, '') = ?4)",
        )?;
        let mut rows = stmt.query(rusqlite::params![from, to, q.provider, q.project])?;
        while let Some(row) = rows.next()? {
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
                ..TokenTotals::default()
            };
            let cost = pricing::estimate_cost(pricing, &model, &t);
            days.entry(bucket_start_ms(ts, Bucket::Day))
                .or_default()
                .add(&t, cost);
            if let Some(slot) = local_weekday_hour(ts) {
                slots.entry(slot).or_default().add(&t, cost);
            }
            totals.add(&t, cost);
        }
    }

    Ok(CalendarResult {
        days: days
            .into_iter()
            .map(|(start, acc)| CalendarDay {
                date: local_date(start),
                totals: acc.finish(),
            })
            .collect(),
        slots: slots
            .into_iter()
            .map(|((weekday, hour), acc)| CalendarSlot {
                weekday,
                hour,
                totals: acc.finish(),
            })
            .collect(),
        totals: totals.finish(),
    })
}

/// One session's aggregate while the scan is running.
#[derive(Default)]
struct SessionAcc {
    first_ts: i64,
    last_ts: i64,
    /// cwd of the latest event seen, so a session that moved keeps its current
    /// directory (exact path, never trimmed — contract §4).
    project: String,
    models: BTreeSet<String>,
    acc: Acc,
}

/// Per-session totals for the selected range, capped server-side.
///
/// Sessions are `(provider, session_id)`; events without a session id share the
/// empty-string session of their provider, mirroring the unassigned-project
/// rule. First/last activity and the duration only cover events **inside** the
/// range, which is what the surrounding history view shows.
pub fn query_sessions(db: &Db, q: &SessionQuery, pricing: &PricingTable) -> Result<SessionsResult> {
    let (from, to) = query_range(&q.from, &q.to)?;
    let limit = q
        .limit
        .unwrap_or(SESSION_LIMIT_DEFAULT)
        .clamp(1, SESSION_LIMIT_MAX) as usize;
    let mut sessions: BTreeMap<(String, String), SessionAcc> = BTreeMap::new();
    let mut totals = Acc::default();

    {
        let conn = db.lock();
        let mut stmt = conn.prepare(
            "SELECT provider, model, ts, input_tokens, cache_write_tokens, cache_read_tokens,
                    output_tokens, reasoning_tokens, total_tokens,
                    COALESCE(session_id, ''), COALESCE(cwd, '')
             FROM usage_events
             WHERE ts >= ?1 AND ts < ?2 AND (?3 IS NULL OR provider = ?3)
               AND (?4 IS NULL OR COALESCE(cwd, '') = ?4)
             ORDER BY ts",
        )?;
        let mut rows = stmt.query(rusqlite::params![from, to, q.provider, q.project])?;
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
                ..TokenTotals::default()
            };
            let session_id: String = row.get(9)?;
            let project: String = row.get(10)?;
            let cost = pricing::estimate_cost(pricing, &model, &t);
            let entry = sessions
                .entry((provider, session_id))
                .or_insert_with(|| SessionAcc {
                    first_ts: ts,
                    last_ts: ts,
                    ..SessionAcc::default()
                });
            entry.first_ts = entry.first_ts.min(ts);
            // rows arrive in `ts` order, so the last write wins for the cwd
            entry.last_ts = entry.last_ts.max(ts);
            entry.project = project;
            entry.models.insert(model);
            entry.acc.add(&t, cost);
            totals.add(&t, cost);
        }
    }

    let total_sessions = sessions.len() as i64;
    let mut rows = sessions
        .into_iter()
        .map(|((provider, session_id), s)| SessionRow {
            session_id,
            provider,
            project: s.project,
            first_ts: local_rfc3339(s.first_ts),
            last_ts: local_rfc3339(s.last_ts),
            duration_ms: s.last_ts - s.first_ts,
            models: s.models.into_iter().collect(),
            totals: s.acc.finish(),
        })
        .collect::<Vec<_>>();
    // Biggest sessions first; the remaining keys break ties deterministically.
    rows.sort_by(|a, b| {
        b.totals
            .total_tokens
            .cmp(&a.totals.total_tokens)
            .then_with(|| b.last_ts.cmp(&a.last_ts))
            .then_with(|| a.provider.cmp(&b.provider))
            .then_with(|| a.session_id.cmp(&b.session_id))
    });
    let truncated = rows.len() > limit;
    rows.truncate(limit);

    Ok(SessionsResult {
        rows,
        total_sessions,
        totals: totals.finish(),
        truncated,
    })
}

/// Local `YYYY-MM-DD` for a unix-ms timestamp.
fn local_date(ms: i64) -> String {
    match Local.timestamp_millis_opt(ms) {
        LocalResult::Single(d) | LocalResult::Ambiguous(d, _) => {
            d.naive_local().date().format("%Y-%m-%d").to_string()
        }
        LocalResult::None => String::new(),
    }
}

/// `(weekday, hour)` in local time, Monday = 0.
fn local_weekday_hour(ms: i64) -> Option<(u8, u8)> {
    let dt = match Local.timestamp_millis_opt(ms) {
        LocalResult::Single(d) | LocalResult::Ambiguous(d, _) => d,
        LocalResult::None => return None,
    };
    let naive = dt.naive_local();
    Some((
        naive.date().weekday().num_days_from_monday() as u8,
        naive.hour() as u8,
    ))
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
            project: None,
            group_by_project: false,
        }
    }

    fn in_project(mut event: UsageEvent, project: Option<&str>) -> UsageEvent {
        event.cwd = project.map(str::to_owned);
        event
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
        assert!((r.by_provider["codex"].known_cost_usd.unwrap() - 0.0035).abs() < 1e-10);
        assert_eq!(r.by_provider["codex"].unpriced_requests, 1);
        assert_eq!(r.by_provider["claude"].unpriced_requests, 0);
        assert!((r.totals.known_cost_usd.unwrap() - 10.00356).abs() < 1e-10);
        assert_eq!(r.totals.unpriced_requests, 1);
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
    fn a_family_priced_model_marks_the_result_approximate() {
        let db = Db::open_in_memory().unwrap();
        let table = pricing::default_table();
        let range = query("2026-09-14", "2026-09-20", Bucket::Day);

        // A model the table knows exactly: an honest, exact estimate.
        let known = event(
            "codex",
            "gpt-5.3-codex",
            ms("2026-09-16T10:00:00"),
            "a",
            1_000_000,
            0,
        );
        insert_usage_events(&db, &[known]).unwrap();
        let exact = query_history(&db, &range, &table).unwrap();
        assert!(!exact.cost_approximate);
        assert_eq!(exact.totals.estimated_cost_usd, Some(1.75));

        // A model released after this build: priced from its family, and the
        // result says so rather than silently showing "—".
        let fresh = event(
            "codex",
            "gpt-5.3-codex-spark",
            ms("2026-09-16T11:00:00"),
            "b",
            1_000_000,
            0,
        );
        insert_usage_events(&db, &[fresh]).unwrap();
        let approximate = query_history(&db, &range, &table).unwrap();
        assert!(approximate.cost_approximate, "family match is approximate");
        assert_eq!(approximate.totals.estimated_cost_usd, Some(3.5));

        // Something from another vendor is still unknown, not guessed at.
        let alien = event("codex", "who-knows", ms("2026-09-16T12:00:00"), "c", 10, 0);
        insert_usage_events(&db, &[alien]).unwrap();
        let unknown = query_history(&db, &range, &table).unwrap();
        assert!(unknown.totals.estimated_cost_usd.is_none());
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

    #[test]
    fn project_and_model_groups_keep_independent_totals_and_prices() {
        let db = Db::open_in_memory().unwrap();
        let at = ms("2026-09-15T12:00:00");
        let linux = "/home/dev/项目 one";
        let windows = r"C:\Work\O'Reilly, Inc\项目 one";
        let events = [
            in_project(event("codex", "gpt-5", at, "a", 1_000_000, 0), Some(linux)),
            in_project(event("codex", "gpt-5", at, "b", 1_000_000, 0), Some(linux)),
            in_project(
                event("codex", "gpt-5-mini", at, "c", 1_000_000, 0),
                Some(linux),
            ),
            in_project(
                event("codex", "gpt-5", at, "d", 3_000_000, 0),
                Some(windows),
            ),
            in_project(event("codex", "unknown", at, "e", 100, 0), Some(windows)),
            in_project(event("codex", "gpt-5", at, "f", 10, 0), None),
            in_project(event("codex", "gpt-5", at, "g", 10, 0), Some("")),
            in_project(
                event("claude", "claude-opus-4-5", at, "h", 1_000_000, 0),
                Some(linux),
            ),
        ];
        insert_usage_events(&db, &events).unwrap();
        let table = pricing::default_table();
        let q = HistoryQuery {
            group_by_project: true,
            group_by_model: true,
            ..query("2026-09-15", "2026-09-16", Bucket::Day)
        };
        let grouped = query_history(&db, &q, &table).unwrap();
        assert_eq!(grouped.rows.len(), 6);
        assert_eq!(grouped.projects, vec!["", linux, windows]);
        let linux_gpt = grouped
            .rows
            .iter()
            .find(|row| {
                row.project.as_deref() == Some(linux) && row.model.as_deref() == Some("gpt-5")
            })
            .unwrap();
        assert_eq!(linux_gpt.totals.requests, 2);
        assert_eq!(linux_gpt.totals.estimated_cost_usd, Some(2.5));
        let windows_gpt = grouped
            .rows
            .iter()
            .find(|row| {
                row.project.as_deref() == Some(windows) && row.model.as_deref() == Some("gpt-5")
            })
            .unwrap();
        assert_eq!(windows_gpt.totals.estimated_cost_usd, Some(3.75));
        let unassigned = grouped
            .rows
            .iter()
            .find(|row| row.project.as_deref() == Some(""))
            .unwrap();
        assert_eq!(
            unassigned.totals.requests, 2,
            "NULL and empty cwd share the unassigned group"
        );
        assert_eq!(grouped.by_provider["codex"].requests, 7);
        assert_eq!(grouped.by_provider["claude"].estimated_cost_usd, Some(5.0));
        assert!(grouped.totals.estimated_cost_usd.is_none());

        let per_project = query_history(
            &db,
            &HistoryQuery {
                group_by_model: false,
                ..q.clone()
            },
            &table,
        )
        .unwrap();
        assert_eq!(per_project.rows.len(), 4);
        let linux_codex = per_project
            .rows
            .iter()
            .find(|row| row.provider == "codex" && row.project.as_deref() == Some(linux))
            .unwrap();
        assert_eq!(linux_codex.totals.estimated_cost_usd, Some(2.75));
        assert!(per_project
            .rows
            .iter()
            .find(|row| row.project.as_deref() == Some(windows))
            .unwrap()
            .totals
            .estimated_cost_usd
            .is_none());
        let per_model = query_history(
            &db,
            &HistoryQuery {
                group_by_project: false,
                ..q
            },
            &table,
        )
        .unwrap();
        assert_eq!(per_model.rows.len(), 4);
        assert!(per_model.rows.iter().all(|row| row.project.is_none()));
        let ungrouped =
            query_history(&db, &query("2026-09-15", "2026-09-16", Bucket::Day), &table).unwrap();
        assert_eq!(ungrouped.rows.len(), 2);
        assert_eq!(ungrouped.totals, grouped.totals);
        assert!(ungrouped
            .rows
            .iter()
            .all(|row| row.project.is_none() && row.model.is_none()));
    }

    #[test]
    fn project_filter_is_exact_and_options_ignore_only_the_project_filter() {
        let db = Db::open_in_memory().unwrap();
        let at = ms("2026-09-15T12:00:00");
        let paths = [
            r"C:\Work\O'Reilly, Inc\项目",
            r"c:\Work\O'Reilly, Inc\项目",
            r"\\server\share\项目",
            " /Users/me/spaces ",
            "' OR 1=1 --",
        ];
        for (i, path) in paths.iter().enumerate() {
            insert_usage_events(
                &db,
                &[in_project(
                    event("codex", "gpt-5", at, &i.to_string(), 10, 0),
                    Some(path),
                )],
            )
            .unwrap();
        }
        insert_usage_events(
            &db,
            &[
                in_project(event("codex", "gpt-5", at, "null", 10, 0), None),
                in_project(event("codex", "gpt-5", at, "empty", 10, 0), Some("")),
                in_project(
                    event("claude", "claude-opus-4-5", at, "other-provider", 10, 0),
                    Some("/claude-only"),
                ),
                in_project(
                    event("codex", "gpt-5", ms("2026-09-14T12:00:00"), "old", 10, 0),
                    Some("/outside-range"),
                ),
            ],
        )
        .unwrap();
        let table = pricing::default_table();
        let mut q = HistoryQuery {
            provider: Some("codex".into()),
            ..query("2026-09-15", "2026-09-16", Bucket::Day)
        };
        let mut expected = paths
            .iter()
            .map(|p| p.to_string())
            .chain([String::new()])
            .collect::<Vec<_>>();
        expected.sort();
        for path in paths {
            q.project = Some(path.into());
            let result = query_history(&db, &q, &table).unwrap();
            assert_eq!(
                result.totals.requests, 1,
                "path filter must use a bound exact value"
            );
            assert_eq!(result.projects, expected);
            assert_eq!(
                result.rows[0].project.as_deref(),
                Some(path),
                "filtered rows retain the full path even without grouping"
            );
        }
        q.project = Some("".into());
        let unassigned = query_history(&db, &q, &table).unwrap();
        assert_eq!(unassigned.totals.requests, 2);
        assert_eq!(unassigned.rows[0].project.as_deref(), Some(""));
        q.project = Some("/missing' OR 1=1 --".into());
        let missing = query_history(&db, &q, &table).unwrap();
        assert!(missing.rows.is_empty());
        assert_eq!(
            missing.projects, expected,
            "an empty filtered result must still offer other projects"
        );
    }

    #[test]
    fn history_range_includes_start_and_excludes_end_for_rows_and_projects() {
        let db = Db::open_in_memory().unwrap();
        let start = ms("2026-09-15T00:00:00");
        let end = ms("2026-09-16T00:00:00");
        let events = [
            (start - 1, "/before"),
            (start, "/at-start"),
            (end - 1, "/last-ms"),
            (end, "/at-end"),
        ]
        .into_iter()
        .map(|(ts, path)| in_project(event("codex", "gpt-5", ts, path, 10, 0), Some(path)))
        .collect::<Vec<_>>();
        insert_usage_events(&db, &events).unwrap();
        let result = query_history(
            &db,
            &query("2026-09-15", "2026-09-16", Bucket::Day),
            &pricing::default_table(),
        )
        .unwrap();
        assert_eq!(result.totals.requests, 2);
        assert_eq!(result.projects, vec!["/at-start", "/last-ms"]);
        let empty = query_history(
            &db,
            &query("2026-09-15", "2026-09-15", Bucket::Day),
            &pricing::default_table(),
        )
        .unwrap();
        assert!(empty.rows.is_empty() && empty.projects.is_empty());
    }

    #[test]
    fn history_json_keeps_legacy_queries_and_project_paths_compatible() {
        let old_query: HistoryQuery = serde_json::from_value(
            serde_json::json!({"from":"2026-09-15", "to":"2026-09-16", "bucket":"day"}),
        )
        .unwrap();
        assert!(!old_query.group_by_project);
        assert!(old_query.project.is_none());
        let path = "C:\\团队\\comma, quote\" project";
        let db = Db::open_in_memory().unwrap();
        insert_usage_events(
            &db,
            &[in_project(
                event("codex", "gpt-5", ms("2026-09-15T12:00:00"), "one", 10, 0),
                Some(path),
            )],
        )
        .unwrap();
        let grouped = HistoryQuery {
            group_by_project: true,
            ..old_query
        };
        let query_json = serde_json::to_value(&grouped).unwrap();
        assert_eq!(query_json["groupByProject"], true);
        let result = query_history(&db, &grouped, &pricing::default_table()).unwrap();
        let wire = serde_json::to_value(&result).unwrap();
        assert_eq!(wire["projects"][0], path);
        assert_eq!(wire["rows"][0]["project"], path);
        assert_eq!(
            wire["rows"][0]["totalTokens"], 10,
            "CSV consumes flattened row totals"
        );
        assert!(wire["rows"][0].get("totals").is_none());
        assert_eq!(
            serde_json::from_value::<HistoryResult>(wire).unwrap(),
            result
        );
    }

    // ---------------------------------------------- calendar / sessions ----

    fn calendar(from: &str, to: &str) -> CalendarQuery {
        CalendarQuery {
            from: from.into(),
            to: to.into(),
            provider: None,
            project: None,
        }
    }

    fn sessions(from: &str, to: &str) -> SessionQuery {
        SessionQuery {
            from: from.into(),
            to: to.into(),
            provider: None,
            project: None,
            limit: None,
        }
    }

    fn in_session(mut event: UsageEvent, session: Option<&str>) -> UsageEvent {
        event.session_id = session.map(str::to_owned);
        event
    }

    #[test]
    fn calendar_reports_local_days_and_a_weekday_hour_punch_card() {
        let db = Db::open_in_memory().unwrap();
        // 2026-09-15 is a Tuesday (weekday 1), 2026-09-20 a Sunday (weekday 6).
        let events = [
            event(
                "claude",
                "claude-opus-4-5",
                ms("2026-09-15T09:30:00"),
                "a",
                1_000_000,
                0,
            ),
            event(
                "claude",
                "claude-opus-4-5",
                ms("2026-09-15T23:59:59"),
                "b",
                10,
                0,
            ),
            event("codex", "gpt-5", ms("2026-09-15T09:05:00"), "c", 20, 0),
            event("codex", "who-knows", ms("2026-09-20T13:00:00"), "d", 30, 0),
        ];
        insert_usage_events(&db, &events).unwrap();
        let table = pricing::default_table();

        let r = query_calendar(&db, &calendar("2026-09-14", "2026-09-21"), &table).unwrap();
        assert_eq!(
            r.days.iter().map(|d| d.date.as_str()).collect::<Vec<_>>(),
            vec!["2026-09-15", "2026-09-20"],
            "only days with activity are returned, in calendar order"
        );
        assert_eq!(r.days[0].totals.requests, 3, "23:59 local stays in its day");
        assert_eq!(r.days[0].totals.total_tokens, 1_000_030);
        assert!(
            r.days[1].totals.estimated_cost_usd.is_none(),
            "an unpriced model leaves the day's estimate unknown"
        );
        assert_eq!(r.totals.requests, 4);
        assert!(r.totals.estimated_cost_usd.is_none());

        let slots = r
            .slots
            .iter()
            .map(|s| (s.weekday, s.hour, s.totals.requests))
            .collect::<Vec<_>>();
        assert_eq!(slots, vec![(1, 9, 2), (1, 23, 1), (6, 13, 1)]);

        let claude = query_calendar(
            &db,
            &CalendarQuery {
                provider: Some("claude".into()),
                ..calendar("2026-09-14", "2026-09-21")
            },
            &table,
        )
        .unwrap();
        assert_eq!(claude.totals.requests, 2);
        assert_eq!(claude.days.len(), 1);
        assert_eq!(claude.totals.estimated_cost_usd, Some(5.00005));
    }

    #[test]
    fn calendar_range_is_half_open_and_respects_the_exact_project_filter() {
        let db = Db::open_in_memory().unwrap();
        let start = ms("2026-09-15T00:00:00");
        let end = ms("2026-09-16T00:00:00");
        let path = r"C:\Work\O'Reilly, Inc\项目";
        let events = [
            in_project(
                event("codex", "gpt-5", start - 1, "before", 10, 0),
                Some(path),
            ),
            in_project(
                event("codex", "gpt-5", start, "at-start", 10, 0),
                Some(path),
            ),
            in_project(
                event("codex", "gpt-5", end - 1, "last-ms", 10, 0),
                Some(path),
            ),
            in_project(event("codex", "gpt-5", end, "at-end", 10, 0), Some(path)),
            in_project(
                event("codex", "gpt-5", start, "other", 10, 0),
                Some("/other"),
            ),
            in_project(event("codex", "gpt-5", start, "none", 10, 0), None),
        ];
        insert_usage_events(&db, &events).unwrap();
        let table = pricing::default_table();

        let all = query_calendar(&db, &calendar("2026-09-15", "2026-09-16"), &table).unwrap();
        assert_eq!(
            all.totals.requests, 4,
            "[from, to) excludes the end instant"
        );

        let filtered = query_calendar(
            &db,
            &CalendarQuery {
                project: Some(path.into()),
                ..calendar("2026-09-15", "2026-09-16")
            },
            &table,
        )
        .unwrap();
        assert_eq!(filtered.totals.requests, 2);

        let unassigned = query_calendar(
            &db,
            &CalendarQuery {
                project: Some(String::new()),
                ..calendar("2026-09-15", "2026-09-16")
            },
            &table,
        )
        .unwrap();
        assert_eq!(unassigned.totals.requests, 1);

        assert!(query_calendar(&db, &calendar("nope", "2026-09-16"), &table).is_err());
        assert!(query_calendar(&db, &calendar("2026-09-16", "2026-09-15"), &table).is_err());
    }

    #[test]
    fn sessions_aggregate_per_provider_session_with_boundaries_and_models() {
        let db = Db::open_in_memory().unwrap();
        let day = "2026-09-15T";
        let events = [
            in_session(
                in_project(
                    event(
                        "claude",
                        "claude-opus-4-5",
                        ms(&format!("{day}09:00:00")),
                        "a",
                        1_000_000,
                        0,
                    ),
                    Some("/home/dev/api"),
                ),
                Some("sess-1"),
            ),
            in_session(
                in_project(
                    event(
                        "claude",
                        "claude-haiku-4-5",
                        ms(&format!("{day}11:30:00")),
                        "b",
                        10,
                        5,
                    ),
                    Some("/home/dev/api-moved"),
                ),
                Some("sess-1"),
            ),
            in_session(
                event(
                    "claude",
                    "claude-opus-4-5",
                    ms(&format!("{day}10:00:00")),
                    "c",
                    200_000,
                    0,
                ),
                Some("sess-2"),
            ),
            // same session id, different provider: never merged
            in_session(
                event("codex", "gpt-5", ms(&format!("{day}12:00:00")), "d", 100, 0),
                Some("sess-1"),
            ),
            in_session(
                event(
                    "codex",
                    "who-knows",
                    ms(&format!("{day}12:30:00")),
                    "e",
                    50,
                    0,
                ),
                None,
            ),
            in_session(
                event("codex", "gpt-5", ms(&format!("{day}13:00:00")), "f", 60, 0),
                Some(""),
            ),
        ];
        insert_usage_events(&db, &events).unwrap();
        let table = pricing::default_table();
        let r = query_sessions(&db, &sessions("2026-09-15", "2026-09-16"), &table).unwrap();

        assert_eq!(
            r.total_sessions, 4,
            "claude/sess-1, claude/sess-2, codex/sess-1, codex/unassigned"
        );
        assert!(!r.truncated);
        assert_eq!(r.totals.requests, 6);
        assert_eq!(r.rows.len(), 4);
        assert_eq!(r.rows[0].session_id, "sess-1", "biggest session first");
        assert_eq!(r.rows[0].provider, "claude");
        assert_eq!(r.rows[0].totals.requests, 2);
        assert_eq!(r.rows[0].duration_ms, 2 * 3_600_000 + 30 * 60_000);
        assert!(r.rows[0].first_ts.starts_with("2026-09-15T09:00:00"));
        assert!(r.rows[0].last_ts.starts_with("2026-09-15T11:30:00"));
        assert_eq!(
            r.rows[0].models,
            vec![
                "claude-haiku-4-5".to_string(),
                "claude-opus-4-5".to_string()
            ]
        );
        assert_eq!(
            r.rows[0].project, "/home/dev/api-moved",
            "the session keeps the cwd of its latest event"
        );
        let unassigned = r
            .rows
            .iter()
            .find(|row| row.provider == "codex" && row.session_id.is_empty())
            .unwrap();
        assert_eq!(
            unassigned.totals.requests, 2,
            "NULL and empty session ids share the unassigned session"
        );
        assert!(
            unassigned.totals.estimated_cost_usd.is_none(),
            "one unpriced model makes the session estimate unknown"
        );
        assert_eq!(unassigned.duration_ms, 30 * 60_000);
        assert_eq!(
            r.rows
                .iter()
                .find(|row| row.provider == "claude" && row.session_id == "sess-2")
                .unwrap()
                .duration_ms,
            0,
            "a single-event session has no duration"
        );
    }

    #[test]
    fn sessions_cap_rows_server_side_and_keep_the_range_half_open() {
        let db = Db::open_in_memory().unwrap();
        let start = ms("2026-09-15T00:00:00");
        let end = ms("2026-09-16T00:00:00");
        let mut events = Vec::new();
        for i in 0..250i64 {
            events.push(in_session(
                event("codex", "gpt-5", start + i * 60_000, &format!("r{i}"), i, 0),
                Some(&format!("sess-{i:03}")),
            ));
        }
        events.push(in_session(
            event("codex", "gpt-5", start - 1, "before", 9_000, 0),
            Some("outside-before"),
        ));
        events.push(in_session(
            event("codex", "gpt-5", end, "at-end", 9_000, 0),
            Some("outside-after"),
        ));
        insert_usage_events(&db, &events).unwrap();
        let table = pricing::default_table();

        let capped = query_sessions(&db, &sessions("2026-09-15", "2026-09-16"), &table).unwrap();
        assert_eq!(capped.total_sessions, 250, "the count ignores the cap");
        assert_eq!(capped.rows.len(), 200, "default cap");
        assert!(capped.truncated);
        assert_eq!(capped.totals.requests, 250);
        assert_eq!(capped.rows[0].session_id, "sess-249", "sorted by tokens");
        assert!(capped
            .rows
            .iter()
            .all(|row| !row.session_id.starts_with("outside")));

        let few = query_sessions(
            &db,
            &SessionQuery {
                limit: Some(3),
                ..sessions("2026-09-15", "2026-09-16")
            },
            &table,
        )
        .unwrap();
        assert_eq!(few.rows.len(), 3);
        assert!(few.truncated);
        assert_eq!(few.totals.requests, 250, "totals cover every session");

        let huge = query_sessions(
            &db,
            &SessionQuery {
                limit: Some(u32::MAX),
                ..sessions("2026-09-15", "2026-09-16")
            },
            &table,
        )
        .unwrap();
        assert_eq!(huge.rows.len(), 250);
        assert!(!huge.truncated);
        assert!(query_sessions(&db, &sessions("2026-09-16", "2026-09-15"), &table).is_err());
    }

    #[test]
    fn calendar_and_session_json_use_camel_case_and_flattened_totals() {
        let db = Db::open_in_memory().unwrap();
        insert_usage_events(
            &db,
            &[in_session(
                in_project(
                    event("codex", "gpt-5", ms("2026-09-15T12:00:00"), "a", 10, 0),
                    Some("/tmp/x"),
                ),
                Some("sess"),
            )],
        )
        .unwrap();
        let table = pricing::default_table();
        let cal = serde_json::to_value(
            query_calendar(&db, &calendar("2026-09-15", "2026-09-16"), &table).unwrap(),
        )
        .unwrap();
        assert_eq!(cal["days"][0]["date"], "2026-09-15");
        assert_eq!(cal["days"][0]["totalTokens"], 10);
        assert!(cal["days"][0].get("totals").is_none());
        assert_eq!(cal["slots"][0]["weekday"], 1);
        assert_eq!(cal["slots"][0]["hour"], 12);

        let ses = serde_json::to_value(
            query_sessions(&db, &sessions("2026-09-15", "2026-09-16"), &table).unwrap(),
        )
        .unwrap();
        assert_eq!(ses["rows"][0]["sessionId"], "sess");
        assert_eq!(ses["rows"][0]["durationMs"], 0);
        assert_eq!(ses["rows"][0]["project"], "/tmp/x");
        assert_eq!(ses["rows"][0]["totalTokens"], 10);
        assert_eq!(ses["totalSessions"], 1);
        assert_eq!(ses["truncated"], false);
        // legacy-shaped queries (no provider/project/limit) still deserialize
        let q: SessionQuery =
            serde_json::from_value(serde_json::json!({"from":"2026-09-15","to":"2026-09-16"}))
                .unwrap();
        assert!(q.limit.is_none() && q.provider.is_none() && q.project.is_none());
    }
}

#[cfg(test)]
mod partial_cost_tests {
    use super::*;

    #[test]
    fn known_subtotals_and_missing_counts_are_order_independent() {
        for costs in [[Some(12.5), None, Some(2.0)], [None, Some(2.0), Some(12.5)]] {
            let mut acc = Acc::default();
            for cost in costs {
                acc.add(
                    &TokenTotals {
                        requests: 2,
                        total_tokens: 10,
                        ..Default::default()
                    },
                    cost,
                );
            }
            let totals = acc.finish();
            assert_eq!(totals.estimated_cost_usd, None);
            assert_eq!(totals.known_cost_usd, Some(14.5));
            assert_eq!(totals.unpriced_requests, 2);
            assert_eq!(totals.requests, 6);
            assert_eq!(totals.total_tokens, 30);
        }
    }

    #[test]
    fn absent_prices_are_distinct_from_legitimately_free_records() {
        let event = TokenTotals {
            requests: 1,
            ..Default::default()
        };
        let mut unknown = Acc::default();
        unknown.add(&event, None);
        let unknown = unknown.finish();
        assert_eq!(unknown.known_cost_usd, None);
        assert_eq!(unknown.estimated_cost_usd, None);
        assert_eq!(unknown.unpriced_requests, 1);

        let mut free = Acc::default();
        free.add(&event, Some(0.0));
        let complete = free.finish();
        assert_eq!(complete.estimated_cost_usd, Some(0.0));
        assert_eq!(complete.known_cost_usd, Some(0.0));
        assert_eq!(complete.unpriced_requests, 0);

        let mut mixed = Acc::default();
        mixed.add(&event, None);
        mixed.add(&event, Some(0.0));
        let partial = mixed.finish();
        assert_eq!(partial.estimated_cost_usd, None);
        assert_eq!(partial.known_cost_usd, Some(0.0));
        assert_eq!(partial.unpriced_requests, 1);
        let empty = Acc::default().finish();
        assert_eq!(empty.known_cost_usd, None);
        assert_eq!(empty.unpriced_requests, 0);
    }
}
