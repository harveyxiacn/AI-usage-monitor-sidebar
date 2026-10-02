//! Token usage inside arbitrary time windows, used to relate quota cycles to
//! the tokens consumed during them. [BACKEND]

use super::usage::query_range;
use super::Db;
use crate::commands::pricing;
use crate::model::{PricingTable, TokenTotals, WindowUsageQuery};
use anyhow::Result;

/// Upper bound on the windows of one query; the UI asks for recent cycles only.
pub const MAX_WINDOWS: usize = 200;

/// Totals of one provider's `usage_events` in each `[from, to)` window, in the
/// order of `q.windows`. A cost is `None` as soon as one request in the window
/// has no known price (never an understated total).
pub fn query_window_usage(
    db: &Db,
    q: &WindowUsageQuery,
    pricing: &PricingTable,
) -> Result<Vec<TokenTotals>> {
    anyhow::ensure!(
        q.windows.len() <= MAX_WINDOWS,
        "too many windows (max {MAX_WINDOWS})"
    );
    let conn = db.lock();
    let mut stmt = conn.prepare(
        "SELECT model, input_tokens, cache_write_tokens, cache_read_tokens,
                output_tokens, reasoning_tokens, total_tokens
         FROM usage_events WHERE provider = ?1 AND ts >= ?2 AND ts < ?3",
    )?;
    let mut out = Vec::with_capacity(q.windows.len());
    for w in &q.windows {
        let (from, to) = query_range(&w.from, &w.to)?;
        let mut totals = TokenTotals::default();
        let mut cost = 0.0;
        let mut priced = 0i64;
        let mut rows = stmt.query(rusqlite::params![q.provider, from, to])?;
        while let Some(row) = rows.next()? {
            let model: String = row.get(0)?;
            let t = TokenTotals {
                input_tokens: row.get(1)?,
                cache_write_tokens: row.get(2)?,
                cache_read_tokens: row.get(3)?,
                output_tokens: row.get(4)?,
                reasoning_tokens: row.get(5)?,
                total_tokens: row.get(6)?,
                requests: 1,
                ..TokenTotals::default()
            };
            totals.input_tokens += t.input_tokens;
            totals.cache_write_tokens += t.cache_write_tokens;
            totals.cache_read_tokens += t.cache_read_tokens;
            totals.output_tokens += t.output_tokens;
            totals.reasoning_tokens += t.reasoning_tokens;
            totals.total_tokens += t.total_tokens;
            totals.requests += 1;
            match pricing::estimate_cost_kind(pricing, &model, &t) {
                Some((c, _)) => {
                    cost += c;
                    priced += 1;
                }
                None => totals.unpriced_requests += 1,
            }
        }
        if priced > 0 {
            totals.known_cost_usd = Some(cost);
            if totals.unpriced_requests == 0 {
                totals.estimated_cost_usd = Some(cost);
            }
        }
        out.push(totals);
    }
    Ok(out)
}

/// `(ts, total_tokens)` of every event of `provider` at or after `since` (unix
/// ms), oldest first — the token-based fallback of the burn-rate forecast.
pub fn usage_token_events(
    db: &Db,
    provider: &str,
    since: i64,
) -> Result<Vec<crate::commands::forecast::TokenEvent>> {
    let conn = db.lock();
    let mut stmt = conn.prepare_cached(
        "SELECT ts, total_tokens FROM usage_events
         WHERE provider = ?1 AND ts >= ?2 ORDER BY ts",
    )?;
    let rows = stmt.query_map(rusqlite::params![provider, since], |r| {
        Ok(crate::commands::forecast::TokenEvent {
            ts_ms: r.get(0)?,
            tokens: r.get(1)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::store::{insert_usage_events, UsageEvent};
    use crate::model::TimeWindow;

    fn ev(provider: &str, id: &str, ts: i64, input: i64, output: i64) -> UsageEvent {
        UsageEvent {
            provider: provider.into(),
            model: "claude-sonnet-4-5".into(),
            ts,
            input_tokens: input,
            output_tokens: output,
            total_tokens: input + output,
            request_id: id.into(),
            ..UsageEvent::default()
        }
    }

    fn win(from: &str, to: &str) -> TimeWindow {
        TimeWindow {
            from: from.into(),
            to: to.into(),
        }
    }

    #[test]
    fn sums_each_window_half_open_and_per_provider() {
        let db = Db::open_in_memory().unwrap();
        let t0 = 1_789_430_400_000i64; // 2026-09-15T00:00:00Z
        let h = 3_600_000;
        insert_usage_events(
            &db,
            &[
                ev("claude", "a", t0, 100, 10),
                ev("claude", "b", t0 + h, 200, 20),
                ev("claude", "c", t0 + 5 * h, 400, 40), // exactly at `to`: excluded
                ev("claude", "d", t0 + 6 * h, 800, 80),
                ev("codex", "e", t0 + h, 9_000, 9_000),
            ],
        )
        .unwrap();
        let q = WindowUsageQuery {
            provider: "claude".into(),
            windows: vec![
                win("2026-09-15T00:00:00Z", "2026-09-15T05:00:00Z"),
                win("2026-09-15T05:00:00Z", "2026-09-15T10:00:00Z"),
                win("2026-09-16T00:00:00Z", "2026-09-16T05:00:00Z"),
            ],
        };
        let r = query_window_usage(&db, &q, &PricingTable::default()).unwrap();
        assert_eq!(r.len(), 3);
        assert_eq!(r[0].total_tokens, 330);
        assert_eq!(r[0].requests, 2);
        assert_eq!(r[1].total_tokens, 440 + 880);
        assert_eq!(r[2].total_tokens, 0);
        assert_eq!(r[2].estimated_cost_usd, None);
    }

    #[test]
    fn rejects_bad_ranges_and_oversized_requests() {
        let db = Db::open_in_memory().unwrap();
        let bad = WindowUsageQuery {
            provider: "claude".into(),
            windows: vec![win("2026-09-16T00:00:00Z", "2026-09-15T00:00:00Z")],
        };
        assert!(query_window_usage(&db, &bad, &PricingTable::default()).is_err());
        let many = WindowUsageQuery {
            provider: "claude".into(),
            windows: (0..=MAX_WINDOWS)
                .map(|_| win("2026-09-15T00:00:00Z", "2026-09-15T01:00:00Z"))
                .collect(),
        };
        assert!(query_window_usage(&db, &many, &PricingTable::default()).is_err());
    }

    #[test]
    fn token_events_are_per_provider_from_a_start_time_oldest_first() {
        let db = Db::open_in_memory().unwrap();
        let t0 = 1_789_430_400_000i64;
        insert_usage_events(
            &db,
            &[
                ev("claude", "b", t0 + 2_000, 200, 20),
                ev("claude", "a", t0, 100, 10),
                ev("claude", "old", t0 - 1, 1, 1),
                ev("codex", "x", t0 + 1_000, 9, 9),
            ],
        )
        .unwrap();
        let events = usage_token_events(&db, "claude", t0).unwrap();
        let got: Vec<(i64, i64)> = events.iter().map(|e| (e.ts_ms, e.tokens)).collect();
        assert_eq!(got, vec![(t0, 110), (t0 + 2_000, 220)]);
        assert!(usage_token_events(&db, "copilot", t0).unwrap().is_empty());
    }
}
