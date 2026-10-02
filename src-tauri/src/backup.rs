//! Backup and restore of the app's own data: `settings.json` plus a
//! consistent copy of the SQLite database (`VACUUM INTO`, safe while the app
//! runs) in one timestamped folder.
//!
//! A backup holds local file paths (project directories, session source files)
//! and session metadata taken from the usage database. It holds no
//! credentials, but treat it as private.
//!
//! Restoring never touches the live database. `restore_data` validates the
//! backup and *stages* it under `<data dir>/restore-pending/`; the swap happens
//! at the next start, before the database is opened (`apply_pending`, called
//! from `lib.rs`). The replaced files are kept in `<data dir>/pre-restore/`.

use crate::commands::settings;
use crate::commands::store::{Db, SCHEMA_VERSION};
use anyhow::{bail, Context, Result};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;

pub const FOLDER_PREFIX: &str = "ai-usage-sidebar-backup-";
pub const MANIFEST: &str = "backup.json";
pub const DB_FILE: &str = "usage.db";
pub const SETTINGS_FILE: &str = "settings.json";
pub const PENDING_DIR: &str = "restore-pending";
pub const PRE_RESTORE_DIR: &str = "pre-restore";
pub const FAILED_DIR: &str = "restore-failed";
/// Written last into the staging folder: its presence means "complete".
const READY: &str = "READY";
const FORMAT: u32 = 1;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    format: u32,
    created_at: String,
    app_version: String,
    schema_version: i64,
}

/// What a backup folder contains, as shown to the user before and after a
/// restore is staged.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    pub path: String,
    pub created_at: Option<String>,
    pub app_version: Option<String>,
    pub schema_version: Option<i64>,
    pub has_database: bool,
    pub has_settings: bool,
}

fn sql_literal(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "''"))
}

/// Create `<dest_root>/ai-usage-sidebar-backup-<stamp>/` with the settings
/// file, a database copy and a manifest. Returns that folder.
pub fn create_backup(
    config_dir: &Path,
    db: Option<&Db>,
    dest_root: &Path,
    stamp: &str,
    app_version: &str,
    created_at: &str,
) -> Result<PathBuf> {
    std::fs::create_dir_all(dest_root)
        .with_context(|| format!("create {}", dest_root.display()))?;
    let mut dir = dest_root.join(format!("{FOLDER_PREFIX}{stamp}"));
    let mut n = 1;
    while dir.exists() {
        n += 1;
        dir = dest_root.join(format!("{FOLDER_PREFIX}{stamp}-{n}"));
    }
    std::fs::create_dir(&dir).with_context(|| format!("create {}", dir.display()))?;
    let result = (|| -> Result<()> {
        let src_settings = settings::settings_path(config_dir);
        if src_settings.exists() {
            std::fs::copy(&src_settings, dir.join(SETTINGS_FILE)).context("copy settings.json")?;
        }
        let mut schema_version = SCHEMA_VERSION;
        if let Some(db) = db {
            let conn = db.lock();
            conn.execute_batch(&format!("VACUUM INTO {}", sql_literal(&dir.join(DB_FILE))))
                .context("copy the usage database")?;
            schema_version = read_schema_version(&conn).unwrap_or(SCHEMA_VERSION);
        }
        let manifest = Manifest {
            format: FORMAT,
            created_at: created_at.to_string(),
            app_version: app_version.to_string(),
            schema_version,
        };
        std::fs::write(dir.join(MANIFEST), serde_json::to_vec_pretty(&manifest)?)
            .context("write backup.json")?;
        Ok(())
    })();
    if let Err(e) = result {
        std::fs::remove_dir_all(&dir).ok();
        return Err(e);
    }
    Ok(dir)
}

fn read_schema_version(conn: &Connection) -> Option<i64> {
    conn.query_row(
        "SELECT value FROM meta WHERE key='schema_version'",
        [],
        |r| r.get::<_, String>(0),
    )
    .ok()
    .and_then(|s| s.parse().ok())
}

