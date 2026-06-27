//! Automatic app tiling — split the workspace among open grid-managed windows.

use crate::layout_engine::move_item;
use crate::model::{GridLayout, ReflowError, TileKind, TileRect};

/// Re-tile the given app tiles into the region below desk widgets.
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

    let region = app_tiling_region(layout);
    let mut assignments = split_region(region, &order);
    // Smallest slots first so overlapping starting rects get pushed aside cleanly.
    assignments.sort_by(|(_, a), (_, b)| (a.w * a.h).cmp(&(b.w * b.h)));

    for (id, target) in assignments {
        move_item(layout, &id, target)?;
    }
    Ok(())
}

/// Rows at and below the lowest desk-widget edge, full width.
fn app_tiling_region(layout: &GridLayout) -> TileRect {
    let widget_bottom = layout
        .tiles
        .iter()
        .filter(|t| matches!(t.kind, TileKind::Widget { .. }))
        .map(|t| t.rect.bottom())
        .max()
        .unwrap_or(0);

    let start_row = if widget_bottom >= layout.rows {
        0
    } else {
        widget_bottom
    };
    let h = layout.rows.saturating_sub(start_row).max(1);
    TileRect::new(0, start_row, layout.columns, h)
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

    fn widget(id: &str, rect: TileRect) -> GridTile {
        GridTile {
            id: id.into(),
            rect,
            kind: TileKind::Widget {
                module: id.into(),
            },
            glow: "cool".into(),
            pinned: false,
            min_w: None,
            max_w: None,
            min_h: None,
            max_h: None,
        }
    }

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
    fn two_apps_split_below_widgets() {
        let mut layout = GridLayout {
            columns: 12,
            rows: 8,
            tiles: vec![
                widget("clock", TileRect::new(0, 0, 3, 2)),
                widget("weather", TileRect::new(3, 0, 3, 2)),
                app("app-1"),
                app("app-2"),
            ],
        };
        auto_tile_apps(
            &mut layout,
            None,
            &["app-1".into(), "app-2".into()],
        )
        .expect("auto tile");

        let a = layout.tiles.iter().find(|t| t.id == "app-1").unwrap();
        let b = layout.tiles.iter().find(|t| t.id == "app-2").unwrap();
        assert_eq!(a.rect.row, 2);
        assert_eq!(b.rect.row, 2);
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
}
