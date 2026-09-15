//! The edge bar window: placement, expand/collapse, reveal and the geometry
//! watchdog. [PLATFORM]

use crate::model::{events, windows, SidebarState};
use crate::window::{self, monitors, monitors::LogicalRect};
use std::time::Duration;
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager};

/// Compute where the bar should be right now, without touching any window.
pub fn desired_rect(app: &AppHandle, expanded: bool) -> Option<LogicalRect> {
    let settings = window::settings_of(app);
    let mon = monitors::target_monitor(app, &settings)?;
    let (content_w, content_h) = window::snapshot(app)?.sidebar_content;
    Some(monitors::sidebar_rect(
        &mon, &settings, content_w, content_h, expanded,
    ))
}

/// The rect the bar currently occupies according to our own bookkeeping — the
/// popover anchors against this rather than against the WM's idea of it.
pub fn current_rect(app: &AppHandle) -> Option<LogicalRect> {
    let expanded = window::snapshot(app)?.expanded;
    desired_rect(app, expanded)
}

/// Resize + reposition the bar. `expanded` decides the width only.
pub fn place_sidebar(app: &AppHandle, expanded: bool) -> Option<LogicalRect> {
    let rect = desired_rect(app, expanded)?;
    let Some(win) = app.get_webview_window(windows::SIDEBAR) else {
        log::warn!("sidebar window is gone, cannot place it");
        return None;
    };
    // GTK/X11 occasionally keeps the pre-resize origin when a window is moved
    // and resized in the same frame, so the origin is written on both sides of
    // the resize; the 5 s watchdog catches whatever still slips through.
    if let Err(e) = win.set_position(LogicalPosition::new(rect.x, rect.y)) {
        log::warn!("sidebar set_position failed: {e}");
    }
    if let Err(e) = win.set_size(LogicalSize::new(rect.w, rect.h)) {
        log::warn!("sidebar set_size failed: {e}");
    }
    if let Err(e) = win.set_position(LogicalPosition::new(rect.x, rect.y)) {
        log::warn!("sidebar set_position failed: {e}");
    }
    window::apply_stacking(&win, window::settings_of(app).always_on_top);
    log::debug!("sidebar placed at {rect:?} (expanded={expanded})");
    Some(rect)
}

/// Place the bar using the currently known expanded state.
pub fn place(app: &AppHandle) -> Option<LogicalRect> {
    let expanded = window::snapshot(app)?.expanded;
    place_sidebar(app, expanded)
}

/// `sidebar_set_expanded`: resize/reposition and tell the frontend.
pub fn set_expanded(app: &AppHandle, expanded: bool) {
    let pinned = window::with_state(app, |inner| {
        inner.expanded = expanded;
        inner.pinned
    })
    .unwrap_or(false);
    place_sidebar(app, expanded);
    let payload = SidebarState { expanded, pinned };
    if let Err(e) = app.emit(events::SIDEBAR_STATE, payload) {
        log::warn!("emitting {} failed: {e}", events::SIDEBAR_STATE);
    }
}

/// `sidebar_relayout`: the webview measured its content. Also the trigger that
/// reveals the window for the first time.
pub fn relayout(app: &AppHandle, width: f64, height: f64) {
    if !(width.is_finite() && height.is_finite()) || width <= 0.0 || height <= 0.0 {
        log::warn!("sidebar_relayout ignored: {width}x{height}");
        return;
    }
    window::with_state(app, |inner| inner.sidebar_content = (width, height));
    place(app);
    // A popover that is open keeps hugging the (possibly moved) bar.
    super::popover::reposition(app);
    reveal(app);
}

/// Show the bar once, the first time we have a layout (or after the fallback
/// timeout) so the user never sees an unpainted window.
pub fn reveal(app: &AppHandle) {
    let first = window::with_state(app, |inner| {
        let first = !inner.revealed;
        inner.revealed = true;
        first
    })
    .unwrap_or(false);
    if !first {
        return;
    }
    place(app);
    let Some(win) = app.get_webview_window(windows::SIDEBAR) else {
        log::warn!("cannot reveal a missing sidebar window");
        return;
    };
    if let Err(e) = win.show() {
        log::error!("showing the sidebar failed: {e}");
        return;
    }
    window::after_show(&win, window::settings_of(app).always_on_top);
    log::info!("sidebar revealed");
}

/// The reveal fallback and the periodic geometry check.
pub fn start_watchdogs(app: &AppHandle) {
    {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_millis(window::REVEAL_FALLBACK_MS)).await;
            reveal(&app);
        });
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(window::GEOMETRY_CHECK_SEC)).await;
            check_geometry(&app);
        }
    });
}

/// Re-place the bar if the window manager (or a resolution / monitor change)
/// moved it away from where it belongs.
fn check_geometry(app: &AppHandle) {
    let Some(state) = window::snapshot(app) else {
        return;
    };
    if !state.revealed {
        return;
    }
    let Some(expected) = desired_rect(app, state.expanded) else {
        return;
    };
    let Some(win) = app.get_webview_window(windows::SIDEBAR) else {
        return;
    };
    let scale = win.scale_factor().unwrap_or(1.0).max(0.1);
    let (Ok(pos), Ok(size)) = (win.outer_position(), win.outer_size()) else {
        return;
    };
    let actual = LogicalRect::new(
        pos.x as f64 / scale,
        pos.y as f64 / scale,
        size.width as f64 / scale,
        size.height as f64 / scale,
    );
    // Only the *origin* is watched. The size is authoritative on our side only
    // as a request: GTK refuses to shrink a window below the webview's minimum
    // size, and re-sending the same size every 5 s would just spin.
    let drift = (actual.x - expected.x)
        .abs()
        .max((actual.y - expected.y).abs());
    if drift > window::GEOMETRY_TOLERANCE {
        log::debug!("sidebar drifted by {drift:.1}px ({actual:?} != {expected:?}), re-placing");
        place_sidebar(app, state.expanded);
        super::popover::reposition(app);
        return;
    }
    let size_drift = (actual.w - expected.w)
        .abs()
        .max((actual.h - expected.h).abs());
    if size_drift > window::GEOMETRY_TOLERANCE {
        log::debug!(
            "sidebar is {}x{} but {}x{} was requested — the webview's minimum size is larger than \
             the content it reported",
            actual.w,
            actual.h,
            expected.w,
            expected.h
        );
    }
}
