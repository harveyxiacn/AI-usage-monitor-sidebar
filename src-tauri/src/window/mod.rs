//! Window / tray / platform commands. [PLATFORM owns this directory]
//! Signatures are part of the contract (docs/ARCHITECTURE.md §5).

use crate::model::*;
use tauri::AppHandle;

/// Called once from `lib.rs` inside `.setup()`.
pub fn setup(app: &AppHandle) -> anyhow::Result<()> {
    let _ = app;
    // TODO(platform): create/position sidebar, popover, dashboard windows;
    // tray icon + menu; autostart; hover state machine.
    Ok(())
}

/// Name of the windowing backend in use ("x11" | "wayland" | "cocoa" | "win32").
pub fn backend_name() -> String {
    #[cfg(target_os = "linux")]
    {
        if std::env::var("GDK_BACKEND").map(|v| v == "x11").unwrap_or(false)
            || std::env::var("WAYLAND_DISPLAY").is_err()
        {
            return "x11".into();
        }
        return "wayland".into();
    }
    #[cfg(target_os = "macos")]
    {
        "cocoa".into()
    }
    #[cfg(target_os = "windows")]
    {
        "win32".into()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        "unknown".into()
    }
}

#[tauri::command]
pub async fn sidebar_set_expanded(app: AppHandle, expanded: bool) -> Result<(), String> {
    let _ = (app, expanded);
    Ok(())
}

#[tauri::command]
pub async fn sidebar_relayout(app: AppHandle, width: f64, height: f64) -> Result<(), String> {
    let _ = (app, width, height);
    Ok(())
}

#[tauri::command]
pub async fn popover_show(app: AppHandle, req: PopoverRequest) -> Result<(), String> {
    let _ = (app, req);
    Ok(())
}

#[tauri::command]
pub async fn popover_hide(app: AppHandle) -> Result<(), String> {
    let _ = app;
    Ok(())
}

#[tauri::command]
pub async fn popover_set_pinned(app: AppHandle, pinned: bool) -> Result<(), String> {
    let _ = (app, pinned);
    Ok(())
}

#[tauri::command]
pub async fn hover_report(app: AppHandle, source: String, hovered: bool) -> Result<(), String> {
    let _ = (app, source, hovered);
    Ok(())
}

#[tauri::command]
pub async fn open_dashboard(app: AppHandle, tab: Option<String>) -> Result<(), String> {
    let _ = (app, tab);
    Ok(())
}

#[tauri::command]
pub async fn apply_window_settings(app: AppHandle) -> Result<(), String> {
    let _ = app;
    Ok(())
}

#[tauri::command]
pub async fn get_monitors(app: AppHandle) -> Result<Vec<MonitorInfo>, String> {
    let _ = app;
    Ok(vec![])
}

#[tauri::command]
pub async fn quit_app(app: AppHandle) -> Result<(), String> {
    app.exit(0);
    Ok(())
}
