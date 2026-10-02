//! Tray icon and menu. [PLATFORM]
//!
//! The icon itself comes from `tauri.conf.json` (`app.trayIcon`, id `main`);
//! here we only attach the menu and the event handlers, so the bundled tray
//! asset stays a build-time concern.

use crate::model::{windows, AppSnapshot, Settings};
use crate::window::tray_status::{self, Severity};
use crate::window::{self, dashboard, sidebar};
use anyhow::anyhow;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Emitter, Manager};

pub const TRAY_ID: &str = "main";

/// What the tray currently shows, so a new snapshot only touches the native
/// menu/icon when something actually changed.
#[derive(Default)]
struct UsageCache {
    /// Provider ids whose usage line is in the menu, in menu order.
    shown: Vec<String>,
    texts: Vec<String>,
    tooltip: String,
    severity: Option<Severity>,
    focus_title: String,
}

#[derive(Clone)]
pub struct MenuItems {
    menu: Menu<tauri::Wry>,
    /// Read-only usage lines, one per registered provider; only the enabled
    /// ones are inserted into the menu (above `usage_sep`).
    usage: Vec<(&'static str, MenuItem<tauri::Wry>)>,
    usage_sep: PredefinedMenuItem<tauri::Wry>,
    focus: Submenu<tauri::Wry>,
    /// 1 hour, until tomorrow, until turned off, off (see `focus_labels`).
    focus_items: Vec<MenuItem<tauri::Wry>>,
    cache: Arc<Mutex<UsageCache>>,
    toggle: MenuItem<tauri::Wry>,
    always_show: CheckMenuItem<tauri::Wry>,
    refresh: MenuItem<tauri::Wry>,
    /// Checked while `pollingPaused` is on.
    pause: CheckMenuItem<tauri::Wry>,
    dashboard: MenuItem<tauri::Wry>,
    settings: MenuItem<tauri::Wry>,
    /// "Check for updates" until one is found, then "Update x.y.z available…".
    update: MenuItem<tauri::Wry>,
    /// Kept distinct from the app updater: this checks the price-list source.
    pricing_update: MenuItem<tauri::Wry>,
    quit: MenuItem<tauri::Wry>,
}

mod ids {
    pub const TOGGLE: &str = "toggle_sidebar";
    pub const ALWAYS_SHOW: &str = "always_show";
    pub const REFRESH: &str = "refresh_now";
    pub const PAUSE: &str = "pause_polling";
    pub const DASHBOARD: &str = "open_dashboard";
    pub const SETTINGS: &str = "open_settings";
    pub const UPDATE: &str = "update";
    pub const PRICING_UPDATE: &str = "pricing_update";
    pub const QUIT: &str = "quit";
    pub const FOCUS_HOUR: &str = "focus_hour";
    pub const FOCUS_MORNING: &str = "focus_morning";
    pub const FOCUS_FOREVER: &str = "focus_forever";
    pub const FOCUS_OFF: &str = "focus_off";
}

/// Tray labels in the user's language (see [`prefers_chinese`]).
struct Labels {
    toggle: &'static str,
    always_show: &'static str,
    refresh: &'static str,
    pause_polling: &'static str,
    dashboard: &'static str,
    settings: &'static str,
    update_check: &'static str,
    /// `{version}` is replaced with the offered version.
    update_available: &'static str,
    pricing_check: &'static str,
    pricing_available: &'static str,
    quit: &'static str,
    focus: &'static str,
    /// `{time}` is replaced with the local `HH:MM` the focus ends.
    focus_until_time: &'static str,
    focus_until_off: &'static str,
    /// hour, tomorrow, forever, off
    focus_choices: [&'static str; 4],
}

/// `Settings.language` resolved to a yes/no for the two catalogues the native
/// side carries (tray menu, notifications). `auto` follows the OS display
/// language (Windows UI language, macOS `AppleLanguages`, `LANG` elsewhere).
pub fn prefers_chinese(settings: &Settings) -> bool {
    match settings.language.as_str() {
        "zh-CN" | "zh" => true,
        "auto" => tray_status::system_prefers_chinese(),
        _ => false,
    }
}

fn labels(settings: &Settings) -> Labels {
    if prefers_chinese(settings) {
        Labels {
            toggle: "显示 / 隐藏侧边栏",
            always_show: "始终显示侧边栏",
            refresh: "立即刷新",
            pause_polling: "暂停轮询",
            dashboard: "打开仪表盘",
            settings: "设置…",
            update_check: "检查程序更新",
            update_available: "有程序新版本 {version}…",
            pricing_check: "检查价格更新",
            pricing_available: "有价格表更新…",
            quit: "退出",
            focus: "专注模式",
            focus_until_time: "专注模式 · 至 {time}",
            focus_until_off: "专注模式 · 已开启",
            focus_choices: ["1 小时", "到明天 08:00", "直到手动关闭", "关闭"],
        }
    } else {
        Labels {
            toggle: "Show/Hide sidebar",
            always_show: "Always show sidebar",
            refresh: "Refresh now",
            pause_polling: "Pause polling",
            dashboard: "Open dashboard",
            settings: "Settings…",
            update_check: "Check program updates",
            update_available: "Program update {version} available…",
            pricing_check: "Check pricing updates",
            pricing_available: "Pricing update available…",
            quit: "Quit",
            focus: "Focus mode",
            focus_until_time: "Focus mode · until {time}",
            focus_until_off: "Focus mode · on",
            focus_choices: [
                "For 1 hour",
                "Until tomorrow 08:00",
                "Until turned off",
                "Off",
            ],
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

fn focus_title(l: &Labels, focus_until: i64, now_ms: i64) -> String {
    if !crate::focus::is_active(focus_until, now_ms) {
        l.focus.to_string()
    } else if focus_until == crate::focus::UNTIL_OFF {
        l.focus_until_off.to_string()
    } else {
        l.focus_until_time
            .replace("{time}", &crate::focus::clock_label(focus_until))
    }
}

fn pricing_update_label(l: &Labels, status: &crate::model::PriceUpdateStatus) -> String {
    if status.available {
        l.pricing_available.to_string()
    } else {
        l.pricing_check.to_string()
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
    let pause = CheckMenuItem::with_id(
        app,
        ids::PAUSE,
        l.pause_polling,
        true,
        settings.polling_paused,
        None::<&str>,
    )?;
    let open_dashboard = MenuItem::with_id(app, ids::DASHBOARD, l.dashboard, true, None::<&str>)?;
    let open_settings = MenuItem::with_id(app, ids::SETTINGS, l.settings, true, None::<&str>)?;
    let update = MenuItem::with_id(
        app,
        ids::UPDATE,
        update_label(&l, &crate::updater::status(app)),
        true,
        None::<&str>,
    )?;
    let pricing_update = MenuItem::with_id(
        app,
        ids::PRICING_UPDATE,
        pricing_update_label(&l, &crate::commands::pricing::status(app)),
        true,
        None::<&str>,
    )?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, ids::QUIT, l.quit, true, None::<&str>)?;

    let now = crate::commands::store::now_ms();
    let focus_active = crate::focus::is_active(settings.focus_until, now);
    let mut focus_items = Vec::new();
    for (i, id) in [
        ids::FOCUS_HOUR,
        ids::FOCUS_MORNING,
        ids::FOCUS_FOREVER,
        ids::FOCUS_OFF,
    ]
    .into_iter()
    .enumerate()
    {
        // "Off" only makes sense while focus is on.
        let enabled = i < 3 || focus_active;
        focus_items.push(MenuItem::with_id(
            app,
            id,
            l.focus_choices[i],
            enabled,
            None::<&str>,
        )?);
    }
    let focus = Submenu::with_id_and_items(
        app,
        "focus_mode",
        focus_title(&l, settings.focus_until, now),
        true,
        &[
            &focus_items[0],
            &focus_items[1],
            &focus_items[2],
            &focus_items[3],
        ],
    )?;
    let mut usage = Vec::new();
    for id in crate::commands::providers::DEFAULT_PROVIDER_ORDER {
        let item = MenuItem::with_id(app, format!("usage_{id}"), *id, false, None::<&str>)?;
        usage.push((*id, item));
    }
    let usage_sep = PredefinedMenuItem::separator(app)?;

    let menu = Menu::with_items(
        app,
        &[
            &toggle,
            &always_show,
            &refresh,
            &pause,
            &open_dashboard,
            &open_settings,
            &update,
            &pricing_update,
            &focus,
            &separator,
            &quit,
        ],
    )?;

    let tray = app
        .tray_by_id(TRAY_ID)
        .ok_or_else(|| anyhow!("no tray icon with id `{TRAY_ID}` (check tauri.conf.json)"))?;
    tray.set_menu(Some(menu.clone()))?;
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
            menu,
            usage,
            usage_sep,
            focus,
            focus_items,
            cache: Arc::new(Mutex::new(UsageCache::default())),
            toggle,
            always_show,
            refresh,
            pause,
            dashboard: open_dashboard,
            settings: open_settings,
            update,
            pricing_update,
            quit,
        });
    }
    // Seed the usage lines from the cached snapshot and start the focus timer.
    sync(app, &settings);
    start_focus_watch(app);
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
        ids::PAUSE => {
            // The check mark flips on click; the setting is the truth, so
            // persist the opposite of what it says now (`sync` re-checks).
            let paused = !window::settings_of(app).polling_paused;
            if let Err(e) = crate::commands::settings::update(
                app,
                &serde_json::json!({"pollingPaused": paused}),
            ) {
                log::error!("could not persist pause polling: {e:#}");
            }
            sync(app, &window::settings_of(app));
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
        // A pending table takes the user to Settings where its revision and
        // explicit Apply button are visible. Otherwise this is a manual check.
        ids::PRICING_UPDATE => {
            if crate::commands::pricing::status(app).available {
                dashboard::open(app, Some("settings".into()));
            } else {
                let handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    crate::commands::pricing::check(&handle).await;
                });
            }
        }
        ids::FOCUS_HOUR => set_focus(app, crate::commands::store::now_ms() + 3_600_000),
        ids::FOCUS_MORNING => set_focus(app, crate::focus::tomorrow_morning_from_now()),
        ids::FOCUS_FOREVER => set_focus(app, crate::focus::UNTIL_OFF),
        ids::FOCUS_OFF => set_focus(app, 0),
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
        // Logged on purpose: a bar that "disappeared" is otherwise impossible
        // to tell apart from a compositor problem after the fact.
        log::info!("sidebar hidden on request (tray menu or shortcut)");
        hide_sidebar(app, &win);
    } else {
        show_sidebar(app);
    }
}

