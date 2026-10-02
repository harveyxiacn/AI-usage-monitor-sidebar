//! Downgrade safety for `usage.db`: the compatibility policy, the automatic
//! pre-migration backup and the listing/staging of those backups.
//!
//! # Policy
//!
//! `meta.schema_version` says what the database *is*; `meta.min_reader_version`
//! says the oldest schema whose code can still safely read **and write** it.
//!
//! * Additive changes (a new nullable/defaulted column, a new table, a new
//!   index) bump `schema_version` but keep `min_reader_version`.
//! * Only a breaking change (a dropped/renamed column, a changed meaning, a
//!   rewritten table) raises `min_reader_version` to the new version.
//!
//! On open, a build that understands schema `S`:
//!
//! | database                                  | result                                  |
//! |-------------------------------------------|-----------------------------------------|
//! | `schema_version < S`                      | back up, migrate, stamp both keys       |
//! | `schema_version == S`                     | open                                    |
//! | `schema_version > S`, `min_reader <= S`   | open as is; the version is never lowered|
//! | `schema_version > S`, `min_reader >  S`   | refuse ([`SchemaTooNew`]), app runs on  |
//!
//! A missing `min_reader_version` (every database written before v0.7) counts
//! as "equal to `schema_version`": those builds had no compatibility notion.
//!
//! The refusal is not fatal: `AppState.db` is `None`, quota rings, the
//! snapshot and the CLI keep working, and the dashboard shows a banner.
//!
//! History: v0.5 (schema 2) and v0.6 (schema 3) refuse any newer schema
//! outright, whatever `min_reader_version` says; the keys only protect
//! downgrades *from* v0.7 on. Schema 2 -> 3 (v0.6) was additive, so its
//! `min_reader_version` is 2.
//!
//! # Pre-migration backup
//!
//! Before any migration step runs, `VACUUM INTO` writes
//! `<data dir>/backups/usage-pre-v<from>-to-v<to>-<YYYYMMDD-HHMMSS>.db`, plus
//! `<same stem>.settings.json` when a settings file exists. The newest
//! [`KEEP`] are kept. If the backup cannot be written the migration is aborted
//! ([`PreUpgradeBackupFailed`]), the database is left untouched and the app
//! runs without it. That includes a full disk: a migration that cannot even
//! copy the file will not do better in place, and "no safety net" is the one
//! outcome the user did not agree to. Escape hatch for people who accept the
//! risk: `AI_USAGE_SIDEBAR_SKIP_MIGRATION_BACKUP=1`.

use anyhow::{bail, Context, Result};
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Name of the sub-folder of the data dir holding pre-upgrade backups.
pub const BACKUP_DIR: &str = "backups";
/// How many pre-upgrade backups are kept.
pub const KEEP: usize = 3;
pub const MIN_READER_KEY: &str = "min_reader_version";
pub const SKIP_BACKUP_ENV: &str = "AI_USAGE_SIDEBAR_SKIP_MIGRATION_BACKUP";
const PREFIX: &str = "usage-pre-v";
const SETTINGS_SUFFIX: &str = ".settings.json";

/// The database needs a newer app than this one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaTooNew {
    pub found: i64,
    pub min_reader: i64,
    pub supported: i64,
}

impl std::fmt::Display for SchemaTooNew {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "usage.db uses schema {} and needs an app that understands schema {} or newer; this build understands up to {}",
            self.found, self.min_reader, self.supported
        )
    }
}

impl std::error::Error for SchemaTooNew {}

/// A migration was due but its safety backup could not be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreUpgradeBackupFailed(pub String);

impl std::fmt::Display for PreUpgradeBackupFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "the database was not upgraded because its safety backup failed: {}",
            self.0
        )
    }
}

impl std::error::Error for PreUpgradeBackupFailed {}

