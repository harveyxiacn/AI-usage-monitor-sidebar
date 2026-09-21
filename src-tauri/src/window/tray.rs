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

#[derive(Clone)]
pub struct MenuItems {
    toggle: MenuItem<tauri::Wry>,
    always_show: CheckMenuItem<tauri::Wry>,
    refresh: MenuItem<tauri::Wry>,
    dashboard: MenuItem<tauri::Wry>,
    settings: MenuItem<tauri::Wry>,
    /// "Check for updates" until one is found, then "Update x.y.z available…".
    update: MenuItem<tauri::Wry>,
    quit: MenuItem<tauri::Wry>,
}

mod ids {
    pub const TOGGLE: &str = "toggle_sidebar";
    pub const ALWAYS_SHOW: &str = "always_show";
    pub const REFRESH: &str = "refresh_now";
    pub const DASHBOARD: &str = "open_dashboard";
    pub const SETTINGS: &str = "open_settings";
    pub const UPDATE: &str = "update";
    pub const QUIT: &str = "quit";
}

/// Tray labels in the user's language. `auto` follows `LANG`/`LC_ALL`.
struct Labels {
    toggle: &'static str,
    always_show: &'static str,
    refresh: &'static str,
    dashboard: &'static str,
    settings: &'static str,
    update_check: &'static str,
    /// `{version}` is replaced with the offered version.
    update_available: &'static str,
    quit: &'static str,
}

/// `Settings.language` resolved to a yes/no for the two catalogues the native
/// side carries (tray menu, notifications). `auto` follows `LANG`/`LC_ALL`.
pub fn prefers_chinese(settings: &Settings) -> bool {
    match settings.language.as_str() {
        "zh-CN" | "zh" => true,
        "auto" => ["LC_ALL", "LC_MESSAGES", "LANG"]
            .into_iter()
            .find_map(|key| std::env::var(key).ok().filter(|value| !value.is_empty()))
            .map(|v| v.starts_with("zh"))
            .unwrap_or(false),
        _ => false,
    }
}

fn labels(settings: &Settings) -> Labels {
    if prefers_chinese(settings) {
        Labels {
            toggle: "显示 / 隐藏侧边栏",
            always_show: "始终显示侧边栏",
            refresh: "立即刷新",
            dashboard: "打开仪表盘",
            settings: "设置…",
            update_check: "检查更新",
            update_available: "有新版本 {version}…",
            quit: "退出",
        }
    } else {
        Labels {
            toggle: "Show/Hide sidebar",
            always_show: "Always show sidebar",
            refresh: "Refresh now",
            dashboard: "Open dashboard",
            settings: "Settings…",
            update_check: "Check for updates",
            update_available: "Update {version} available…",
            quit: "Quit",
        }
    }
}

/// Tray label for the update item: an offer when there is one, otherwise the
/// manual "check" action that must always be reachable.
fn update_label(l: &Labels, status: &crate::model::UpdateStatus) -> String {
    match status.available.as_deref() {
        Some(version) => l.update_available.replace("{version}", version),
        None => l.update_check.to_string(),
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
    let update = MenuItem::with_id(
        app,
        ids::UPDATE,
        update_label(&l, &crate::updater::status(app)),
        true,
        None::<&str>,
    )?;
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
            &update,
            &separator,
            &quit,
        ],
    )?;

    let tray = app
        .tray_by_id(TRAY_ID)
        .ok_or_else(|| anyhow!("no tray icon with id `{TRAY_ID}` (check tauri.conf.json)"))?;
    tray.set_menu(Some(menu))?;
    tray.on_menu_event(|app, event: MenuEvent| on_menu(app, event.id.as_ref()));

    // The configured icon is a white macOS template glyph; Windows draws it
    // verbatim, which is invisible on a light taskbar.
    #[cfg(target_os = "windows")]
    match tauri::image::Image::from_bytes(include_bytes!("../../icons/32x32.png")) {
        Ok(icon) => {
            if let Err(e) = tray.set_icon(Some(icon)) {
                log::debug!("tray set_icon: {e}");
            }
        }
        Err(e) => log::debug!("tray icon decode: {e}"),
    }

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
        *state.tray_items.lock() = Some(MenuItems {
            toggle,
            always_show,
            refresh,
            dashboard: open_dashboard,
            settings: open_settings,
            update,
            quit,
        });
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
        // An offer takes the user to the About card, which holds the release
        // notes and the install button; otherwise this is the manual check.
        ids::UPDATE => {
            if crate::updater::status(app).available.is_some() {
                dashboard::open(app, Some("settings".into()));
            } else {
                let handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    crate::updater::check(&handle).await;
                });
            }
        }
        ids::QUIT => {
            log::info!("quit from tray");
            app.exit(0);
        }
        other => log::debug!("unhandled tray menu id `{other}`"),
    }
}

/// Show or hide the whole bar window (different from collapse/expand).
/// Also the target of the `shortcutToggleSidebar` global shortcut.
pub fn toggle_sidebar(app: &AppHandle) {
    let Some(win) = app.get_webview_window(windows::SIDEBAR) else {
        log::warn!("sidebar window is missing");
        return;
    };
    if win.is_visible().unwrap_or(false) {
        crate::window::popover::hide(app, true);
        window::with_state(app, |inner| {
            inner.bar_hovered = false;
            inner.generation = inner.generation.wrapping_add(1);
            inner.revealed = true;
        });
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
        window::with_state(app, |inner| inner.revealed = true);
        crate::window::hover::schedule_idle_timers(app);
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

/// Keep the check state and language in sync with settings changed elsewhere.
pub fn sync(app: &AppHandle, settings: &Settings) {
    let Some(state) = app.try_state::<window::PlatformState>() else {
        return;
    };
    let items = state.tray_items.lock().clone();
    if let Some(items) = items {
        if let Err(e) = items.always_show.set_checked(!settings.auto_hide) {
            log::debug!("tray check item update failed: {e}");
        }
        let l = labels(settings);
        for (item, text) in [
            (&items.toggle, l.toggle),
            (&items.refresh, l.refresh),
            (&items.dashboard, l.dashboard),
            (&items.settings, l.settings),
            (&items.quit, l.quit),
        ] {
            if let Err(e) = item.set_text(text) {
                log::debug!("tray label update failed: {e}");
            }
        }
        if let Err(e) = items.always_show.set_text(l.always_show) {
            log::debug!("tray check label update failed: {e}");
        }
        if let Err(e) = items
            .update
            .set_text(update_label(&l, &crate::updater::status(app)))
        {
            log::debug!("tray update label failed: {e}");
        }
    }
}

/// Re-label the update item after a check. Separate from [`sync`] because the
/// updater has no reason to touch the rest of the menu.
pub fn sync_update(app: &AppHandle, status: &crate::model::UpdateStatus) {
    let Some(state) = app.try_state::<window::PlatformState>() else {
        return;
    };
    let items = state.tray_items.lock().clone();
    let Some(items) = items else { return };
    let text = update_label(&labels(&window::settings_of(app)), status);
    if let Err(e) = items.update.set_text(text) {
        log::debug!("tray update label failed: {e}");
    }
}
