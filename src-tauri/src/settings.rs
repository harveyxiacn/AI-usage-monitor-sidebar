//! `settings.json` load / merge / clamp / atomic save. [BACKEND owns this file]
//!
//! Loading never fails: a missing, partial or partly-invalid file degrades to
//! the defaults for the fields it cannot supply.

use crate::model::{ProviderSettings, Settings};
use anyhow::{Context, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub const SETTINGS_FILE: &str = "settings.json";

pub fn settings_path(config_dir: &Path) -> PathBuf {
    config_dir.join(SETTINGS_FILE)
}

/// Read `settings.json`, tolerating anything the user (or a future version)
/// may have put in it. Always returns clamped, usable settings.
pub fn load(config_dir: &Path) -> Settings {
    let path = settings_path(config_dir);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            log::info!("no settings file at {}, using defaults", path.display());
            return Settings::default();
        }
        Err(e) => {
            log::warn!("cannot read {}: {}", path.display(), e);
            return Settings::default();
        }
    };
    let value: Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            log::warn!(
                "{} is not valid JSON ({}), using defaults",
                path.display(),
                e
            );
            return Settings::default();
        }
    };
    // `merge` is field-by-field, so a single bad field cannot poison the rest.
    merge(&Settings::default(), &value)
}

/// Shallow merge of a JSON patch onto `base`.
///
/// Objects replace wholesale, **except** `providers`, which is merged per key.
/// Fields that fail to deserialize are logged and skipped. The result is
/// always clamped.
pub fn merge(base: &Settings, patch: &Value) -> Settings {
    let Some(patch) = patch.as_object() else {
        return clamp(base.clone());
    };
    let mut current = match serde_json::to_value(base) {
        Ok(Value::Object(o)) => o,
        _ => return clamp(base.clone()),
    };
    for (key, value) in patch {
        let mut candidate = current.clone();
        if key == "providers" {
            // per-key merge so a patch can toggle one provider only
            let mut merged = match current.get("providers") {
                Some(Value::Object(o)) => o.clone(),
                _ => serde_json::Map::new(),
            };
            match value.as_object() {
                Some(src) => {
                    for (pk, pv) in src {
                        merged.insert(pk.clone(), pv.clone());
                    }
                }
                None => {
                    log::warn!("settings patch: `providers` is not an object, ignored");
                    continue;
                }
            }
            candidate.insert("providers".into(), Value::Object(merged));
        } else {
            candidate.insert(key.clone(), value.clone());
        }
        match serde_json::from_value::<Settings>(Value::Object(candidate.clone())) {
            Ok(_) => current = candidate,
            Err(e) => log::warn!("settings patch: ignoring field `{}` ({})", key, e),
        }
    }
    let merged = serde_json::from_value::<Settings>(Value::Object(current)).unwrap_or_default();
    clamp(merged)
}

/// Force every value into its supported range (ARCHITECTURE §7).
pub fn clamp(mut s: Settings) -> Settings {
    s.opacity = clamp_f64(s.opacity, 0.3, 1.0, 1.0);
    s.scale = clamp_f64(s.scale, 0.75, 1.5, 1.0);
    s.refresh_interval_sec = s.refresh_interval_sec.max(15);
    s.collapsed_width = s.collapsed_width.clamp(2, 24);
    s.auto_hide_delay_ms = s.auto_hide_delay_ms.min(600_000);

    let mut warn = clamp_f64(s.thresholds.warn, 1.0, 100.0, 70.0);
    let mut critical = clamp_f64(s.thresholds.critical, 1.0, 100.0, 90.0);
    if warn >= critical {
        warn = (critical - 1.0).max(1.0);
        if warn >= critical {
            critical = (warn + 1.0).min(100.0);
        }
    }
    s.thresholds.warn = warn;
    s.thresholds.critical = critical;

    // Every known provider must have an entry so the UI can render a toggle.
    for (id, order) in [("claude", 0), ("codex", 1)] {
        s.providers
            .entry(id.to_string())
            .or_insert(ProviderSettings {
                enabled: true,
                order,
            });
    }
    s
}

fn clamp_f64(v: f64, min: f64, max: f64, fallback: f64) -> f64 {
    if v.is_finite() {
        v.clamp(min, max)
    } else {
        fallback
    }
}

/// Write `settings.json` atomically (temp file + rename).
pub fn save(config_dir: &Path, settings: &Settings) -> Result<()> {
    std::fs::create_dir_all(config_dir).ok();
    let bytes = serde_json::to_vec_pretty(settings).context("serialize settings")?;
    write_atomic(&settings_path(config_dir), &bytes)
}

/// Write `bytes` to `path` via a sibling temp file + rename, so a crash can
/// never leave a truncated file behind.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir).ok();
    let tmp = dir.join(format!(
        ".{}.{}.tmp",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("tmp"),
        std::process::id()
    ));
    std::fs::write(&tmp, bytes).with_context(|| format!("write {}", tmp.display()))?;
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            std::fs::remove_file(&tmp).ok();
            Err(e).with_context(|| format!("rename into {}", path.display()))
        }
    }
}

// ---------- app-level helpers (state + persistence + event) ----------

use crate::model::events;
use crate::state::AppState;
use tauri::{AppHandle, Emitter, Manager};

