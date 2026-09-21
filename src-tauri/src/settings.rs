//! `settings.json` load / merge / clamp / atomic save. [BACKEND owns this file]
//!
//! Loading never fails: a missing, partial or partly-invalid file degrades to
//! the defaults for the fields it cannot supply.

use crate::model::{ColorSettings, ProviderSettings, Settings, SizeSettings};
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
/// Known setting groups merge per key, including fields inside providers.
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
        if matches!(
            key.as_str(),
            "providers" | "colors" | "sizes" | "thresholds"
        ) {
            // per-key merge so a patch can toggle one provider / one colour only
            let mut merged = match current.get(key.as_str()) {
                Some(Value::Object(o)) => o.clone(),
                _ => serde_json::Map::new(),
            };
            match value.as_object() {
                Some(src) => {
                    for (pk, pv) in src {
                        let mut nested = merged.clone();
                        let value = if key == "providers" {
                            match (nested.get(pk).and_then(Value::as_object), pv.as_object()) {
                                (Some(base), Some(patch)) => {
                                    let mut provider = base.clone();
                                    provider.extend(patch.clone());
                                    Value::Object(provider)
                                }
                                _ => pv.clone(),
                            }
                        } else {
                            pv.clone()
                        };
                        nested.insert(pk.clone(), value);
                        candidate.insert(key.clone(), Value::Object(nested.clone()));
                        if serde_json::from_value::<Settings>(Value::Object(candidate.clone()))
                            .is_ok()
                        {
                            merged = nested;
                        } else {
                            log::warn!("settings patch: ignoring field `{key}.{pk}`");
                        }
                    }
                }
                None => {
                    log::warn!("settings patch: `{key}` is not an object, ignored");
                    continue;
                }
            }
            candidate.insert(key.clone(), Value::Object(merged));
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

/// `value` if it is a CSS hex colour (or empty), otherwise `fallback`.
fn hex_or(value: &str, fallback: &str) -> String {
    let v = value.trim();
    if v.is_empty() {
        return fallback.to_string();
    }
    let ok = v.starts_with('#')
        && matches!(v.len(), 4 | 7 | 9)
        && v[1..].chars().all(|c| c.is_ascii_hexdigit());
    if ok {
        v.to_ascii_lowercase()
    } else {
        fallback.to_string()
    }
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

    // Geometry (px at scale 1) — ARCHITECTURE §7 "Colours and sizes".
    let d = SizeSettings::default();
    s.sizes.ring_size = clamp_f64(s.sizes.ring_size, 40.0, 96.0, d.ring_size);
    s.sizes.ring_stroke = clamp_f64(s.sizes.ring_stroke, 3.0, 8.0, d.ring_stroke);
    s.sizes.bar_gap = clamp_f64(s.sizes.bar_gap, 6.0, 40.0, d.bar_gap);
    s.sizes.bar_padding = clamp_f64(s.sizes.bar_padding, 4.0, 24.0, d.bar_padding);
    s.sizes.corner_radius = clamp_f64(s.sizes.corner_radius, 8.0, 40.0, d.corner_radius);
    s.sizes.label_size = clamp_f64(s.sizes.label_size, 9.0, 18.0, d.label_size);

    // Colours must be CSS hex (#rgb, #rrggbb, #rrggbbaa); anything else falls
    // back to the default (or the theme default for the optional ones).
    let dc = ColorSettings::default();
    s.colors.claude = hex_or(&s.colors.claude, &dc.claude);
    s.colors.codex = hex_or(&s.colors.codex, &dc.codex);
    s.colors.warn = hex_or(&s.colors.warn, &dc.warn);
    s.colors.critical = hex_or(&s.colors.critical, &dc.critical);
    s.colors.surface = hex_or(&s.colors.surface, "");
    s.colors.text = hex_or(&s.colors.text, "");

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
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir).context("create settings directory")?;
    let tmp = dir.join(format!(
        ".{}.{}.{}.tmp",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("tmp"),
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| -> Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .with_context(|| format!("create {}", tmp.display()))?;
        file.write_all(bytes)
            .with_context(|| format!("write {}", tmp.display()))?;
        file.sync_all().context("flush settings file")?;
        drop(file);
        std::fs::rename(&tmp, path).with_context(|| format!("rename into {}", path.display()))?;
        Ok(())
    })();
    if result.is_err() {
        std::fs::remove_file(&tmp).ok();
    }
    result
}

// ---------- app-level helpers (state + persistence + event) ----------

use crate::model::events;
use crate::state::AppState;
use tauri::{AppHandle, Emitter, Manager};

/// Apply a JSON patch to the live settings: merge, clamp, store, persist and
/// emit `settings-updated`. Returns the effective settings.
pub fn update(app: &AppHandle, patch: &Value) -> Result<Settings> {
    let state = app.state::<AppState>();
    persist_patch(&state.settings, &state.config_dir, patch, |merged| {
        emit_updated(app, merged)
    })
}

/// Persist the settings currently held in `state`.
pub fn save_settings(state: &AppState) -> Result<()> {
    let current = state.settings.read();
    save(&state.config_dir, &current)
}

