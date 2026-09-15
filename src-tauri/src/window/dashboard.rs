//! The normal window (overview / history / settings). [PLATFORM]

use crate::model::{events, windows};
use tauri::{AppHandle, Emitter, Manager};

pub const DEFAULT_TAB: &str = "overview";

/// `open_dashboard`: show, focus and navigate.
pub fn open(app: &AppHandle, tab: Option<String>) {
    let tab = tab
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| DEFAULT_TAB.to_string());

    let Some(win) = app.get_webview_window(windows::DASHBOARD) else {
        log::error!("dashboard window is missing from tauri.conf.json");
        return;
    };
    if let Err(e) = win.unminimize() {
        log::debug!("dashboard unminimize: {e}");
    }
    if let Err(e) = win.show() {
        log::warn!("showing the dashboard failed: {e}");
    }
    // Unlike the overlay windows the dashboard *should* take focus.
    if let Err(e) = win.set_focus() {
        log::warn!("focusing the dashboard failed: {e}");
    }
    if let Err(e) = app.emit_to(
        windows::DASHBOARD,
        events::DASHBOARD_NAVIGATE,
        serde_json::json!({ "tab": tab }),
    ) {
        log::warn!("emitting {} failed: {e}", events::DASHBOARD_NAVIGATE);
    }
}

/// Used by the single-instance handler: a second launch focuses the dashboard
/// instead of starting another copy.
pub fn focus(app: &AppHandle) {
    open(app, None);
}

/// Tray left-click on platforms where that is the convention.
#[cfg(not(target_os = "linux"))]
pub fn toggle(app: &AppHandle) {
    match app.get_webview_window(windows::DASHBOARD) {
        Some(win) if win.is_visible().unwrap_or(false) => {
            if let Err(e) = win.hide() {
                log::warn!("hiding the dashboard failed: {e}");
            }
        }
        Some(_) => open(app, None),
        None => log::error!("dashboard window is missing"),
    }
}
