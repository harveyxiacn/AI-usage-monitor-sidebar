//! Monitor enumeration and the pure geometry maths. [PLATFORM]
//!
//! Everything in here works in **logical** pixels (CSS px): the OS reports
//! monitor position/size in physical pixels. Layout uses the target monitor's
//! scale, then converts back to physical pixels before calling the window API.
//! Logical window setters would use the window's old DPI while moving between
//! monitors, which can put an overlay on the wrong screen.

use crate::model::{Edge, MonitorInfo, Settings, VerticalAlign};
use tauri::{AppHandle, Monitor, PhysicalPosition, PhysicalSize};

/// A rectangle in logical pixels in the virtual-desktop coordinate space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LogicalRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl LogicalRect {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }
    pub fn right(&self) -> f64 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }
    pub fn center_x(&self) -> f64 {
        self.x + self.w / 2.0
    }
    pub fn center_y(&self) -> f64 {
        self.y + self.h / 2.0
    }

    pub fn to_physical(&self, scale: f64) -> (PhysicalPosition<i32>, PhysicalSize<u32>) {
        (
            PhysicalPosition::new(
                (self.x * scale).round() as i32,
                (self.y * scale).round() as i32,
            ),
            PhysicalSize::new(
                (self.w * scale).round() as u32,
                (self.h * scale).round() as u32,
            ),
        )
    }
}

/// A monitor reduced to what the placement maths needs.
#[derive(Clone, Debug, PartialEq)]
pub struct MonitorRect {
    pub name: String,
    /// logical geometry
    pub rect: LogicalRect,
    pub scale: f64,
}

impl MonitorRect {
    pub fn from_monitor(m: &Monitor) -> Self {
        let scale = if m.scale_factor().is_finite() && m.scale_factor() > 0.0 {
            m.scale_factor()
        } else {
            1.0
        };
        // Keep overlays clear of the Dock, menu bar, taskbar and desktop panels.
        // A compositor that cannot report a work area falls back to full bounds.
        let work_area = m.work_area();
        let (pos, size) = if work_area.size.width > 0 && work_area.size.height > 0 {
            (&work_area.position, &work_area.size)
        } else {
            (m.position(), m.size())
        };
        Self {
            name: m.name().cloned().unwrap_or_else(|| "unknown".to_string()),
            rect: LogicalRect::new(
                pos.x as f64 / scale,
                pos.y as f64 / scale,
                size.width as f64 / scale,
                size.height as f64 / scale,
            ),
            scale,
        }
    }

    /// Test helper / fallback constructor.
    pub fn new(name: &str, x: f64, y: f64, w: f64, h: f64, scale: f64) -> Self {
        Self {
            name: name.to_string(),
            rect: LogicalRect::new(x, y, w, h),
            scale,
        }
    }
}

/// The monitor the sidebar should live on: `settings.monitor` by name, else the
/// primary monitor, else the first monitor the OS reports.
pub fn target_monitor(app: &AppHandle, settings: &Settings) -> Option<MonitorRect> {
    let monitors = match app.available_monitors() {
        Ok(m) => m,
        Err(e) => {
            log::warn!("available_monitors() failed: {e}");
            Vec::new()
        }
    };
    if let Some(wanted) = settings.monitor.as_deref().filter(|n| !n.is_empty()) {
        if let Some(m) = monitors
            .iter()
            .find(|m| m.name().map(|n| n == wanted).unwrap_or(false))
        {
            return Some(MonitorRect::from_monitor(m));
        }
        log::warn!("configured monitor `{wanted}` not found, falling back to the primary monitor");
    }
    match app.primary_monitor() {
        Ok(Some(m)) => return Some(MonitorRect::from_monitor(&m)),
        Ok(None) => {}
        Err(e) => log::warn!("primary_monitor() failed: {e}"),
    }
    monitors.first().map(MonitorRect::from_monitor)
}

/// Every monitor the OS reports, reduced to placement geometry.
pub fn all_monitors(app: &AppHandle) -> Vec<MonitorRect> {
    match app.available_monitors() {
        Ok(m) => m.iter().map(MonitorRect::from_monitor).collect(),
        Err(e) => {
            log::warn!("available_monitors() failed: {e}");
            Vec::new()
        }
    }
}

