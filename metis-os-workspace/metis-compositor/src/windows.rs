use std::collections::HashMap;

use metis_grid::PixelRect;
use metis_protocol::WindowInfo;
use smithay::reexports::wayland_server::backend::ObjectId;
use smithay::reexports::wayland_server::Resource;
use smithay::{
    desktop::Window,
    wayland::shell::xdg::ToplevelSurface,
};

#[derive(Clone)]
pub struct WindowRecord {
    pub id: u32,
    pub window: Window,
    pub toplevel: ToplevelSurface,
    pub title: String,
    pub app_id: Option<String>,
    pub target_rect: PixelRect,
    pub restore_rect: Option<PixelRect>,
    /// Pre-snap floating geometry is in `restore_rect`; this flag is set while the
    /// window occupies a snap/maximize layout so a titlebar drag can restore size.
    pub snapped: bool,
    pub fullscreen: bool,
    pub maximized: bool,
    pub minimized: bool,
    /// Virtual workspace this window lives on (1-based), scoped to its `output`.
    /// A window is mapped into `Space` only while its output's active workspace
    /// equals this value.
    pub workspace: u32,
    /// Name of the output (monitor) this window belongs to. Empty until assigned
    /// at registration. Per-output workspaces key off `(output, workspace)`.
    pub output: String,
    /// True after the first buffer commit; probe toplevels that never commit are dropped quietly.
    pub ready: bool,
}

pub struct WindowRegistry {
    next_id: u32,
    by_id: HashMap<u32, WindowRecord>,
    // Keyed on the surface's globally-unique `ObjectId` — NOT `protocol_id()`, which
    // is only unique per client connection, so surfaces from two different clients
    // (shell, terminal, settings, …) can share a number and clobber each other.
    surface_to_id: HashMap<ObjectId, u32>,
}

impl WindowRegistry {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            by_id: HashMap::new(),
            surface_to_id: HashMap::new(),
        }
    }

    pub fn register(&mut self, window: Window, title: String, app_id: Option<String>) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        let toplevel = window.toplevel().unwrap().clone();
        let surface_id = toplevel.wl_surface().id();
        let rect = PixelRect {
            x: 0,
            y: 0,
            width: 800,
            height: 600,
        };
        self.surface_to_id.insert(surface_id, id);
        self.by_id.insert(
            id,
            WindowRecord {
                id,
                window,
                toplevel,
                title,
                app_id,
                target_rect: rect,
                restore_rect: None,
                snapped: false,
                fullscreen: false,
                maximized: false,
                minimized: false,
                workspace: 1,
                output: String::new(),
                ready: false,
            },
        );
        id
    }

    pub fn set_ready(&mut self, id: u32, ready: bool) {
        if let Some(record) = self.by_id.get_mut(&id) {
            record.ready = ready;
        }
    }

    pub fn is_ready(&self, id: u32) -> bool {
        self.by_id.get(&id).is_some_and(|r| r.ready)
    }

    pub fn set_metadata(&mut self, id: u32, title: String, app_id: Option<String>) {
        if let Some(record) = self.by_id.get_mut(&id) {
            record.title = title;
            record.app_id = app_id;
        }
    }

    pub fn get(&self, id: u32) -> Option<&WindowRecord> {
        self.by_id.get(&id)
    }

    pub fn ids(&self) -> Vec<u32> {
        self.by_id.keys().copied().collect()
    }

    pub fn id_for_surface(
        &self,
        surface: &smithay::reexports::wayland_server::protocol::wl_surface::WlSurface,
    ) -> Option<u32> {
        self.surface_to_id.get(&surface.id()).copied()
    }

    pub fn id_for_window(&self, window: &Window) -> Option<u32> {
        window
            .toplevel()
            .and_then(|t| self.id_for_surface(t.wl_surface()))
    }

    pub fn set_target_rect(&mut self, id: u32, rect: PixelRect) {
        if let Some(record) = self.by_id.get_mut(&id) {
            record.target_rect = rect;
        }
    }

    pub fn target_rect(&self, id: u32) -> Option<PixelRect> {
        self.by_id.get(&id).map(|r| r.target_rect)
    }

    pub fn set_rect(&mut self, id: u32, rect: PixelRect) {
        if let Some(record) = self.by_id.get_mut(&id) {
            record.target_rect = rect;
        }
    }

    pub fn set_fullscreen(&mut self, id: u32, enabled: bool) {
        if let Some(record) = self.by_id.get_mut(&id) {
            record.fullscreen = enabled;
            if enabled {
                record.maximized = false;
            }
        }
    }

    pub fn set_maximized(&mut self, id: u32, enabled: bool) {
        if let Some(record) = self.by_id.get_mut(&id) {
            record.maximized = enabled;
            if enabled {
                record.fullscreen = false;
            }
        }
    }

    pub fn set_minimized(&mut self, id: u32, enabled: bool) {
        if let Some(record) = self.by_id.get_mut(&id) {
            record.minimized = enabled;
        }
    }

    pub fn set_restore_rect(&mut self, id: u32, rect: PixelRect) {
        if let Some(record) = self.by_id.get_mut(&id) {
            record.restore_rect = Some(rect);
        }
    }

    pub fn take_restore_rect(&mut self, id: u32) -> Option<PixelRect> {
        self.by_id.get_mut(&id).and_then(|r| r.restore_rect.take())
    }

    pub fn set_snapped(&mut self, id: u32, snapped: bool) {
        if let Some(record) = self.by_id.get_mut(&id) {
            record.snapped = snapped;
        }
    }

    pub fn is_snapped(&self, id: u32) -> bool {
        self.by_id.get(&id).is_some_and(|r| r.snapped)
    }

    pub fn is_minimized(&self, id: u32) -> bool {
        self.by_id.get(&id).is_some_and(|r| r.minimized)
    }

    pub fn set_workspace(&mut self, id: u32, workspace: u32) {
        if let Some(record) = self.by_id.get_mut(&id) {
            record.workspace = workspace;
        }
    }

    pub fn workspace(&self, id: u32) -> Option<u32> {
        self.by_id.get(&id).map(|r| r.workspace)
    }

    pub fn set_output(&mut self, id: u32, output: String) {
        if let Some(record) = self.by_id.get_mut(&id) {
            record.output = output;
        }
    }

    pub fn output_name(&self, id: u32) -> Option<String> {
        self.by_id.get(&id).map(|r| r.output.clone())
    }

    /// Snapshot of ready windows. `focused` is left `false` here (the registry
    /// has no seat access); the caller patches the focused id. `output` is the
    /// name of the monitor the window lives on (per-output workspaces).
    pub fn list(&self) -> Vec<WindowInfo> {
        self.by_id
            .values()
            .filter(|r| r.ready)
            .map(|r| WindowInfo {
                id: r.id,
                title: r.title.clone(),
                app_id: r.app_id.clone(),
                rect: r.target_rect,
                fullscreen: r.fullscreen,
                minimized: r.minimized,
                focused: false,
                output: r.output.clone(),
                workspace: r.workspace,
            })
            .collect()
    }

    pub fn unregister(&mut self, id: u32) -> Option<WindowRecord> {
        let record = self.by_id.remove(&id)?;
        let surface_id = record.toplevel.wl_surface().id();
        self.surface_to_id.remove(&surface_id);
        Some(record)
    }
}
