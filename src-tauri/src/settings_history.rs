//! The last few `settings.json` versions, for "Undo last change". [BACKEND]
//!
//! Before every settings write the version it replaces is pushed into a small
//! ring stored next to `settings.json` (`settings.history.json`, newest
//! first, at most [`MAX_VERSIONS`] entries). Everything here is best effort:
//! a history that cannot be read or written never blocks a settings save.
//!
//! A slider fires an update per pixel, so a *burst* of edits (each less than
//! [`BURST_MS`] after the previous one) records only the version the burst
//! started from. Otherwise five drags of one slider would fill the ring with
//! near-identical entries and "undo" could never get past them.

use super::settings::write_atomic;
use crate::model::Settings;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const HISTORY_FILE: &str = "settings.history.json";
pub const MAX_VERSIONS: usize = 5;
/// Edits closer together than this belong to one burst.
pub const BURST_MS: i64 = 3_000;

/// One earlier version of the settings.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SettingsVersion {
    /// Epoch ms at which this version stopped being the live one.
    pub replaced_at: i64,
    pub settings: Settings,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct Ring {
    /// Epoch ms of the latest recorded edit, to recognise a burst.
    last_edit_ms: i64,
    /// Newest first.
    versions: Vec<SettingsVersion>,
}

pub fn history_path(config_dir: &Path) -> PathBuf {
    config_dir.join(HISTORY_FILE)
}

fn load_ring(config_dir: &Path) -> Ring {
    let Ok(text) = std::fs::read_to_string(history_path(config_dir)) else {
        return Ring::default();
    };
    match serde_json::from_str::<Ring>(&text) {
        Ok(mut ring) => {
            ring.versions.truncate(MAX_VERSIONS);
            ring
        }
        Err(e) => {
            log::warn!("ignoring unreadable {HISTORY_FILE}: {e}");
            Ring::default()
        }
    }
}

/// The saved versions, newest first. Missing or corrupt file = none.
pub fn load(config_dir: &Path) -> Vec<SettingsVersion> {
    load_ring(config_dir).versions
}

/// Ring after pushing `previous`: unchanged when it equals the newest entry,
/// otherwise prepended and trimmed to [`MAX_VERSIONS`].
pub fn pushed(
    mut versions: Vec<SettingsVersion>,
    previous: &Settings,
    now_ms: i64,
) -> Vec<SettingsVersion> {
    if versions.first().is_some_and(|v| v.settings == *previous) {
        return versions;
    }
    versions.insert(
        0,
        SettingsVersion {
            replaced_at: now_ms,
            settings: previous.clone(),
        },
    );
    versions.truncate(MAX_VERSIONS);
    versions
}

/// True when an edit at `now_ms` continues a burst that last edited at
/// `last_edit_ms` (and so must not add a ring entry of its own).
pub fn continues_burst(last_edit_ms: i64, now_ms: i64) -> bool {
    last_edit_ms > 0 && (0..BURST_MS).contains(&(now_ms - last_edit_ms))
}

/// Record that `previous` is about to be replaced by an edit at `now_ms`.
/// An edit that continues a burst (see the module docs) adds no entry; a
/// restore passes `force` so it can always be undone.
pub fn record(config_dir: &Path, previous: &Settings, now_ms: i64, force: bool) -> Result<()> {
    let mut ring = load_ring(config_dir);
    let burst = !force && continues_burst(ring.last_edit_ms, now_ms);
    ring.last_edit_ms = now_ms;
    if !burst {
        ring.versions = pushed(ring.versions, previous, now_ms);
    }
    let bytes = serde_json::to_vec_pretty(&ring).context("serialize history")?;
    write_atomic(&history_path(config_dir), &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_support::tempdir;

    fn with_scale(scale: f64) -> Settings {
        Settings {
            scale,
            ..Settings::default()
        }
    }

    #[test]
    fn the_ring_keeps_the_newest_five_newest_first() {
        let mut ring = Vec::new();
        for i in 0..8 {
            ring = pushed(ring, &with_scale(1.0 + i as f64 / 100.0), i);
        }
        assert_eq!(ring.len(), MAX_VERSIONS);
        assert_eq!(ring[0].settings.scale, 1.07);
        assert_eq!(ring[4].settings.scale, 1.03);
    }

    #[test]
    fn pushing_the_same_version_twice_does_not_duplicate_it() {
        let ring = pushed(Vec::new(), &with_scale(1.1), 1);
        let ring = pushed(ring, &with_scale(1.1), 2);
        assert_eq!(ring.len(), 1);
        assert_eq!(ring[0].replaced_at, 1);
    }

    #[test]
    fn a_burst_is_edits_less_than_three_seconds_apart() {
        assert!(!continues_burst(0, 10_000), "no earlier edit");
        assert!(continues_burst(10_000, 10_500));
        assert!(continues_burst(10_000, 12_999));
        assert!(!continues_burst(10_000, 13_000));
        assert!(!continues_burst(10_000, 5_000), "clock went backwards");
    }

    #[test]
    fn records_round_trip_through_the_file_and_survive_garbage() {
        let dir = tempdir();
        assert!(load(&dir).is_empty());
        record(&dir, &with_scale(1.2), 10_000, false).unwrap();
        // 4 s later: a new burst.
        record(&dir, &with_scale(1.3), 14_000, false).unwrap();
        // 1 s later: the same burst, nothing added.
        record(&dir, &with_scale(1.4), 15_000, false).unwrap();
        let versions = load(&dir);
        assert_eq!(versions.len(), 2, "the burst edit added nothing");
        assert_eq!(versions[0].settings.scale, 1.3);
        assert_eq!(versions[0].replaced_at, 14_000);
        // a restore is never swallowed by a burst
        record(&dir, &with_scale(1.5), 15_500, true).unwrap();
        assert_eq!(load(&dir)[0].settings.scale, 1.5);
        std::fs::write(history_path(&dir), "{ not json").unwrap();
        assert!(load(&dir).is_empty());
        record(&dir, &with_scale(1.5), 40_000, false).unwrap();
        assert_eq!(load(&dir).len(), 1, "a corrupt file starts over");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