/// Open read-only and make sure this really is a usage database this build
/// can run on. Returns its schema version.
fn check_database(path: &Path) -> Result<i64> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .context("the database cannot be opened")?;
    let verdict: String = conn
        .query_row("PRAGMA quick_check(1)", [], |r| r.get(0))
        .context("the file is not an SQLite database")?;
    if verdict != "ok" {
        bail!("the database is damaged ({verdict})");
    }
    let tables: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('meta','usage_events','quota_samples')",
        [],
        |r| r.get(0),
    )?;
    if tables != 3 {
        bail!("this is not an AI Usage Sidebar database");
    }
    let version = read_schema_version(&conn).context("the database has no schema version")?;
    if version > SCHEMA_VERSION {
        bail!(
            "the database uses schema {version}, newer than this version of the app understands ({SCHEMA_VERSION}); update the app first"
        );
    }
    if version < 1 {
        bail!("the database has an invalid schema version {version}");
    }
    Ok(version)
}

/// A user may pick the backup folder itself or the folder that contains it;
/// in the second case the newest backup inside is used.
fn resolve_backup_dir(src: &Path) -> Result<PathBuf> {
    let looks_like_backup = |d: &Path| d.join(DB_FILE).is_file() || d.join(SETTINGS_FILE).is_file();
    if looks_like_backup(src) {
        return Ok(src.to_path_buf());
    }
    let mut newest: Option<PathBuf> = None;
    if let Ok(entries) = std::fs::read_dir(src) {
        for entry in entries.flatten() {
            let path = entry.path();
            let is_backup = path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(FOLDER_PREFIX));
            if is_backup
                && path.is_dir()
                && looks_like_backup(&path)
                && newest.as_ref() < Some(&path)
            {
                newest = Some(path);
            }
        }
    }
    newest.with_context(|| {
        format!(
            "{} does not contain a backup (no {DB_FILE} or {SETTINGS_FILE})",
            src.display()
        )
    })
}

/// Check that `src` is a restorable backup without changing anything.
pub fn validate_backup(src: &Path) -> Result<BackupInfo> {
    let dir = resolve_backup_dir(src)?;
    let manifest: Option<Manifest> = std::fs::read(dir.join(MANIFEST))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
    if let Some(m) = &manifest {
        if m.format > FORMAT {
            bail!("this backup was made by a newer version of the app; update the app first");
        }
    }
    let has_settings = dir.join(SETTINGS_FILE).is_file();
    if has_settings {
        let text =
            std::fs::read_to_string(dir.join(SETTINGS_FILE)).context("read settings.json")?;
        if settings::parse(&text).is_none() {
            bail!("settings.json in the backup is not valid");
        }
    }
    let has_database = dir.join(DB_FILE).is_file();
    let schema_version = if has_database {
        Some(check_database(&dir.join(DB_FILE))?)
    } else {
        None
    };
    Ok(BackupInfo {
        path: dir.to_string_lossy().into_owned(),
        created_at: manifest.as_ref().map(|m| m.created_at.clone()),
        app_version: manifest.as_ref().map(|m| m.app_version.clone()),
        schema_version,
        has_database,
        has_settings,
    })
}

fn pending_dir(data_dir: &Path) -> PathBuf {
    data_dir.join(PENDING_DIR)
}

/// Validate `src` and copy it to the staging folder; the swap itself happens
/// at the next start. A previous staged restore is replaced.
pub fn stage_restore(data_dir: &Path, src: &Path) -> Result<BackupInfo> {
    let info = validate_backup(src)?;
    let dir = PathBuf::from(&info.path);
    let pending = pending_dir(data_dir);
    if pending.exists() {
        std::fs::remove_dir_all(&pending).context("remove the previous staged restore")?;
    }
    std::fs::create_dir_all(&pending).context("create the staging folder")?;
    let result = (|| -> Result<()> {
        for name in [DB_FILE, SETTINGS_FILE] {
            if dir.join(name).is_file() {
                std::fs::copy(dir.join(name), pending.join(name))
                    .with_context(|| format!("stage {name}"))?;
            }
        }
        std::fs::write(pending.join(READY), serde_json::to_vec(&info)?)
            .context("finish staging")?;
        Ok(())
    })();
    if let Err(e) = result {
        std::fs::remove_dir_all(&pending).ok();
        return Err(e);
    }
    Ok(info)
}

/// Whether a staged restore is waiting for the next start.
pub fn has_pending(data_dir: &Path) -> bool {
    pending_dir(data_dir).join(READY).is_file()
}

fn move_file(from: &Path, to: &Path) -> Result<()> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    std::fs::copy(from, to).with_context(|| format!("copy {}", from.display()))?;
    std::fs::remove_file(from).with_context(|| format!("remove {}", from.display()))
}

