pub use metis_grid::{GridLayout, GridMetrics, MonitorRect, PixelRect};

/// Commands sent from the Metis shell to the compositor.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum CompositorCommand {
    Ping,
    GetMonitor,
    GetLayout,
    ListWindows,
    MoveWindow { id: u32, rect: PixelRect },
    CloseWindow { id: u32 },
    FocusWindow { id: u32 },
    /// Minimize or restore a window by id (works for grid and floating windows).
    SetMinimized { id: u32, minimized: bool },
    /// Bring a window to the foreground: unminimize (if needed), raise, and focus.
    /// Used by the taskbar to surface a background/minimized app.
    ActivateWindow { id: u32 },
    SetFullscreen { id: u32, enabled: bool },
    ApplyLayout { layout: GridLayout, gutter_px: u32 },
    SetTileMode { tile_id: String, mode: TileMode },
    Launch { program: String },
    /// End the Metis session: stop the compositor event loop so the session host
    /// (run script / display manager) tears the session down cleanly. Used by the
    /// app menu's "Log Out" action.
    EndSession,
    /// Re-read `wallpaper.json` and apply the desktop background live (picture,
    /// solid colour, or gradient).
    ApplyBackground,
    SubscribeEvents,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TileMode {
    Grid,
    Immersive,
    AppFullscreen,
    Minimized,
}

/// Events emitted by the compositor to the shell.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "evt", rename_all = "snake_case")]
pub enum CompositorEvent {
    Pong,
    Monitor { rect: MonitorRect },
    LayoutChanged {
        layout: GridLayout,
        gutter_px: u32,
        metrics: GridMetrics,
    },
    WindowList { windows: Vec<WindowInfo> },
    WindowOpened {
        id: u32,
        title: String,
        app_id: Option<String>,
        suggested_rect: PixelRect,
    },
    WindowClosed { id: u32 },
    WindowFocused { id: u32 },
    WindowMinimized { id: u32, minimized: bool },
    WindowMetadata {
        id: u32,
        title: String,
        app_id: Option<String>,
    },
    LayoutApplied,
    MonitorChanged { rect: MonitorRect },
    Error { message: String },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WindowInfo {
    pub id: u32,
    pub title: String,
    pub app_id: Option<String>,
    pub rect: PixelRect,
    pub fullscreen: bool,
    #[serde(default)]
    pub minimized: bool,
    #[serde(default)]
    pub focused: bool,
    /// Output the window is currently on (0 until multi-output lands in Phase 3).
    #[serde(default)]
    pub output: u32,
    /// Workspace the window belongs to (0 until workspaces land in Phase 3).
    #[serde(default)]
    pub workspace: u32,
}

pub fn ipc_socket_path() -> std::path::PathBuf {
    runtime_dir().join("compositor.sock")
}

pub fn events_socket_path() -> std::path::PathBuf {
    runtime_dir().join("compositor-events.sock")
}

pub fn runtime_command_path() -> std::path::PathBuf {
    runtime_dir().join("command")
}

fn runtime_dir() -> std::path::PathBuf {
    std::env::var("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("/tmp/metis"))
        .join("metis")
}

pub fn write_runtime_command(action: &str) -> std::io::Result<()> {
    let path = runtime_command_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, action)
}
