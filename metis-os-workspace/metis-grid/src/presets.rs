use serde::{Deserialize, Serialize};

use super::{GridLayout, ReflowError, TileRect};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TilePreset {
    HalfLeft,
    HalfRight,
    HalfTop,
    HalfBottom,
    Quarter,
    Full,
}

impl TilePreset {
    pub fn label(self) -> &'static str {
        match self {
            Self::HalfLeft => "Half screen · left",
            Self::HalfRight => "Half screen · right",
            Self::HalfTop => "Half screen · top",
            Self::HalfBottom => "Half screen · bottom",
            Self::Quarter => "Quarter screen",
            Self::Full => "Full screen",
        }
    }

    pub fn rect(self, columns: u32, rows: u32, anchor_col: u32, anchor_row: u32) -> TileRect {
        let half_w = columns / 2;
        let half_h = rows / 2;
        match self {
            Self::HalfLeft => TileRect::new(0, 0, half_w.max(1), rows),
            Self::HalfRight => TileRect::new(half_w, 0, columns - half_w, rows),
            Self::HalfTop => TileRect::new(0, 0, columns, half_h.max(1)),
            Self::HalfBottom => TileRect::new(0, half_h, columns, rows - half_h),
            Self::Quarter => TileRect::new(
                anchor_col.min(columns.saturating_sub(half_w)),
                anchor_row.min(rows.saturating_sub(half_h)),
                half_w.max(1),
                half_h.max(1),
            ),
            Self::Full => TileRect::new(0, 0, columns, rows),
        }
    }
}

pub fn apply_preset(
    layout: &mut GridLayout,
    id: &str,
    preset: TilePreset,
) -> Result<(), ReflowError> {
    let (anchor_col, anchor_row) = layout
        .tiles
        .iter()
        .find(|t| t.id == id)
        .map(|t| (t.rect.col, t.rect.row))
        .ok_or_else(|| ReflowError::NotFound(id.to_string()))?;

    let target = preset.rect(layout.columns, layout.rows, anchor_col, anchor_row);
    layout.resize_and_move(id, target)
}

pub fn remove_tile(layout: &mut GridLayout, id: &str) -> bool {
    let before = layout.tiles.len();
    layout.tiles.retain(|t| t.id != id);
    layout.tiles.len() < before
}
