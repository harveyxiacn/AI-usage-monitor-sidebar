//! `quota_samples` writes and reads. [BACKEND]

use super::{now_ms, Db};
use crate::model::{QuotaHistoryQuery, QuotaSample, QuotaWindow, WindowKind};
use anyhow::Result;
use chrono::TimeZone;

/// Minimum spacing between two identical samples for the same window.
const MIN_SAMPLE_GAP_MS: i64 = 5 * 60 * 1000;

/// Store one sample for `window` — but only if the percentage changed or the
/// previous sample for that provider/kind/scope is at least 5 minutes old.
/// Returns `true` when a row was written.
pub fn insert_quota_sample(
    db: &Db,
    provider: &str,
    plan: Option<&str>,
    window: &QuotaWindow,
    now: i64,
) -> Result<bool> {
    let conn = db.lock();
    let scope = window.scope.clone();
    let kind = window.kind.as_str();
    let mut stmt = conn.prepare(
        "SELECT used_percent, ts FROM quota_samples
         WHERE provider = ?1 AND kind = ?2 AND ((scope IS NULL AND ?3 IS NULL) OR scope = ?3)
         ORDER BY ts DESC LIMIT 1",
    )?;
    let last: Option<(f64, i64)> = stmt
        .query_row(rusqlite::params![provider, kind, scope], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .ok();
    if let Some((pct, ts)) = last {
        let unchanged = (pct - window.used_percent).abs() < f64::EPSILON;
        if unchanged && now - ts < MIN_SAMPLE_GAP_MS {
            return Ok(false);
        }
    }
    let resets_at = window
        .resets_at
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.timestamp_millis());
    drop(stmt);
    conn.execute(
        "INSERT INTO quota_samples(provider, kind, scope, used_percent, resets_at, plan, ts)
         VALUES (?1,?2,?3,?4,?5,?6,?7)",
        rusqlite::params![
            provider,
            kind,
            scope,
            window.used_percent,
            resets_at,
            plan,
            now
        ],
    )?;
    Ok(true)
}

/// Convenience wrapper used by the scheduler: sample every window of a quota.
pub fn insert_quota_samples(
    db: &Db,
    provider: &str,
    plan: Option<&str>,
    windows: &[QuotaWindow],
) -> Result<u64> {
    let now = now_ms();
    let mut n = 0;
    for w in windows {
        if insert_quota_sample(db, provider, plan, w, now)? {
            n += 1;
        }
    }
    Ok(n)
}

/// Read stored samples in `[from, to]`, oldest first.
pub fn query_quota_history(db: &Db, q: &QuotaHistoryQuery) -> Result<Vec<QuotaSample>> {
    let (from, to) = super::usage::query_range(&q.from, &q.to)?;
    let conn = db.lock();
    let sql = "SELECT provider, kind, scope, used_percent, resets_at, plan, ts
               FROM quota_samples
               WHERE ts >= ?1 AND ts <= ?2 AND (?3 IS NULL OR provider = ?3)
               ORDER BY ts";
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(rusqlite::params![from, to, q.provider], |r| {
        let kind: String = r.get(1)?;
        let resets_at: Option<i64> = r.get(4)?;
        Ok(QuotaSample {
            provider: r.get(0)?,
            kind: WindowKind::parse(&kind),
            scope: r.get(2)?,
            used_percent: r.get(3)?,
            resets_at: resets_at.and_then(ms_to_rfc3339),
            plan: r.get(5)?,
            ts: ms_to_rfc3339(r.get(6)?).unwrap_or_default(),
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

fn ms_to_rfc3339(ms: i64) -> Option<String> {
    chrono::Utc
        .timestamp_millis_opt(ms)
        .single()
        .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::WindowKind;

    fn window(kind: WindowKind, percent: f64, scope: Option<&str>) -> QuotaWindow {
        QuotaWindow {
            kind,
            label: "w".into(),
            window_seconds: None,
            used_percent: percent,
            resets_at: Some("2026-09-18T20:00:00Z".into()),
            scope: scope.map(|s| s.to_string()),
            is_primary: false,
        }
    }

    #[test]
    fn samples_are_throttled_per_provider_kind_and_scope() {
        let db = Db::open_in_memory().unwrap();
        let t0 = 1_789_430_400_000i64; // 2026-09-15T00:00:00Z
        let w = window(WindowKind::FiveHour, 10.0, None);

        assert!(insert_quota_sample(&db, "claude", Some("Claude Max 5x"), &w, t0).unwrap());
        // same percent, 1 minute later → dropped
        assert!(!insert_quota_sample(&db, "claude", None, &w, t0 + 60_000).unwrap());
        // same percent, 6 minutes later → kept
        assert!(insert_quota_sample(&db, "claude", None, &w, t0 + 6 * 60_000).unwrap());
        // changed percent right away → kept
        let changed = window(WindowKind::FiveHour, 11.0, None);
        assert!(insert_quota_sample(&db, "claude", None, &changed, t0 + 6 * 60_000 + 1).unwrap());
        // a scoped window is tracked separately
        let scoped = window(WindowKind::FiveHour, 10.0, Some("Fable"));
        assert!(insert_quota_sample(&db, "claude", None, &scoped, t0 + 60_000).unwrap());
        // another provider too
        assert!(insert_quota_sample(&db, "codex", None, &w, t0 + 60_000).unwrap());

        let all = query_quota_history(
            &db,
            &QuotaHistoryQuery {
                from: "2026-09-01T00:00:00Z".into(),
                to: "2026-09-30T00:00:00Z".into(),
                provider: None,
            },
        )
        .unwrap();
        assert_eq!(all.len(), 5);
        assert_eq!(all[0].provider, "claude");
        assert_eq!(all[0].plan.as_deref(), Some("Claude Max 5x"));
        assert_eq!(all[0].kind, WindowKind::FiveHour);
        assert_eq!(all[0].resets_at.as_deref(), Some("2026-09-18T20:00:00Z"));
        assert_eq!(all[0].ts, "2026-09-15T00:00:00Z");

        let claude_only = query_quota_history(
            &db,
            &QuotaHistoryQuery {
                from: "2026-09-01T00:00:00Z".into(),
                to: "2026-09-30T00:00:00Z".into(),
                provider: Some("claude".into()),
            },
        )
        .unwrap();
        assert_eq!(claude_only.len(), 4);
        assert!(claude_only
            .iter()
            .any(|s| s.scope.as_deref() == Some("Fable")));
    }
}
