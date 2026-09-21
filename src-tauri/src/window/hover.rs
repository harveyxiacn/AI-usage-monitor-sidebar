//! The hover state machine. [PLATFORM]
//!
//! State: `{bar_hovered, popover_hovered, pinned, expanded}`.
//!
//! * `bar/true`  → cancel every pending timer, expand a collapsed bar.
//! * `*/false`   → if neither the bar nor the popover is hovered, start the
//!   idle timers: hide the popover after 250 ms (a pinned one after 8 s) and,
//!   when `autoHide` is on, collapse the bar after `autoHideDelayMs` (unless
//!   the pointer came back).
//! * While the popover is visible a slow pointer check corrects hover flags
//!   whose `mouseleave` the webview never delivered.
//!
//! Timers are plain `tauri::async_runtime::spawn` + `tokio::sleep` tasks. They
//! are "cancelled" by bumping a generation counter: a task that wakes up with a
//! stale generation returns without doing anything, which keeps the whole
//! machine lock-free and free of `JoinHandle` bookkeeping.

use crate::window::{self, popover, sidebar};
use std::time::Duration;
use tauri::AppHandle;

/// `hover_report`.
pub fn report(app: &AppHandle, source: &str, hovered: bool) {
    let source = match source {
        "bar" => Source::Bar,
        "popover" => Source::Popover,
        other => {
            log::warn!("hover_report: unknown source `{other}`");
            return;
        }
    };
    let Some((idle, expanded)) = window::with_state(app, |inner| {
        match source {
            Source::Bar => inner.bar_hovered = hovered,
            Source::Popover => inner.popover_hovered = hovered,
        }
        inner.last_activity = std::time::Instant::now();
        // Any hover transition invalidates the timers that are in flight.
        inner.generation = inner.generation.wrapping_add(1);
        (!inner.bar_hovered && !inner.popover_hovered, inner.expanded)
    }) else {
        return;
    };

    log::debug!(
        "hover {}: {hovered} (idle={idle})",
        if source == Source::Bar {
            "bar"
        } else {
            "popover"
        }
    );
    if hovered {
        if source == Source::Bar && !expanded {
            sidebar::set_expanded(app, true);
        }
        return;
    }
    if idle {
        schedule_idle_timers(app);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Source {
    Bar,
    Popover,
}

/// Start the "pointer left everything" timers. Safe to call when they are
/// already running: every call retires the previous generation.
pub fn schedule_idle_timers(app: &AppHandle) {
    let Some((generation, pinned)) = window::with_state(app, |inner| {
        inner.generation = inner.generation.wrapping_add(1);
        if inner.bar_hovered || inner.popover_hovered {
            None
        } else {
            Some((inner.generation, inner.pinned))
        }
    })
    .flatten() else {
        return;
    };
    let settings = window::settings_of(app);
    let auto_hide = settings.auto_hide;
    let collapse_after = settings.auto_hide_delay_ms;
    let hide_after = if pinned {
        window::PINNED_POPOVER_HIDE_DELAY_MS
    } else {
        window::POPOVER_HIDE_DELAY_MS
    };
    let app = app.clone();

    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(hide_after)).await;
        if !still_away(&app, generation) {
            return;
        }
        // Forcing also unpins, which retires `generation`; carry on with the
        // one that replaced it so the collapse below still belongs to us.
        popover::hide(&app, pinned);
        let generation = if pinned {
            match window::snapshot(&app) {
                Some(state) => state.generation,
                None => return,
            }
        } else {
            generation
        };

        if !auto_hide {
            return;
        }
        let rest = collapse_after.saturating_sub(hide_after);
        if rest > 0 {
            tokio::time::sleep(Duration::from_millis(rest)).await;
        }
        if !still_away(&app, generation) || !window::settings_of(&app).auto_hide {
            return;
        }
        sidebar::set_expanded(&app, false);
    });
}