/// What to do with a database at `found`, for a build that understands
/// `supported`. `min_reader` of `None` = key missing (see module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compat {
    /// Brand new (0) or older than this build: migrate.
    Migrate {
        from: i64,
    },
    Current,
    /// Newer, but declared compatible: open without touching the version.
    NewerCompatible {
        found: i64,
        min_reader: i64,
    },
    TooNew {
        found: i64,
        min_reader: i64,
    },
}

pub fn classify(found: i64, min_reader: Option<i64>, supported: i64) -> Compat {
    if found < supported {
        Compat::Migrate { from: found }
    } else if found == supported {
        Compat::Current
    } else {
        let min_reader = min_reader.unwrap_or(found);
        if min_reader <= supported {
            Compat::NewerCompatible { found, min_reader }
        } else {
            Compat::TooNew { found, min_reader }
        }
    }
}

/// `(schema_version, min_reader_version)` as stored; both `None` for a file
/// without a `meta` table (new or foreign).
pub fn read_versions(conn: &Connection) -> (Option<i64>, Option<i64>) {
    let get = |key: &str| -> Option<i64> {
        conn.query_row("SELECT value FROM meta WHERE key=?1", [key], |r| {
            r.get::<_, String>(0)
        })
        .ok()
        .and_then(|s| s.trim().parse().ok())
    };
    (get("schema_version"), get(MIN_READER_KEY))
}

/// Run before the migration transaction. Returns the backup written (if a
/// migration is due) or an error that must abort the open.
pub fn guard_open(
    conn: &Connection,
    data_dir: &Path,
    config_dir: Option<&Path>,
    supported: i64,
) -> Result<Option<PathBuf>> {
    let (schema, min_reader) = read_versions(conn);
    let found = schema.unwrap_or(0);
    match classify(found, min_reader, supported) {
        Compat::TooNew { found, min_reader } => Err(SchemaTooNew {
            found,
            min_reader,
            supported,
        }
        .into()),
        Compat::NewerCompatible { found, min_reader } => {
            log::info!(
                "usage.db is schema {found} (this build: {supported}); declared readable from {min_reader}, opening as is"
            );
            Ok(None)
        }
        Compat::Current => Ok(None),
        // Nothing worth protecting in a database that has never held data.
        Compat::Migrate { from } if from < 1 => Ok(None),
        Compat::Migrate { from } => {
            if std::env::var(SKIP_BACKUP_ENV).as_deref() == Ok("1") {
                log::warn!("{SKIP_BACKUP_ENV}=1: migrating usage.db without a safety backup");
                return Ok(None);
            }
            let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
            match backup_before_migration(conn, data_dir, config_dir, from, supported, &stamp) {
                Ok(path) => {
                    log::info!("pre-upgrade backup written: {}", path.display());
                    Ok(Some(path))
                }
                Err(e) => {
                    log::error!("pre-upgrade backup failed, migration aborted: {e:#}");
                    Err(PreUpgradeBackupFailed(format!("{e:#}")).into())
                }
            }
        }
    }
}

fn sql_literal(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "''"))
}

/// Write the backup pair and rotate. Returns the database file.
pub fn backup_before_migration(
    conn: &Connection,
    data_dir: &Path,
    config_dir: Option<&Path>,
    from: i64,
    to: i64,
    stamp: &str,
) -> Result<PathBuf> {
    let dir = data_dir.join(BACKUP_DIR);
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let mut stem = format!("{PREFIX}{from}-to-v{to}-{stamp}");
    let mut n = 1;
    while dir.join(format!("{stem}.db")).exists() {
        n += 1;
        stem = format!("{PREFIX}{from}-to-v{to}-{stamp}-{n}");
    }
    let db_file = dir.join(format!("{stem}.db"));
    let settings_file = dir.join(format!("{stem}{SETTINGS_SUFFIX}"));
    let result = (|| -> Result<()> {
        conn.execute_batch(&format!("VACUUM INTO {}", sql_literal(&db_file)))
            .context("copy the usage database")?;
        if let Some(config_dir) = config_dir {
            let src = crate::commands::settings::settings_path(config_dir);
            if src.is_file() {
                std::fs::copy(&src, &settings_file).context("copy settings.json")?;
            }
        }
        Ok(())
    })();
    if let Err(e) = result {
        std::fs::remove_file(&db_file).ok();
        std::fs::remove_file(&settings_file).ok();
        return Err(e);
    }
    rotate(&dir, KEEP);
    Ok(db_file)
}

