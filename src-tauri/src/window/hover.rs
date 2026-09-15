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
/// already running: the generation bump in [`report`] retires the old ones.
pub fn schedule_idle_timers(app: &AppHandle) {
    let Some(state) = window::snapshot(app) else {
        return;
    };
    if state.pinned {
        return;
    }
    let generation = state.generation;
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
        if !still_idle(&app, generation) {
            return;
        }
        sidebar::set_expanded(&app, false);
    });
}

/// A timer may only fire while it is the newest one, nothing is pinned and the
/// pointer is still away from both windows.
fn still_idle(app: &AppHandle, generation: u64) -> bool {
    match window::snapshot(app) {
        Some(state) => {
            state.generation == generation
                && !state.pinned
                && !state.bar_hovered
                && !state.popover_hovered
        }
        None => false,
    }
}