/// Swap a staged restore in. Must run before the database is opened. The
/// replaced settings and database are kept in `pre-restore/` (only the newest
/// generation). Returns `Ok(None)` when nothing is staged. A staged restore
/// that cannot be applied is moved to `restore-failed/` so it never loops.
pub fn apply_pending(config_dir: &Path, data_dir: &Path) -> Result<Option<BackupInfo>> {
    let pending = pending_dir(data_dir);
    if !pending.exists() {
        return Ok(None);
    }
    if !pending.join(READY).is_file() {
        // Staging was interrupted: nothing usable, nothing to keep.
        std::fs::remove_dir_all(&pending).ok();
        return Ok(None);
    }
    let result = swap_in(config_dir, data_dir, &pending);
    match result {
        Ok(info) => {
            std::fs::remove_dir_all(&pending).ok();
            Ok(Some(info))
        }
        Err(e) => {
            let failed = data_dir.join(FAILED_DIR);
            std::fs::remove_dir_all(&failed).ok();
            if std::fs::rename(&pending, &failed).is_err() {
                std::fs::remove_dir_all(&pending).ok();
            }
            Err(e.context("the staged restore could not be applied and was set aside"))
        }
    }
}

fn swap_in(config_dir: &Path, data_dir: &Path, pending: &Path) -> Result<BackupInfo> {
    // Re-check what was staged: the files may have been touched in between.
    let checked = validate_backup(pending)?;
    // Report the backup the user picked, not our staging copy of it.
    let info = std::fs::read(pending.join(READY))
        .ok()
        .and_then(|b| serde_json::from_slice::<BackupInfo>(&b).ok())
        .unwrap_or(checked);
    let keep = data_dir.join(PRE_RESTORE_DIR);
    std::fs::remove_dir_all(&keep).ok();
    std::fs::create_dir_all(&keep).context("create pre-restore")?;

    let staged_db = pending.join(DB_FILE);
    let live_db = data_dir.join(DB_FILE);
    if staged_db.is_file() {
        for suffix in ["", "-wal", "-shm"] {
            let name = format!("{DB_FILE}{suffix}");
            let live = data_dir.join(&name);
            if live.is_file() {
                move_file(&live, &keep.join(&name))?;
            }
        }
        move_file(&staged_db, &live_db)?;
    }
    let staged_settings = pending.join(SETTINGS_FILE);
    if staged_settings.is_file() {
        let live = settings::settings_path(config_dir);
        if live.is_file() {
            std::fs::copy(&live, keep.join(SETTINGS_FILE)).context("keep the old settings")?;
        }
        let bytes = std::fs::read(&staged_settings)?;
        settings::write_atomic(&live, &bytes)?;
    }
    Ok(info)
}

// ---------- commands ----------

fn pick_folder(window: &tauri::WebviewWindow) -> Result<Option<PathBuf>, String> {
    match window
        .dialog()
        .file()
        .set_parent(window)
        .blocking_pick_folder()
    {
        Some(p) => p.into_path().map(Some).map_err(|e| e.to_string()),
        None => Ok(None),
    }
}

/// Write a backup into `dest` (a folder; asked with a native dialog when
/// absent). Returns the created backup folder, or `None` when cancelled.
#[tauri::command]
pub async fn backup_data(
    window: tauri::WebviewWindow,
    state: State<'_, crate::state::AppState>,
    dest: Option<String>,
) -> Result<Option<String>, String> {
    let config_dir = state.config_dir.clone();
    let db = state.db.clone();
    let version = window.app_handle().package_info().version.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let root = match dest {
            Some(d) if !d.trim().is_empty() => PathBuf::from(d),
            _ => match pick_folder(&window)? {
                Some(p) => p,
                None => return Ok(None),
            },
        };
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
        let created_at = crate::commands::providers::now_rfc3339();
        create_backup(
            &config_dir,
            db.as_deref(),
            &root,
            &stamp,
            &version,
            &created_at,
        )
        .map(|p| Some(p.to_string_lossy().into_owned()))
        .map_err(|e| format!("{e:#}"))
    })
    .await
    .map_err(|e| format!("backup failed: {e}"))?
}

