//! CSV export through a native, user-selected destination on every desktop OS.

use tauri_plugin_dialog::DialogExt;

fn write_csv(path: &std::path::Path, csv: &str) -> Result<(), String> {
    // A UTF-8 BOM allows Excel on Windows to recognise non-ASCII model names.
    let content = format!("\u{feff}{}", csv.trim_start_matches('\u{feff}'));
    crate::commands::settings::write_atomic(path, content.as_bytes())
        .map_err(|e| format!("Could not save CSV: {e}"))
}

fn suggested_name(name: &str, extension: &str, fallback: &str) -> String {
    // A suggestion is only a basename; the dialog owns destination selection.
    let name = name.rsplit(['/', '\\']).next().unwrap_or_default();
    if name.is_empty() || name.chars().any(char::is_control) {
        return fallback.into();
    }
    if name
        .to_ascii_lowercase()
        .ends_with(&format!(".{extension}"))
    {
        name.to_string()
    } else {
        format!("{name}.{extension}")
    }
}

fn suggested_csv_name(name: &str) -> String {
    suggested_name(name, "csv", "ai-usage.csv")
}

/// A share card is a 1200x630 PNG of well under a megabyte; refuse anything
/// that is not a PNG or is implausibly large before a dialog is even shown.
const MAX_PNG_BYTES: usize = 8 * 1024 * 1024;
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

fn check_png(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() > MAX_PNG_BYTES {
        return Err("The image is too large to save".into());
    }
    if !bytes.starts_with(&PNG_SIGNATURE) {
        return Err("The image is not a PNG".into());
    }
    Ok(())
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

/// Save the usage share card (rendered in the webview) through a native
/// save dialog. `None` = the user cancelled.
#[tauri::command]
pub async fn save_share_card(
    window: tauri::WebviewWindow,
    png: Vec<u8>,
    suggested_name_hint: String,
) -> Result<Option<String>, String> {
    check_png(&png)?;
    tauri::async_runtime::spawn_blocking(move || {
        let Some(destination) = window
            .dialog()
            .file()
            .set_parent(&window)
            .add_filter("PNG", &["png"])
            .set_file_name(suggested_name(
                &suggested_name_hint,
                "png",
                "ai-usage-card.png",
            ))
            .blocking_save_file()
        else {
            return Ok(None);
        };
        let path = destination.into_path().map_err(|e| e.to_string())?;
        crate::commands::settings::write_atomic(&path, &png)
            .map_err(|e| format!("Could not save the image: {e}"))?;
        Ok(Some(path.to_string_lossy().into_owned()))
    })
    .await
    .map_err(|e| format!("Saving the image failed: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_names_are_portable_basenames_and_images_are_checked() {
        assert_eq!(suggested_name("/tmp/card", "png", "x.png"), "card.png");
        assert_eq!(
            suggested_name("C:\\a\\card.PNG", "png", "x.png"),
            "card.PNG"
        );
        assert_eq!(suggested_name("bad\nname", "png", "x.png"), "x.png");
        assert!(check_png(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 1]).is_ok());
        assert!(check_png(b"<html>").is_err());
        assert!(check_png(&vec![0x89; MAX_PNG_BYTES + 1]).is_err());
    }

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
