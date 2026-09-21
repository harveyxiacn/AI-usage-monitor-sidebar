//! The detail bubble: anchored to the hovered ring, never takes focus.
//! [PLATFORM]

use crate::model::{events, windows, PopoverRequest};
use crate::window::{self, monitors, monitors::LogicalRect};
use tauri::{AppHandle, Emitter, Manager};

/// Where the popover belongs for a given request, or `None` when there is
/// nothing to anchor to.
pub fn desired_rect(app: &AppHandle, req: &PopoverRequest) -> Option<(LogicalRect, f64)> {
    let settings = window::settings_of(app);
    let mon = monitors::target_monitor(app, &settings)?;
    let sidebar = super::sidebar::current_rect(app)?;
    let (w, h) = window::snapshot(app)?.popover_content;
    // The anchor runs along the bar's own axis: `anchorX` for a horizontal bar
    // (older frontends never send it), `anchorY` for a vertical one. Anything
    // missing or not finite falls back to the middle of the bar.
    let anchor = if settings.edge.is_horizontal() {
        req.anchor_x.filter(|x| x.is_finite())
    } else {
        Some(req.anchor_y).filter(|y| y.is_finite())
    };
    let along = if settings.edge.is_horizontal() {
        sidebar.w
    } else {
        sidebar.h
    };
    let anchor = anchor.unwrap_or(along / 2.0);
    Some((
        monitors::popover_rect(
            &mon,
            &sidebar,
            w,
            h,
            anchor,
            window::POPOVER_GAP,
            settings.edge,
        ),
        mon.scale,
    ))
}

/// `popover_show`: remember the request, tell the popover webview what to
/// render, position it and show it **without** focus.
pub fn show(app: &AppHandle, req: PopoverRequest) {
    window::with_state(app, |inner| {
        inner.popover_req = Some(req.clone());
        inner.popover_visible = true;
    });
    if let Err(e) = app.emit_to(windows::POPOVER, events::POPOVER_TARGET, req.clone()) {
        log::warn!("emitting {} failed: {e}", events::POPOVER_TARGET);
    }
    let Some(win) = app.get_webview_window(windows::POPOVER) else {
        log::warn!("popover window is missing");
        return;
    };
    // Never `set_focus()` here: the bar must not steal focus from the editor.
    if let Err(e) = win.set_focusable(false) {
        log::debug!("popover set_focusable(false) unsupported here: {e}");
    }
    apply_rect(app, &win, &req);
    if let Err(e) = win.show() {
        log::warn!("showing the popover failed: {e}");
        return;
    }
    window::after_show(&win, window::settings_of(app).always_on_top);
    // The WM may have nudged the window while mapping it.
    apply_rect(app, &win, &req);
}

fn apply_rect(app: &AppHandle, win: &tauri::WebviewWindow, req: &PopoverRequest) {
    let Some((rect, scale)) = desired_rect(app, req) else {
        return;
    };
    window::place_overlay(win, rect, scale);
    log::debug!(
        "popover placed at {rect:?} for {}#{}",
        req.provider,
        req.ring_index
    );
}

/// `popover_relayout`: the bubble measured itself — resize and re-anchor it to
/// the ring of the last request.
pub fn relayout(app: &AppHandle, width: f64, height: f64) {
    if !(width.is_finite() && height.is_finite()) || width <= 0.0 || height <= 0.0 {
        log::warn!("popover_relayout ignored: {width}x{height}");
        return;
    }
    window::with_state(app, |inner| inner.popover_content = (width, height));
    // A popover page that (re)loaded while visible has lost its target
    // (dev-server reloads, webview restarts); re-sending it is idempotent for
    // a page that already renders it, so always refresh the target here.
    if let Some(state) = window::snapshot(app) {
        if state.popover_visible {
            if let Some(req) = state.popover_req {
                if let Err(e) = app.emit_to(windows::POPOVER, events::POPOVER_TARGET, req) {
                    log::debug!("re-emitting popover target failed: {e}");
                }
            }
        }
    }
    reposition(app);
}

/// Re-apply the geometry of a visible popover (settings change, bar moved,
/// content resized).
pub fn reposition(app: &AppHandle) {
    let Some(state) = window::snapshot(app) else {
        return;
    };
    if !state.popover_visible {
        return;
    }
    let Some(req) = state.popover_req else { return };
    let Some(win) = app.get_webview_window(windows::POPOVER) else {
        return;
    };
    apply_rect(app, &win, &req);
}

/// `popover_hide`. A pinned popover only hides when `force` is set.
pub fn hide(app: &AppHandle, force: bool) {
    let Some((should_hide, state_changed, expanded)) = window::with_state(app, |inner| {
        if inner.pinned && !force {
            return (false, false, inner.expanded);
        }
        let state_changed = force && inner.pinned;
        if force {
            inner.pinned = false;
            inner.generation = inner.generation.wrapping_add(1);
        }
        // A hidden webview need not receive mouseleave from the OS.
        inner.popover_hovered = false;
        let was = inner.popover_visible;
        inner.popover_visible = false;
        (was, state_changed, inner.expanded)
    }) else {
        return;
    };
    if state_changed {
        let _ = app.emit(
            events::SIDEBAR_STATE,
            crate::model::SidebarState {
                expanded,
                pinned: false,
            },
        );
    }
    if !should_hide {
        return;
    }
    let Some(win) = app.get_webview_window(windows::POPOVER) else {
        return;
    };
    if let Err(e) = win.hide() {
        log::warn!("hiding the popover failed: {e}");
    }
}

/// `popover_set_pinned`: a pinned popover ignores hover-out; unpinning starts
/// the hide timers again if the pointer is elsewhere already.
pub fn set_pinned(app: &AppHandle, pinned: bool) {
    let Some((idle, expanded)) = window::with_state(app, |inner| {
        inner.pinned = pinned;
        // Invalidate pending timers either way.
        inner.generation = inner.generation.wrapping_add(1);
        (!inner.bar_hovered && !inner.popover_hovered, inner.expanded)
    }) else {
        return;
    };
    if let Err(e) = app.emit(
        events::SIDEBAR_STATE,
        crate::model::SidebarState { expanded, pinned },
    ) {
        log::warn!("emitting {} failed: {e}", events::SIDEBAR_STATE);
    }
    if !pinned && idle {
        super::hover::schedule_idle_timers(app);
    }
}
