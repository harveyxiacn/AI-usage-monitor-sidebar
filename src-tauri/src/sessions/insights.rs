//! Aggregate session analytics for the Insights view. Everything here is
//! derived from metrics and metadata (token usage, timing, tool names and
//! outcomes); transcript content is never read, so this works with local
//! content access switched off.
use super::{
    model::*,
    store::{summary, Range, CANDIDATES_CTE},
};
use crate::{commands::store::Db, model::PricingTable};
use anyhow::Result;
use rusqlite::params;
use std::cmp::Ordering;

/// The newest matching sessions that are analysed; the rest are only counted.
pub const MAX_SESSIONS: i64 = 1000;
const TOP_N: usize = 8;
const TOOL_LIMIT: i64 = 12;
const BINS_PER_DECADE: f64 = 3.0;
const MAX_BINS: i64 = 60;
/// A top-failure row needs at least this many calls to have a meaningful rate.
const MIN_CALLS_FOR_RATE: i64 = 3;

pub const THRESHOLDS: InsightThresholds = InsightThresholds {
    min_failures: 3,
    failure_rate: 0.2,
    min_repeats: 5,
    repeat_rate: 0.3,
};

pub fn insights(db: &Db, q: &SessionListQuery, pricing: &PricingTable) -> Result<SessionInsights> {
    let range = Range::query(q)?;
    let search = q.search.as_deref().unwrap_or("").trim();
    anyhow::ensure!(search.len() <= 512, "session search is too long");
    let cte = CANDIDATES_CTE;
    let order = "WHERE session_id<>'' ORDER BY last_ts DESC,provider,session_id LIMIT ?6";
    let (total, keys, tools) = {
        let conn = db.lock();
        let total: i64 = conn.query_row(
            &format!("{cte} SELECT COUNT(*) FROM filtered WHERE session_id<>'' "),
            params![range.from, range.to, q.provider, range.project, search],
            |r| r.get(0),
        )?;
        let mut stmt = conn.prepare(&format!(
            "{cte} SELECT provider,session_id FROM filtered {order}"
        ))?;
        let keys = stmt
            .query_map(
                params![
                    range.from,
                    range.to,
                    q.provider,
                    range.project,
                    search,
                    MAX_SESSIONS
                ],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        // Tool names are metadata kept in the index; no message text is read.
        let mut stmt = conn.prepare(&format!(
            "{cte}, capped AS (SELECT provider,session_id FROM filtered {order})
            SELECT r.tool_name,SUM(r.is_call),SUM(r.is_error),COUNT(DISTINCT r.provider||char(0)||r.session_id)
            FROM session_message_refs r JOIN capped c ON c.provider=r.provider AND c.session_id=r.session_id
            WHERE r.tool_name IS NOT NULL AND r.tool_name<>'' AND (?1 IS NULL OR r.ts>=?1) AND (?2 IS NULL OR r.ts<?2)
            GROUP BY r.tool_name HAVING SUM(r.is_call)>0 ORDER BY 2 DESC,1 LIMIT ?7"
        ))?;
        let tools = stmt
            .query_map(
                params![
                    range.from,
                    range.to,
                    q.provider,
                    range.project,
                    search,
                    MAX_SESSIONS,
                    TOOL_LIMIT
                ],
                |r| {
                    let calls: i64 = r.get(1)?;
                    let failures: i64 = r.get(2)?;
                    Ok(ToolStat {
                        tool: r.get(0)?,
                        calls,
                        failures,
                        failure_rate: ratio(failures, calls).unwrap_or(0.0),
                        sessions: r.get(3)?,
                    })
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        (total, keys, tools)
    };
    let mut rows = Vec::with_capacity(keys.len());
    for (provider, session) in keys {
        let s = summary(db, &provider, &session, pricing, false, &range)?;
        rows.push(InsightSession {
            cost_usd: s.usage.totals.estimated_cost_usd,
            failure_rate: ratio(s.tool_failures, s.tool_calls),
            repeat_rate: ratio(s.repeated_tool_calls, s.tool_calls),
            flags: flags(&s),
            provider: s.usage.provider,
            session_id: s.usage.session_id,
            title: s.title,
            project: s.usage.project,
            last_ts: s.usage.last_ts,
            total_tokens: s.usage.totals.total_tokens,
            active_duration_ms: s.active_duration_ms,
            user_turns: s.user_turns,
            tool_calls: s.tool_calls,
            tool_failures: s.tool_failures,
            repeated_tool_calls: s.repeated_tool_calls,
        });
    }
    Ok(build(rows, tools, total))
}

fn flags(s: &SessionSummary) -> Vec<String> {
    let t = THRESHOLDS;
    let mut out = Vec::new();
    if s.tool_failures >= t.min_failures
        && ratio(s.tool_failures, s.tool_calls).is_some_and(|r| r >= t.failure_rate)
    {
        out.push("failures".into());
    }
    if s.repeated_tool_calls >= t.min_repeats
        && ratio(s.repeated_tool_calls, s.tool_calls).is_some_and(|r| r >= t.repeat_rate)
    {
        out.push("repeats".into());
    }
    out
}

fn ratio(n: i64, d: i64) -> Option<f64> {
    (d > 0).then(|| n as f64 / d as f64)
}

fn top<F: Fn(&InsightSession) -> Option<f64>>(
    rows: &[InsightSession],
    key: F,
) -> Vec<InsightSession> {
    let mut list = rows
        .iter()
        .filter_map(|r| key(r).map(|k| (k, r)))
        .collect::<Vec<_>>();
    list.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(Ordering::Equal)
            .then_with(|| a.1.provider.cmp(&b.1.provider))
            .then_with(|| a.1.session_id.cmp(&b.1.session_id))
    });
    list.into_iter()
        .take(TOP_N)
        .map(|(_, r)| r.clone())
        .collect()
}

/// Pure aggregation over already-summarised sessions.
pub(super) fn build(
    rows: Vec<InsightSession>,
    tools: Vec<ToolStat>,
    total: i64,
) -> SessionInsights {
    let costs = rows.iter().filter_map(|r| r.cost_usd).collect::<Vec<_>>();
    let durations = rows
        .iter()
        .filter_map(|r| r.active_duration_ms.map(|v| v as f64))
        .collect::<Vec<_>>();
    let turns = rows.iter().map(|r| r.user_turns as f64).collect::<Vec<_>>();
    let calls = rows.iter().map(|r| r.tool_calls).sum::<i64>();
    let failures = rows.iter().map(|r| r.tool_failures).sum::<i64>();
    let repeats = rows.iter().map(|r| r.repeated_tool_calls).sum::<i64>();
    let kpis = InsightKpis {
        sessions: rows.len() as i64,
        priced_sessions: costs.len() as i64,
        median_cost_usd: percentile(&costs, 0.5),
        median_active_ms: percentile(&durations, 0.5),
        median_turns: percentile(&turns, 0.5),
        tool_calls: calls,
        tool_failures: failures,
        repeated_tool_calls: repeats,
        failure_rate: ratio(failures, calls),
        repeat_rate: ratio(repeats, calls),
    };
    SessionInsights {
        total_sessions: total,
        truncated: total > rows.len() as i64,
        kpis,
        cost_histogram: log_histogram(&costs),
        duration_histogram: log_histogram(&durations),
        top_cost: top(&rows, |r| r.cost_usd.filter(|c| *c > 0.0)),
        top_duration: top(&rows, |r| {
            r.active_duration_ms.filter(|d| *d > 0).map(|d| d as f64)
        }),
        top_failures: top(&rows, |r| {
            r.failure_rate
                .filter(|_| r.tool_failures > 0 && r.tool_calls >= MIN_CALLS_FOR_RATE)
        }),
        top_repeats: top(&rows, |r| {
            (r.repeated_tool_calls > 0).then_some(r.repeated_tool_calls as f64)
        }),
        points: rows,
        tools,
        thresholds: THRESHOLDS,
    }
}

/// Linear-interpolated percentile (`p` in 0..=1); `None` for an empty sample.
pub(super) fn percentile(values: &[f64], p: f64) -> Option<f64> {
    let mut sorted = values
        .iter()
        .copied()
        .filter(|v| v.is_finite())
        .collect::<Vec<_>>();
    if sorted.is_empty() {
        return None;
    }
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    let pos = p.clamp(0.0, 1.0) * (sorted.len() - 1) as f64;
    let (lo, hi) = (pos.floor() as usize, pos.ceil() as usize);
    Some(sorted[lo] + (sorted[hi] - sorted[lo]) * (pos - lo as f64))
}

/// Bins span `10^(k/3)`..`10^((k+1)/3)`, so they are uniform on a log axis.
pub(super) fn log_histogram(values: &[f64]) -> Histogram {
    let finite = values
        .iter()
        .copied()
        .filter(|v| v.is_finite() && *v >= 0.0)
        .collect::<Vec<_>>();
    let positive = finite
        .iter()
        .copied()
        .filter(|v| *v > 0.0)
        .collect::<Vec<_>>();
    let mut hist = Histogram {
        zero_count: (finite.len() - positive.len()) as i64,
        sample: finite.len() as i64,
        median: percentile(&finite, 0.5),
        p90: percentile(&finite, 0.9),
        bins: Vec::new(),
    };
    if positive.is_empty() {
        return hist;
    }
    let index = |v: f64| (v.log10() * BINS_PER_DECADE).floor() as i64;
    let min = positive.iter().copied().fold(f64::INFINITY, f64::min);
    let max = positive.iter().copied().fold(0.0, f64::max);
    let hi = index(max);
    let lo = index(min).max(hi - (MAX_BINS - 1));
    let edge = |k: i64| 10f64.powf(k as f64 / BINS_PER_DECADE);
    hist.bins = (lo..=hi)
        .map(|k| HistogramBin {
            from: edge(k),
            to: edge(k + 1),
            count: 0,
        })
        .collect();
    for v in positive {
        let i = (index(v).clamp(lo, hi) - lo) as usize;
        hist.bins[i].count += 1;
    }
    hist
}
