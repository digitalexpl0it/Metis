use super::{CompactType, LayoutItem};

pub fn sort_layout_items(layout: &[LayoutItem], compact_type: CompactType) -> Vec<LayoutItem> {
    match compact_type {
        CompactType::Horizontal => sort_by_col_row(layout),
        CompactType::Vertical | CompactType::Wrap => sort_by_row_col(layout),
        CompactType::Null => layout.to_vec(),
    }
}

fn sort_by_row_col(layout: &[LayoutItem]) -> Vec<LayoutItem> {
    let mut sorted = layout.to_vec();
    sorted.sort_by(|a, b| a.row.cmp(&b.row).then(a.col.cmp(&b.col)));
    sorted
}

fn sort_by_col_row(layout: &[LayoutItem]) -> Vec<LayoutItem> {
    let mut sorted = layout.to_vec();
    sorted.sort_by(|a, b| a.col.cmp(&b.col).then(a.row.cmp(&b.row)));
    sorted
}
