//! The edge bar window: placement, expand/collapse, reveal and the geometry
//! watchdog. [PLATFORM]

use crate::model::{events, windows, SidebarState};
use crate::window::{self, monitors, monitors::LogicalRect};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

/// Compute where the bar should be right now, without touching any window.
pub fn desired_rect(app: &AppHandle, expanded: bool) -> Option<(LogicalRect, f64)> {
    let settings = window::settings_of(app);
    let mon = monitors::target_monitor(app, &settings)?;
    let (content_w, content_h) = window::snapshot(app)?.sidebar_content;
    Some((
        monitors::sidebar_rect(&mon, &settings, content_w, content_h, expanded),
        mon.scale,
    ))
}

/// The rect the bar currently occupies according to our own bookkeeping — the
/// popover anchors against this rather than against the WM's idea of it.
pub fn current_rect(app: &AppHandle) -> Option<LogicalRect> {
    let expanded = window::snapshot(app)?.expanded;
    desired_rect(app, expanded).map(|(rect, _)| rect)
}

/// Resize + reposition the bar. `expanded` decides the width only.
pub fn place_sidebar(app: &AppHandle, expanded: bool) -> Option<LogicalRect> {
    let (rect, scale) = desired_rect(app, expanded)?;
    let Some(win) = app.get_webview_window(windows::SIDEBAR) else {
        log::warn!("sidebar window is gone, cannot place it");
        return None;
    };
    window::place_overlay(&win, rect, scale);
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
    super::popover::reposition(app);
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
    window::with_state(app, |inner| {
        // A collapsed handle is not the expanded content. Remember the latter
        // so expansion restores its width before the webview's next layout.
        if inner.expanded {
            inner.sidebar_content = (width, height);
        }
    });
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
        window::with_state(app, |inner| inner.revealed = false);
        return;
    }
    window::after_show(&win, window::settings_of(app).always_on_top);
    super::hover::schedule_idle_timers(app);
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
        reveal(app);
        return;
    }
    // A layer surface has no position of its own: `outer_position` reports
    // nothing meaningful, so a drift check could only ever re-place the bar
    // in a loop. The compositor keeps it anchored; settings changes and
    // relayouts still go through `place_sidebar`.
    if window::layer_shell_active() {
        return;
    }
    let Some((expected, scale)) = desired_rect(app, state.expanded) else {
        return;
    };
    let Some(win) = app.get_webview_window(windows::SIDEBAR) else {
        return;
    };
    // Compare in the target monitor's coordinate system, even while the
    // window is on another screen with a different scale factor.
    let (Ok(pos), Ok(size)) = (win.outer_position(), win.outer_size()) else {
        return;
    };
    let actual = LogicalRect::new(
        pos.x as f64 / scale,
        pos.y as f64 / scale,
        size.width as f64 / scale,
        size.height as f64 / scale,
    );
    let drift = (actual.x - expected.x)
        .abs()
        .max((actual.y - expected.y).abs())
        .max((actual.w - expected.w).abs())
        .max((actual.h - expected.h).abs());
    if drift > window::GEOMETRY_TOLERANCE {
        log::debug!("sidebar drifted by {drift:.1}px ({actual:?} != {expected:?}), re-placing");
        place_sidebar(app, state.expanded);
        super::popover::reposition(app);
    }
}
