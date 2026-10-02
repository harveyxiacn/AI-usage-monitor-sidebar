//! Historical `usage.db` fixtures migrated by the current `Db::open`.

use crate::commands::store::{self, Db, SCHEMA_VERSION};
use crate::commands::test_support::tempdir;
use crate::model::{Bucket, HistoryQuery, PricingTable, QuotaHistoryQuery};
use rusqlite::Connection;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

struct Fixture {
    name: &'static str,
    sql: &'static str,
    schema: i64,
    usage_rows: i64,
    quota_rows: i64,
    ingest_rows: i64,
    sessions: bool,
}

const FIXTURES: &[Fixture] = &[
    Fixture {
        name: "v0.2.2",
        sql: include_str!("../../tests/fixtures/db/v0.2.2.sql"),
        schema: 1,
        usage_rows: 4,
        quota_rows: 5,
        ingest_rows: 2,
        sessions: false,
    },
    Fixture {
        name: "v0.3.0",
        sql: include_str!("../../tests/fixtures/db/v0.3.0.sql"),
        schema: 2,
        usage_rows: 5,
        quota_rows: 5,
        ingest_rows: 2,
        sessions: false,
    },
    Fixture {
        name: "v0.5.0",
        sql: include_str!("../../tests/fixtures/db/v0.5.0.sql"),
        schema: 2,
        usage_rows: 5,
        quota_rows: 5,
        ingest_rows: 2,
        sessions: true,
    },
    Fixture {
        name: "v0.6.0",
        sql: include_str!("../../tests/fixtures/db/v0.6.0.sql"),
        schema: 3,
        usage_rows: 5,
        quota_rows: 5,
        ingest_rows: 2,
        sessions: true,
    },
];

type Rows = Vec<Vec<(String, String)>>;

/// A fresh directory holding `usage.db` built from the fixture's SQL, exactly
/// as the old version would have left it.
fn write_fixture(f: &Fixture) -> PathBuf {
    let dir = tempdir();
    let conn = Connection::open(dir.join("usage.db")).expect("create fixture db");
    conn.execute_batch(f.sql)
        .unwrap_or_else(|e| panic!("fixture {} is not valid SQL: {e}", f.name));
    dir.join("usage.db")
}

fn cleanup(db_path: &Path) {
    if let Some(dir) = db_path.parent() {
        std::fs::remove_dir_all(dir).ok();
    }
}

fn raw(db_path: &Path) -> Connection {
    Connection::open(db_path).expect("open raw")
}

fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

fn schema_version(conn: &Connection) -> i64 {
    conn.query_row(
        "SELECT value FROM meta WHERE key='schema_version'",
        [],
        |r| r.get::<_, String>(0),
    )
    .unwrap()
    .parse()
    .unwrap()
}

fn names(conn: &Connection, sql: &str) -> Vec<String> {
    conn.prepare(sql)
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

/// `(table, sorted column names)` pairs, and the set of index names. The
/// evaluation table is created lazily by `evaluation.rs`, not by `Db::open`.
fn shape(conn: &Connection) -> (Vec<(String, Vec<String>)>, BTreeSet<String>) {
    let tables = names(
        conn,
        "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' AND name <> 'session_evaluations' ORDER BY name",
    )
    .into_iter()
    .map(|t| {
        let mut cols = names(conn, &format!("SELECT name FROM pragma_table_info('{t}')"));
        cols.sort();
        (t, cols)
    })
    .collect();
    let indexes = names(
        conn,
        "SELECT name FROM sqlite_master WHERE type='index' AND name NOT LIKE 'sqlite_%' AND name <> 'idx_evaluation_session'",
    )
    .into_iter()
    .collect();
    (tables, indexes)
}

/// Every row of `table` as `(column, debug-printed value)` lists.
fn dump(conn: &Connection, table: &str, order: &str) -> Rows {
    let mut stmt = conn
        .prepare(&format!("SELECT * FROM {table} ORDER BY {order}"))
        .unwrap();
    let cols: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
    let rows = stmt
        .query_map([], |r| {
            Ok(cols
                .iter()
                .enumerate()
                .map(|(i, n)| {
                    let v: rusqlite::types::Value = r.get(i).unwrap();
                    (n.clone(), format!("{v:?}"))
                })
                .collect::<Vec<_>>())
        })
        .unwrap()
        .map(Result::unwrap)
        .collect();
    rows
}

fn value<'a>(row: &'a [(String, String)], col: &str) -> Option<&'a str> {
    row.iter().find(|(n, _)| n == col).map(|(_, v)| v.as_str())
}

