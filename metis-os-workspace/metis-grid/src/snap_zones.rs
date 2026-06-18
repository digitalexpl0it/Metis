use super::layout::{pixel_to_grid_cell, GridMetrics};
use super::model::{GridLayout, TileRect};

/// Distance from monitor edge to trigger a half/corner snap (macOS-style).
const EDGE_SNAP_PX: i32 = 48;

/// Drop target while dragging `dragged_id` — edge snaps resize the tile; otherwise it keeps its size.
pub fn drop_target_for_tile(
    dragged_id: &str,
    x: i32,
    y: i32,
    layout: &GridLayout,
    metrics: &GridMetrics,
) -> TileRect {
    let Some(dragged) = layout.tiles.iter().find(|t| t.id == dragged_id) else {
        return snap_target_at_point(x, y, layout, metrics);
    };

    // Edge / corner snaps change the dragged tile to half- or quarter-screen presets.
    if let Some(snap) = edge_snap_zone(x, y, layout.columns, layout.rows, metrics) {
        return snap;
    }

    let (w, h) = (dragged.rect.w, dragged.rect.h);
    let (col, row) = pixel_to_grid_cell(x, y, metrics);
    let col = col.saturating_sub(w / 2).min(layout.columns.saturating_sub(w));
    let row = row.saturating_sub(h / 2).min(layout.rows.saturating_sub(h));
    TileRect::new(col, row, w, h)
}

/// Pick a snap target for the pointer (used when the dragged tile id is unknown).
pub fn snap_target_at_point(
    x: i32,
    y: i32,
    layout: &GridLayout,
    metrics: &GridMetrics,
) -> TileRect {
    if let Some(edge) = edge_snap_zone(x, y, layout.columns, layout.rows, metrics) {
        return edge;
    }

    let (col, row) = pixel_to_grid_cell(x, y, metrics);
    TileRect::new(col, row, 1, 1)
}

fn edge_snap_zone(
    x: i32,
    y: i32,
    columns: u32,
    rows: u32,
    metrics: &GridMetrics,
) -> Option<TileRect> {
    let m = metrics.monitor;
    let rx = x - m.x;
    let ry = y - m.y;
    if rx < 0 || ry < 0 || rx > m.width || ry > m.height {
        return None;
    }

    let near_left = rx < EDGE_SNAP_PX;
    let near_right = rx > m.width - EDGE_SNAP_PX;
    let near_top = ry < EDGE_SNAP_PX;
    let near_bottom = ry > m.height - EDGE_SNAP_PX;

    if !near_left && !near_right && !near_top && !near_bottom {
        return None;
    }

    let half_w = (columns / 2).max(1);
    let half_h = (rows / 2).max(1);

    match (near_top, near_bottom, near_left, near_right) {
        (true, _, true, _) => Some(TileRect::new(0, 0, half_w, half_h)),
        (true, _, _, true) => Some(TileRect::new(half_w, 0, columns.saturating_sub(half_w), half_h)),
        (_, true, true, _) => Some(TileRect::new(0, half_h, half_w, rows.saturating_sub(half_h))),
        (_, true, _, true) => Some(TileRect::new(
            half_w,
            half_h,
            columns.saturating_sub(half_w),
            rows.saturating_sub(half_h),
        )),
        (true, _, _, _) => Some(TileRect::new(0, 0, columns, half_h)),
        (_, true, _, _) => Some(TileRect::new(0, half_h, columns, rows.saturating_sub(half_h))),
        (_, _, true, _) => Some(TileRect::new(0, 0, half_w, rows)),
        (_, _, _, true) => Some(TileRect::new(half_w, 0, columns.saturating_sub(half_w), rows)),
        _ => None,
    }
}

pub fn monitor_point_from_grid_local(
    grid_x: f64,
    grid_y: f64,
    monitor: super::layout::MonitorRect,
) -> (i32, i32) {
    (
        monitor.x + grid_x.round() as i32,
        monitor.y + grid_y.round() as i32,
    )
}

pub fn snap_label(zone: &TileRect, layout: &GridLayout) -> &'static str {
    if layout.tiles.iter().any(|t| t.rect == *zone) {
        return "Snap to tile";
    }
    if zone.col == 0
        && zone.row == 0
        && zone.w == layout.columns
        && zone.h == layout.rows
    {
        return "Full screen";
    }
    if zone.w == layout.columns && zone.h == layout.rows / 2 {
        return if zone.row == 0 {
            "Top half"
        } else {
            "Bottom half"
        };
    }
    if zone.h == layout.rows && zone.w == layout.columns / 2 {
        return if zone.col == 0 {
            "Left half"
        } else {
            "Right half"
        };
    }
    if zone.w == 1 && zone.h == 1 {
        return "Grid cell";
    }
    "Drop here"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MonitorRect;

    fn metrics() -> GridMetrics {
        GridMetrics {
            columns: 12,
            rows: 8,
            gutter: 8,
            monitor: MonitorRect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
        }
    }

    #[test]
    fn edge_left_returns_left_half() {
        let layout = GridLayout::default();
        let zone = drop_target_for_tile("clock", 10, 540, &layout, &metrics());
        assert_eq!(zone, TileRect::new(0, 0, 6, 8));
    }

    #[test]
    fn edge_right_returns_right_half() {
        let layout = GridLayout::default();
        let zone = drop_target_for_tile("clock", 1910, 540, &layout, &metrics());
        assert_eq!(zone, TileRect::new(6, 0, 6, 8));
    }
}
