//! CSV export through a native, user-selected destination on every desktop OS.

use tauri_plugin_dialog::DialogExt;

fn write_csv(path: &std::path::Path, csv: &str) -> Result<(), String> {
    // A UTF-8 BOM allows Excel on Windows to recognise non-ASCII model names.
    let content = format!("\u{feff}{}", csv.trim_start_matches('\u{feff}'));
    crate::commands::settings::write_atomic(path, content.as_bytes())
        .map_err(|e| format!("Could not save CSV: {e}"))
}

fn suggested_csv_name(name: &str) -> String {
    // A suggestion is only a basename; the dialog owns destination selection.
    let name = name.rsplit(['/', '\\']).next().unwrap_or_default();
    if name.is_empty() || name.chars().any(char::is_control) {
        return "ai-usage.csv".into();
    }
    if name.to_ascii_lowercase().ends_with(".csv") {
        name.to_string()
    } else {
        format!("{name}.csv")
    }
}

#[tauri::command]
pub async fn export_usage_csv(
    window: tauri::WebviewWindow,
    csv: String,
    suggested_name: String,
) -> Result<Option<String>, String> {
    // The dialog may wait indefinitely; never block the GUI or async executor.
    tauri::async_runtime::spawn_blocking(move || {
        let Some(destination) = window
            .dialog()
            .file()
            .set_parent(&window)
            .add_filter("CSV", &["csv"])
            .set_file_name(suggested_csv_name(&suggested_name))
            .blocking_save_file()
        else {
            return Ok(None);
        };
        let path = destination.into_path().map_err(|e| e.to_string())?;
        write_csv(&path, &csv)?;
        Ok(Some(path.to_string_lossy().into_owned()))
    })
    .await
    .map_err(|e| format!("CSV export failed: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggested_names_are_portable_basenames() {
        assert_eq!(suggested_csv_name("/tmp/usage.csv"), "usage.csv");
        assert_eq!(suggested_csv_name("C:\\reports\\usage.CSV"), "usage.CSV");
        assert_eq!(suggested_csv_name("使用量"), "使用量.csv");
        assert_eq!(suggested_csv_name(""), "ai-usage.csv");
        assert_eq!(suggested_csv_name("bad\nname.csv"), "ai-usage.csv");
    }

    #[test]
    fn export_replaces_csv_with_one_utf8_bom_and_preserves_files_on_failure() {
        let dir = crate::commands::test_support::tempdir();
        let path = dir.join("usage.csv");
        std::fs::write(&path, "previous report").unwrap();
        write_csv(&path, "\u{feff}model,tokens\r\n模型,42\r\n").unwrap();
        let expected = "\u{feff}model,tokens\r\n模型,42\r\n";
        assert_eq!(std::fs::read_to_string(&path).unwrap(), expected);
        // The existing report is a file, so a child destination cannot exist.
        assert!(write_csv(&path.join("child.csv"), "replacement").is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), expected);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
