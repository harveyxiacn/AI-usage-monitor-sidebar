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
    // While the user holds the bar, timers and relayouts must not yank it back.
    if window::snapshot(app)?.drag_origin.is_some() {
        return None;
    }
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

/// Rebuild a mapped window's presentation after a display change or an
/// explicit request to bring the app back. The webview and its data survive.
pub fn remap(app: &AppHandle) {
    let handle = app.clone();
    if let Err(e) = app.run_on_main_thread(move || {
        let Some(win) = handle.get_webview_window(windows::SIDEBAR) else {
            return;
        };
        // Recheck here, not before queueing the main-thread work: the user may
        // have hidden the bar or started dragging while this was pending.
        let Some(state) = window::snapshot(&handle) else {
            return;
        };
        let visible = match win.is_visible() {
            Ok(visible) => visible,
            Err(e) => {
                log::warn!("checking sidebar visibility before remapping failed: {e}");
                return;
            }
        };
        if !can_remap(&state, visible) {
            return;
        }
        if let Err(e) = win.hide() {
            log::warn!("hiding the sidebar for remapping failed: {e}");
            return;
        }
        super::popover::hide(&handle, true);
        window::with_state(&handle, |inner| {
            inner.bar_hovered = false;
            inner.popover_hovered = false;
            inner.generation = inner.generation.wrapping_add(1);
        });
        place(&handle);
        if let Err(e) = win.show() {
            log::warn!("showing the sidebar after remapping failed: {e}");
            // Let the existing reveal watchdog retry a failed recovery.
            window::with_state(&handle, |inner| inner.revealed = false);
            return;
        }
        window::after_show(&win, window::settings_of(&handle).always_on_top);
        super::hover::schedule_idle_timers(&handle);
        log::info!("sidebar remapped to restore presentation");
    }) {
        log::warn!("scheduling sidebar remapping failed: {e}");
    }
}

fn can_remap(state: &window::Inner, visible: bool) -> bool {
    state.revealed && visible && state.drag_origin.is_none()
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
        let mut displays = DisplayWatch::default();
        loop {
            tokio::time::sleep(Duration::from_secs(window::GEOMETRY_CHECK_SEC)).await;
            let seen = monitors::all_monitors(&app);
            if displays.needs_remap(&seen, std::time::SystemTime::now()) {
                remap(&app);
            }
            check_geometry(&app);
        }
    });
}

/// Notices when the set of displays changed or the machine slept.
///
/// Turning a monitor off (DisplayPort drops off the bus) removes and later
/// re-creates the output. Afterwards the X server still reports the bar as
/// viewable at the right coordinates, but the compositor no longer draws the
/// dock window — so the drift check below sees nothing wrong and the bar stays
/// invisible until the app is restarted. Mapping the window again fixes it.
#[derive(Default)]
struct DisplayWatch {
    /// `None` until the first tick, and while no display is connected.
    signature: Option<Vec<String>>,
    // Linux's monotonic clock can exclude system suspend. Wall time catches
    // waking up on the same monitor layout as well; a clock adjustment is at
    // worst one harmless remap, and a backwards adjustment resets the baseline.
    last_tick: Option<std::time::SystemTime>,
}

impl DisplayWatch {
    /// Recover after a long gap, including system suspend on Linux.
    const STALL: Duration = Duration::from_secs(window::GEOMETRY_CHECK_SEC * 6);