fn hide_sidebar(app: &AppHandle, win: &tauri::WebviewWindow) {
    crate::window::popover::hide(app, true);
    window::with_state(app, |inner| {
        inner.bar_hovered = false;
        inner.generation = inner.generation.wrapping_add(1);
        inner.revealed = true;
    });
    if let Err(e) = win.hide() {
        log::warn!("hiding the sidebar failed: {e}");
    }
}

/// Bring a hidden bar back, or recover its presentation if already visible.
/// Launching the app again (dock, app grid, a second command-line start) goes
/// through here, so that is always a way to get the widget back.
pub fn show_sidebar(app: &AppHandle) {
    let Some(win) = app.get_webview_window(windows::SIDEBAR) else {
        log::warn!("sidebar window is missing");
        return;
    };
    if win.is_visible().unwrap_or(false) {
        sidebar::remap(app);
        return;
    }
    log::info!("sidebar shown again");
    sidebar::place(app);
    if let Err(e) = win.show() {
        log::warn!("showing the sidebar failed: {e}");
        return;
    }
    window::after_show(&win, window::settings_of(app).always_on_top);
    window::with_state(app, |inner| inner.revealed = true);
    crate::window::hover::schedule_idle_timers(app);
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
        if let Err(e) = items.pause.set_checked(settings.polling_paused) {
            log::debug!("tray pause check failed: {e}");
        }
        if let Err(e) = items.pause.set_text(l.pause_polling) {
            log::debug!("tray pause label failed: {e}");
        }
        if let Err(e) = items
            .update
            .set_text(update_label(&l, &crate::updater::status(app)))
        {
            log::debug!("tray update label failed: {e}");
        }
        if let Err(e) = items.pricing_update.set_text(pricing_update_label(
            &l,
            &crate::commands::pricing::status(app),
        )) {
            log::debug!("tray pricing-update label failed: {e}");
        }
        for (item, text) in items.focus_items.iter().zip(l.focus_choices) {
            if let Err(e) = item.set_text(text) {
                log::debug!("tray focus label failed: {e}");
            }
        }
    }
    sync_focus(app, settings);
    let snapshot = match app.try_state::<crate::state::AppState>() {
        Some(state) => state.snapshot.read().clone(),
        None => AppSnapshot::default(),
    };
    sync_usage_with(app, settings, &snapshot);
}