/// `usage-pre-v2-to-v3-20261002-101500[-2].db` -> `(2, 3, "20261002-101500", 1)`.
fn parse_name(file_name: &str) -> Option<(i64, i64, String, u32)> {
    let stem = file_name.strip_suffix(".db")?.strip_prefix(PREFIX)?;
    let mut parts = stem.split('-');
    let from = parts.next()?.parse().ok()?;
    if parts.next()? != "to" {
        return None;
    }
    let to = parts.next()?.strip_prefix('v')?.parse().ok()?;
    let date = parts.next()?;
    let time = parts.next()?;
    if date.len() != 8 || time.len() != 6 {
        return None;
    }
    let seq = match parts.next() {
        Some(s) => s.parse().ok()?,
        None => 1,
    };
    if parts.next().is_some() {
        return None;
    }
    Some((from, to, format!("{date}-{time}"), seq))
}

/// One pre-upgrade backup as shown to the user.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PreUpgradeBackup {
    /// File name, e.g. `usage-pre-v2-to-v3-20261002-101500.db`.
    pub name: String,
    pub path: String,
    pub settings_path: Option<String>,
    pub from_version: i64,
    pub to_version: i64,
    /// `YYYYMMDD-HHMMSS`, local time.
    pub stamp: String,
    pub size_bytes: u64,
}

/// All pre-upgrade backups in `<data dir>/backups`, newest first.
pub fn list(data_dir: &Path) -> Vec<PreUpgradeBackup> {
    let dir = data_dir.join(BACKUP_DIR);
    let mut found: Vec<((String, u32), PreUpgradeBackup)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some((from, to, stamp, seq)) = parse_name(&name) else {
                continue;
            };
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let settings = dir.join(format!(
                "{}{SETTINGS_SUFFIX}",
                name.strip_suffix(".db").unwrap_or(&name)
            ));
            let backup = PreUpgradeBackup {
                size_bytes: entry.metadata().map(|m| m.len()).unwrap_or(0),
                settings_path: settings
                    .is_file()
                    .then(|| settings.to_string_lossy().into_owned()),
                path: path.to_string_lossy().into_owned(),
                name,
                from_version: from,
                to_version: to,
                stamp: stamp.clone(),
            };
            found.push(((stamp, seq), backup));
        }
    }
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found.into_iter().map(|(_, b)| b).collect()
}

/// Delete everything but the newest `keep` backups (and their settings copy).
fn rotate(dir: &Path, keep: usize) {
    let data_dir = dir.parent().unwrap_or(dir);
    for old in list(data_dir).into_iter().skip(keep) {
        for p in [Some(old.path), old.settings_path].into_iter().flatten() {
            if let Err(e) = std::fs::remove_file(&p) {
                log::warn!("cannot remove old pre-upgrade backup {p}: {e}");
            }
        }
    }
}

/// Pick a backup: `None` = newest; otherwise a file name (or a path ending in
/// one).
pub fn select(data_dir: &Path, wanted: Option<&str>) -> Result<PreUpgradeBackup> {
    let all = list(data_dir);
    match wanted {
        None => all.into_iter().next().with_context(|| {
            format!(
                "there is no pre-upgrade backup in {}",
                data_dir.join(BACKUP_DIR).display()
            )
        }),
        Some(w) => {
            let base = Path::new(w)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| w.to_string());
            match all.into_iter().find(|b| b.name == base) {
                Some(b) => Ok(b),
                None => bail!("no pre-upgrade backup named `{base}` (use --list)"),
            }
        }
    }
}

