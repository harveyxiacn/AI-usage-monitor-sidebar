//! `quota_samples` writes and reads. [BACKEND]

use super::{now_ms, Db};
use crate::model::{split_key, QuotaHistoryQuery, QuotaSample, QuotaWindow, WindowKind};
use anyhow::Result;
use chrono::TimeZone;
use rusqlite::OptionalExtension;

/// Minimum spacing between two identical samples for the same window.
const MIN_SAMPLE_GAP_MS: i64 = 5 * 60 * 1000;

/// Store percentage and cycle/plan changes immediately, plus one unchanged
/// heartbeat at least every 5 minutes for each provider/kind/scope.
/// Returns `true` when a row was written.
///
/// `provider` is a registry key: `"claude"` is the primary account (stored
/// with `account = ''`), `"claude@work"` the extra account `work`. Every
/// function in this module takes the key and splits it, so the account is
/// part of each series' identity (throttling, forecasts, thinning).
pub fn insert_quota_sample(
    db: &Db,
    provider: &str,
    plan: Option<&str>,
    window: &QuotaWindow,
    now: i64,
) -> Result<bool> {
    let (provider, account) = split_key(provider);
    let conn = db.lock();
    let scope = window.scope.clone();
    let kind = window.kind.as_str();
    let resets_at = window
        .resets_at
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.timestamp_millis());
    let mut stmt = conn.prepare(
        "SELECT used_percent, ts, resets_at, plan FROM quota_samples
         WHERE provider = ?1 AND account = ?4 AND kind = ?2
           AND ((scope IS NULL AND ?3 IS NULL) OR scope = ?3)
         ORDER BY ts DESC, id DESC LIMIT 1",
    )?;
    let last: Option<(f64, i64, Option<i64>, Option<String>)> = stmt
        .query_row(rusqlite::params![provider, kind, scope, account], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .optional()?;
    if let Some((pct, ts, previous_reset, previous_plan)) = last {
        let unchanged = (pct - window.used_percent).abs() < f64::EPSILON
            && previous_reset == resets_at
            && previous_plan.as_deref() == plan;
        if unchanged && now - ts < MIN_SAMPLE_GAP_MS {
            return Ok(false);
        }
    }
    drop(stmt);
    conn.execute(
        "INSERT INTO quota_samples(provider, account, kind, scope, used_percent, resets_at, plan, ts)
         VALUES (?1,?8,?2,?3,?4,?5,?6,?7)",
        rusqlite::params![
            provider,
            kind,
            scope,
            window.used_percent,
            resets_at,
            plan,
            now,
            account
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

/// Samples of exactly one window (provider + kind + scope) taken at or after
/// `since` (unix ms), oldest first — the input of the burn-rate forecast.
pub fn window_samples(
    db: &Db,
    provider: &str,
    kind: WindowKind,
    scope: Option<&str>,
    since: i64,
) -> Result<Vec<crate::commands::forecast::Sample>> {
    let (provider, account) = split_key(provider);
    let conn = db.lock();
    let mut stmt = conn.prepare(
        "SELECT ts, used_percent, resets_at FROM quota_samples
         WHERE provider = ?1 AND account = ?5 AND kind = ?2
           AND ((scope IS NULL AND ?3 IS NULL) OR scope = ?3)
           AND ts >= ?4
         ORDER BY ts, id",
    )?;
    let rows = stmt.query_map(
        rusqlite::params![provider, kind.as_str(), scope, since, account],
        |r| {
            Ok(crate::commands::forecast::Sample {
                ts_ms: r.get(0)?,
                used_percent: r.get(1)?,
                resets_at_ms: r.get(2)?,
            })
        },
    )?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// One window whose samples `window_samples_many` should load.
pub struct WindowQuery<'a> {
    /// registry key (`claude`, `claude@work`)
    pub provider: &'a str,
    pub kind: WindowKind,
    pub scope: Option<&'a str>,
    pub since: i64,
}

/// `window_samples` for several windows under one lock acquisition and one
/// prepared statement; the result has one entry per query, in order.
pub fn window_samples_many(
    db: &Db,
    queries: &[WindowQuery<'_>],
) -> Result<Vec<Vec<crate::commands::forecast::Sample>>> {
    let conn = db.lock();
    let mut stmt = conn.prepare_cached(
        "SELECT ts, used_percent, resets_at FROM quota_samples
         WHERE provider = ?1 AND account = ?5 AND kind = ?2
           AND ((scope IS NULL AND ?3 IS NULL) OR scope = ?3)
           AND ts >= ?4
         ORDER BY ts, id",
    )?;
    let mut out = Vec::with_capacity(queries.len());
    for q in queries {
        let (provider, account) = split_key(q.provider);
        let rows = stmt.query_map(
            rusqlite::params![provider, q.kind.as_str(), q.scope, q.since, account],
            |r| {
                Ok(crate::commands::forecast::Sample {
                    ts_ms: r.get(0)?,
                    used_percent: r.get(1)?,
                    resets_at_ms: r.get(2)?,
                })
            },
        )?;
        out.push(rows.collect::<rusqlite::Result<Vec<_>>>()?);
    }
    Ok(out)
}

/// Read stored samples in `[from, to)`, oldest first.
pub fn query_quota_history(db: &Db, q: &QuotaHistoryQuery) -> Result<Vec<QuotaSample>> {
    let (from, to) = super::usage::query_range(&q.from, &q.to)?;
    let conn = db.lock();
    // `account`: NULL = every account, '' = the primary one, else that account.
    let sql = "SELECT provider, kind, scope, used_percent, resets_at, plan, ts, account
               FROM quota_samples
               WHERE ts >= ?1 AND ts < ?2 AND (?3 IS NULL OR provider = ?3)
                 AND (?4 IS NULL OR account = ?4)
               ORDER BY ts, id";
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(rusqlite::params![from, to, q.provider, q.account], |r| {
        let kind: String = r.get(1)?;
        let resets_at: Option<i64> = r.get(4)?;
        let account: String = r.get(7)?;
        Ok(QuotaSample {
            provider: r.get(0)?,
            account: Some(account).filter(|a| !a.is_empty()),
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

/// Samples older than this are thinned to one row per hour.
const DOWNSAMPLE_AFTER_MS: i64 = 14 * 24 * 3_600_000;
const HOUR_MS: i64 = 3_600_000;

/// What one maintenance pass removed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MaintenanceStats {
    pub expired: usize,
    pub thinned: usize,
}

/// Daily housekeeping of `quota_samples`: delete rows older than
/// `retention_days` (0 = keep forever), thin everything older than 14 days to
/// the highest-`used_percent` row per (provider, account, kind, scope, hour) so cycle
/// peaks survive, then checkpoint the WAL and let SQLite refresh its planner
/// statistics. `usage_events` are never touched. The database's auto_vacuum
/// mode is not changed; only an already-incremental database is vacuumed.
pub fn maintain_quota_samples(db: &Db, now: i64, retention_days: u32) -> Result<MaintenanceStats> {
    let conn = db.lock();
    let mut stats = MaintenanceStats::default();
    if retention_days > 0 {
        let cutoff = now.saturating_sub(retention_days as i64 * 24 * HOUR_MS);
        stats.expired = conn.execute("DELETE FROM quota_samples WHERE ts < ?1", [cutoff])?;
    }
    stats.thinned = conn.execute(
        "DELETE FROM quota_samples WHERE ts < ?1 AND id NOT IN (
           SELECT id FROM (
             SELECT id, ROW_NUMBER() OVER (
                      PARTITION BY provider, account, kind, scope, ts / ?2
                      ORDER BY used_percent DESC, id DESC) AS rn
             FROM quota_samples WHERE ts < ?1)
           WHERE rn = 1)",
        rusqlite::params![now.saturating_sub(DOWNSAMPLE_AFTER_MS), HOUR_MS],
    )?;
    if stats.expired + stats.thinned > 0 {
        let mode: i64 = conn.query_row("PRAGMA auto_vacuum", [], |r| r.get(0))?;
        if mode == 2 {
            conn.execute_batch("PRAGMA incremental_vacuum;")?;
        }
    }
    // The checkpoint returns a (busy, log, checkpointed) row; a busy reader
    // only means it could not truncate this time.
    let _ = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
    conn.execute_batch("PRAGMA optimize;")?;
    Ok(stats)
}

fn ms_to_rfc3339(ms: i64) -> Option<String> {
    chrono::Utc
        .timestamp_millis_opt(ms)
        .single()
        .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true))
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
            forecast: None,
        }
    }

    #[test]
    fn samples_are_throttled_per_provider_kind_and_scope() {
        let db = Db::open_in_memory().unwrap();
        let t0 = 1_789_430_400_000i64; // 2026-09-15T00:00:00Z
        let w = window(WindowKind::FiveHour, 10.0, None);

        assert!(insert_quota_sample(&db, "claude", Some("Claude Max 5x"), &w, t0).unwrap());
        // same percent, 1 minute later → dropped
        assert!(
            !insert_quota_sample(&db, "claude", Some("Claude Max 5x"), &w, t0 + 60_000).unwrap()
        );
        // same percent, 6 minutes later → kept
        assert!(
            insert_quota_sample(&db, "claude", Some("Claude Max 5x"), &w, t0 + 6 * 60_000).unwrap()
        );
        // changed percent right away → kept
        let changed = window(WindowKind::FiveHour, 11.0, None);
        assert!(insert_quota_sample(
            &db,
            "claude",
            Some("Claude Max 5x"),
            &changed,
            t0 + 6 * 60_000 + 1
        )
        .unwrap());
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
                account: None,
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
                account: None,
            },
        )
        .unwrap();
        assert_eq!(claude_only.len(), 4);
        assert!(claude_only
            .iter()
            .any(|s| s.scope.as_deref() == Some("Fable")));
    }

    #[test]
    fn window_samples_are_scoped_to_one_window_and_start_at_since() {
        let db = Db::open_in_memory().unwrap();
        let t0 = 1_789_430_400_000i64;
        for (i, ts) in [t0, t0 + 6 * 60_000, t0 + 12 * 60_000].iter().enumerate() {
            let w = window(WindowKind::FiveHour, i as f64, None);
            insert_quota_sample(&db, "claude", None, &w, *ts).unwrap();
        }
        // a different kind, a different scope and a different provider
        insert_quota_sample(
            &db,
            "claude",
            None,
            &window(WindowKind::SevenDay, 9.0, None),
            t0,
        )
        .unwrap();
        insert_quota_sample(
            &db,
            "claude",
            None,
            &window(WindowKind::FiveHour, 9.0, Some("Fable")),
            t0,
        )
        .unwrap();
        insert_quota_sample(
            &db,
            "codex",
            None,
            &window(WindowKind::FiveHour, 9.0, None),
            t0,
        )
        .unwrap();

        let all = window_samples(&db, "claude", WindowKind::FiveHour, None, 0).unwrap();
        assert_eq!(all.len(), 3, "only the non-scoped claude 5-hour window");
        assert_eq!(all[0].ts_ms, t0);
        assert_eq!(all[0].used_percent, 0.0);
        assert_eq!(
            all[0].resets_at_ms,
            Some(
                chrono::DateTime::parse_from_rfc3339("2026-09-18T20:00:00Z")
                    .unwrap()
                    .timestamp_millis()
            )
        );
        assert_eq!(all[2].used_percent, 2.0, "oldest first");

        let recent =
            window_samples(&db, "claude", WindowKind::FiveHour, None, t0 + 6 * 60_000).unwrap();
        assert_eq!(recent.len(), 2, "`since` is inclusive");

        let scoped = window_samples(&db, "claude", WindowKind::FiveHour, Some("Fable"), 0).unwrap();
        assert_eq!(scoped.len(), 1);
    }

    #[test]
    fn quota_history_range_includes_start_and_excludes_end() {
        let db = Db::open_in_memory().unwrap();
        let start = 1_789_430_400_000;
        let end = start + 86_400_000;
        for (index, ts) in [start - 1, start, end - 1, end].into_iter().enumerate() {
            insert_quota_sample(
                &db,
                "codex",
                None,
                &window(WindowKind::FiveHour, index as f64, None),
                ts,
            )
            .unwrap();
        }
        let samples = query_quota_history(
            &db,
            &QuotaHistoryQuery {
                from: "2026-09-15T00:00:00Z".into(),
                to: "2026-09-16T00:00:00Z".into(),
                provider: Some("codex".into()),
                account: None,
            },
        )
        .unwrap();
        assert_eq!(samples.len(), 2);
        assert_eq!(samples[0].used_percent, 1.0);
        assert_eq!(samples[1].used_percent, 2.0);
    }

    fn september_history(db: &Db) -> Vec<QuotaSample> {
        query_quota_history(
            db,
            &QuotaHistoryQuery {
                from: "2026-09-01T00:00:00Z".into(),
                to: "2026-09-30T00:00:00Z".into(),
                provider: None,
                account: None,
            },
        )
        .unwrap()
    }

    #[test]
    fn unchanged_heartbeat_keeps_exact_five_minute_boundary() {
        let db = Db::open_in_memory().unwrap();
        let t0 = 1_789_430_400_000;
        let w = window(WindowKind::FiveHour, 10.0, None);
        assert!(insert_quota_sample(&db, "codex", Some("Pro"), &w, t0).unwrap());
        assert!(!insert_quota_sample(&db, "codex", Some("Pro"), &w, t0 + 299_999).unwrap());
        assert!(insert_quota_sample(&db, "codex", Some("Pro"), &w, t0 + 300_000).unwrap());
        let history = september_history(&db);
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].ts, "2026-09-15T00:00:00Z");
        assert_eq!(history[1].ts, "2026-09-15T00:05:00Z");
    }

    #[test]
    fn reset_metadata_changes_are_recorded_even_at_equal_percentages() {
        let db = Db::open_in_memory().unwrap();
        let t0 = 1_789_430_400_000;
        let mut w = window(WindowKind::FiveHour, 10.0, None);
        assert!(insert_quota_sample(&db, "codex", Some("Pro"), &w, t0).unwrap());
        w.resets_at = Some("2026-09-19T04:00:00+08:00".into());
        assert!(
            !insert_quota_sample(&db, "codex", Some("Pro"), &w, t0 + 1).unwrap(),
            "equivalent instant"
        );
        w.resets_at = Some("2026-09-19T20:00:00.123Z".into());
        assert!(insert_quota_sample(&db, "codex", Some("Pro"), &w, t0 + 2).unwrap());
        w.resets_at = None;
        assert!(insert_quota_sample(&db, "codex", Some("Pro"), &w, t0 + 3).unwrap());
        w.resets_at = Some("invalid".into());
        assert!(
            !insert_quota_sample(&db, "codex", Some("Pro"), &w, t0 + 4).unwrap(),
            "invalid deadline is still unknown"
        );
        w.resets_at = Some("2026-09-20T20:00:00Z".into());
        assert!(insert_quota_sample(&db, "codex", Some("Pro"), &w, t0 + 5).unwrap());
        let history = september_history(&db);
        assert_eq!(
            history.iter().map(|s| s.used_percent).collect::<Vec<_>>(),
            [10.0; 4]
        );
        assert_eq!(
            history
                .iter()
                .map(|s| s.resets_at.as_deref())
                .collect::<Vec<_>>(),
            [
                Some("2026-09-18T20:00:00Z"),
                Some("2026-09-19T20:00:00.123Z"),
                None,
                Some("2026-09-20T20:00:00Z")
            ]
        );
    }

    #[test]
    fn plan_metadata_changes_are_recorded_in_both_directions() {
        let db = Db::open_in_memory().unwrap();
        let t0 = 1_789_430_400_000;
        let w = window(WindowKind::FiveHour, 10.0, None);
        for (i, plan) in [None, Some("Pro"), Some("Plus"), None]
            .into_iter()
            .enumerate()
        {
            assert!(insert_quota_sample(&db, "codex", plan, &w, t0 + i as i64).unwrap());
        }
        assert!(!insert_quota_sample(&db, "codex", None, &w, t0 + 4).unwrap());
        let history = september_history(&db);
        assert_eq!(
            history
                .iter()
                .map(|s| s.plan.as_deref())
                .collect::<Vec<_>>(),
            [None, Some("Pro"), Some("Plus"), None]
        );
    }

    #[test]
    fn same_timestamp_observations_use_latest_id_and_read_in_insertion_order() {
        let db = Db::open_in_memory().unwrap();
        let t0 = 1_789_430_400_000;
        let w = window(WindowKind::FiveHour, 10.0, None);
        assert!(insert_quota_sample(&db, "codex", Some("Pro"), &w, t0).unwrap());
        let changed = window(WindowKind::FiveHour, 20.0, None);
        assert!(insert_quota_sample(&db, "codex", Some("Plus"), &changed, t0).unwrap());
        assert!(
            !insert_quota_sample(&db, "codex", Some("Plus"), &changed, t0).unwrap(),
            "last row at a tied timestamp is authoritative"
        );
        assert!(insert_quota_sample(&db, "codex", Some("Plus"), &w, t0 + 1).unwrap());
        let history = september_history(&db);
        assert_eq!(
            history.iter().map(|s| s.used_percent).collect::<Vec<_>>(),
            [10.0, 20.0, 10.0]
        );
        assert_eq!(
            history.iter().map(|s| s.ts.as_str()).collect::<Vec<_>>(),
            [
                "2026-09-15T00:00:00Z",
                "2026-09-15T00:00:00Z",
                "2026-09-15T00:00:00.001Z"
            ]
        );
        let forecast = window_samples(&db, "codex", WindowKind::FiveHour, None, t0).unwrap();
        assert_eq!(
            forecast
                .iter()
                .map(|s| (s.ts_ms, s.used_percent))
                .collect::<Vec<_>>(),
            [(t0, 10.0), (t0, 20.0), (t0 + 1, 10.0)]
        );
    }

    #[test]
    fn empty_scope_is_distinct_from_null_and_named_scopes() {
        let db = Db::open_in_memory().unwrap();
        let t0 = 1_789_430_400_000;
        for scope in [None, Some(""), Some("Fable")] {
            let w = window(WindowKind::SevenDay, 10.0, scope);
            assert!(insert_quota_sample(&db, "claude", None, &w, t0).unwrap());
            assert!(!insert_quota_sample(&db, "claude", None, &w, t0 + 1).unwrap());
            assert_eq!(
                window_samples(&db, "claude", WindowKind::SevenDay, scope, t0)
                    .unwrap()
                    .len(),
                1
            );
        }
        let history = september_history(&db);
        assert_eq!(
            history
                .iter()
                .map(|s| s.scope.as_deref())
                .collect::<Vec<_>>(),
            [None, Some(""), Some("Fable")]
        );
    }

    fn raw_sample(db: &Db, provider: &str, scope: Option<&str>, pct: f64, ts: i64) {
        db.lock()
            .execute(
                "INSERT INTO quota_samples(provider, kind, scope, used_percent, ts)
                 VALUES (?1,'five_hour',?2,?3,?4)",
                rusqlite::params![provider, scope, pct, ts],
            )
            .unwrap();
    }

    fn count(db: &Db) -> i64 {
        db.lock()
            .query_row("SELECT COUNT(*) FROM quota_samples", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn retention_deletes_old_samples_and_zero_keeps_everything() {
        let db = Db::open_in_memory().unwrap();
        let now = 1_789_430_400_000i64;
        let day = 24 * HOUR_MS;
        raw_sample(&db, "claude", None, 10.0, now - 400 * day);
        raw_sample(&db, "claude", None, 20.0, now - 30 * day);
        raw_sample(&db, "claude", None, 30.0, now - day);

        let kept = maintain_quota_samples(&db, now, 0).unwrap();
        assert_eq!(kept.expired, 0);
        assert_eq!(count(&db), 3);

        let stats = maintain_quota_samples(&db, now, 365).unwrap();
        assert_eq!(stats.expired, 1);
        assert_eq!(count(&db), 2);
    }

    #[test]
    fn old_samples_are_thinned_to_the_hourly_peak_per_series() {
        let db = Db::open_in_memory().unwrap();
        let now = 1_789_430_400_000i64;
        let hour0 = (now / HOUR_MS - 30 * 24) * HOUR_MS; // 30 days back, hour aligned
                                                         // One hour of rising then reset samples, plus a scoped series.
        for (i, pct) in [5.0, 40.0, 95.0, 2.0].iter().enumerate() {
            raw_sample(&db, "claude", None, *pct, hour0 + i as i64 * 600_000);
        }
        raw_sample(&db, "claude", Some("Fable"), 7.0, hour0 + 1_000);
        raw_sample(&db, "claude", Some("Fable"), 8.0, hour0 + 2_000);
        // The next hour keeps its own peak; recent rows are untouched.
        raw_sample(&db, "claude", None, 50.0, hour0 + HOUR_MS);
        raw_sample(&db, "claude", None, 1.0, now - HOUR_MS);
        raw_sample(&db, "claude", None, 2.0, now - HOUR_MS + 1);

        let stats = maintain_quota_samples(&db, now, 0).unwrap();
        assert_eq!(stats.thinned, 4);
        assert_eq!(count(&db), 5);
        let peak: f64 = db
            .lock()
            .query_row(
                "SELECT used_percent FROM quota_samples WHERE scope IS NULL AND ts < ?1 AND ts >= ?2",
                [hour0 + HOUR_MS, hour0],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(peak, 95.0);
        // Idempotent.
        assert_eq!(maintain_quota_samples(&db, now, 0).unwrap().thinned, 0);
        // The forecast window query still sees the recent rows.
        let recent = window_samples(
            &db,
            "claude",
            WindowKind::FiveHour,
            None,
            now - 24 * HOUR_MS,
        )
        .unwrap();
        assert_eq!(recent.len(), 2);
    }

    #[test]
    fn batched_window_samples_match_the_single_window_query() {
        let db = Db::open_in_memory().unwrap();
        let now = 1_789_430_400_000i64;
        raw_sample(&db, "claude", None, 10.0, now - 1_000);
        raw_sample(&db, "claude", Some("Fable"), 20.0, now - 500);
        raw_sample(&db, "codex", None, 30.0, now - 100);
        let queries = [
            WindowQuery {
                provider: "claude",
                kind: WindowKind::FiveHour,
                scope: None,
                since: 0,
            },
            WindowQuery {
                provider: "claude",
                kind: WindowKind::FiveHour,
                scope: Some("Fable"),
                since: 0,
            },
            WindowQuery {
                provider: "codex",
                kind: WindowKind::FiveHour,
                scope: None,
                since: now,
            },
        ];
        let many = window_samples_many(&db, &queries).unwrap();
        assert_eq!(many.len(), 3);
        assert_eq!(many[0].len(), 1);
        assert_eq!(many[0][0].used_percent, 10.0);
        assert_eq!(many[1][0].used_percent, 20.0);
        assert!(many[2].is_empty(), "`since` still filters");
    }

    #[test]
    fn each_account_is_its_own_series_for_throttling_forecast_input_and_history() {
        let db = Db::open_in_memory().unwrap();
        let t0 = 1_789_430_400_000i64;
        let w = window(WindowKind::FiveHour, 10.0, None);
        assert!(insert_quota_sample(&db, "claude", None, &w, t0).unwrap());
        // identical value one minute later would be throttled for the same
        // series, but the other account is a different one
        assert!(insert_quota_sample(&db, "claude@work", None, &w, t0 + 60_000).unwrap());
        assert!(!insert_quota_sample(&db, "claude@work", None, &w, t0 + 90_000).unwrap());
        assert!(!insert_quota_sample(&db, "claude", None, &w, t0 + 90_000).unwrap());
        // another extra account of the same provider
        assert!(insert_quota_sample(&db, "claude@side", None, &w, t0 + 90_000).unwrap());

        let primary = window_samples(&db, "claude", WindowKind::FiveHour, None, 0).unwrap();
        let work = window_samples(&db, "claude@work", WindowKind::FiveHour, None, 0).unwrap();
        assert_eq!((primary.len(), work.len()), (1, 1));
        assert_eq!(primary[0].ts_ms, t0);
        assert_eq!(work[0].ts_ms, t0 + 60_000);
        let many = window_samples_many(
            &db,
            &[
                WindowQuery {
                    provider: "claude@work",
                    kind: WindowKind::FiveHour,
                    scope: None,
                    since: 0,
                },
                WindowQuery {
                    provider: "claude",
                    kind: WindowKind::FiveHour,
                    scope: None,
                    since: 0,
                },
            ],
        )
        .unwrap();
        assert_eq!(many[0][0].ts_ms, t0 + 60_000);
        assert_eq!(many[1][0].ts_ms, t0);

        let q = |provider: Option<&str>, account: Option<&str>| {
            query_quota_history(
                &db,
                &QuotaHistoryQuery {
                    from: "2026-09-01T00:00:00Z".into(),
                    to: "2026-09-30T00:00:00Z".into(),
                    provider: provider.map(str::to_string),
                    account: account.map(str::to_string),
                },
            )
            .unwrap()
        };
        assert_eq!(q(None, None).len(), 3, "no account filter = every account");
        let only_primary = q(Some("claude"), Some(""));
        assert_eq!(only_primary.len(), 1);
        assert_eq!(only_primary[0].account, None);
        let only_work = q(Some("claude"), Some("work"));
        assert_eq!(only_work.len(), 1);
        assert_eq!(only_work[0].account.as_deref(), Some("work"));
    }

    #[test]
    fn thinning_keeps_one_peak_per_account_and_hour() {
        let db = Db::open_in_memory().unwrap();
        let old = 1_789_430_400_000i64;
        let now = old + 60 * 24 * 3_600_000; // the samples are > 14 days old
        for key in ["claude", "claude@work"] {
            for (i, pct) in [10.0, 30.0, 20.0].into_iter().enumerate() {
                insert_quota_sample(
                    &db,
                    key,
                    None,
                    &window(WindowKind::FiveHour, pct, None),
                    old + i as i64 * 60_000,
                )
                .unwrap();
            }
        }
        let stats = maintain_quota_samples(&db, now, 0).unwrap();
        assert_eq!(stats.thinned, 4, "two of three rows go, per account");
        for key in ["claude", "claude@work"] {
            let left = window_samples(&db, key, WindowKind::FiveHour, None, 0).unwrap();
            assert_eq!(left.len(), 1);
            assert_eq!(left[0].used_percent, 30.0, "the peak survives for {key}");
        }
    }
}
