use serde::{Deserialize, Serialize};

use crate::{GridLayout, TileRect};

/// Temporary display mode for a tile — does not mutate the saved grid slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TileMode {
    Grid,
    Immersive,
    AppFullscreen,
    Minimized,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TileModeSnapshot {
    pub tile_id: String,
    pub saved_rect: TileRect,
    pub mode: TileMode,
}

#[derive(Debug, Clone, Default)]
pub struct TileModeState {
    snapshots: Vec<TileModeSnapshot>,
}

impl TileModeState {
    pub fn snapshot_for(&self, tile_id: &str) -> Option<&TileModeSnapshot> {
        self.snapshots.iter().find(|s| s.tile_id == tile_id)
    }

    pub fn enter(&mut self, layout: &GridLayout, tile_id: &str, mode: TileMode) -> Option<TileRect> {
        let tile = layout.tiles.iter().find(|t| t.id == tile_id)?;
        let saved_rect = tile.rect;
        if !self.snapshots.iter().any(|s| s.tile_id == tile_id) {
            self.snapshots.push(TileModeSnapshot {
                tile_id: tile_id.to_string(),
                saved_rect,
                mode,
            });
        } else if let Some(entry) = self.snapshots.iter_mut().find(|s| s.tile_id == tile_id) {
            entry.mode = mode;
        }
        Some(saved_rect)
    }

    pub fn exit(&mut self, tile_id: &str) -> Option<TileRect> {
        let idx = self.snapshots.iter().position(|s| s.tile_id == tile_id)?;
        let snapshot = self.snapshots.remove(idx);
        Some(snapshot.saved_rect)
    }
}