    fn needs_remap(&mut self, seen: &[monitors::MonitorRect], now: std::time::SystemTime) -> bool {
        let stalled = self
            .last_tick
            .is_some_and(|last| now.duration_since(last).is_ok_and(|gap| gap >= Self::STALL));
        let had_sample = self.last_tick.replace(now).is_some();
        if seen.is_empty() {
            // Displays are off right now; remember that so their return counts
            // as a change even if the very same monitor comes back.
            let had_one = self.signature.take().is_some();
            if had_one {
                log::info!("all displays went away");
            }
            return false;
        }
        let mut signature = seen
            .iter()
            .map(|m| {
                let r = m.rect;
                format!("{:?}@{},{},{}x{}*{}", m.name, r.x, r.y, r.w, r.h, m.scale)
            })
            .collect::<Vec<_>>();
        // Monitor enumeration order is not a layout change.
        signature.sort_unstable();
        let changed = self.signature.as_ref() != Some(&signature);
        self.signature = Some(signature);
        // The first nonempty observation is just the initial baseline. A
        // previous empty observation, including at startup, counts as loss.
        (changed && had_sample) || stalled
    }
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
    // The user is holding the bar; snapping it back would fight the drag.
    if state.drag_origin.is_some() {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    fn monitor(name: &str) -> monitors::MonitorRect {
        monitors::MonitorRect::new(name, 0.0, 0.0, 1920.0, 1080.0, 1.0)
    }

    #[test]
    fn first_observation_and_unchanged_displays_do_not_remap() {
        let mut watch = DisplayWatch::default();
        let now = SystemTime::now();
        let seen = [monitor("primary")];
        assert!(!watch.needs_remap(&seen, now));
        assert!(!watch.needs_remap(&seen, now + Duration::from_secs(5)));
        assert!(!watch.needs_remap(&seen, now + Duration::from_secs(10)));
    }

    #[test]
    fn enumeration_order_is_not_a_display_change() {
        let mut watch = DisplayWatch::default();
        let now = SystemTime::now();
        let a = monitor("primary");
        let b = monitors::MonitorRect::new("secondary", -1280.0, 0.0, 1280.0, 720.0, 2.0);
        assert!(!watch.needs_remap(&[a.clone(), b.clone()], now));
        assert!(!watch.needs_remap(&[b, a], now + Duration::from_secs(5)));
    }

    #[test]
    fn removing_and_reconnecting_a_display_each_remap_once() {
        let mut watch = DisplayWatch::default();
        let now = SystemTime::now();
        let a = monitor("primary");
        let b = monitor("secondary");
        assert!(!watch.needs_remap(&[a.clone(), b.clone()], now));
        assert!(watch.needs_remap(std::slice::from_ref(&a), now + Duration::from_secs(5)));
        assert!(!watch.needs_remap(std::slice::from_ref(&a), now + Duration::from_secs(10)));
        assert!(watch.needs_remap(&[a.clone(), b.clone()], now + Duration::from_secs(15)));
        assert!(!watch.needs_remap(&[a, b], now + Duration::from_secs(20)));
    }

    #[test]
    fn all_displays_lost_only_remaps_when_they_return() {
        let mut watch = DisplayWatch::default();
        let now = SystemTime::now();
        let seen = [monitor("primary")];
        assert!(!watch.needs_remap(&seen, now));
        assert!(!watch.needs_remap(&[], now + Duration::from_secs(5)));
        assert!(!watch.needs_remap(&[], now + Duration::from_secs(40)));
        assert!(watch.needs_remap(&seen, now + Duration::from_secs(45)));
        assert!(!watch.needs_remap(&seen, now + Duration::from_secs(50)));
    }

    #[test]
    fn initially_missing_displays_are_recovered() {
        let mut watch = DisplayWatch::default();
        let now = SystemTime::now();
        assert!(!watch.needs_remap(&[], now));
        assert!(!watch.needs_remap(&[], now + Duration::from_secs(5)));
        assert!(watch.needs_remap(&[monitor("primary")], now + Duration::from_secs(10)));
        assert!(!watch.needs_remap(&[monitor("primary")], now + Duration::from_secs(15)));
    }

    #[test]
    fn geometry_scale_and_display_identity_changes_remap() {
        let original = monitor("primary");
        let mut variants = vec![original.clone(); 6];
        variants[0].rect.x = -100.0;
        variants[1].rect.y = 24.0;
        variants[2].rect.w = 2560.0;
        variants[3].rect.h = 1440.0;
        variants[4].scale = 1.5;
        variants[5].name = "replacement".into();
        for changed in variants {
            let mut watch = DisplayWatch::default();
            let now = SystemTime::now();
            assert!(!watch.needs_remap(std::slice::from_ref(&original), now));
            assert!(watch.needs_remap(std::slice::from_ref(&changed), now + Duration::from_secs(5)));
            assert!(!watch.needs_remap(&[changed], now + Duration::from_secs(10)));
        }
    }

    #[test]
    fn stall_boundary_remaps_even_when_display_geometry_is_unchanged() {
        for (delay, expected) in [
            (DisplayWatch::STALL - Duration::from_millis(1), false),
            (DisplayWatch::STALL, true),
            (DisplayWatch::STALL + Duration::from_secs(1), true),
        ] {
            let mut watch = DisplayWatch::default();
            let now = SystemTime::now();
            let seen = [monitor("primary")];
            assert!(!watch.needs_remap(&seen, now));
            assert_eq!(watch.needs_remap(&seen, now + delay), expected);
            assert!(!watch.needs_remap(&seen, now + delay + Duration::from_secs(5)));
        }
    }

    #[test]
    fn backwards_clock_adjustment_resets_the_stall_baseline() {
        let mut watch = DisplayWatch::default();
        let now = SystemTime::now();
        let seen = [monitor("primary")];
        assert!(!watch.needs_remap(&seen, now));
        let earlier = now - Duration::from_secs(60);
        assert!(!watch.needs_remap(&seen, earlier));
        assert!(!watch.needs_remap(&seen, earlier + Duration::from_secs(5)));
        assert!(watch.needs_remap(&seen, earlier + Duration::from_secs(35)));
    }

    #[test]
    fn remapping_respects_user_visibility_and_active_drag() {
        let mut state = window::Inner::default();
        assert!(!can_remap(&state, true));
        state.revealed = true;
        assert!(!can_remap(&state, false));
        assert!(can_remap(&state, true));
        state.drag_origin = Some(LogicalRect::new(0.0, 0.0, 76.0, 160.0));
        assert!(!can_remap(&state, true));
        state.drag_origin = None;
        state.expanded = false;
        assert!(
            can_remap(&state, true),
            "a visible collapsed handle can recover"
        );
    }
}
