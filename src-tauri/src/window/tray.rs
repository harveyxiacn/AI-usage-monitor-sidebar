//! Tray icon and menu. [PLATFORM]
//!
//! The icon itself comes from `tauri.conf.json` (`app.trayIcon`, id `main`);
//! here we only attach the menu and the event handlers, so the bundled tray
//! asset stays a build-time concern.

use crate::model::{windows, Settings};
use crate::window::{self, dashboard, sidebar};
use anyhow::anyhow;
use tauri::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::{AppHandle, Emitter, Manager};

pub const TRAY_ID: &str = "main";

mod ids {
    pub const TOGGLE: &str = "toggle_sidebar";
    pub const ALWAYS_SHOW: &str = "always_show";
    pub const REFRESH: &str = "refresh_now";
    pub const DASHBOARD: &str = "open_dashboard";
    pub const SETTINGS: &str = "open_settings";
    pub const QUIT: &str = "quit";
}

/// Tray labels in the user's language. `auto` follows `LANG`/`LC_ALL`.
struct Labels {
    toggle: &'static str,
    always_show: &'static str,
    refresh: &'static str,
    dashboard: &'static str,
    settings: &'static str,
    quit: &'static str,
}

fn labels(settings: &Settings) -> Labels {
    let chinese = match settings.language.as_str() {
        "zh-CN" | "zh" => true,
        "auto" => std::env::var("LC_ALL")
            .or_else(|_| std::env::var("LC_MESSAGES"))
            .or_else(|_| std::env::var("LANG"))
            .map(|v| v.starts_with("zh"))
            .unwrap_or(false),
        _ => false,
    };
    if chinese {
        Labels {
            toggle: "显示 / 隐藏侧边栏",
            always_show: "始终显示侧边栏",
            refresh: "立即刷新",
            dashboard: "打开仪表盘",
            settings: "设置…",
            quit: "退出",
        }
    } else {
        Labels {
            toggle: "Show/Hide sidebar",
            always_show: "Always show sidebar",
            refresh: "Refresh now",
            dashboard: "Open dashboard",
            settings: "Settings…",
            quit: "Quit",
        }
    }
}

/// Attach the menu to the tray icon declared in `tauri.conf.json`.
pub fn build(app: &AppHandle) -> anyhow::Result<()> {
    let settings = window::settings_of(app);
    let l = labels(&settings);

    let toggle = MenuItem::with_id(app, ids::TOGGLE, l.toggle, true, None::<&str>)?;
    // Checked means "do not auto-hide".
    let always_show = CheckMenuItem::with_id(
        app,
        ids::ALWAYS_SHOW,
        l.always_show,
        true,
        !settings.auto_hide,
        None::<&str>,
    )?;
    let refresh = MenuItem::with_id(app, ids::REFRESH, l.refresh, true, None::<&str>)?;
    let open_dashboard = MenuItem::with_id(app, ids::DASHBOARD, l.dashboard, true, None::<&str>)?;
    let open_settings = MenuItem::with_id(app, ids::SETTINGS, l.settings, true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, ids::QUIT, l.quit, true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[
            &toggle,
            &always_show,
            &refresh,
            &open_dashboard,
            &open_settings,
            &separator,
            &quit,
        ],
    )?;

    let tray = app
        .tray_by_id(TRAY_ID)
        .ok_or_else(|| anyhow!("no tray icon with id `{TRAY_ID}` (check tauri.conf.json)"))?;
    tray.set_menu(Some(menu))?;
    tray.on_menu_event(|app, event: MenuEvent| on_menu(app, event.id.as_ref()));

    #[cfg(not(target_os = "linux"))]
    {
        use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
        // Left-click opens the dashboard, right-click shows the menu.
        if let Err(e) = tray.set_show_menu_on_left_click(false) {
            log::debug!("set_show_menu_on_left_click: {e}");
        }
        tray.on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                dashboard::toggle(tray.app_handle());
            }
        });
    }

    if let Some(state) = app.try_state::<window::PlatformState>() {
        *state.always_show_item.lock() = Some(always_show);
    }
    log::info!("tray menu ready");
    Ok(())
}

fn on_menu(app: &AppHandle, id: &str) {
    match id {
        ids::TOGGLE => toggle_sidebar(app),
        ids::ALWAYS_SHOW => {
            let auto_hide = window::settings_of(app).auto_hide;
            // Checked == "always show" == auto_hide off.
            set_auto_hide(app, !auto_hide);
        }
        ids::REFRESH => {
            if let Err(e) = app.emit(window::REFRESH_REQUESTED, ()) {
                log::warn!("emitting {} failed: {e}", window::REFRESH_REQUESTED);
            }
        }
        ids::DASHBOARD => dashboard::open(app, None),
        ids::SETTINGS => dashboard::open(app, Some("settings".into())),
        ids::QUIT => {
            log::info!("quit from tray");
            app.exit(0);
        }
        other => log::debug!("unhandled tray menu id `{other}`"),
    }
}

/// Show or hide the whole bar window (different from collapse/expand).
fn toggle_sidebar(app: &AppHandle) {
    let Some(win) = app.get_webview_window(windows::SIDEBAR) else {
        log::warn!("sidebar window is missing");
        return;
    };
    if win.is_visible().unwrap_or(false) {
        crate::window::popover::hide(app, true);
        if let Err(e) = win.hide() {
            log::warn!("hiding the sidebar failed: {e}");
        }
    } else {
        sidebar::place(app);
        if let Err(e) = win.show() {
            log::warn!("showing the sidebar failed: {e}");
            return;
        }
        window::after_show(&win, window::settings_of(app).always_on_top);
    }
}

/// Persist `autoHide` through the backend, which owns `settings.json` and the
/// `settings-updated` event.
fn set_auto_hide(app: &AppHandle, auto_hide: bool) {
    crate::commands::settings::set_auto_hide(app, auto_hide);
    sync(app, &window::settings_of(app));
    if !auto_hide {
        // "Always show" must take effect immediately, not after the next hover.
        sidebar::set_expanded(app, true);
    }
}

/// Keep the check item in sync with settings changed elsewhere.
pub fn sync(app: &AppHandle, settings: &Settings) {
    let Some(state) = app.try_state::<window::PlatformState>() else {
        return;
    };
    let guard = state.always_show_item.lock();
    if let Some(item) = guard.as_ref() {
        if let Err(e) = item.set_checked(!settings.auto_hide) {
            log::debug!("tray check item update failed: {e}");
        }
    }
}