/// Stage `backup` with the existing restore mechanism; the next start of an
/// app that has `backup::apply_pending` swaps it in.
pub fn stage(data_dir: &Path, backup: &PreUpgradeBackup) -> Result<crate::backup::BackupInfo> {
    let db = PathBuf::from(&backup.path);
    let settings = backup.settings_path.as_ref().map(PathBuf::from);
    crate::backup::stage_files(
        data_dir,
        &db,
        settings.as_deref(),
        Some(format!("{} (local time)", backup.stamp)),
    )
}

/// What the dashboard needs to know about the database.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DbStatus {
    /// `ok` | `schemaTooNew` | `migrationBackupFailed` | `unavailable`
    pub state: String,
    pub message: String,
    pub found: Option<i64>,
    pub min_reader: Option<i64>,
    pub supported: i64,
}

impl DbStatus {
    pub fn ok(supported: i64) -> Self {
        DbStatus {
            state: "ok".into(),
            message: String::new(),
            found: None,
            min_reader: None,
            supported,
        }
    }

    pub fn from_error(e: &anyhow::Error, supported: i64) -> Self {
        let mut s = DbStatus::ok(supported);
        s.message = format!("{e:#}");
        if let Some(t) = e.downcast_ref::<SchemaTooNew>() {
            s.state = "schemaTooNew".into();
            s.found = Some(t.found);
            s.min_reader = Some(t.min_reader);
        } else if e.downcast_ref::<PreUpgradeBackupFailed>().is_some() {
            s.state = "migrationBackupFailed".into();
        } else {
            s.state = "unavailable".into();
        }
        s
    }
}

