//! The hover state machine. [PLATFORM]
//!
//! State: `{bar_hovered, popover_hovered, pinned, expanded}`.
//!
//! * `bar/true`  → cancel every pending timer, expand a collapsed bar.
//! * `*/false`   → if neither the bar nor the popover is hovered, start the
//!   idle timers: hide the popover after 250 ms (unless pinned) and, when
//!   `autoHide` is on, collapse the bar after `autoHideDelayMs` (unless pinned
//!   or the pointer came back).
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
        // Any hover transition invalidates the timers that are in flight.
        inner.generation = inner.generation.wrapping_add(1);
        (!inner.bar_hovered && !inner.popover_hovered, inner.expanded)
    }) else {
        return;
    };

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
    let Some(generation) = window::with_state(app, |inner| {
        inner.generation = inner.generation.wrapping_add(1);
        if inner.pinned || inner.bar_hovered || inner.popover_hovered {
            None
        } else {
            Some(inner.generation)
        }
    })
    .flatten() else {
        return;
    };
    let settings = window::settings_of(app);
    let auto_hide = settings.auto_hide;
    let collapse_after = settings.auto_hide_delay_ms;
    let app = app.clone();

    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(window::POPOVER_HIDE_DELAY_MS)).await;
        if !still_idle(&app, generation) {
            return;
        }
        popover::hide(&app, false);

        if !auto_hide {
            return;
        }
        let rest = collapse_after.saturating_sub(window::POPOVER_HIDE_DELAY_MS);
        if rest > 0 {
            tokio::time::sleep(Duration::from_millis(rest)).await;
        }
        if !still_idle(&app, generation) || !window::settings_of(&app).auto_hide {
            return;
        }
        sidebar::set_expanded(&app, false);
    });
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

/// A timer may only fire while it is the newest one, nothing is pinned and the
/// pointer is still away from both windows.
fn still_idle(app: &AppHandle, generation: u64) -> bool {
    match window::snapshot(app) {
        Some(state) => idle_at_generation(&state, generation),
        None => false,
    }
}

fn idle_at_generation(state: &window::Inner, generation: u64) -> bool {
    state.generation == generation && !state.pinned && !state.bar_hovered && !state.popover_hovered
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_timer_cannot_act_after_a_new_generation() {
        let mut state = window::Inner::default();
        let scheduled = state.generation;
        assert!(idle_at_generation(&state, scheduled));
        state.generation = state.generation.wrapping_add(1);
        assert!(!idle_at_generation(&state, scheduled));
        assert!(idle_at_generation(&state, state.generation));
    }

    #[test]
    fn pointer_returning_to_either_window_or_pinning_cancels_idle_action() {
        for (bar, popover, pinned) in [
            (true, false, false),
            (false, true, false),
            (false, false, true),
        ] {
            let state = window::Inner {
                bar_hovered: bar,
                popover_hovered: popover,
                pinned,
                ..Default::default()
            };
            assert!(!idle_at_generation(&state, state.generation));
        }
    }
}
