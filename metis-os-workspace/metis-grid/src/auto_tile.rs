//! Automatic app tiling — split the workspace among open grid-managed windows.

use crate::model::{GridLayout, ReflowError, TileKind, TileRect};

/// Re-tile the given app tiles across the full workspace grid.
///
/// Only tile ids listed in `include_ids` are repositioned; pinned apps and apps
/// omitted from the list keep their current rects. The focused tile (if any) is
/// placed in the primary (largest) slot when three or more windows share the area.
pub fn auto_tile_apps(
    layout: &mut GridLayout,
    focus_id: Option<&str>,
    include_ids: &[String],
) -> Result<(), ReflowError> {
    if include_ids.is_empty() {
        return Ok(());
    }

    let mut order: Vec<String> = include_ids
        .iter()
        .filter(|id| {
            layout
                .tiles
                .iter()
                .any(|t| &t.id == *id && matches!(t.kind, TileKind::App { .. }) && !t.pinned)
        })
        .cloned()
        .collect();

    if order.is_empty() {
        return Ok(());
    }

    if let Some(focus) = focus_id {
        order.sort_by(|a, b| {
            if a == focus {
                std::cmp::Ordering::Less
            } else if b == focus {
                std::cmp::Ordering::Greater
            } else {
                a.cmp(b)
            }
        });
    } else {
        order.sort();
    }

    let region = layout.app_tiling_region();
    if region.h == 0 {
        return Err(ReflowError::NoSpace {
            col: region.col,
            row: region.row,
        });
    }

    let assignments = split_region(region, &order);
    for (_, target) in &assignments {
        if !region_contains(region, *target) {
            return Err(ReflowError::NoSpace {
                col: target.col,
                row: target.row,
            });
        }
    }

    for (id, target) in assignments {
        if let Some(tile) = layout.tile_mut(&id) {
            tile.rect = target;
        }
    }
    Ok(())
}

fn region_contains(outer: TileRect, inner: TileRect) -> bool {
    inner.col >= outer.col
        && inner.row >= outer.row
        && inner.right() <= outer.right()
        && inner.bottom() <= outer.bottom()
}