/// Correct hover flags against the real pointer while the popover is up. A
/// webview occasionally never sees `mouseleave` (fast exit, the window moving
/// from under the pointer, an unfocusable X11 window), which used to leave the
/// popover open until the next hover. Only a pointer that is *verifiably*
/// outside both windows counts; where the OS cannot say, nothing changes.
pub fn start_pointer_check(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut last = None;
        loop {
            tokio::time::sleep(Duration::from_millis(window::POINTER_CHECK_MS)).await;
            let Some(state) = window::snapshot(&app) else {
                continue;
            };
            if inactivity_expired(&state, window::settings_of(&app).popover_timeout_sec) {
                // The failsafe that does not depend on `mouseleave` at all: a
                // pointer that is really over the bar or the popover keeps
                // sending heartbeats, so silence means it is gone (or asleep).
                log::debug!(
                    "popover idle for {:?} (pinned={}), closing it",
                    state.last_activity.elapsed(),
                    state.pinned
                );
                window::with_state(&app, |inner| {
                    inner.bar_hovered = false;
                    inner.popover_hovered = false;
                });
                popover::hide(&app, true);
                schedule_idle_timers(&app);
                last = None;
                continue;
            }
            if !state.popover_visible
                || state.drag_origin.is_some()
                || !(state.bar_hovered || state.popover_hovered)
            {
                last = None;
                continue;
            }
            // Two consecutive verdicts, so a `mouseenter` that is still on its
            // way to us is never overruled. (On XWayland the pointer freezes at
            // its last in-window position over native Wayland surfaces; that
            // reads as "inside" and simply changes nothing.)
            let outside = pointer_is_outside(&app) == Some(true);
            let confirmed = outside && last == Some(true);
            last = Some(outside);
            if !confirmed {
                continue;
            }
            log::debug!("pointer is outside both windows, correcting stale hover flags");
            window::with_state(&app, |inner| {
                inner.bar_hovered = false;
                inner.popover_hovered = false;
            });
            schedule_idle_timers(&app);
        }
    });
}

/// Whether the popover has outlived `popoverTimeoutSec` (×6 while pinned)
/// without any pointer activity. 0 disables the failsafe; a drag is activity.
fn inactivity_expired(state: &window::Inner, timeout_sec: u64) -> bool {
    if timeout_sec == 0 || !state.popover_visible || state.drag_origin.is_some() {
        return false;
    }
    let factor = if state.pinned {
        window::PINNED_TIMEOUT_FACTOR
    } else {
        1
    };
    state.last_activity.elapsed() >= Duration::from_secs(timeout_sec * factor)
}

/// `None` when the OS cannot tell. Everything is physical px straight from
/// the windowing system, so mixed-DPI desktops need no scale guessing.
fn pointer_is_outside(app: &AppHandle) -> Option<bool> {
    use crate::model::windows;
    use tauri::Manager;
    // A layer surface has no position to compare against.
    if window::layer_shell_active() {
        return None;
    }
    let p = app.cursor_position().ok()?;
    let mut rects = Vec::with_capacity(2);
    for label in [windows::SIDEBAR, windows::POPOVER] {
        let win = app.get_webview_window(label)?;
        let (pos, size) = (win.outer_position().ok()?, win.outer_size().ok()?);
        rects.push(window::monitors::LogicalRect::new(
            pos.x as f64,
            pos.y as f64,
            size.width as f64,
            size.height as f64,
        ));
    }
    Some(outside_all((p.x, p.y), rects.iter()))
}

/// Slack around the windows, px: covers the bar ↔ popover gap (10 logical px,
/// so up to 30 physical at 300 %) and window-manager frame rounding.
const POINTER_SLACK: f64 = 32.0;

fn outside_all<'a>(
    (x, y): (f64, f64),
    rects: impl Iterator<Item = &'a window::monitors::LogicalRect>,
) -> bool {
    let mut any = false;
    for r in rects {
        any = true;
        if x >= r.x - POINTER_SLACK
            && x <= r.right() + POINTER_SLACK
            && y >= r.y - POINTER_SLACK
            && y <= r.bottom() + POINTER_SLACK
        {
            return false;
        }
    }
    any
}

