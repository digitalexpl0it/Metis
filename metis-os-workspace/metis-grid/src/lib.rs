mod auto_tile;
mod immersive;
mod layout;
mod layout_engine;
mod model;
mod presets;
mod scroll;
mod snap_zones;
mod tile_mode;

pub use auto_tile::auto_tile_apps;
pub use immersive::{ImmersiveController, ImmersiveSnapshot};
pub use layout::{
    app_tile_body_rect, app_tile_border_px, app_tile_chrome_rect, cell_to_pixels, desk_pixel_rect,
    ease_out_cubic, pixel_to_grid_cell, set_app_tile_border_px, GridMetrics, MonitorRect, PixelRect,
    APP_TILE_BORDER_PX, APP_TILE_HEADER_PX, MAX_APP_TILE_BORDER_PX,
};
pub use layout_engine::{
    move_item, preview_move, repair_layout, resize_item, sanitize_layout, validate_layout,
    CompactType, EngineConfig,
};
pub use model::{GridLayout, GridTile, LayoutKind, ReflowError, TileKind, TileRect};
pub use presets::{apply_preset, remove_tile, TilePreset};
pub use scroll::{ColumnWidth, ScrollColumn, ScrollState};
pub use snap_zones::{
    drop_target_for_tile, monitor_point_from_grid_local, pixel_snap_target, snap_label,
    snap_target_at_point,
};
pub use tile_mode::{TileMode, TileModeSnapshot, TileModeState};
