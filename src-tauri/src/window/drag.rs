//! Dragging the edge bar. [PLATFORM]
//!
//! The webview reports how far the pointer travelled since the drag began; the
//! bar follows freely and, on release, snaps to the nearer vertical edge of the
//! monitor it was dropped on. The drop is stored in the ordinary settings
//! (`edge`, `monitor`, `verticalOffset`), so it survives restarts and the
//! settings tab keeps telling the truth.

use crate::model::{windows, Edge, Settings, VerticalAlign};
use crate::window::{self, monitors, monitors::LogicalRect, monitors::MonitorRect};
use serde::Deserialize;
use tauri::{AppHandle, Manager};

#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DragPhase {
    Start,
    Move,
    End,
    Cancel,
}

/// Where a dropped bar ends up.
#[derive(Debug, Clone, PartialEq)]
pub struct Drop {
    pub monitor: String,
    pub edge: Edge,
    pub vertical_offset: i32,
}

/// The monitor holding the centre of `rect`, else the closest one.
fn monitor_at<'a>(all: &'a [MonitorRect], rect: &LogicalRect) -> Option<&'a MonitorRect> {
    let (cx, cy) = (rect.center_x(), rect.center_y());
    let distance = |m: &MonitorRect| {
        let dx = (m.rect.x - cx).max(cx - m.rect.right()).max(0.0);
        let dy = (m.rect.y - cy).max(cy - m.rect.bottom()).max(0.0);
        dx * dx + dy * dy
    };
    all.iter()
        .min_by(|a, b| distance(a).total_cmp(&distance(b)))
}

/// Pure snap maths: nearer edge of the drop monitor, and the offset that makes
/// [`monitors::sidebar_rect`] reproduce the dropped `y` for the current align.
pub fn snap(all: &[MonitorRect], rect: &LogicalRect, settings: &Settings) -> Option<Drop> {
    let mon = monitor_at(all, rect)?;
    let m = mon.rect;
    let edge = if rect.center_x() < m.center_x() {
        Edge::Left
    } else {
        Edge::Right
    };
    let height = rect.h.min(m.h);
    let y = monitors::clamp_span(rect.y, height, m.y, m.h);
    let base = match settings.vertical_align {
        VerticalAlign::Top => m.y,
        VerticalAlign::Center => m.y + (m.h - height) / 2.0,
        VerticalAlign::Bottom => m.bottom() - height,
    };
    Some(Drop {
        monitor: mon.name.clone(),
        edge,
        vertical_offset: (y - base).round() as i32,
    })
}

/// `sidebar_drag`: `dx`/`dy` are CSS px travelled since `Start`.
pub fn drag(app: &AppHandle, phase: DragPhase, dx: f64, dy: f64) {
    match phase {
        DragPhase::Start => {
            let origin = super::sidebar::current_rect(app);
            window::with_state(app, |inner| inner.drag_origin = origin);
            super::popover::hide(app, true);
        }
        DragPhase::Move => {
            if let Some(rect) = dragged_rect(app, dx, dy) {
                move_to(app, rect);
            }
        }
        DragPhase::End => {
            let rect = dragged_rect(app, dx, dy);
            window::with_state(app, |inner| inner.drag_origin = None);
            if let Some(rect) = rect {
                drop_at(app, rect);
            }
            // Also covers a drop that changed nothing: snap back to the edge.
            super::sidebar::place(app);
        }
        DragPhase::Cancel => {
            window::with_state(app, |inner| inner.drag_origin = None);
            super::sidebar::place(app);
        }
    }
}

fn dragged_rect(app: &AppHandle, dx: f64, dy: f64) -> Option<LogicalRect> {
    if !(dx.is_finite() && dy.is_finite()) {
        return None;
    }
    let origin = window::snapshot(app)?.drag_origin?;
    Some(LogicalRect::new(
        origin.x + dx,
        origin.y + dy,
        origin.w,
        origin.h,
    ))
}

fn move_to(app: &AppHandle, rect: LogicalRect) {
    let Some(win) = app.get_webview_window(windows::SIDEBAR) else {
        return;
    };
    let Some((_, scale)) = super::sidebar::desired_rect(app, true) else {
        return;
    };
    if window::layer_shell_active() {
        // Anchors and margins: the bar slides along its edge until dropped.
        window::place_overlay(&win, rect, scale);
        return;
    }
    let (position, _) = rect.to_physical(scale);
    if let Err(e) = win.set_position(position) {
        log::debug!("drag set_position failed: {e}");
    }
}

fn drop_at(app: &AppHandle, rect: LogicalRect) {
    let settings = window::settings_of(app);
    let all = monitors::all_monitors(app);
    let Some(drop) = snap(&all, &rect, &settings) else {
        return;
    };
    let mut patch = serde_json::json!({
        "edge": drop.edge,
        "verticalOffset": drop.vertical_offset,
    });
    // Leave "follow the primary monitor" (null) alone unless the bar left it.
    let current = monitors::target_monitor(app, &settings).map(|m| m.name);
    if current.as_deref() != Some(drop.monitor.as_str()) {
        patch["monitor"] = drop.monitor.clone().into();
    }
    log::info!("sidebar dropped: {drop:?}");
    if let Err(e) = crate::commands::settings::update(app, &patch) {
        log::error!("could not persist the dropped position: {e:#}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn desk() -> Vec<MonitorRect> {
        vec![
            MonitorRect::new("main", 0.0, 0.0, 2560.0, 1440.0, 1.0),
            MonitorRect::new("side", -1280.0, 24.0, 1280.0, 696.0, 2.0),
        ]
    }

    #[test]
    fn drop_snaps_to_the_nearer_edge_and_keeps_the_height() {
        let settings = Settings::default(); // right edge, centred
        let drop = snap(
            &desk(),
            &LogicalRect::new(300.0, 100.0, 76.0, 200.0),
            &settings,
        )
        .unwrap();
        assert_eq!(drop.monitor, "main");
        assert_eq!(drop.edge, Edge::Left);
        assert_eq!(drop.vertical_offset, 100 - 620);

        // The stored offset reproduces the dropped position exactly.
        let dropped = Settings {
            edge: drop.edge,
            vertical_offset: drop.vertical_offset,
            ..settings.clone()
        };
        let rect = monitors::sidebar_rect(&desk()[0], &dropped, 76.0, 200.0, true);
        assert_eq!((rect.x, rect.y), (0.0, 100.0));
    }

    #[test]
    fn drop_on_another_monitor_selects_it_and_clamps_to_its_work_area() {
        let settings = Settings {
            vertical_align: VerticalAlign::Top,
            ..Settings::default()
        };
        let drop = snap(
            &desk(),
            &LogicalRect::new(-200.0, -500.0, 76.0, 200.0),
            &settings,
        )
        .unwrap();
        assert_eq!(drop.monitor, "side");
        assert_eq!(drop.edge, Edge::Right);
        assert_eq!(drop.vertical_offset, 0);
    }

    #[test]
    fn a_drop_outside_every_monitor_uses_the_closest_one() {
        let drop = snap(
            &desk(),
            &LogicalRect::new(9000.0, 5000.0, 76.0, 200.0),
            &Settings::default(),
        )
        .unwrap();
        assert_eq!(drop.monitor, "main");
        assert_eq!(drop.edge, Edge::Right);
        assert!(snap(
            &[],
            &LogicalRect::new(0.0, 0.0, 1.0, 1.0),
            &Settings::default()
        )
        .is_none());
    }
}
