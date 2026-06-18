use crate::model::TileRect;

/// Header strip rendered by the shell for app tiles; body is the live app window.
pub const APP_TILE_HEADER_PX: i32 = 44;

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

/// Shell layer chrome for an app tile — header strip only, not the app body.
pub fn app_tile_chrome_rect(full: PixelRect) -> PixelRect {
    let height = APP_TILE_HEADER_PX.min(full.height.max(1));
    PixelRect {
        x: full.x,
        y: full.y,
        width: full.width.max(1),
        height,
    }
}

/// Region where the compositor maps the app window (below shell header chrome).
pub fn app_tile_body_rect(full: PixelRect) -> PixelRect {
    let header = APP_TILE_HEADER_PX.min(full.height);
    PixelRect {
        x: full.x,
        y: full.y + header,
        width: full.width.max(1),
        height: full.height.saturating_sub(header).max(1),
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
