//! Window / tray / platform commands. [PLATFORM owns this directory]
//! Signatures are part of the contract (docs/ARCHITECTURE.md §5).
//!
//! Layout of this module:
//! * [`monitors`] — monitor enumeration + the pure geometry maths (unit tested)
//! * [`sidebar`]  — placement / expand / collapse of the edge bar
//! * [`popover`]  — the detail bubble anchored to a ring
//! * [`hover`]    — the hover state machine and its cancellable timers
//! * [`dashboard`]— the normal window (settings + history)
//! * [`tray`]     — tray icon and menu
//! * [`linux`]    — Wayland layer-shell docking and the KDE X11 blur hint
//!
//! Everything positional works in **logical** (CSS) pixels; see
//! `docs/PLATFORM.md`.

pub mod dashboard;
pub mod hover;
#[cfg(target_os = "linux")]
pub mod linux;
pub mod monitors;
pub mod popover;
pub mod sidebar;
pub mod tray;

use crate::model::*;
use crate::window::monitors::LogicalRect;
use parking_lot::Mutex;
use tauri::{AppHandle, Listener, Manager, WebviewWindow, WindowEvent};

/// Sidebar content size (logical px) until the frontend reports a real one.
pub const DEFAULT_SIDEBAR_SIZE: (f64, f64) = (76.0, 160.0);
/// Popover content size (logical px) until the frontend reports a real one.
pub const DEFAULT_POPOVER_SIZE: (f64, f64) = (340.0, 220.0);
/// Gap between the sidebar and the popover, logical px.
pub const POPOVER_GAP: f64 = 10.0;
/// The sidebar is revealed at the latest this long after start-up, even if the
/// frontend never calls `sidebar_relayout` (blank window is better than none).
pub const REVEAL_FALLBACK_MS: u64 = 1_500;
/// How long after the pointer left both windows the popover disappears.
pub const POPOVER_HIDE_DELAY_MS: u64 = 250;
/// Period of the geometry watchdog (monitor hot-plug, WM moved us, …).
pub const GEOMETRY_CHECK_SEC: u64 = 5;
/// Tolerance of the geometry watchdog, logical px.
pub const GEOMETRY_TOLERANCE: f64 = 2.0;
/// Emitted by the tray's "Refresh now" item; the frontend turns it into
/// `refresh_now`.
pub const REFRESH_REQUESTED: &str = "refresh-requested";

/// Mutable platform state. Kept behind one small lock — every field is a
/// primitive and no lock is ever held across an `await`.
#[derive(Debug, Clone)]
pub struct Inner {
    pub expanded: bool,
    pub pinned: bool,
    pub bar_hovered: bool,
    pub popover_hovered: bool,
    /// Content size the sidebar webview asked for (logical px).
    pub sidebar_content: (f64, f64),
    /// Content size the popover webview asked for (logical px).
    pub popover_content: (f64, f64),
    /// The last `popover_show` request, used to re-anchor on relayout.
    pub popover_req: Option<PopoverRequest>,
    pub popover_visible: bool,
    /// The sidebar window is only shown once (first relayout or the fallback).
    pub revealed: bool,
    /// Bumped on every hover event; pending timers with an older generation
    /// are stale and do nothing when they wake up.
    pub generation: u64,
}

impl Default for Inner {
    fn default() -> Self {
        Self {
            expanded: true,
            pinned: false,
            bar_hovered: false,
            popover_hovered: false,
            sidebar_content: DEFAULT_SIDEBAR_SIZE,
            popover_content: DEFAULT_POPOVER_SIZE,
            popover_req: None,
            popover_visible: false,
            revealed: false,
            generation: 0,
        }
    }
}

#[derive(Default)]
pub struct PlatformState {
    pub inner: Mutex<Inner>,
    /// Tray items are updated in place when language or auto-hide changes.
    pub tray_items: Mutex<Option<tray::MenuItems>>,
}

/// Read a copy of the platform state, or `None` before `setup` ran.
pub fn snapshot(app: &AppHandle) -> Option<Inner> {
    app.try_state::<PlatformState>()
        .map(|s| s.inner.lock().clone())
}

/// Mutate the platform state; returns whatever the closure returns.
pub fn with_state<T>(app: &AppHandle, f: impl FnOnce(&mut Inner) -> T) -> Option<T> {
    let state = app.try_state::<PlatformState>()?;
    let mut guard = state.inner.lock();
    Some(f(&mut guard))
}

/// The current settings, or the defaults if the backend state is not managed
/// yet (only possible very early during start-up).
pub fn settings_of(app: &AppHandle) -> Settings {
    match app.try_state::<crate::state::AppState>() {
        Some(state) => state.settings.read().clone(),
        None => {
            log::warn!("AppState is not managed yet, using default settings");
            Settings::default()
        }
    }
}