/// `get_monitors` payload. Position/size are **physical** pixels as reported by
/// the OS (with `scaleFactor` alongside), which is what a settings UI wants to
/// show; the placement maths above uses the logical derivation instead.
pub fn monitor_infos(app: &AppHandle) -> Vec<MonitorInfo> {
    let primary_name = app
        .primary_monitor()
        .ok()
        .flatten()
        .and_then(|m| m.name().cloned());
    let monitors = match app.available_monitors() {
        Ok(m) => m,
        Err(e) => {
            log::warn!("available_monitors() failed: {e}");
            return Vec::new();
        }
    };
    monitors
        .iter()
        .map(|m| {
            let name = m.name().cloned().unwrap_or_else(|| "unknown".to_string());
            MonitorInfo {
                is_primary: primary_name.as_deref() == Some(name.as_str()),
                name,
                x: m.position().x,
                y: m.position().y,
                width: m.size().width,
                height: m.size().height,
                scale_factor: m.scale_factor(),
            }
        })
        .collect()
}

/// Clamp `pos..pos+len` inside `min..min+span`. When the content is taller /
/// wider than the monitor it is pinned to the top-left corner of the monitor.
pub fn clamp_span(pos: f64, len: f64, min: f64, span: f64) -> f64 {
    let max = min + span - len;
    if max <= min {
        min
    } else {
        pos.clamp(min, max)
    }
}

/// Where the sidebar window goes, in logical px.
///
/// * width  — `expanded ? content_w : settings.collapsed_width`
/// * x      — flush against `settings.edge`
/// * height — the content height, never taller than the monitor
/// * y      — per `settings.vertical_align`, then `+ vertical_offset`, clamped
pub fn sidebar_rect(
    mon: &MonitorRect,
    settings: &Settings,
    content_w: f64,
    content_h: f64,
    expanded: bool,
) -> LogicalRect {
    let m = mon.rect;
    let width = if expanded {
        content_w
    } else {
        settings.collapsed_width as f64
    };
    let width = width.max(1.0).min(m.w);
    let height = content_h.max(1.0).min(m.h);

    let x = match settings.edge {
        Edge::Right => m.right() - width,
        Edge::Left => m.x,
    };
    let y = match settings.vertical_align {
        VerticalAlign::Top => m.y,
        VerticalAlign::Center => m.y + (m.h - height) / 2.0,
        VerticalAlign::Bottom => m.bottom() - height,
    } + settings.vertical_offset as f64;

    LogicalRect::new(x, clamp_span(y, height, m.y, m.h), width, height)
}