/// Persist a focus deadline; the resulting `settings-updated` re-syncs the tray.
fn set_focus(app: &AppHandle, until: i64) {
    if let Err(e) =
        crate::commands::settings::update(app, &serde_json::json!({"focusUntil": until}))
    {
        log::error!("could not persist focus mode: {e:#}");
    }
}

/// Set while the sidebar is hidden because of focus mode, so it is brought
/// back (and only then) when focus ends.
static HIDDEN_BY_FOCUS: AtomicBool = AtomicBool::new(false);

/// Focus menu title/enabled state, plus hiding/restoring the bar on a change.
fn sync_focus(app: &AppHandle, settings: &Settings) {
    let now = crate::commands::store::now_ms();
    let active = crate::focus::is_active(settings.focus_until, now);
    let items = app
        .try_state::<window::PlatformState>()
        .and_then(|s| s.tray_items.lock().clone());
    if let Some(items) = items {
        let title = focus_title(&labels(settings), settings.focus_until, now);
        let mut cache = items.cache.lock();
        if cache.focus_title != title {
            if let Err(e) = items.focus.set_text(&title) {
                log::debug!("tray focus title failed: {e}");
            }
            if let Some(off) = items.focus_items.last() {
                if let Err(e) = off.set_enabled(active) {
                    log::debug!("tray focus off item failed: {e}");
                }
            }
            cache.focus_title = title;
        }
    }
    let want_hidden = active && settings.focus_hides_sidebar;
    if want_hidden && !HIDDEN_BY_FOCUS.swap(true, Ordering::SeqCst) {
        if let Some(win) = app.get_webview_window(windows::SIDEBAR) {
            if win.is_visible().unwrap_or(false) {
                log::info!("sidebar hidden by focus mode");
                hide_sidebar(app, &win);
            }
        }
    } else if !want_hidden && HIDDEN_BY_FOCUS.swap(false, Ordering::SeqCst) {
        log::info!("focus mode ended, showing the sidebar again");
        show_sidebar(app);
    }
}