fn persist_patch(
    settings: &parking_lot::RwLock<Settings>,
    config_dir: &Path,
    patch: &Value,
    notify: impl FnOnce(&Settings),
) -> Result<Settings> {
    // Hold one write lock through read/merge/save/notify so concurrent windows
    // cannot overwrite patches or deliver stale events after newer settings.
    let mut current = settings.write();
    let merged = merge(&current, patch);
    save(config_dir, &merged)?;
    *current = merged.clone();
    notify(&merged);
    Ok(merged)
}

/// Toggle `autoHide` from outside the command layer (the tray menu).
pub fn set_auto_hide(app: &AppHandle, auto_hide: bool) {
    if let Err(e) = update(app, &serde_json::json!({"autoHide": auto_hide})) {
        log::error!("could not persist settings: {e:#}");
    }
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
    fn colors_and_sizes_are_merged_and_clamped() {
        let merged = merge(
            &Settings::default(),
            &json!({"colors": {"claude": "#123ABC", "warn": "not-a-colour"}, "sizes": {"ringSize": 500, "ringStroke": 1}}),
        );
        assert_eq!(
            merged.colors.claude, "#123abc",
            "valid hex is kept, lower-cased"
        );
        assert_eq!(
            merged.colors.warn, "#f5c542",
            "invalid hex falls back to the default"
        );
        assert_eq!(
            merged.colors.codex, "#10a37f",
            "untouched keys survive a per-key merge"
        );
        assert_eq!(
            merged.sizes.ring_size, 96.0,
            "ring size is clamped to its maximum"
        );
        assert_eq!(
            merged.sizes.ring_stroke, 3.0,
            "stroke is clamped to its minimum"
        );
        assert_eq!(
            merged.sizes.bar_gap, 18.0,
            "untouched sizes keep their default"
        );
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

    #[test]
    fn nested_patches_preserve_other_members_and_salvage_valid_values() {
        let base = Settings::default();
        let merged = merge(
            &base,
            &json!({
                "providers": {"codex": {"enabled": false}, "claude": "invalid"},
                "colors": {"claude": "#123456", "codex": 7},
                "sizes": {"ringSize": 80, "barGap": "invalid"},
                "thresholds": {"warn": 60}
            }),
        );
        assert!(!merged.providers["codex"].enabled);
        assert_eq!(
            merged.providers["codex"].order,
            base.providers["codex"].order
        );
        assert_eq!(merged.providers["claude"], base.providers["claude"]);
        assert_eq!(merged.colors.claude, "#123456");
        assert_eq!(merged.colors.codex, base.colors.codex);
        assert_eq!(merged.sizes.ring_size, 80.0);
        assert_eq!(merged.sizes.bar_gap, base.sizes.bar_gap);
        assert_eq!(merged.thresholds.warn, 60.0);
        assert_eq!(merged.thresholds.critical, base.thresholds.critical);
    }

    /// A `settings.json` written before the updater and the global shortcuts
    /// existed must keep working and pick up their defaults.
    #[test]
    fn settings_files_without_the_newer_keys_keep_working() {
        let dir = tempdir();
        std::fs::write(
            settings_path(&dir),
            r#"{"edge":"left","autostart":true,"theme":"light"}"#,
        )
        .unwrap();
        let loaded = load(&dir);
        assert_eq!(loaded.edge, Edge::Left);
        assert!(loaded.autostart, "the old keys still apply");
        assert!(
            loaded.auto_update_check,
            "checking for updates is the default"
        );
        assert_eq!(loaded.shortcut_toggle_sidebar, "");
        assert_eq!(
            loaded.shortcut_open_dashboard, "",
            "no global shortcut is registered unless the user asks for one"
        );

        let patched = merge(
            &loaded,
            &json!({"autoUpdateCheck": false, "shortcutToggleSidebar": "Ctrl+Alt+U"}),
        );
        assert!(!patched.auto_update_check);
        assert_eq!(patched.shortcut_toggle_sidebar, "Ctrl+Alt+U");
        assert_eq!(patched.shortcut_open_dashboard, "");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn persistence_failure_leaves_live_settings_unchanged() {
        let dir = tempdir();
        let impossible_dir = dir.join("plain-file");
        std::fs::write(&impossible_dir, "file, not a directory").unwrap();
        let current = parking_lot::RwLock::new(Settings::default());
        assert!(persist_patch(
            &current,
            &impossible_dir,
            &json!({"autoHide": true}),
            |_| { panic!("a failed save must not emit an update") }
        )
        .is_err());
        assert!(!current.read().auto_hide);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn concurrent_patches_are_serialized_and_persisted_together() {
        let dir = tempdir();
        let current = parking_lot::RwLock::new(Settings::default());
        let emitted = parking_lot::Mutex::new(Vec::new());
        let notify = |settings: &Settings| {
            assert!(
                current.try_read().is_none(),
                "the write lock must cover notification delivery"
            );
            emitted.lock().push(settings.clone());
        };
        std::thread::scope(|scope| {
            scope.spawn(|| {
                persist_patch(&current, &dir, &json!({"autoHide": true}), notify).unwrap()
            });
            scope.spawn(|| {
                persist_patch(&current, &dir, &json!({"theme": "light"}), notify).unwrap()
            });
        });
        let saved = load(&dir);
        assert!(saved.auto_hide);
        assert_eq!(saved.theme, Theme::Light);
        assert_eq!(saved, *current.read());
        assert_eq!(emitted.lock().len(), 2);
        assert_eq!(emitted.lock().last(), Some(&saved));
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
