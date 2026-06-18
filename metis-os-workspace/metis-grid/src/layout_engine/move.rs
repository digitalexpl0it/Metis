use super::collision::{get_all_collisions, get_first_collision};
use super::sort::sort_layout_items;
use super::{CompactType, EngineConfig, LayoutItem};

const MAX_RECURSE: u32 = 64;

pub fn correct_bounds(items: &mut [LayoutItem], cols: u32, rows: u32) {
    for item in items.iter_mut() {
        if item.right() > cols {
            item.col = cols.saturating_sub(item.w);
        }
        if item.bottom() > rows {
            item.row = rows.saturating_sub(item.h);
        }
    }

    let pinned: Vec<LayoutItem> = items.iter().filter(|i| i.pinned).cloned().collect();
    for item in items.iter_mut().filter(|i| i.pinned) {
        let mut collides_with = pinned.clone();
        while get_first_collision(&collides_with, item).is_some() {
            item.row = item.row.saturating_add(1);
            if item.bottom() > rows {
                item.row = rows.saturating_sub(item.h);
                break;
            }
        }
        collides_with.push(item.clone());
    }
}

pub fn move_element(
    layout: &mut [LayoutItem],
    id: &str,
    col: Option<u32>,
    row: Option<u32>,
    is_user_action: bool,
    config: &EngineConfig,
    depth: u32,
) -> bool {
    if depth > MAX_RECURSE {
        return false;
    }

    let Some(idx) = layout.iter().position(|i| i.id == id) else {
        return false;
    };

    if layout[idx].pinned {
        return false;
    }

    let old_col = layout[idx].col;
    let old_row = layout[idx].row;
    let new_col = col.unwrap_or(old_col);
    let new_row = row.unwrap_or(old_row);

    if old_col == new_col && old_row == new_row {
        return true;
    }

    layout[idx].col = new_col;
    layout[idx].row = new_row;
    layout[idx].moved = true;

    let moving = layout[idx].clone();
    let sorted = sort_layout_items(layout, config.compact_type);
    let collisions: Vec<LayoutItem> = get_all_collisions(&sorted, &moving)
        .into_iter()
        .cloned()
        .collect();

    if collisions.is_empty() {
        correct_bounds(layout, config.cols, config.rows);
        return layout_fits(layout, config.cols, config.rows);
    }

    if config.allow_overlap {
        correct_bounds(layout, config.cols, config.rows);
        return true;
    }

    if config.prevent_collision {
        layout[idx].col = old_col;
        layout[idx].row = old_row;
        layout[idx].moved = false;
        return false;
    }

    for collision in collisions {
        if collision.moved {
            continue;
        }
        let ok = if collision.pinned {
            move_element_away_from_collision(
                layout,
                &collision,
                &moving,
                is_user_action,
                config,
                depth + 1,
            )
        } else {
            move_element_away_from_collision(
                layout,
                &moving,
                &collision,
                is_user_action,
                config,
                depth + 1,
            )
        };
        if !ok {
            layout[idx].col = old_col;
            layout[idx].row = old_row;
            layout[idx].moved = false;
            return false;
        }
    }

    correct_bounds(layout, config.cols, config.rows);
    layout_fits(layout, config.cols, config.rows)
}

fn move_element_away_from_collision(
    layout: &mut [LayoutItem],
    item_to_move: &LayoutItem,
    collides_with: &LayoutItem,
    is_user_action: bool,
    config: &EngineConfig,
    depth: u32,
) -> bool {
    let compact_h = config.compact_type == CompactType::Horizontal;
    let compact_v = config.compact_type == CompactType::Vertical;

    if is_user_action {
        let fake = LayoutItem {
            id: "_fake".into(),
            col: if compact_h {
                collides_with.col.saturating_sub(item_to_move.w)
            } else {
                item_to_move.col
            },
            row: if compact_v {
                collides_with.row.saturating_sub(item_to_move.h)
            } else {
                item_to_move.row
            },
            w: item_to_move.w,
            h: item_to_move.h,
            pinned: false,
            moved: false,
        };

        let first = get_first_collision(layout, &fake);
        if first.is_none() {
            return move_element(
                layout,
                &item_to_move.id,
                if compact_h { Some(fake.col) } else { None },
                if compact_v { Some(fake.row) } else { None },
                false,
                config,
                depth,
            );
        }

        if config.compact_type == CompactType::Null && !collides_with.pinned {
            let Some(other_idx) = layout.iter().position(|i| i.id == collides_with.id) else {
                return false;
            };
            let Some(mover_idx) = layout.iter().position(|i| i.id == item_to_move.id) else {
                return false;
            };
            let swap_row = layout[mover_idx].row;
            layout[other_idx].row = swap_row;
            layout[mover_idx].row = swap_row.saturating_add(item_to_move.h);
            return layout_fits(layout, config.cols, config.rows);
        }
    }

    move_element(
        layout,
        &item_to_move.id,
        if compact_h {
            Some(item_to_move.col.saturating_add(1))
        } else {
            None
        },
        if compact_v || config.compact_type == CompactType::Null {
            Some(item_to_move.row.saturating_add(1))
        } else {
            None
        },
        false,
        config,
        depth,
    )
}

pub fn layout_fits(layout: &[LayoutItem], cols: u32, rows: u32) -> bool {
    use super::collision::collides;
    for item in layout {
        if item.right() > cols || item.bottom() > rows {
            return false;
        }
    }
    for (i, a) in layout.iter().enumerate() {
        for b in layout.iter().skip(i + 1) {
            if collides(a, b) {
                return false;
            }
        }
    }
    true
}