/// Read-only peek used by the backup validator.
pub fn peek_versions(path: &Path) -> Result<(Option<i64>, Option<i64>)> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .context("the database cannot be opened")?;
    Ok(read_versions(&conn))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_support::tempdir;

    #[test]
    fn compatibility_matrix() {
        use Compat::*;
        let s = 3;
        assert_eq!(classify(0, None, s), Migrate { from: 0 });
        assert_eq!(classify(2, None, s), Migrate { from: 2 });
        assert_eq!(classify(2, Some(2), s), Migrate { from: 2 });
        assert_eq!(classify(3, None, s), Current);
        assert_eq!(classify(3, Some(3), s), Current);
        // additive change in a newer build
        assert_eq!(
            classify(4, Some(2), s),
            NewerCompatible {
                found: 4,
                min_reader: 2
            }
        );
        assert_eq!(
            classify(5, Some(3), s),
            NewerCompatible {
                found: 5,
                min_reader: 3
            }
        );
        // breaking change
        assert_eq!(
            classify(5, Some(4), s),
            TooNew {
                found: 5,
                min_reader: 4
            }
        );
        // no key: written by a build without the policy, assume the worst
        assert_eq!(
            classify(4, None, s),
            TooNew {
                found: 4,
                min_reader: 4
            }
        );
    }

    #[test]
    fn names_round_trip_and_junk_is_ignored() {
        assert_eq!(
            parse_name("usage-pre-v2-to-v3-20261002-101500.db"),
            Some((2, 3, "20261002-101500".into(), 1))
        );
        assert_eq!(
            parse_name("usage-pre-v2-to-v3-20261002-101500-4.db"),
            Some((2, 3, "20261002-101500".into(), 4))
        );
        for bad in [
            "usage.db",
            "usage-pre-v2-to-v3-20261002-101500.db-wal",
            "usage-pre-vx-to-v3-20261002-101500.db",
            "usage-pre-v2-to-v3-2026-1.db",
            "usage-pre-v2-to-v3-20261002-101500.settings.json",
        ] {
            assert_eq!(parse_name(bad), None, "{bad}");
        }
    }

    fn conn_with_data() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT);
             INSERT INTO meta VALUES('schema_version','2');
             CREATE TABLE t (x INTEGER); INSERT INTO t VALUES (42);",
        )
        .unwrap();
        conn
    }

    #[test]
    fn backups_keep_the_newest_three_with_their_settings_copy() {
        let data = tempdir();
        let config = tempdir();
        std::fs::write(crate::commands::settings::settings_path(&config), "{}").unwrap();
        let conn = conn_with_data();
        for i in 0..5 {
            backup_before_migration(
                &conn,
                &data,
                Some(&config),
                2,
                3,
                &format!("2026100{}-101500", i + 1),
            )
            .unwrap();
        }
        let all = list(&data);
        assert_eq!(all.len(), KEEP);
        let stamps: Vec<_> = all.iter().map(|b| b.stamp.as_str()).collect();
        assert_eq!(
            stamps,
            ["20261005-101500", "20261004-101500", "20261003-101500"]
        );
        assert!(all.iter().all(|b| b.settings_path.is_some()));
        // no orphaned settings copies of rotated-out backups
        let files = std::fs::read_dir(data.join(BACKUP_DIR)).unwrap().count();
        assert_eq!(files, KEEP * 2);
        // the copy really holds the data
        let copy = Connection::open(&all[0].path).unwrap();
        let x: i64 = copy.query_row("SELECT x FROM t", [], |r| r.get(0)).unwrap();
        assert_eq!(x, 42);
        // same stamp twice does not overwrite
        let a = backup_before_migration(&conn, &data, None, 2, 3, "20261005-101500").unwrap();
        assert!(a.to_string_lossy().ends_with("-2.db"));
        assert_eq!(
            select(&data, None).unwrap().name,
            "usage-pre-v2-to-v3-20261005-101500-2.db"
        );
    }

    #[test]
    fn select_by_name_or_path() {
        let data = tempdir();
        let conn = conn_with_data();
        backup_before_migration(&conn, &data, None, 2, 3, "20261001-000001").unwrap();
        backup_before_migration(&conn, &data, None, 2, 3, "20261002-000001").unwrap();
        assert_eq!(select(&data, None).unwrap().stamp, "20261002-000001");
        let old = select(&data, Some("usage-pre-v2-to-v3-20261001-000001.db")).unwrap();
        assert_eq!(old.stamp, "20261001-000001");
        assert_eq!(select(&data, Some(&old.path)).unwrap().name, old.name);
        assert!(select(&data, Some("nope.db")).is_err());
        assert!(select(&tempdir(), None).is_err());
    }

    #[test]
    fn guard_open_decisions() {
        let data = tempdir();
        let conn = conn_with_data();
        // migration due -> backup
        let b = guard_open(&conn, &data, None, 3).unwrap();
        assert!(b.is_some_and(|p| p.is_file()));
        // fresh database -> nothing to protect
        let fresh = Connection::open_in_memory().unwrap();
        assert!(guard_open(&fresh, &data, None, 3).unwrap().is_none());
        // too new -> typed refusal, nothing written
        conn.execute_batch(
            "UPDATE meta SET value='5' WHERE key='schema_version';
             INSERT INTO meta VALUES('min_reader_version','4');",
        )
        .unwrap();
        let e = guard_open(&conn, &data, None, 3).unwrap_err();
        assert_eq!(
            e.downcast_ref::<SchemaTooNew>(),
            Some(&SchemaTooNew {
                found: 5,
                min_reader: 4,
                supported: 3
            })
        );
        assert_eq!(DbStatus::from_error(&e, 3).state, "schemaTooNew");
        // newer but compatible -> opens
        conn.execute_batch("UPDATE meta SET value='3' WHERE key='min_reader_version';")
            .unwrap();
        assert!(guard_open(&conn, &data, None, 3).unwrap().is_none());
    }

    #[test]
    fn a_failed_backup_aborts_with_a_typed_error() {
        let data = tempdir();
        // `backups` exists as a plain file, so the folder cannot be created.
        std::fs::write(data.join(BACKUP_DIR), "in the way").unwrap();
        let conn = conn_with_data();
        let e = guard_open(&conn, &data, None, 3).unwrap_err();
        assert!(
            e.downcast_ref::<PreUpgradeBackupFailed>().is_some(),
            "{e:#}"
        );
        assert_eq!(DbStatus::from_error(&e, 3).state, "migrationBackupFailed");
    }
}