fn split_region(region: TileRect, order: &[String]) -> Vec<(String, TileRect)> {
    let n = order.len();
    match n {
        0 => Vec::new(),
        1 => vec![(order[0].clone(), region)],
        2 => {
            let half = (region.w / 2).max(1);
            vec![
                (
                    order[0].clone(),
                    TileRect::new(region.col, region.row, half, region.h),
                ),
                (
                    order[1].clone(),
                    TileRect::new(region.col + half, region.row, region.w - half, region.h),
                ),
            ]
        }
        3 => {
            let half_w = (region.w / 2).max(1);
            let right_w = region.w - half_w;
            let half_h = (region.h / 2).max(1);
            vec![
                (
                    order[0].clone(),
                    TileRect::new(region.col, region.row, half_w, region.h),
                ),
                (
                    order[1].clone(),
                    TileRect::new(region.col + half_w, region.row, right_w, half_h),
                ),
                (
                    order[2].clone(),
                    TileRect::new(
                        region.col + half_w,
                        region.row + half_h,
                        right_w,
                        region.h - half_h,
                    ),
                ),
            ]
        }
        _ => {
            let cols = (n as f64).sqrt().ceil() as u32;
            let cols = cols.max(1);
            let rows = (n as u32).div_ceil(cols).max(1);
            let cell_w = (region.w / cols).max(1);
            let cell_h = (region.h / rows).max(1);
            order
                .iter()
                .enumerate()
                .map(|(i, id)| {
                    let c = (i as u32) % cols;
                    let r = i as u32 / cols;
                    let w = if c + 1 == cols {
                        region.w.saturating_sub(cell_w * (cols - 1))
                    } else {
                        cell_w
                    };
                    let h = if r + 1 == rows {
                        region.h.saturating_sub(cell_h * (rows - 1))
                    } else {
                        cell_h
                    };
                    (
                        id.clone(),
                        TileRect::new(
                            region.col + c * cell_w,
                            region.row + r * cell_h,
                            w.max(1),
                            h.max(1),
                        ),
                    )
                })
                .collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::GridTile;

    fn app(id: &str) -> GridTile {
        GridTile {
            id: id.into(),
            rect: TileRect::new(0, 4, 12, 4),
            kind: TileKind::App {
                window_id: Some(1),
                class: Some("foot".into()),
            },
            glow: "cool".into(),
            pinned: false,
            min_w: None,
            max_w: None,
            min_h: None,
            max_h: None,
        }
    }

    fn no_overlap(layout: &GridLayout) {
        for (i, a) in layout.tiles.iter().enumerate() {
            for b in layout.tiles.iter().skip(i + 1) {
                assert!(
                    !a.rect.intersects(&b.rect),
                    "{} ({:?}) overlaps {} ({:?})",
                    a.id,
                    a.rect,
                    b.id,
                    b.rect
                );
            }
        }
    }

    #[test]
    fn two_apps_split_full_grid() {
        let mut layout = GridLayout {
            columns: 12,
            rows: 8,
            tiles: vec![app("app-1"), app("app-2")],
        };
        auto_tile_apps(
            &mut layout,
            None,
            &["app-1".into(), "app-2".into()],
        )
        .expect("auto tile");

        let a = layout.tiles.iter().find(|t| t.id == "app-1").unwrap();
        let b = layout.tiles.iter().find(|t| t.id == "app-2").unwrap();
        assert_eq!(a.rect.row, 0);
        assert_eq!(b.rect.row, 0);
        assert_eq!(a.rect.h, 8);
        assert_eq!(a.rect.w + b.rect.w, 12);
        no_overlap(&layout);
    }

    #[test]
    fn three_apps_master_stack() {
        let mut layout = GridLayout {
            columns: 12,
            rows: 8,
            tiles: vec![app("app-1"), app("app-2"), app("app-3")],
        };
        auto_tile_apps(
            &mut layout,
            Some("app-2"),
            &["app-1".into(), "app-2".into(), "app-3".into()],
        )
        .expect("auto tile");

        let master = layout.tiles.iter().find(|t| t.id == "app-2").unwrap();
        assert_eq!(master.rect, TileRect::new(0, 0, 6, 8));
        no_overlap(&layout);
    }

    #[test]
    fn legacy_widget_tiles_stripped_on_sanitize() {
        let raw = r#"{
            "columns": 12,
            "rows": 8,
            "tiles": [
                {"id": "clock", "rect": {"col": 0, "row": 6, "w": 3, "h": 2}, "kind": {"type": "widget", "module": "clock"}, "glow": "cool"},
                {"id": "weather", "rect": {"col": 3, "row": 6, "w": 3, "h": 2}, "kind": {"type": "widget", "module": "weather"}, "glow": "warm"},
                {"id": "rss", "rect": {"col": 6, "row": 6, "w": 6, "h": 2}, "kind": {"type": "widget", "module": "rss"}, "glow": "violet"},
                {"id": "settings", "rect": {"col": 10, "row": 2, "w": 2, "h": 2}, "kind": {"type": "widget", "module": "settings"}, "glow": "cool"},
                {"id": "app-8", "rect": {"col": 2, "row": 1, "w": 8, "h": 5}, "kind": {"type": "app", "window_id": 8, "class": "kitty"}, "glow": "cool"}
            ]
        }"#;
        let mut layout: GridLayout = serde_json::from_str(raw).expect("parse desk.json fixture");
        crate::layout_engine::sanitize_layout(&mut layout);
        assert!(
            !layout.tiles.iter().any(|t| matches!(t.kind, TileKind::Widget { .. })),
            "legacy widget tiles should be stripped"
        );
        auto_tile_apps(&mut layout, None, &["app-8".into()]).expect("auto tile after sanitize");
        let app = layout.tiles.iter().find(|t| t.id == "app-8").unwrap();
        assert_eq!(app.rect, TileRect::new(0, 0, 12, 8));
        no_overlap(&layout);
    }
}
