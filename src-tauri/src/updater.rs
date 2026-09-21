//! In-app update check (tauri-plugin-updater). [PLATFORM owns this file]
//!
//! Policy — nothing happens behind the user's back:
//! * a check runs [`FIRST_CHECK_DELAY_SEC`] after start-up and then once a day,
//!   but only while `settings.autoUpdateCheck` is on;
//! * a check never downloads anything, it only reads `latest.json`;
//! * the result is surfaced in the tray menu and in the dashboard, and the
//!   download/install only starts from `install_update`, i.e. from a click;
//! * builds we must not overwrite ([`can_install_in_place`]) get a link to the
//!   release page instead of an install button.
//!
//! The signing key story (why `latest.json` may be missing from a release) is
//! documented in `docs/RELEASING.md`.

use crate::model::{events, UpdateStatus};
use parking_lot::Mutex;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::UpdaterExt;

/// Release page for builds that cannot be replaced in place.
pub const RELEASES_URL: &str = "https://github.com/harveyxiacn/AI-usage-monitor-sidebar/releases";
/// Delay before the first automatic check, so start-up stays quiet and fast.
pub const FIRST_CHECK_DELAY_SEC: u64 = 30;
/// Period of the automatic check afterwards.
pub const CHECK_INTERVAL_SEC: u64 = 24 * 60 * 60;

/// The last known update state, shared by the tray and the three windows.
#[derive(Default)]
pub struct UpdateState {
    pub status: Mutex<UpdateStatus>,
}

/// Can the running build replace itself?
///
/// Tauri's updater knows how to swap an AppImage, a macOS `.app` and a Windows
/// installer. A `.deb`/`.rpm` belongs to the package manager, and
/// `scripts/install-linux.sh` drops a plain binary into `~/.local/bin` — for
/// both of those an in-place install would fight the real owner of the files,
/// so we only offer the release page. An AppImage identifies itself with the
/// `APPIMAGE` environment variable its runtime sets.
pub fn can_install_in_place() -> bool {
    #[cfg(target_os = "linux")]
    {
        std::env::var_os("APPIMAGE").is_some()
    }
    #[cfg(not(target_os = "linux"))]
    {
        true
    }
}

/// Called once from `lib.rs` inside `.setup()`.
pub fn setup(app: &AppHandle) {
    app.manage(UpdateState::default());
    let initial = UpdateStatus {
        current_version: app.package_info().version.to_string(),
        release_url: RELEASES_URL.to_string(),
        can_install: can_install_in_place(),
        ..UpdateStatus::default()
    };
    store(app, initial);

    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(FIRST_CHECK_DELAY_SEC)).await;
        loop {
            if crate::window::settings_of(&handle).auto_update_check {
                check(&handle).await;
            }
            tokio::time::sleep(std::time::Duration::from_secs(CHECK_INTERVAL_SEC)).await;
        }
    });
}

/// Current state; the defaults are good enough before `setup` ran.
pub fn status(app: &AppHandle) -> UpdateStatus {
    match app.try_state::<UpdateState>() {
        Some(state) => state.status.lock().clone(),
        None => UpdateStatus::default(),
    }
}

/// Replace the state and tell everyone who shows it.
fn store(app: &AppHandle, status: UpdateStatus) {
    if let Some(state) = app.try_state::<UpdateState>() {
        *state.status.lock() = status.clone();
    }
    crate::window::tray::sync_update(app, &status);
    if let Err(e) = app.emit(events::UPDATE_STATUS, &status) {
        log::warn!("could not emit {}: {e}", events::UPDATE_STATUS);
    }
}

/// Mutate the state in place and publish the result.
fn update_status(app: &AppHandle, f: impl FnOnce(&mut UpdateStatus)) -> UpdateStatus {
    let mut next = status(app);
    f(&mut next);
    store(app, next.clone());
    next
}

/// Ask the release feed once. Errors are kept in the status, never fatal —
/// a laptop that is offline must not produce a dialog every day.
pub async fn check(app: &AppHandle) -> UpdateStatus {
    if status(app).checking {
        return status(app);
    }
    update_status(app, |s| {
        s.checking = true;
        s.error = None;
    });

    let result = match app.updater() {
        Ok(updater) => updater.check().await.map_err(|e| e.to_string()),
        Err(e) => Err(e.to_string()),
    };
    let now = chrono::Utc::now().to_rfc3339();

    update_status(app, |s| {
        s.checking = false;
        s.checked_at = Some(now);
        match result {
            Ok(Some(update)) => {
                log::info!("update {} available (running {})", update.version, s.current_version);
                s.available = Some(update.version);
                s.notes = update.body;
            }
            Ok(None) => {
                log::debug!("no update available");
                s.available = None;
                s.notes = None;
            }
            Err(e) => {
                log::warn!("update check failed: {e}");
                s.error = Some(e);
            }
        }
    })
}

/// Download and install the offered update, then restart. Only reachable from
/// an explicit user action, and only for bundles we own ([`can_install_in_place`]).
pub async fn install(app: &AppHandle) -> Result<(), String> {
    let current = status(app);
    if current.installing {
        return Err("an install is already running".into());
    }
    if !current.can_install {
        return Err(format!(
            "this build was installed by a package manager; download the new version from {RELEASES_URL}"
        ));
    }
    let Some(version) = current.available.clone() else {
        return Err("no update is available".into());
    };

    update_status(app, |s| {
        s.installing = true;
        s.error = None;
    });

    let outcome = async {
        let update = app
            .updater()
            .map_err(|e| e.to_string())?
            .check()
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "the update disappeared from the release feed".to_string())?;
        log::info!("installing update {}", update.version);
        update
            .download_and_install(|_chunk, _total| {}, || log::info!("update downloaded"))
            .await
            .map_err(|e| e.to_string())
    }
    .await;

    match outcome {
        Ok(()) => {
            log::info!("update {version} installed, restarting");
            // Windows exits inside `install`; macOS and Linux need this.
            app.restart();
        }
        Err(e) => {
            log::error!("installing the update failed: {e}");
            update_status(app, |s| {
                s.installing = false;
                s.error = Some(e.clone());
            });
            Err(e)
        }
    }
}

// ---------------------------------------------------------------- commands --

#[tauri::command]
pub async fn get_update_status(app: AppHandle) -> Result<UpdateStatus, String> {
    Ok(status(&app))
}

#[tauri::command]
pub async fn check_for_updates(app: AppHandle) -> Result<UpdateStatus, String> {
    Ok(check(&app).await)
}

#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    install(&app).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Linux rule is the whole point of `can_install`: only an AppImage
    /// may be replaced in place, everything else gets the release link.
    #[test]
    #[cfg(target_os = "linux")]
    fn only_appimages_install_themselves_on_linux() {
        // The test binary is not an AppImage, so no `APPIMAGE` is set.
        assert!(std::env::var_os("APPIMAGE").is_none());
        assert!(!can_install_in_place());
    }

    #[test]
    fn the_release_url_points_at_the_project_releases() {
        assert!(RELEASES_URL.ends_with("/releases"));
        assert!(RELEASES_URL.starts_with("https://github.com/"));
    }
}