#[test]
fn the_fixtures_are_what_their_headers_claim() {
    for f in FIXTURES {
        let path = write_fixture(f);
        let conn = raw(&path);
        assert_eq!(schema_version(&conn), f.schema, "{}", f.name);
        assert_eq!(count(&conn, "usage_events"), f.usage_rows, "{}", f.name);
        assert_eq!(count(&conn, "quota_samples"), f.quota_rows, "{}", f.name);
        assert_eq!(count(&conn, "ingest_files"), f.ingest_rows, "{}", f.name);
        drop(conn);
        cleanup(&path);
    }
}

#[test]
fn every_released_schema_migrates_to_the_current_one_with_its_data() {
    for f in FIXTURES {
        let path = write_fixture(f);
        let before = {
            let c = raw(&path);
            (
                dump(&c, "usage_events", "id"),
                dump(&c, "quota_samples", "id"),
                if f.sessions {
                    dump(&c, "session_metadata", "session_id")
                } else {
                    vec![]
                },
            )
        };
        drop(Db::open(&path).unwrap_or_else(|e| panic!("{} did not migrate: {e:#}", f.name)));
        let conn = raw(&path);
        assert_eq!(schema_version(&conn), SCHEMA_VERSION, "{}", f.name);

        // usage_events: nothing lost, nothing altered (except the documented
        // child-attribution rewrite of the subagent row in pre-v0.5 data)
        let usage = dump(&conn, "usage_events", "id");
        assert_eq!(usage.len() as i64, f.usage_rows, "{}", f.name);
        for (old, new) in before.0.iter().zip(&usage) {
            for (col, v) in old {
                if col == "session_id" && value(old, "request_id").unwrap().contains("req-sub1") {
                    assert!(
                        value(new, col).unwrap().contains(":agent:abc"),
                        "{}: subagent row must be attributed to its child session",
                        f.name
                    );
                    continue;
                }
                assert_eq!(value(new, col), Some(v.as_str()), "{} usage.{col}", f.name);
            }
            if value(old, "reasoning_effort").is_none() {
                assert_eq!(value(new, "reasoning_effort"), Some("Null"), "{}", f.name);
            }
        }

        // quota_samples: preserved, and every row now has an account
        let quota = dump(&conn, "quota_samples", "id");
        assert_eq!(quota.len() as i64, f.quota_rows, "{}", f.name);
        for (old, new) in before.1.iter().zip(&quota) {
            for (col, v) in old {
                assert_eq!(value(new, col), Some(v.as_str()), "{} quota.{col}", f.name);
            }
            if value(old, "account").is_none() {
                assert_eq!(
                    value(new, "account"),
                    Some("Text(\"\")"),
                    "{}: a pre-v3 sample belongs to the primary account",
                    f.name
                );
            }
        }
        if f.schema == 3 {
            assert_eq!(value(&quota[2], "account"), Some("Text(\"work\")"));
        }

        // session tables of v0.5+ are untouched
        if f.sessions {
            assert_eq!(dump(&conn, "session_metadata", "session_id"), before.2);
            assert_eq!(count(&conn, "session_evaluations"), 1);
            assert_eq!(count(&conn, "session_aliases"), 1);
        }

        // v2 replays the logs once: bookkeeping is dropped only below v2
        let want_ingest = if f.schema < 2 { 0 } else { f.ingest_rows };
        assert_eq!(count(&conn, "ingest_files"), want_ingest, "{}", f.name);
        drop(conn);
        cleanup(&path);
    }
}