/// `always_on_top` + the flags that X11 window managers like to forget.
pub fn apply_stacking(win: &WebviewWindow, always_on_top: bool) {
    if let Err(e) = win.set_always_on_top(always_on_top) {
        log::warn!(
            "set_always_on_top({always_on_top}) on `{}`: {e}",
            win.label()
        );
    }
    if let Err(e) = win.set_visible_on_all_workspaces(true) {
        log::debug!("set_visible_on_all_workspaces on `{}`: {e}", win.label());
    }
    if let Err(e) = win.set_skip_taskbar(true) {
        log::debug!("set_skip_taskbar on `{}`: {e}", win.label());
    }
}

/// GTK treats non-resizable windows as their webview's natural size (200px),
/// which prevents a narrow sidebar or collapsed handle. Equal min/max hints
/// allow the requested size while still preventing the user from resizing it.
pub fn set_overlay_size(win: &WebviewWindow, size: tauri::PhysicalSize<u32>) -> tauri::Result<()> {
    #[cfg(target_os = "linux")]
    {
        win.set_resizable(true)?;
        win.set_size_constraints(tauri::WindowSizeConstraints {
            min_width: Some(tauri::PhysicalUnit::new(size.width as i32).into()),
            min_height: Some(tauri::PhysicalUnit::new(size.height as i32).into()),
            max_width: Some(tauri::PhysicalUnit::new(size.width as i32).into()),
            max_height: Some(tauri::PhysicalUnit::new(size.height as i32).into()),
        })?;
    }
    win.set_size(size)
}

/// True when the Linux overlays are docked through Wayland layer-shell. The
/// compositor then owns their position, so `set_position`, `outer_position`
/// and the size constraints above are meaningless for them.
pub fn layer_shell_active() -> bool {
    #[cfg(target_os = "linux")]
    {
        linux::is_active()
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

/// The single place where a computed overlay rectangle reaches the OS.
///
/// Normally that is move → resize → move (GTK/X11 sometimes keeps the
/// pre-resize origin when both happen in one frame; the watchdog catches the
/// rest). Under Wayland layer-shell the compositor positions the surface from
/// anchors and margins instead, and `linux::place` takes over completely.
pub fn place_overlay(win: &WebviewWindow, rect: LogicalRect, scale: f64) {
    #[cfg(target_os = "linux")]
    if linux::place(win, rect) {
        return;
    }
    let (position, size) = rect.to_physical(scale);
    if let Err(e) = win.set_position(position) {
        log::warn!("set_position on `{}` failed: {e}", win.label());
    }
    if let Err(e) = set_overlay_size(win, size) {
        log::warn!("set_size on `{}` failed: {e}", win.label());
    }
    if let Err(e) = win.set_position(position) {
        log::warn!("set_position on `{}` failed: {e}", win.label());
    }
}

/// Native translucency for the overlay windows, where the OS provides it.
///
/// * **macOS** — `NSVisualEffectView` behind the webview (HUD material).
/// * **Windows** — the acrylic backdrop of DWM.
/// * **Linux** — `_KDE_NET_WM_BLUR_BEHIND_REGION` on X11, which KWin honours
///   and every other X11 compositor ignores (see [`linux::apply_blur`]).
///   Wayland has no client-side blur protocol, so there the webview's own
///   translucent fill is all you get.
///
/// The frontend always paints a translucent surface itself, so a failure here
/// only costs the background blur, never readability.
pub fn apply_surface_style(win: &WebviewWindow, style: SurfaceStyle) {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        let native_window = win.clone();
        // Settings arrive on an async worker. AppKit effect views must be
        // updated on the main thread; otherwise macOS rejects live changes.
        if let Err(e) = win.run_on_main_thread(move || {
            #[cfg(target_os = "macos")]
            let result = {
                use window_vibrancy::{apply_vibrancy, clear_vibrancy, NSVisualEffectMaterial};
                match style {
                    SurfaceStyle::Glass => apply_vibrancy(
                        &native_window,
                        NSVisualEffectMaterial::HudWindow,
                        None,
                        Some(16.0),
                    ),
                    SurfaceStyle::Solid => clear_vibrancy(&native_window).map(|_| ()),
                }
            };
            #[cfg(target_os = "windows")]
            let result = {
                use window_vibrancy::{apply_acrylic, clear_acrylic};
                match style {
                    SurfaceStyle::Glass => apply_acrylic(&native_window, Some((0, 0, 0, 10))),
                    SurfaceStyle::Solid => clear_acrylic(&native_window),
                }
            };
            if let Err(e) = result {
                log::debug!("native backdrop on `{}`: {e}", native_window.label());
            }
        }) {
            log::debug!("scheduling native backdrop on `{}`: {e}", win.label());
        }
    }
    #[cfg(target_os = "linux")]
    {
        linux::apply_blur(win, style);
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = (win, style);
    }
}

/// Some X11 window managers (mutter/XWayland included) drop the
/// `_NET_WM_STATE_ABOVE` hint when a window is mapped, so re-assert it right
/// after `show()` and once more when the map has settled.
pub fn after_show(win: &WebviewWindow, always_on_top: bool) {
    apply_stacking(win, always_on_top);
    #[cfg(target_os = "linux")]
    {
        let win = win.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(120)).await;
            if let Err(e) = win.set_always_on_top(always_on_top) {
                log::debug!("re-assert always-on-top on `{}`: {e}", win.label());
            }
            let _ = win.set_visible_on_all_workspaces(true);
        });
    }
}

