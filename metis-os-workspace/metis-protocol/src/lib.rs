pub use metis_grid::{GridLayout, GridMetrics, LayoutKind, MonitorRect, PixelRect};

/// Commands sent from the Metis shell to the compositor.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum CompositorCommand {
    Ping,
    GetMonitor,
    /// List the connected outputs (name + geometry), so the settings app can offer
    /// per-display options (e.g. per-output wallpaper).
    ListOutputs,
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
    /// Switch the active virtual workspace (1-based) on a specific output. Out-of-range
    /// ids are clamped to the configured workspace count. `output` is an output name
    /// (as reported by `ListOutputs`); `None`/empty targets the output under the
    /// pointer. Each output owns an independent set of workspaces.
    SwitchWorkspace {
        #[serde(default)]
        output: Option<String>,
        id: u32,
    },
    /// Move a window to another virtual workspace (1-based). If the target is not
    /// the active workspace the window is hidden until that workspace is shown.
    MoveWindowToWorkspace { window_id: u32, workspace: u32 },
    /// Move a window to another output (monitor). Keeps its workspace number on
    /// the destination output. `output` is an output name from `ListOutputs`;
    /// `None`/empty targets the output under the pointer.
    MoveWindowToOutput {
        window_id: u32,
        #[serde(default)]
        output: Option<String>,
    },
    /// Move every window on a workspace to another output (same workspace number).
    /// `output`/`workspace` default to the output under the pointer and its active
    /// workspace. Requires independent per-output workspace mode.
    MoveWorkspaceToOutput {
        #[serde(default)]
        output: Option<String>,
        #[serde(default)]
        workspace: Option<u32>,
        target_output: String,
    },
    /// Set the layout mode (grid vs. scrolling) of a workspace. `output` is an
    /// output name (`None`/empty targets the output under the pointer); `workspace`
    /// `None` targets that output's currently-active workspace.
    SetWorkspaceLayout {
        #[serde(default)]
        output: Option<String>,
        #[serde(default)]
        workspace: Option<u32>,
        kind: LayoutKind,
    },
    /// Apply a layout mode to every workspace on every output at once (used when
    /// the settings "New workspace layout" default changes, so it acts as a live
    /// global on/off rather than only seeding future workspaces).
    SetDefaultLayout { kind: LayoutKind },
    Launch { program: String },
    /// End the Metis session: stop the compositor event loop so the session host
    /// (run script / display manager) tears the session down cleanly. Used by the
    /// app menu's "Log Out" action.
    EndSession,
    /// Re-read `wallpaper.json` and apply the desktop background live (picture,
    /// solid colour, or gradient).
    ApplyBackground,
    /// Re-read `input.json` and apply mouse/touchpad/keyboard settings live.
    ReloadInput,
    SubscribeEvents,
    /// Set the Wayland clipboard from the shell (text or image file on disk).
    SetClipboard {
        mime: String,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        image_path: Option<String>,
    },
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
    /// Reply to `ListOutputs`: every connected output, primary first.
    OutputList { outputs: Vec<OutputInfo> },
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
    /// The active virtual workspace changed (1-based) on `output`, with the current
    /// total count. Each output reports its own active workspace independently.
    WorkspaceChanged {
        #[serde(default)]
        output: String,
        active: u32,
        count: u32,
    },
    Error { message: String },
    /// Clipboard contents changed (text preview and/or image path under runtime dir).
    ClipboardChanged {
        mime: String,
        #[serde(default)]
        preview_text: Option<String>,
        #[serde(default)]
        image_path: Option<String>,
    },
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
    /// Name of the output (monitor) the window is currently on (e.g. `metis-0`).
    /// Empty when not yet known (an event-folded entry before the next reconcile).
    #[serde(default)]
    pub output: String,
    /// Virtual workspace the window belongs to (1-based).
    #[serde(default)]
    pub workspace: u32,
}

/// A connected output, as reported to the settings app for per-display options.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OutputInfo {
    /// Compositor output name (e.g. `metis-0`). This is the key used in
    /// `wallpaper.json`'s `per_output` map.
    pub name: String,
    /// Whether this is the primary (first) output.
    #[serde(default)]
    pub primary: bool,
    /// Output position and size in global logical pixels.
    pub rect: MonitorRect,
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

pub fn runtime_dir() -> std::path::PathBuf {
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