/// Validate and stage a restore from `src` (asked with a native dialog when
/// absent). `None` = cancelled. The data is swapped in at the next start; the
/// frontend then offers `restart_app`.
#[tauri::command]
pub async fn restore_data(
    window: tauri::WebviewWindow,
    state: State<'_, crate::state::AppState>,
    src: Option<String>,
) -> Result<Option<BackupInfo>, String> {
    let data_dir = state.data_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let src = match src {
            Some(s) if !s.trim().is_empty() => PathBuf::from(s),
            _ => match pick_folder(&window)? {
                Some(p) => p,
                None => return Ok(None),
            },
        };
        stage_restore(&data_dir, &src)
            .map(Some)
            .map_err(|e| format!("{e:#}"))
    })
    .await
    .map_err(|e| format!("restore failed: {e}"))?
}

/// Relaunch the app (used after a restore was staged).
#[tauri::command]
pub fn restart_app(app: AppHandle) {
    app.restart()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_support::tempdir;

    /// A config dir with settings and a data dir whose database carries a
    /// `marker` meta row, so tests can tell which database they are looking at.
    fn fixture(marker: &str) -> (PathBuf, PathBuf, Db) {
        let config = tempdir();
        let data = tempdir();
        std::fs::write(
            settings::settings_path(&config),
            format!(r#"{{"edge":"left","language":"{marker}"}}"#),
        )
        .unwrap();
        let db = Db::open(&data.join(DB_FILE)).unwrap();
        db.lock()
            .execute("INSERT INTO meta(key,value) VALUES('marker',?1)", [marker])
            .unwrap();
        (config, data, db)
    }

    fn marker_of(db_path: &Path) -> String {
        Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap()
            .query_row("SELECT value FROM meta WHERE key='marker'", [], |r| {
                r.get(0)
            })
            .unwrap()
    }

    fn backup_of(config: &Path, db: &Db, root: &Path) -> PathBuf {
        create_backup(
            config,
            Some(db),
            root,
            "20261002-101500",
            "0.6.0",
            "2026-10-02T10:15:00Z",
        )
        .unwrap()
    }

    #[test]
    fn a_backup_holds_settings_a_database_copy_and_a_manifest() {
        let (config, data, db) = fixture("en");
        let root = tempdir();
        let dir = backup_of(&config, &db, &root);
        assert!(dir.ends_with("ai-usage-sidebar-backup-20261002-101500"));
        for f in [DB_FILE, SETTINGS_FILE, MANIFEST] {
            assert!(dir.join(f).is_file(), "{f}");
        }
        assert_eq!(marker_of(&dir.join(DB_FILE)), "en");
        // A second backup in the same second does not overwrite the first.
        let again = backup_of(&config, &db, &root);
        assert_ne!(dir, again);
        let info = validate_backup(&dir).unwrap();
        assert_eq!(info.schema_version, Some(SCHEMA_VERSION));
        assert_eq!(info.app_version.as_deref(), Some("0.6.0"));
        assert!(info.has_database && info.has_settings);
        drop(db);
        for d in [config, data, root] {
            std::fs::remove_dir_all(d).ok();
        }
    }

    #[test]
    fn validation_accepts_the_parent_folder_and_picks_the_newest_backup() {
        let (config, _data, db) = fixture("en");
        let root = tempdir();
        let old =
            create_backup(&config, Some(&db), &root, "20250101-000000", "0.5.0", "x").unwrap();
        let new = backup_of(&config, &db, &root);
        let info = validate_backup(&root).unwrap();
        assert_eq!(PathBuf::from(info.path), new);
        assert_ne!(PathBuf::from(validate_backup(&old).unwrap().path), new);
    }

    #[test]
    fn validation_rejects_unusable_folders() {
        let (config, _data, db) = fixture("en");
        let root = tempdir();

        // empty
        assert!(validate_backup(&root).is_err());

        // garbage instead of a database
        let bad = root.join("bad");
        std::fs::create_dir_all(&bad).unwrap();
        std::fs::write(bad.join(DB_FILE), vec![7u8; 4096]).unwrap();
        assert!(validate_backup(&bad).is_err());

        // a valid SQLite file that is not our database
        let other = root.join("other");
        std::fs::create_dir_all(&other).unwrap();
        Connection::open(other.join(DB_FILE))
            .unwrap()
            .execute_batch("CREATE TABLE t(x)")
            .unwrap();
        let err = validate_backup(&other).unwrap_err().to_string();
        assert!(err.contains("not an AI Usage Sidebar database"), "{err}");

        // a database from a newer app
        let newer = backup_of(&config, &db, &root);
        Connection::open(newer.join(DB_FILE))
            .unwrap()
            .execute("UPDATE meta SET value='99' WHERE key='schema_version'", [])
            .unwrap();
        let err = validate_backup(&newer).unwrap_err().to_string();
        assert!(err.contains("newer"), "{err}");

        // invalid settings
        let bad_settings = root.join("bad-settings");
        std::fs::create_dir_all(&bad_settings).unwrap();
        std::fs::write(bad_settings.join(SETTINGS_FILE), "{ nope").unwrap();
        assert!(validate_backup(&bad_settings).is_err());

        // a manifest from the future
        let future = backup_of(&config, &db, &root);
        std::fs::write(
            future.join(MANIFEST),
            r#"{"format":9,"createdAt":"x","appVersion":"9","schemaVersion":1}"#,
        )
        .unwrap();
        assert!(validate_backup(&future).is_err());
    }

    #[test]
    fn a_staged_restore_is_swapped_in_once_and_the_old_data_is_kept() {
        // the data to restore
        let (src_config, _src_data, src_db) = fixture("zh-CN");
        let root = tempdir();
        let backup = backup_of(&src_config, &src_db, &root);

        // the current installation
        let (config, data, db) = fixture("en");
        drop(db); // the swap happens before the app opens the database

        let info = stage_restore(&data, &backup).unwrap();
        assert!(has_pending(&data));
        assert_eq!(
            marker_of(&data.join(DB_FILE)),
            "en",
            "staging changes nothing live"
        );

        let applied = apply_pending(&config, &data).unwrap().unwrap();
        assert_eq!(applied.path, info.path);
        assert_eq!(marker_of(&data.join(DB_FILE)), "zh-CN");
        assert_eq!(marker_of(&data.join(PRE_RESTORE_DIR).join(DB_FILE)), "en");
        let live = std::fs::read_to_string(settings::settings_path(&config)).unwrap();
        assert!(live.contains("zh-CN"));
        let kept = std::fs::read_to_string(data.join(PRE_RESTORE_DIR).join(SETTINGS_FILE)).unwrap();
        assert!(kept.contains(r#""language":"en""#));
        assert!(!pending_dir(&data).exists());
        assert!(!has_pending(&data));
        assert!(
            apply_pending(&config, &data).unwrap().is_none(),
            "only once"
        );
    }

    #[test]
    fn an_interrupted_or_broken_staging_never_replaces_anything() {
        let (config, data, db) = fixture("en");
        drop(db);

        // staging that never finished: no READY marker
        std::fs::create_dir_all(pending_dir(&data)).unwrap();
        std::fs::write(pending_dir(&data).join(DB_FILE), b"half").unwrap();
        assert!(apply_pending(&config, &data).unwrap().is_none());
        assert!(!pending_dir(&data).exists());
        assert_eq!(marker_of(&data.join(DB_FILE)), "en");

        // complete marker but a corrupt database: set aside, not applied, no loop
        std::fs::create_dir_all(pending_dir(&data)).unwrap();
        std::fs::write(pending_dir(&data).join(DB_FILE), vec![1u8; 2048]).unwrap();
        std::fs::write(pending_dir(&data).join(READY), b"{}").unwrap();
        assert!(apply_pending(&config, &data).is_err());
        assert!(data.join(FAILED_DIR).exists());
        assert!(!pending_dir(&data).exists());
        assert_eq!(marker_of(&data.join(DB_FILE)), "en");
        assert!(apply_pending(&config, &data).unwrap().is_none());
    }

    #[test]
    fn staging_a_second_time_replaces_the_first() {
        let (config, data, db) = fixture("en");
        let root = tempdir();
        let first = backup_of(&config, &db, &root);
        std::fs::remove_dir_all(&first).ok();
        let (c2, _d2, db2) = fixture("zh-CN");
        let second = backup_of(&c2, &db2, &root);
        stage_restore(&data, &second).unwrap();
        assert_eq!(marker_of(&pending_dir(&data).join(DB_FILE)), "zh-CN");
        assert!(stage_restore(&data, &root.join("missing")).is_err());
        assert_eq!(
            marker_of(&pending_dir(&data).join(DB_FILE)),
            "zh-CN",
            "a rejected backup leaves the staged one alone"
        );
    }
}