/// Called once from `lib.rs` inside `.setup()`, after `AppState` is managed.
pub fn setup(app: &AppHandle) -> anyhow::Result<()> {
    app.manage(PlatformState::default());

    // Must run before anything shows or realises an overlay: gtk-layer-shell
    // can only adopt an unrealised window. Failing is never fatal — it just
    // leaves the overlays as ordinary (X)Wayland toplevels.
    #[cfg(target_os = "linux")]
    if let Err(e) = linux::initialize(app) {
        log::info!("native layer-shell docking not used: {e:#}");
    }

    let settings = settings_of(app);
    log::info!(
        "platform setup: backend={}, edge={:?}, align={:?}, autoHide={}",
        backend_name(),
        settings.edge,
        settings.vertical_align,
        settings.auto_hide
    );

    // The overlay windows are created hidden by tauri.conf.json; give them
    // their stacking flags and make sure the popover can never take focus.
    if let Some(win) = app.get_webview_window(windows::SIDEBAR) {
        apply_stacking(&win, settings.always_on_top);
        apply_surface_style(&win, settings.surface_style);
    } else {
        log::error!(
            "window `{}` is missing from tauri.conf.json",
            windows::SIDEBAR
        );
    }
    if let Some(win) = app.get_webview_window(windows::POPOVER) {
        apply_stacking(&win, settings.always_on_top);
        apply_surface_style(&win, settings.surface_style);
        if let Err(e) = win.set_focusable(false) {
            log::debug!("popover set_focusable(false) unsupported here: {e}");
        }
    } else {
        log::error!(
            "window `{}` is missing from tauri.conf.json",
            windows::POPOVER
        );
    }

    install_close_handlers(app);
    sidebar::place(app);
    sidebar::start_watchdogs(app);

    if let Err(e) = tray::build(app) {
        log::error!("tray icon could not be created: {e:#}");
    }
    autostart::apply(app, settings.autostart);

    // The backend owns settings; whenever it publishes a change, re-apply the
    // parts that are ours (geometry, stacking, autostart, tray check state).
    {
        let handle = app.clone();
        app.listen(events::SETTINGS_UPDATED, move |_event| {
            let handle = handle.clone();
            tauri::async_runtime::spawn(async move {
                apply_settings(&handle);
            });
        });
    }

    // Debug-only demo hook for screenshots: AI_USAGE_SIDEBAR_DEMO=popover|dashboard|both
    // shows the popover for the first ring / opens the dashboard a few seconds
    // after start, without needing a real pointer.
    #[cfg(debug_assertions)]
    if let Ok(demo) = std::env::var("AI_USAGE_SIDEBAR_DEMO") {
        let handle = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(6)).await;
            if demo == "popover" || demo == "both" {
                popover::show(
                    &handle,
                    PopoverRequest {
                        provider: "claude".into(),
                        ring_index: 0,
                        anchor_y: 52.0,
                        window_kind: None,
                    },
                );
                popover::set_pinned(&handle, true);
            }
            if demo == "dashboard" || demo == "both" {
                let tab = std::env::var("AI_USAGE_SIDEBAR_DEMO_TAB")
                    .unwrap_or_else(|_| "overview".into());
                dashboard::open(&handle, Some(tab));
            }
        });
    }

    Ok(())
}

fn install_close_handlers(app: &AppHandle) {
    // Overlay windows can never be closed; the dashboard hides instead so the
    // app keeps running in the tray.
    for label in [windows::SIDEBAR, windows::POPOVER] {
        if let Some(win) = app.get_webview_window(label) {
            win.on_window_event(move |event| {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                }
            });
        }
    }
    if let Some(win) = app.get_webview_window(windows::DASHBOARD) {
        let hide_me = win.clone();
        win.on_window_event(move |event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if let Err(e) = hide_me.hide() {
                    log::warn!("hiding the dashboard failed: {e}");
                }
            }
        });
    }
}

