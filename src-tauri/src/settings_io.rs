//! Export / import of `settings.json` and the undo ring. [BACKEND]
//!
//! Both dialogs are native and run from Rust (like the CSV export), so the
//! frontend needs no dialog plugin and no filesystem permission. An imported
//! file is nothing special: it goes through the same `settings::update` merge
//! as any other patch, so unknown keys and invalid values are ignored and the
//! result is clamped. The command reports what it ignored and returns the
//! before/after settings so the dashboard can show what changed.

use super::settings::{self, write_atomic};
use super::settings_history::{self, SettingsVersion};
use crate::model::Settings;
use crate::state::AppState;
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;

/// A settings file is a few KB; refuse to read anything absurd.
const MAX_IMPORT_BYTES: u64 = 1024 * 1024;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub path: String,
    pub before: Settings,
    pub after: Settings,
    /// Keys of the file that were unknown, invalid or had to be clamped.
    pub ignored: Vec<String>,
}

/// Parse an imported file into the patch to apply: must be a JSON object, and
/// never carries the transient focus deadline or a schema version over from
/// another machine.
pub fn prepare_import(text: &str) -> Result<Value, String> {
    let mut value: Value =
        serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| e.to_string())?;
    let Some(object) = value.as_object_mut() else {
        return Err("a settings file is a JSON object".into());
    };
    object.remove("focusUntil");
    object.remove("version");
    Ok(value)
}

fn same(patch: &Value, merged: &Value) -> bool {
    match (patch, merged) {
        (Value::Number(a), Value::Number(b)) => a.as_f64() == b.as_f64(),
        (Value::Object(a), Value::Object(b)) => {
            a.iter().all(|(k, v)| b.get(k).is_some_and(|m| same(v, m)))
        }
        _ => patch == merged,
    }
}

/// Which keys of `patch` did not survive merging onto `current` unchanged:
/// unknown keys, values that failed validation, values that were clamped.
/// Objects report as `group.key`.
pub fn ignored_keys(current: &Settings, patch: &Value) -> Vec<String> {
    let Some(patch) = patch.as_object() else {
        return Vec::new();
    };
    let merged = serde_json::to_value(settings::merge(current, &Value::Object(patch.clone())))
        .unwrap_or(Value::Null);
    let mut out = Vec::new();
    for (key, value) in patch {
        let Some(merged_value) = merged.get(key) else {
            out.push(key.clone());
            continue;
        };
        match (value, merged_value) {
            (Value::Object(sub), Value::Object(merged_sub)) if key != "customPresets" => {
                for (k, v) in sub {
                    if !merged_sub.get(k).is_some_and(|m| same(v, m)) {
                        out.push(format!("{key}.{k}"));
                    }
                }
            }
            _ if same(value, merged_value) => {}
            _ => out.push(key.clone()),
        }
    }
    out
}

#[tauri::command]
pub async fn export_settings(
    window: tauri::WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    let current = state.settings.read().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(destination) = window
            .dialog()
            .file()
            .set_parent(&window)
            .add_filter("JSON", &["json"])
            .set_file_name("ai-usage-sidebar-settings.json")
            .blocking_save_file()
        else {
            return Ok(None);
        };
        let path = destination.into_path().map_err(|e| e.to_string())?;
        let bytes = serde_json::to_vec_pretty(&current).map_err(|e| e.to_string())?;
        write_atomic(&path, &bytes).map_err(|e| format!("Could not save settings: {e:#}"))?;
        Ok(Some(path.to_string_lossy().into_owned()))
    })
    .await
    .map_err(|e| format!("Settings export failed: {e}"))?
}

#[tauri::command]
pub async fn import_settings(
    app: AppHandle,
    window: tauri::WebviewWindow,
) -> Result<Option<ImportResult>, String> {
    let picker = window.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || {
        picker
            .dialog()
            .file()
            .set_parent(&picker)
            .add_filter("JSON", &["json"])
            .blocking_pick_file()
    })
    .await
    .map_err(|e| format!("Settings import failed: {e}"))?;
    let Some(picked) = picked else {
        return Ok(None);
    };
    let path = picked.into_path().map_err(|e| e.to_string())?;
    let length = std::fs::metadata(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .len();
    if length > MAX_IMPORT_BYTES {
        return Err(format!(
            "{} is too large for a settings file",
            path.display()
        ));
    }
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let patch = prepare_import(&text).map_err(|e| format!("{}: {e}", path.display()))?;

    let before = app.state::<AppState>().settings.read().clone();
    let ignored = ignored_keys(&before, &patch);
    let after = settings::update(&app, &patch).map_err(|e| format!("{e:#}"))?;
    Ok(Some(ImportResult {
        path: path.to_string_lossy().into_owned(),
        before,
        after,
        ignored,
    }))
}

/// The undo ring, newest first.
#[tauri::command]
pub async fn get_settings_history(
    state: State<'_, AppState>,
) -> Result<Vec<SettingsVersion>, String> {
    Ok(settings_history::load(&state.config_dir))
}

/// Make ring entry `index` the live settings again.
#[tauri::command]
pub async fn restore_settings_version(app: AppHandle, index: usize) -> Result<Settings, String> {
    settings::restore_version(&app, index).map_err(|e| format!("{e:#}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_import_must_be_an_object_and_drops_machine_specific_keys() {
        assert!(prepare_import("[]").is_err());
        assert!(prepare_import("nope").is_err());
        let patch =
            prepare_import("\u{feff}{\"edge\":\"left\",\"focusUntil\":-1,\"version\":9}").unwrap();
        assert_eq!(patch, json!({"edge": "left"}));
    }

    #[test]
    fn ignored_keys_lists_unknown_invalid_and_clamped_values() {
        let current = Settings::default();
        let patch = json!({
            "edge": "left",
            "theme": "plaid",
            "bogus": 1,
            "scale": 9,
            "opacity": 1,
            "sizes": {"ringSize": 56, "ringStroke": 1},
            "providers": {"codex": {"enabled": false}},
        });
        let mut ignored = ignored_keys(&current, &patch);
        ignored.sort();
        assert_eq!(ignored, ["bogus", "scale", "sizes.ringStroke", "theme"]);
    }

    #[test]
    fn integers_and_floats_compare_equal() {
        // `1` in a file, `1.0` after the round trip through f64 settings.
        assert!(ignored_keys(&Settings::default(), &json!({"scale": 1, "opacity": 1})).is_empty());
    }

    #[test]
    fn restoring_pushes_the_replaced_version_and_clamps_like_any_input() {
        // The ring itself is covered in settings_history; here: a hand-edited
        // entry cannot smuggle an out-of-range value in.
        let weird = Settings {
            scale: 99.0,
            ..Settings::default()
        };
        let target = settings::merge(&Settings::default(), &serde_json::to_value(&weird).unwrap());
        assert_eq!(target.scale, 1.5);
    }
}