/// Expire a timed focus by itself. Also refreshes the "until HH:MM" title and
/// the relative reset times in the usage lines once in a while.
fn start_focus_watch(app: &AppHandle) {
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        // Let the bar finish its first reveal before a focus hide can apply.
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        loop {
            let settings = window::settings_of(&handle);
            if crate::focus::is_expired(settings.focus_until, crate::commands::store::now_ms()) {
                log::info!("focus mode expired");
                set_focus(&handle, 0);
            } else {
                sync(&handle, &settings);
            }
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        }
    });
}

/// Called after every new snapshot: refresh the usage lines, tooltip and the
/// severity dot. Cheap when nothing changed.
pub fn sync_usage(app: &AppHandle, snapshot: &AppSnapshot) {
    sync_usage_with(app, &window::settings_of(app), snapshot);
}

fn sync_usage_with(app: &AppHandle, settings: &Settings, snapshot: &AppSnapshot) {
    let items = app
        .try_state::<window::PlatformState>()
        .and_then(|s| s.tray_items.lock().clone());
    let Some(items) = items else { return };
    let chinese = prefers_chinese(settings);
    let now = crate::commands::store::now_ms();
    let lines = tray_status::usage_lines(snapshot, settings, now, chinese);
    let mut cache = items.cache.lock();

    // Which lines are in the menu. Membership rarely changes, so rebuild only then.
    let ids: Vec<String> = lines
        .iter()
        .filter(|(id, _)| items.usage.iter().any(|(known, _)| known == id))
        .map(|(id, _)| id.clone())
        .collect();
    if ids != cache.shown {
        for (_, item) in &items.usage {
            let _ = items.menu.remove(item);
        }
        let _ = items.menu.remove(&items.usage_sep);
        for (pos, id) in ids.iter().enumerate() {
            if let Some((_, item)) = items.usage.iter().find(|(known, _)| known == id) {
                if let Err(e) = items.menu.insert(item, pos) {
                    log::debug!("tray usage insert failed: {e}");
                }
            }
        }
        if !ids.is_empty() {
            let _ = items.menu.insert(&items.usage_sep, ids.len());
        }
        cache.shown = ids.clone();
        cache.texts.clear();
    }
    let texts: Vec<String> = ids
        .iter()
        .filter_map(|id| lines.iter().find(|(l, _)| l == id).map(|(_, t)| t.clone()))
        .collect();
    if texts != cache.texts {
        for (id, text) in ids.iter().zip(&texts) {
            if let Some((_, item)) = items.usage.iter().find(|(known, _)| known == id) {
                if let Err(e) = item.set_text(text) {
                    log::debug!("tray usage label failed: {e}");
                }
            }
        }
        cache.texts = texts;
    }

    let (severity, busiest) = tray_status::worst(snapshot, settings);
    let focus_on = crate::focus::is_active(settings.focus_until, now);
    let tip = tray_status::tooltip(busiest.as_ref(), settings, focus_on, chinese);
    let tray = app.tray_by_id(TRAY_ID);
    if let Some(tray) = &tray {
        if tip != cache.tooltip {
            if let Err(e) = tray.set_tooltip(Some(&tip)) {
                log::debug!("tray tooltip failed: {e}");
            }
            cache.tooltip = tip;
        }
    }
    // The dot also depends on the colours, so re-key on severity only and let
    // a colour change show up at the next severity change.
    if cache.severity != Some(severity) {
        if let Some(tray) = &tray {
            apply_severity_icon(tray, severity, settings);
        }
        cache.severity = Some(severity);
    }
}