/// Apply a JSON patch to the live settings: merge, clamp, store, persist and
/// emit `settings-updated`. Returns the effective settings.
pub fn update(app: &AppHandle, patch: &Value) -> Result<Settings> {
    let state = app.state::<AppState>();
    let merged = {
        let current = state.settings.read();
        merge(&current, patch)
    };
    *state.settings.write() = merged.clone();
    if let Err(e) = save(&state.config_dir, &merged) {
        // A read-only config dir must not lose the in-memory change.
        log::error!("could not persist settings: {e:#}");
    }
    emit_updated(app, &merged);
    Ok(merged)
}

/// Persist the settings currently held in `state`.
pub fn save_settings(state: &AppState) -> Result<()> {
    let snapshot = state.settings.read().clone();
    save(&state.config_dir, &snapshot)
}

/// Toggle `autoHide` from outside the command layer (the tray menu).
pub fn set_auto_hide(app: &AppHandle, auto_hide: bool) {
    let state = app.state::<AppState>();
    let updated = {
        let mut guard = state.settings.write();
        guard.auto_hide = auto_hide;
        guard.clone()
    };
    if let Err(e) = save(&state.config_dir, &updated) {
        log::error!("could not persist settings: {e:#}");
    }
    emit_updated(app, &updated);
}

fn emit_updated(app: &AppHandle, settings: &Settings) {
    if let Err(e) = app.emit(events::SETTINGS_UPDATED, settings) {
        log::warn!("could not emit {}: {e}", events::SETTINGS_UPDATED);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_support::tempdir;
    use crate::model::{Edge, RingMode, Theme};
    use serde_json::json;

    #[test]
    fn merge_replaces_scalars_and_keeps_the_rest() {
        let base = Settings::default();
        let merged = merge(
            &base,
            &json!({"edge": "left", "theme": "light", "language": "zh-CN"}),
        );
        assert_eq!(merged.edge, Edge::Left);
        assert_eq!(merged.theme, Theme::Light);
        assert_eq!(merged.language, "zh-CN");
        assert_eq!(merged.ring_mode, base.ring_mode, "untouched fields survive");
    }

    #[test]
    fn providers_are_merged_per_key() {
        let base = Settings::default();
        let merged = merge(
            &base,
            &json!({"providers": {"codex": {"enabled": false, "order": 5}}}),
        );
        assert!(
            merged.providers["claude"].enabled,
            "claude entry is preserved"
        );
        assert!(!merged.providers["codex"].enabled);
        assert_eq!(merged.providers["codex"].order, 5);
    }

    #[test]
    fn invalid_fields_are_ignored_not_fatal() {
        let base = Settings::default();
        let merged = merge(
            &base,
            &json!({"edge": "diagonal", "ringMode": "all", "providers": 7, "unknown": true}),
        );
        assert_eq!(merged.edge, base.edge, "the bad enum value is dropped");
        assert_eq!(merged.ring_mode, RingMode::All, "good fields still apply");
        assert_eq!(merged.providers.len(), 2);
    }

    #[test]
    fn values_are_clamped_into_their_ranges() {
        let base = Settings::default();
        let merged = merge(
            &base,
            &json!({"opacity": 0.05, "scale": 9.0, "refreshIntervalSec": 3, "collapsedWidth": 99}),
        );
        assert_eq!(merged.opacity, 0.3);
        assert_eq!(merged.scale, 1.5);
        assert_eq!(merged.refresh_interval_sec, 15);
        assert_eq!(merged.collapsed_width, 24);

        let low = merge(&base, &json!({"opacity": 2.0, "scale": 0.1}));
        assert_eq!(low.opacity, 1.0);
        assert_eq!(low.scale, 0.75);
    }

    #[test]
    fn thresholds_stay_ordered_and_in_range() {
        let base = Settings::default();
        let m = merge(
            &base,
            &json!({"thresholds": {"warn": 95.0, "critical": 40.0}}),
        );
        assert!(m.thresholds.warn < m.thresholds.critical);
        assert_eq!(m.thresholds.critical, 40.0);
        assert_eq!(m.thresholds.warn, 39.0);

        let m = merge(
            &base,
            &json!({"thresholds": {"warn": -10.0, "critical": 500.0}}),
        );
        assert_eq!(m.thresholds.warn, 1.0);
        assert_eq!(m.thresholds.critical, 100.0);

        let m = merge(
            &base,
            &json!({"thresholds": {"warn": 100.0, "critical": 100.0}}),
        );
        assert_eq!(m.thresholds.warn, 99.0);
        assert_eq!(m.thresholds.critical, 100.0);
    }

    #[test]
    fn load_tolerates_missing_and_broken_files_and_round_trips() {
        let dir = tempdir();
        assert_eq!(load(&dir), Settings::default(), "missing file → defaults");

        std::fs::write(settings_path(&dir), "{ not json").unwrap();
        assert_eq!(load(&dir), Settings::default(), "broken file → defaults");

        std::fs::write(settings_path(&dir), r#"{"edge":"left","opacity":0.1}"#).unwrap();
        let partial = load(&dir);
        assert_eq!(partial.edge, Edge::Left);
        assert_eq!(partial.opacity, 0.3, "clamped on load");

        let s = Settings {
            auto_hide: true,
            vertical_offset: -120,
            ..Settings::default()
        };
        save(&dir, &s).unwrap();
        let back = load(&dir);
        assert!(back.auto_hide);
        assert_eq!(back.vertical_offset, -120);
        std::fs::remove_dir_all(&dir).ok();
    }
}
