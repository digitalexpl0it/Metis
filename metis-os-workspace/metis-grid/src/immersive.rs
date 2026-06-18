use crate::GridLayout;

#[derive(Debug, Clone)]
pub struct ImmersiveSnapshot {
    pub layout: GridLayout,
    pub tile_id: String,
}

pub struct ImmersiveController {
    snapshot: Option<ImmersiveSnapshot>,
}

impl ImmersiveController {
    pub fn new() -> Self {
        Self { snapshot: None }
    }

    pub fn is_active(&self) -> bool {
        self.snapshot.is_some()
    }

    pub fn enter(&mut self, layout: &GridLayout, tile_id: impl Into<String>) {
        self.snapshot = Some(ImmersiveSnapshot {
            layout: layout.clone(),
            tile_id: tile_id.into(),
        });
    }

    pub fn exit(&mut self) -> Option<ImmersiveSnapshot> {
        self.snapshot.take()
    }
}

impl Default for ImmersiveController {
    fn default() -> Self {
        Self::new()
    }
}