/// Plain icon, or the icon with a severity dot. Windows and Linux only: the
/// macOS glyph is a monochrome template image that cannot carry a colour.
fn apply_severity_icon(tray: &tauri::tray::TrayIcon, severity: Severity, settings: &Settings) {
    #[cfg(not(target_os = "macos"))]
    {
        #[cfg(target_os = "windows")]
        const BASE: &[u8] = include_bytes!("../../icons/32x32.png");
        #[cfg(not(target_os = "windows"))]
        const BASE: &[u8] = include_bytes!("../../icons/tray@2x.png");
        let base = match tauri::image::Image::from_bytes(BASE) {
            Ok(image) => image,
            Err(e) => {
                log::debug!("tray icon decode: {e}");
                return;
            }
        };
        let icon = match tray_status::dot_color(severity, settings) {
            Some(color) => {
                let rgba =
                    tray_status::overlay_dot(base.rgba(), base.width(), base.height(), color);
                tauri::image::Image::new_owned(rgba, base.width(), base.height())
            }
            None => base,
        };
        if let Err(e) = tray.set_icon(Some(icon)) {
            log::debug!("tray set_icon: {e}");
        }
    }
    #[cfg(target_os = "macos")]
    let _ = (tray, severity, settings);
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

/// Re-label the independent pricing item after its status changes.
pub fn sync_price_update(app: &AppHandle, status: &crate::model::PriceUpdateStatus) {
    sync_price_update_for_settings(app, status, &window::settings_of(app));
}

/// Variant used while `settings.json` is being persisted. Callers holding the
/// settings write lock pass the already merged value so the tray does not try
/// to acquire that lock again.
pub fn sync_price_update_for_settings(
    app: &AppHandle,
    status: &crate::model::PriceUpdateStatus,
    settings: &Settings,
) {
    let Some(state) = app.try_state::<window::PlatformState>() else {
        return;
    };
    let items = state.tray_items.lock().clone();
    let Some(items) = items else { return };
    let text = pricing_update_label(&labels(settings), status);
    if let Err(e) = items.pricing_update.set_text(text) {
        log::debug!("tray pricing-update label failed: {e}");
    }
}