/// Re-read the settings and re-apply everything the platform layer owns.
pub fn apply_settings(app: &AppHandle) {
    let settings = settings_of(app);
    hover::settings_changed(app);
    sidebar::place(app);
    popover::reposition(app);
    for label in [windows::SIDEBAR, windows::POPOVER] {
        if let Some(win) = app.get_webview_window(label) {
            apply_stacking(&win, settings.always_on_top);
            apply_surface_style(&win, settings.surface_style);
        }
    }
    tray::sync(app, &settings);
    autostart::apply(app, settings.autostart);
}

/// Autostart (login item) handling; failures are never fatal.
pub mod autostart {
    use tauri::AppHandle;
    use tauri_plugin_autostart::ManagerExt;

    pub fn apply(app: &AppHandle, enabled: bool) {
        let manager = app.autolaunch();
        let current = match manager.is_enabled() {
            Ok(v) => Some(v),
            Err(e) => {
                log::warn!("autostart status unavailable: {e}");
                None
            }
        };
        if current == Some(enabled) {
            return;
        }
        let result = if enabled {
            manager.enable()
        } else {
            manager.disable()
        };
        match result {
            Ok(()) => log::info!("autostart {}", if enabled { "enabled" } else { "disabled" }),
            Err(e) => log::warn!(
                "could not {} autostart: {e}",
                if enabled { "enable" } else { "disable" }
            ),
        }
    }
}

/// Name of the windowing backend in use ("x11" | "wayland" | "cocoa" | "win32").
pub fn backend_name() -> String {
    #[cfg(target_os = "linux")]
    {
        // main.rs forces GDK_BACKEND=x11 unless the user opted into Wayland, so
        // the env var is authoritative; fall back to the session type.
        match std::env::var("GDK_BACKEND").ok().as_deref() {
            Some("x11") => return "x11".into(),
            Some("wayland") => return "wayland".into(),
            _ => {}
        }
        if std::env::var("WAYLAND_DISPLAY").is_ok() {
            "wayland".into()
        } else {
            "x11".into()
        }
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

// ---------------------------------------------------------------- commands --

#[tauri::command]
pub async fn sidebar_set_expanded(app: AppHandle, expanded: bool) -> Result<(), String> {
    sidebar::set_expanded(&app, expanded);
    Ok(())
}

#[tauri::command]
pub async fn sidebar_relayout(app: AppHandle, width: f64, height: f64) -> Result<(), String> {
    sidebar::relayout(&app, width, height);
    Ok(())
}

#[tauri::command]
pub async fn popover_show(app: AppHandle, req: PopoverRequest) -> Result<(), String> {
    popover::show(&app, req);
    Ok(())
}

#[tauri::command]
pub async fn popover_relayout(app: AppHandle, width: f64, height: f64) -> Result<(), String> {
    popover::relayout(&app, width, height);
    Ok(())
}

#[tauri::command]
pub async fn popover_hide(app: AppHandle) -> Result<(), String> {
    popover::hide(&app, false);
    Ok(())
}

#[tauri::command]
pub async fn popover_set_pinned(app: AppHandle, pinned: bool) -> Result<(), String> {
    popover::set_pinned(&app, pinned);
    Ok(())
}

#[tauri::command]
pub async fn hover_report(app: AppHandle, source: String, hovered: bool) -> Result<(), String> {
    hover::report(&app, &source, hovered);
    Ok(())
}

#[tauri::command]
pub async fn open_dashboard(app: AppHandle, tab: Option<String>) -> Result<(), String> {
    dashboard::open(&app, tab);
    Ok(())
}

#[tauri::command]
pub async fn apply_window_settings(app: AppHandle) -> Result<(), String> {
    apply_settings(&app);
    Ok(())
}

#[tauri::command]
pub async fn get_monitors(app: AppHandle) -> Result<Vec<MonitorInfo>, String> {
    Ok(monitors::monitor_infos(&app))
}

/// Lets injected diagnostics scripts write into the Rust log (debug builds only).
#[tauri::command]
pub async fn debug_log(msg: String) -> Result<(), String> {
    if cfg!(debug_assertions) {
        log::info!("[webview] {msg}");
    }
    Ok(())
}

#[tauri::command]
pub async fn quit_app(app: AppHandle) -> Result<(), String> {
    log::info!("quit requested");
    app.exit(0);
    Ok(())
}