/// Settings changes must invalidate timers that captured the old auto-hide
/// flag/delay. Disabling auto-hide also restores the expanded window at once.
pub fn settings_changed(app: &AppHandle) {
    let settings = window::settings_of(app);
    if !settings.auto_hide && window::snapshot(app).is_some_and(|inner| !inner.expanded) {
        sidebar::set_expanded(app, true);
    }
    schedule_idle_timers(app);
}

/// A timer may only fire while it is the newest one and the pointer is still
/// away from both windows. Pinning or unpinning retires the generation, so a
/// timer always acts on the pin state it was scheduled with.
fn still_away(app: &AppHandle, generation: u64) -> bool {
    match window::snapshot(app) {
        Some(state) => away_at_generation(&state, generation),
        None => false,
    }
}

fn away_at_generation(state: &window::Inner, generation: u64) -> bool {
    state.generation == generation && !state.bar_hovered && !state.popover_hovered
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_timer_cannot_act_after_a_new_generation() {
        let mut state = window::Inner::default();
        let scheduled = state.generation;
        assert!(away_at_generation(&state, scheduled));
        state.generation = state.generation.wrapping_add(1);
        assert!(!away_at_generation(&state, scheduled));
        assert!(away_at_generation(&state, state.generation));
    }

    #[test]
    fn pointer_returning_to_either_window_cancels_idle_action() {
        for (bar, popover) in [(true, false), (false, true)] {
            let state = window::Inner {
                bar_hovered: bar,
                popover_hovered: popover,
                ..Default::default()
            };
            assert!(!away_at_generation(&state, state.generation));
        }
        // A pinned popover still times out once the pointer is away.
        let pinned = window::Inner {
            pinned: true,
            ..Default::default()
        };
        assert!(away_at_generation(&pinned, pinned.generation));
    }

    #[test]
    fn a_silent_popover_times_out_and_a_pinned_one_gets_six_times_as_long() {
        let ago = |secs| std::time::Instant::now() - Duration::from_secs(secs);
        let shown = |pinned, secs| window::Inner {
            popover_visible: true,
            // stale flags are exactly the case this failsafe exists for
            popover_hovered: true,
            pinned,
            last_activity: ago(secs),
            ..Default::default()
        };
        assert!(!inactivity_expired(&shown(false, 9), 10));
        assert!(inactivity_expired(&shown(false, 10), 10));
        assert!(!inactivity_expired(&shown(true, 59), 10));
        assert!(inactivity_expired(&shown(true, 60), 10));
        // off, hidden, or being dragged: never
        assert!(!inactivity_expired(&shown(false, 3_600), 0));
        let hidden = window::Inner {
            popover_visible: false,
            ..shown(false, 3_600)
        };
        assert!(!inactivity_expired(&hidden, 10));
        let dragging = window::Inner {
            drag_origin: Some(window::monitors::LogicalRect::new(0.0, 0.0, 1.0, 1.0)),
            ..shown(false, 3_600)
        };
        assert!(!inactivity_expired(&dragging, 10));
    }

    #[test]
    fn pointer_check_needs_a_known_window_and_honours_the_slack() {
        use window::monitors::LogicalRect;
        let bar = LogicalRect::new(2484.0, 600.0, 76.0, 200.0);
        let bubble = LogicalRect::new(2134.0, 580.0, 340.0, 220.0);
        let both = [bar, bubble];
        assert!(!outside_all((2500.0, 700.0), both.iter()));
        assert!(!outside_all((2479.0, 700.0), both.iter())); // the gap between them
        assert!(!outside_all((2110.0, 560.0), both.iter())); // within the slack
        assert!(outside_all((1000.0, 700.0), both.iter()));
        assert!(!outside_all((1000.0, 700.0), [].iter())); // nothing known: no verdict
    }
}
