//! Extra-account helpers: the folder picker and the "is this the right
//! folder" check behind the Accounts settings card. [BACKEND]
//!
//! Only the *existence* of the folder and of the credentials file is checked;
//! nothing in them is ever opened here.

use crate::model::AccountCheck;
use std::path::Path;
use tauri_plugin_dialog::DialogExt;

/// Existence checks for one prospective account. Pure apart from `Path::is_*`.
pub fn check(provider: &str, config_dir: &str, macos: bool) -> AccountCheck {
    let dir = Path::new(config_dir.trim());
    let absolute = dir.is_absolute();
    let credentials_file = match provider {
        crate::commands::providers::CLAUDE_ID => {
            crate::commands::providers::claude::credentials_path_in(dir)
        }
        crate::commands::providers::CODEX_ID => {
            crate::commands::providers::codex::auth_path_in(dir)
        }
        _ => dir.join(""),
    };
    let dir_found = absolute && dir.is_dir();
    let credentials_found = absolute && credentials_file.is_file();
    // An extra Claude account is read from its credentials file only (the
    // Keychain item of a non-default dir is not known), so on macOS a missing
    // file is a hard "not supported", not just "not signed in yet".
    let keychain_only =
        macos && provider == crate::commands::providers::CLAUDE_ID && !credentials_found;
    AccountCheck {
        absolute,
        dir_found,
        credentials_found,
        credentials_file: credentials_file.display().to_string(),
        keychain_only,
    }
}

#[tauri::command]
pub async fn check_account_dir(
    provider: String,
    config_dir: String,
) -> Result<AccountCheck, String> {
    tauri::async_runtime::spawn_blocking(move || {
        check(&provider, &config_dir, cfg!(target_os = "macos"))
    })
    .await
    .map_err(|e| format!("background task failed: {e}"))
}

/// Native folder dialog; `None` when the user cancels.
#[tauri::command]
pub async fn pick_account_folder(window: tauri::WebviewWindow) -> Result<Option<String>, String> {
    match window
        .dialog()
        .file()
        .set_parent(&window)
        .blocking_pick_folder()
    {
        Some(p) => p
            .into_path()
            .map(|p| Some(p.display().to_string()))
            .map_err(|e| e.to_string()),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_and_credentials_are_reported_separately_and_contents_are_never_read() {
        let root = crate::commands::test_support::tempdir();
        let dir = root.join("work");
        let s = |d: &Path, p: &str, macos| check(p, &d.display().to_string(), macos);

        assert!(!s(&dir, "claude", false).dir_found);
        std::fs::create_dir_all(&dir).unwrap();
        let empty = s(&dir, "claude", false);
        assert!(empty.dir_found && !empty.credentials_found && !empty.keychain_only);
        assert!(empty.credentials_file.ends_with(".credentials.json"));

        // contents are irrelevant: an unparsable file still counts as present
        std::fs::write(dir.join(".credentials.json"), b"not even json").unwrap();
        assert!(s(&dir, "claude", false).credentials_found);
        assert!(s(&dir, "claude", true).credentials_found);
        assert!(!s(&dir, "codex", false).credentials_found);
        std::fs::write(dir.join("auth.json"), b"{}").unwrap();
        let codex = s(&dir, "codex", false);
        assert!(codex.credentials_found && codex.credentials_file.ends_with("auth.json"));

        // a relative path is never accepted, whatever exists under it
        let relative = check("claude", "work", false);
        assert!(!relative.absolute && !relative.dir_found);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn macos_extra_claude_accounts_need_a_credentials_file() {
        let root = crate::commands::test_support::tempdir();
        let dir = root.display().to_string();
        assert!(check("claude", &dir, true).keychain_only);
        assert!(!check("claude", &dir, false).keychain_only);
        assert!(!check("codex", &dir, true).keychain_only);
        std::fs::remove_dir_all(&root).ok();
    }
}