#[test]
fn an_upgraded_database_has_exactly_the_schema_of_a_fresh_one() {
    let fresh = Db::open_in_memory().unwrap();
    let (fresh_tables, fresh_indexes) = shape(&fresh.lock());
    for f in FIXTURES {
        let path = write_fixture(f);
        drop(Db::open(&path).unwrap());
        let (tables, indexes) = shape(&raw(&path));
        assert_eq!(tables, fresh_tables, "{}: tables/columns differ", f.name);
        assert_eq!(indexes, fresh_indexes, "{}: indexes differ", f.name);
        assert!(indexes.contains("idx_quota_account_ts"));
        assert!(!indexes.contains("idx_quota_ts"), "{}", f.name);
        cleanup(&path);
    }
}

#[test]
fn migrating_twice_changes_nothing() {
    for f in FIXTURES {
        let path = write_fixture(f);
        drop(Db::open(&path).unwrap());
        let snapshot = |p: &Path| {
            let c = raw(p);
            let all: Vec<Rows> = [
                ("usage_events", "id"),
                ("quota_samples", "id"),
                ("ingest_files", "path"),
                ("meta", "key"),
            ]
            .iter()
            .map(|(t, order)| dump(&c, t, order))
            .collect();
            (all, shape(&c))
        };
        let once = snapshot(&path);
        drop(Db::open(&path).unwrap());
        drop(Db::open(&path).unwrap());
        assert_eq!(snapshot(&path), once, "{}", f.name);
        cleanup(&path);
    }
}

#[test]
fn the_current_queries_read_migrated_data() {
    for f in FIXTURES {
        let path = write_fixture(f);
        let db = Db::open(&path).unwrap();
        let (from, to) = ("2025-10-01T00:00:00Z", "2025-11-01T00:00:00Z");
        let history = store::query_history(
            &db,
            &HistoryQuery {
                from: from.into(),
                to: to.into(),
                bucket: Bucket::Month,
                group_by_model: false,
                provider: None,
                project: None,
                group_by_project: false,
            },
            &PricingTable::default(),
        )
        .unwrap();
        let expected = 7300 + 10900 + 4300 + 1300 + if f.usage_rows == 5 { 150 } else { 0 };
        assert_eq!(history.totals.total_tokens, expected, "{}", f.name);
        assert_eq!(history.totals.requests, f.usage_rows, "{}", f.name);

        let quota = |account: Option<String>| {
            store::query_quota_history(
                &db,
                &QuotaHistoryQuery {
                    from: from.into(),
                    to: to.into(),
                    provider: Some("claude".into()),
                    account,
                },
            )
            .unwrap()
        };
        let all = quota(None);
        assert_eq!(all.len(), 3, "{}", f.name);
        assert_eq!(all[0].used_percent, 12.5);
        // '' selects the primary account; the v0.6.0 fixture has one "work" sample
        let primary = quota(Some(String::new()));
        assert_eq!(
            primary.len(),
            if f.schema == 3 { 2 } else { 3 },
            "{}",
            f.name
        );
        drop(db);
        cleanup(&path);
    }
}

#[test]
fn a_migration_that_fails_midway_rolls_back_completely() {
    // v3 adds a column, drops an index, then creates another. Occupying the
    // new index's name with a table makes that last step fail.
    let f = &FIXTURES[1]; // schema 2
    let path = write_fixture(f);
    raw(&path)
        .execute_batch("CREATE TABLE idx_quota_account_ts (x)")
        .unwrap();
    let state = |p: &Path| {
        let c = raw(p);
        (
            shape(&c),
            dump(&c, "quota_samples", "id"),
            schema_version(&c),
        )
    };
    let before = state(&path);
    Db::open(&path)
        .err()
        .expect("the sabotaged migration must fail");
    assert_eq!(
        state(&path),
        before,
        "a failed migration must leave no trace"
    );
    assert_eq!(before.2, 2);

    // fixing the cause lets the next start complete the upgrade, data intact
    raw(&path)
        .execute_batch("DROP TABLE idx_quota_account_ts")
        .unwrap();
    drop(Db::open(&path).unwrap());
    let c = raw(&path);
    assert_eq!(schema_version(&c), SCHEMA_VERSION);
    assert_eq!(count(&c, "usage_events"), f.usage_rows);
    assert_eq!(count(&c, "quota_samples"), f.quota_rows);
    drop(c);
    cleanup(&path);
}
