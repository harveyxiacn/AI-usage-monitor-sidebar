//! Dragging the edge bar. [PLATFORM]
//!
//! The webview reports how far the pointer travelled since the drag began; the
//! bar follows freely and, on release, snaps to the nearest of the four edges
//! of the monitor it was dropped on. The drop is stored in the ordinary
//! settings (`edge`, `monitor`, `verticalOffset`), so it survives restarts and
//! the settings tab keeps telling the truth.

use crate::model::{windows, Edge, Settings};
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

/// Where a dropped bar ends up. `vertical_offset` is the offset **along** the
/// chosen edge (`Settings::vertical_offset`): y for left/right, x for
/// top/bottom.
#[derive(Debug, Clone, PartialEq)]
pub struct Drop {
    pub monitor: String,
    pub edge: Edge,
    pub vertical_offset: i32,
}

/// How much closer another edge has to be before a drop moves the bar off the
/// edge it is already on, as a fraction of the monitor's half extent. Without
/// it a drop into a corner — where two edges are equally near — would flip
/// between them on a single pixel of pointer travel.
const EDGE_HYSTERESIS: f64 = 0.08;

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

/// The edge of `m` that the centre of `rect` is nearest to, keeping `current`
/// when the difference is within [`EDGE_HYSTERESIS`].
///
/// Distances are normalised by the monitor's half width / half height, so the
/// four zones meet at the monitor's diagonals: on a 3440×1440 ultrawide "near
/// the top" must not mean "anywhere in the upper 720 px".
fn nearest_edge(m: LogicalRect, rect: &LogicalRect, current: Edge) -> Edge {
    // A bar dropped past the monitor is treated as dropped on its border.
    let cx = rect.center_x().max(m.x).min(m.right());
    let cy = rect.center_y().max(m.y).min(m.bottom());
    let half_w = (m.w / 2.0).max(1.0);
    let half_h = (m.h / 2.0).max(1.0);
    let distances = [
        (Edge::Left, (cx - m.x) / half_w),
        (Edge::Right, (m.right() - cx) / half_w),
        (Edge::Top, (cy - m.y) / half_h),
        (Edge::Bottom, (m.bottom() - cy) / half_h),
    ];
    let (nearest, shortest) = distances
        .iter()
        .copied()
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap_or((current, 0.0));
    match distances.iter().find(|(edge, _)| *edge == current) {
        Some((_, held)) if *held <= shortest + EDGE_HYSTERESIS => current,
        _ => nearest,
    }
}

/// Pure snap maths: nearest edge of the drop monitor, and the offset along it
/// that makes [`monitors::sidebar_rect`] reproduce the dropped position for
/// the current align.
pub fn snap(all: &[MonitorRect], rect: &LogicalRect, settings: &Settings) -> Option<Drop> {
    let mon = monitor_at(all, rect)?;
    let m = mon.rect;
    let edge = nearest_edge(m, rect, settings.edge);
    // Everything below runs along the edge the bar snapped to.
    let (dropped, len, min, span) = if edge.is_horizontal() {
        (rect.x, rect.w.min(m.w), m.x, m.w)
    } else {
        (rect.y, rect.h.min(m.h), m.y, m.h)
    };
    let along = monitors::clamp_span(dropped, len, min, span);
    let base = monitors::align_start(settings.vertical_align, min, span, len);
    Some(Drop {
        monitor: mon.name.clone(),
        edge,
        vertical_offset: (along - base).round() as i32,
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
    use crate::model::VerticalAlign;

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
            &LogicalRect::new(-200.0, 24.0, 76.0, 200.0),
            &settings,
        )
        .unwrap();
        assert_eq!(drop.monitor, "side");
        assert_eq!(drop.edge, Edge::Right);
        assert_eq!(drop.vertical_offset, 0);

        // Dropped above that monitor: the top edge is the nearest one, and the
        // stored offset now runs along x.
        let drop = snap(
            &desk(),
            &LogicalRect::new(-200.0, -500.0, 76.0, 200.0),
            &settings,
        )
        .unwrap();
        assert_eq!(drop.monitor, "side");
        assert_eq!(drop.edge, Edge::Top);
        assert_eq!(drop.vertical_offset, -200 + 1280);
    }

    #[test]
    fn drop_snaps_to_the_nearest_of_the_four_edges() {
        let desk = desk();
        // main is 2560x1440: the zones meet at its diagonals, so a drop in the
        // middle of the upper half belongs to the top edge, not to a side.
        let cases = [
            (1200.0, 40.0, Edge::Top),
            (1200.0, 1200.0, Edge::Bottom),
            (60.0, 700.0, Edge::Left),
            (2400.0, 700.0, Edge::Right),
        ];
        for (x, y, expected) in cases {
            let drop = snap(
                &desk,
                &LogicalRect::new(x, y, 76.0, 200.0),
                &Settings::default(),
            )
            .unwrap();
            assert_eq!(drop.edge, expected, "dropped at {x},{y}");
        }
    }

    #[test]
    fn a_drop_into_a_corner_keeps_the_edge_the_bar_is_on() {
        let desk = desk();
        // The top-left corner of `main` is equally near the left and the top
        // edge; the bar must not flip edges on a pixel of pointer travel.
        let corner = LogicalRect::new(0.0, 0.0, 76.0, 76.0);
        for edge in [Edge::Left, Edge::Top] {
            let settings = Settings {
                edge,
                ..Settings::default()
            };
            assert_eq!(snap(&desk, &corner, &settings).unwrap().edge, edge);
        }
        // Far enough past the diagonal the hysteresis is overruled.
        let settings = Settings {
            edge: Edge::Top,
            ..Settings::default()
        };
        let low = LogicalRect::new(0.0, 400.0, 76.0, 200.0);
        assert_eq!(snap(&desk, &low, &settings).unwrap().edge, Edge::Left);
    }

    #[test]
    fn a_horizontal_drop_round_trips_through_sidebar_rect() {
        let desk = desk();
        let settings = Settings {
            edge: Edge::Top,
            ..Settings::default()
        };
        let dropped = LogicalRect::new(420.0, 30.0, 320.0, 76.0);
        let drop = snap(&desk, &dropped, &settings).unwrap();
        assert_eq!(drop.monitor, "main");
        assert_eq!(drop.edge, Edge::Top);
        assert_eq!(drop.vertical_offset, 420 - (2560 - 320) / 2);

        // The stored offset reproduces the dropped x exactly; the bar itself
        // goes flush with the edge again.
        let persisted = Settings {
            edge: drop.edge,
            vertical_offset: drop.vertical_offset,
            ..settings
        };
        let rect = monitors::sidebar_rect(&desk[0], &persisted, 320.0, 76.0, true);
        assert_eq!((rect.x, rect.y), (420.0, 0.0));
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
