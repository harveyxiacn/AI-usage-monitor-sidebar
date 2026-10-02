//! First-run help: where each provider's CLI stands on this machine.
//!
//! Only *existence* of the config directory and of the credentials file is
//! checked — nothing in them is ever opened. The frontend combines this with
//! the snapshot statuses to tell the user what is missing ("CLI not found",
//! "not signed in", "ok") and which command signs in.

use std::path::{Path, PathBuf};

use crate::model::ProviderSetup;

/// Existence checks for one provider. Pure apart from `Path::exists`.
pub fn inspect(
    provider: &str,
    config_dir: Option<PathBuf>,
    credentials: Option<PathBuf>,
    login_steps: &[&str],
    keychain: bool,
) -> ProviderSetup {
    let found = |p: &Option<PathBuf>| p.as_deref().is_some_and(Path::exists);
    let config_dir_found = found(&config_dir);
    // On macOS Claude Code keeps its token in the Keychain, which is not
    // probed here (that would trigger a permission prompt): a present config
    // directory is the best cheap signal.
    let credentials_found = found(&credentials) || (keychain && config_dir_found);
    ProviderSetup {
        provider: provider.to_string(),
        config_dir: config_dir
            .map(|d| d.display().to_string())
            .unwrap_or_default(),
        config_dir_found,
        credentials_found,
        login_steps: login_steps.iter().map(|s| (*s).to_string()).collect(),
    }
}

/// Claude Code and Codex, in display order. Copilot is experimental and has
/// no login of its own, so it is not part of the guided set-up.
pub fn provider_setups() -> Vec<ProviderSetup> {
    use crate::commands::providers::{claude, codex};
    vec![
        inspect(
            "claude",
            claude::config_dir(),
            claude::credentials_path(),
            &["claude", "/login"],
            cfg!(target_os = "macos"),
        ),
        inspect(
            "codex",
            codex::codex_home(),
            codex::credentials_path(),
            &["codex login"],
            false,
        ),
    ]
}

#[tauri::command]
pub async fn get_provider_setup() -> Result<Vec<ProviderSetup>, String> {
    tauri::async_runtime::spawn_blocking(provider_setups)
        .await
        .map_err(|e| format!("background task failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_directory_and_missing_credentials_are_told_apart() {
        let root = crate::commands::test_support::tempdir();
        let none = inspect(
            "claude",
            Some(root.join("nope")),
            Some(root.join("nope/c.json")),
            &["claude"],
            false,
        );
        assert!(!none.config_dir_found && !none.credentials_found);

        let dir = root.join(".claude");
        std::fs::create_dir_all(&dir).unwrap();
        let signed_out = inspect(
            "claude",
            Some(dir.clone()),
            Some(dir.join(".credentials.json")),
            &["claude", "/login"],
            false,
        );
        assert!(signed_out.config_dir_found && !signed_out.credentials_found);
        assert_eq!(signed_out.login_steps, ["claude", "/login"]);

        std::fs::write(dir.join(".credentials.json"), b"{}").unwrap();
        let signed_in = inspect(
            "claude",
            Some(dir.clone()),
            Some(dir.join(".credentials.json")),
            &[],
            false,
        );
        assert!(signed_in.config_dir_found && signed_in.credentials_found);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_macos_keychain_counts_as_credentials_when_the_directory_exists() {
        let root = crate::commands::test_support::tempdir();
        let s = inspect(
            "claude",
            Some(root.clone()),
            Some(root.join(".credentials.json")),
            &[],
            true,
        );
        assert!(s.credentials_found);
        let s = inspect("claude", Some(root.join("missing")), None, &[], true);
        assert!(!s.credentials_found);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn both_guided_providers_have_a_login_command() {
        let setups = provider_setups();
        let ids: Vec<_> = setups.iter().map(|s| s.provider.as_str()).collect();
        assert_eq!(ids, ["claude", "codex"]);
        assert!(setups.iter().all(|s| !s.login_steps.is_empty()));
    }
}
