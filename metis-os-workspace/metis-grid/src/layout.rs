use std::sync::atomic::{AtomicI32, Ordering};

use crate::model::TileRect;

/// Header strip rendered by the shell for app tiles; body is the live app window.
/// Height of the compositor-drawn server-side titlebar above each app window.
pub const APP_TILE_HEADER_PX: i32 = 36;

/// Default thickness of the compositor-drawn border on the left/right/bottom of each
/// app window. The live value is configurable at runtime — see [`app_tile_border_px`]
/// / [`set_app_tile_border_px`] — so the client body inset tracks the user's choice.
pub const APP_TILE_BORDER_PX: i32 = 1;

/// Largest border thickness we allow (keeps the client body from collapsing).
pub const MAX_APP_TILE_BORDER_PX: i32 = 16;

/// Runtime border thickness, shared by the layout (client inset) and the compositor's
/// decoration drawing so they always agree. Defaults to [`APP_TILE_BORDER_PX`].
static APP_TILE_BORDER: AtomicI32 = AtomicI32::new(APP_TILE_BORDER_PX);

/// Current compositor-drawn window border thickness (px).
pub fn app_tile_border_px() -> i32 {
    APP_TILE_BORDER.load(Ordering::Relaxed)
}

/// Set the window border thickness (px), clamped to `0..=MAX_APP_TILE_BORDER_PX`.
/// Returns true when the value actually changed (caller may relayout/redamage).
pub fn set_app_tile_border_px(px: i32) -> bool {
    let px = px.clamp(0, MAX_APP_TILE_BORDER_PX);
    APP_TILE_BORDER.swap(px, Ordering::Relaxed) != px
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct MonitorRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct GridMetrics {
    pub columns: u32,
    pub rows: u32,
    pub gutter: u32,
    pub monitor: MonitorRect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PixelRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl PixelRect {
    pub fn lerp(self, other: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            x: lerp_i32(self.x, other.x, t),
            y: lerp_i32(self.y, other.y, t),
            width: lerp_i32(self.width, other.width, t),
            height: lerp_i32(self.height, other.height, t),
        }
    }

    pub fn right(&self) -> i32 {
        self.x + self.width
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.height
    }

    /// True when this rectangle overlaps `other` with positive area.
    pub fn intersects(&self, other: &Self) -> bool {
        self.x < other.right()
            && self.right() > other.x
            && self.y < other.bottom()
            && self.bottom() > other.y
    }
}

/// Pixel rect relative to the desk layer (monitor origin at 0,0).
pub fn desk_pixel_rect(metrics: &GridMetrics, rect: &TileRect) -> PixelRect {
    let abs = cell_to_pixels(metrics, rect);
    PixelRect {
        x: abs.x - metrics.monitor.x,
        y: abs.y - metrics.monitor.y,
        width: abs.width,
        height: abs.height,
    }
}

/// Server-side titlebar strip for an app tile — the draggable chrome at the top.
pub fn app_tile_chrome_rect(full: PixelRect) -> PixelRect {
    let height = APP_TILE_HEADER_PX.min(full.height.max(1));
    PixelRect {
        x: full.x,
        y: full.y,
        width: full.width.max(1),
        height,
    }
}

/// Client region for auto-hide chrome: the window fills the tile and the
/// titlebar is drawn as a hover overlay on the top strip.
pub fn app_tile_auto_hide_body_rect(full: PixelRect) -> PixelRect {
    let b = app_tile_border_px();
    let border = b.min((full.width / 2).max(0));
    let bottom_border = b.min(full.height.max(0));
    PixelRect {
        x: full.x + border,
        y: full.y + border,
        width: (full.width - border * 2).max(1),
        height: (full.height - border - bottom_border).max(1),
    }
}

/// Inner client region where the compositor maps the app window: the full tile
/// frame minus the server-side titlebar (top) and border (left/right/bottom).
pub fn app_tile_body_rect(full: PixelRect) -> PixelRect {
    let b = app_tile_border_px();
    let header = APP_TILE_HEADER_PX.min(full.height);
    let border = b.min((full.width / 2).max(0));
    let bottom_border = b.min((full.height - header).max(0));
    PixelRect {
        x: full.x + border,
        y: full.y + header,
        width: (full.width - border * 2).max(1),
        height: (full.height - header - bottom_border).max(1),
    }
}

pub fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

fn lerp_i32(from: i32, to: i32, t: f32) -> i32 {
    from + ((to - from) as f32 * t).round() as i32
}

pub fn cell_to_pixels(metrics: &GridMetrics, rect: &TileRect) -> PixelRect {
    let cols = metrics.columns.max(1) as i32;
    let rows = metrics.rows.max(1) as i32;
    let gutter = metrics.gutter as i32;
    let usable_w = metrics.monitor.width - gutter * (cols + 1);
    let usable_h = metrics.monitor.height - gutter * (rows + 1);
    let cell_w = usable_w / cols;
    let cell_h = usable_h / rows;

    let x = metrics.monitor.x + gutter + rect.col as i32 * (cell_w + gutter);
    let y = metrics.monitor.y + gutter + rect.row as i32 * (cell_h + gutter);
    let width = rect.w as i32 * cell_w + (rect.w as i32 - 1).max(0) * gutter;
    let height = rect.h as i32 * cell_h + (rect.h as i32 - 1).max(0) * gutter;

    PixelRect {
        x,
        y,
        width,
        height,
    }
}

/// Map a monitor-space pointer to the grid cell under it.
pub fn pixel_to_grid_cell(x: i32, y: i32, metrics: &GridMetrics) -> (u32, u32) {
    let cols = metrics.columns.max(1) as i32;
    let rows = metrics.rows.max(1) as i32;
    let gutter = metrics.gutter as i32;
    let rel_x = x - metrics.monitor.x - gutter;
    let rel_y = y - metrics.monitor.y - gutter;
    let usable_w = metrics.monitor.width - gutter * (cols + 1);
    let usable_h = metrics.monitor.height - gutter * (rows + 1);
    let cell_w = (usable_w / cols).max(1);
    let cell_h = (usable_h / rows).max(1);
    let stride_x = cell_w + gutter;
    let stride_y = cell_h + gutter;

    let col = if rel_x <= 0 {
        0
    } else {
        (rel_x / stride_x).min(cols - 1) as u32
    };
    let row = if rel_y <= 0 {
        0
    } else {
        (rel_y / stride_y).min(rows - 1) as u32
    };
    (col, row)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lerp_rect_midpoint() {
        let a = PixelRect {
            x: 0,
            y: 0,
            width: 100,
            height: 100,
        };
        let b = PixelRect {
            x: 100,
            y: 200,
            width: 300,
            height: 400,
        };
        let mid = a.lerp(b, 0.5);
        assert_eq!(mid.x, 50);
        assert_eq!(mid.y, 100);
        assert_eq!(mid.width, 200);
        assert_eq!(mid.height, 250);
    }

    #[test]
    fn ease_out_cubic_endpoints() {
        assert!((ease_out_cubic(0.0) - 0.0).abs() < f32::EPSILON);
        assert!((ease_out_cubic(1.0) - 1.0).abs() < f32::EPSILON);
    }
}