/// Where the popover goes, in logical px: adjacent to the sidebar on the side
/// that faces the centre of the screen, vertically centred on the hovered ring.
///
/// `anchor_y` is the ring centre in CSS px **relative to the sidebar window**.
pub fn popover_rect(
    mon: &MonitorRect,
    sidebar: &LogicalRect,
    content_w: f64,
    content_h: f64,
    anchor_y: f64,
    gap: f64,
) -> LogicalRect {
    let m = mon.rect;
    let w = content_w.max(1.0).min(m.w);
    let h = content_h.max(1.0).min(m.h);

    // The side facing the screen centre: a bar on the right half opens left.
    let open_left = sidebar.center_x() >= m.center_x();
    let x = if open_left {
        sidebar.x - gap - w
    } else {
        sidebar.right() + gap
    };
    let x = clamp_span(x, w, m.x, m.w);

    let y = sidebar.y + anchor_y - h / 2.0;
    let y = clamp_span(y, h, m.y, m.h);

    LogicalRect::new(x, y, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mon() -> MonitorRect {
        // 5120x2880 @2x -> 2560x1440 logical
        MonitorRect::new("DP-1", 0.0, 0.0, 2560.0, 1440.0, 2.0)
    }

    fn settings() -> Settings {
        Settings::default()
    }

    #[test]
    fn right_edge_center_expanded() {
        let r = sidebar_rect(&mon(), &settings(), 76.0, 160.0, true);
        assert_eq!(
            r,
            LogicalRect::new(2560.0 - 76.0, (1440.0 - 160.0) / 2.0, 76.0, 160.0)
        );
    }

    #[test]
    fn right_edge_collapsed_uses_collapsed_width() {
        let mut s = settings();
        s.collapsed_width = 6;
        let r = sidebar_rect(&mon(), &s, 76.0, 160.0, false);
        assert_eq!(r.w, 6.0);
        assert_eq!(
            r.x,
            2560.0 - 6.0,
            "collapsed bar stays flush with the right edge"
        );
        assert_eq!(r.h, 160.0, "collapsing only changes the width");
    }

    #[test]
    fn left_edge_hugs_x_origin() {
        let mut s = settings();
        s.edge = Edge::Left;
        assert_eq!(sidebar_rect(&mon(), &s, 76.0, 160.0, true).x, 0.0);
        s.collapsed_width = 8;
        assert_eq!(sidebar_rect(&mon(), &s, 76.0, 160.0, false).x, 0.0);
    }

    #[test]
    fn secondary_monitor_offset_is_respected() {
        let m = MonitorRect::new("HDMI-1", 2560.0, -200.0, 1920.0, 1080.0, 1.0);
        let r = sidebar_rect(&m, &settings(), 76.0, 160.0, true);
        assert_eq!(r.x, 2560.0 + 1920.0 - 76.0);
        assert_eq!(r.y, -200.0 + (1080.0 - 160.0) / 2.0);
    }

    #[test]
    fn vertical_align_variants() {
        let mut s = settings();
        s.vertical_align = VerticalAlign::Top;
        assert_eq!(sidebar_rect(&mon(), &s, 76.0, 160.0, true).y, 0.0);
        s.vertical_align = VerticalAlign::Bottom;
        assert_eq!(
            sidebar_rect(&mon(), &s, 76.0, 160.0, true).y,
            1440.0 - 160.0
        );
        s.vertical_align = VerticalAlign::Center;
        s.vertical_offset = 100;
        assert_eq!(
            sidebar_rect(&mon(), &s, 76.0, 160.0, true).y,
            (1440.0 - 160.0) / 2.0 + 100.0
        );
    }

    #[test]
    fn vertical_offset_is_clamped_to_the_monitor() {
        let mut s = settings();
        s.vertical_offset = 100_000;
        let r = sidebar_rect(&mon(), &s, 76.0, 160.0, true);
        assert_eq!(r.y, 1440.0 - 160.0);
        s.vertical_offset = -100_000;
        assert_eq!(sidebar_rect(&mon(), &s, 76.0, 160.0, true).y, 0.0);
    }

    #[test]
    fn content_taller_than_the_monitor_is_capped() {
        let r = sidebar_rect(&mon(), &settings(), 76.0, 5000.0, true);
        assert_eq!(r.h, 1440.0);
        assert_eq!(r.y, 0.0);
    }

    #[test]
    fn popover_opens_left_of_a_right_edge_bar() {
        let m = mon();
        let sb = sidebar_rect(&m, &settings(), 76.0, 300.0, true);
        let p = popover_rect(&m, &sb, 340.0, 220.0, 40.0, 10.0);
        assert_eq!(p.x, sb.x - 10.0 - 340.0);
        // ring centre (sb.y + 40) == popover centre
        assert_eq!(p.y + 220.0 / 2.0, sb.y + 40.0);
    }

    #[test]
    fn popover_opens_right_of_a_left_edge_bar() {
        let m = mon();
        let mut s = settings();
        s.edge = Edge::Left;
        let sb = sidebar_rect(&m, &s, 76.0, 300.0, true);
        let p = popover_rect(&m, &sb, 340.0, 220.0, 40.0, 10.0);
        assert_eq!(p.x, sb.right() + 10.0);
    }

    #[test]
    fn popover_is_clamped_vertically() {
        let m = mon();
        let sb = sidebar_rect(&m, &settings(), 76.0, 1400.0, true);
        // ring near the very top of a tall bar
        let top = popover_rect(&m, &sb, 340.0, 220.0, 5.0, 10.0);
        assert_eq!(top.y, 0.0);
        // ring near the very bottom
        let bottom = popover_rect(&m, &sb, 340.0, 220.0, 1395.0, 10.0);
        assert_eq!(bottom.y, 1440.0 - 220.0);
    }

    #[test]
    fn popover_wider_than_the_gap_stays_on_screen() {
        let m = MonitorRect::new("small", 0.0, 0.0, 400.0, 600.0, 1.0);
        let sb = sidebar_rect(&m, &settings(), 76.0, 200.0, true);
        let p = popover_rect(&m, &sb, 380.0, 220.0, 100.0, 10.0);
        assert!(p.x >= 0.0 && p.right() <= 400.0);
    }

    #[test]
    fn popover_follows_a_collapsed_bar() {
        let m = mon();
        let mut s = settings();
        s.collapsed_width = 6;
        let sb = sidebar_rect(&m, &s, 76.0, 300.0, false);
        let p = popover_rect(&m, &sb, 340.0, 220.0, 40.0, 10.0);
        // The bubble hugs the 6 px handle, not the expanded width.
        assert_eq!(p.x, 2560.0 - 6.0 - 10.0 - 340.0);
    }

    #[test]
    fn popover_opens_towards_the_centre_of_the_active_monitor() {
        // Secondary monitor to the right of the primary one; a left-edge bar
        // there must still open its popover to the right (into that monitor).
        let m = MonitorRect::new("HDMI-1", 2560.0, 0.0, 1920.0, 1080.0, 1.0);
        let mut s = settings();
        s.edge = Edge::Left;
        let sb = sidebar_rect(&m, &s, 76.0, 300.0, true);
        assert_eq!(sb.x, 2560.0);
        let p = popover_rect(&m, &sb, 340.0, 220.0, 40.0, 10.0);
        assert_eq!(p.x, 2560.0 + 76.0 + 10.0);
        assert!(p.x >= m.rect.x && p.right() <= m.rect.right());
    }

    #[test]
    fn clamp_span_pins_oversized_content() {
        assert_eq!(clamp_span(-50.0, 100.0, 0.0, 1000.0), 0.0);
        assert_eq!(clamp_span(950.0, 100.0, 0.0, 1000.0), 900.0);
        assert_eq!(clamp_span(500.0, 2000.0, 0.0, 1000.0), 0.0);
    }

    #[test]
    fn mixed_dpi_placement_uses_the_destination_monitor_scale() {
        // The secondary display begins at physical x=1920 and is scaled 200%.
        // Its global logical origin is 960; passing that to a window still at
        // 100% would wrongly keep it on the primary display.
        let m = MonitorRect::new("retina", 960.0, 0.0, 1920.0, 1080.0, 2.0);
        let r = sidebar_rect(&m, &settings(), 76.0, 160.0, true);
        let (position, size) = r.to_physical(m.scale);
        assert_eq!(position, PhysicalPosition::new(5608, 920));
        assert_eq!(size, PhysicalSize::new(152, 320));
        assert_eq!(position.x + size.width as i32, 5760);
    }

    #[test]
    fn fractional_dpi_preserves_the_physical_right_edge() {
        // Round only after returning to physical pixels. Rounding the logical
        // position first leaves a visible seam on fractional-scale monitors.
        let m = MonitorRect::new(
            "fractional",
            -1921.0 / 1.25,
            -100.0,
            1921.0 / 1.25,
            864.0,
            1.25,
        );
        let (position, size) =
            sidebar_rect(&m, &settings(), 76.0, 160.0, true).to_physical(m.scale);
        assert_eq!(position.x + size.width as i32, 0);
        assert_eq!(size.width, 95);
    }

    #[test]
    fn overlay_bounds_respect_a_work_area_with_panels() {
        let m = MonitorRect::new("work-area", 48.0, 30.0, 1872.0, 1010.0, 1.0);
        let mut s = settings();
        s.edge = Edge::Left;
        s.vertical_align = VerticalAlign::Top;
        let bar = sidebar_rect(&m, &s, 76.0, 160.0, true);
        assert_eq!(bar.x, 48.0);
        assert_eq!(bar.y, 30.0);
        let popover = popover_rect(&m, &bar, 340.0, 220.0, 20.0, 10.0);
        assert_eq!(popover.y, 30.0);
        assert!(popover.bottom() <= 1040.0);
    }
}
