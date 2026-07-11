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
    /// List DRM video modes for one output (resolution + refresh). Returns the
    /// current mode and every mode the connector advertises.
    ListOutputModes { output: String },
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
    /// Re-read `keybinds.json` and apply desktop shortcut bindings live.
    ReloadKeybinds,
    /// While Settings is capturing a new shortcut, suppress global keybind dispatch
    /// so Super+L etc. do not fire mid-edit.
    SetKeybindCapture { active: bool },
    /// Re-read `outputs.json` and apply per-output scale (and related prefs) live.
    ReloadOutputs,
    /// Re-read `power.json` and apply idle preferences live (currently the screen
    /// blank timeout that drives the compositor's idle blanker).
    ReloadPower,
    /// Lock the session now: the compositor enters its locked mode (renders the
    /// lock screen, captures all input, hides clients) until the user
    /// authenticates.
    LockSession,
    /// Re-read `lock.json` and re-decode the lock-screen background live.
    ReloadLock,
    /// Re-read `gaming.json` and apply graphics/offload preferences live.
    ReloadGaming,
    SubscribeEvents,
    /// Set the Wayland clipboard from the shell (text or image file on disk).
    SetClipboard {
        mime: String,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        image_path: Option<String>,
    },
    /// Suppress idle blanking/suspend while an external client (a D-Bus
    /// `org.freedesktop.ScreenSaver` / portal `Inhibit` caller such as a video
    /// player or game) holds an inhibitor. `cookie` is the opaque handle the
    /// inhibit service handed back to the caller; the same cookie releases it via
    /// [`CompositorCommand::UninhibitIdle`]. Wayland `zwp_idle_inhibit` surfaces
    /// are tracked separately inside the compositor.
    InhibitIdle {
        cookie: u32,
        #[serde(default)]
        app_name: Option<String>,
        #[serde(default)]
        reason: Option<String>,
    },
    /// Release an idle inhibitor previously taken via
    /// [`CompositorCommand::InhibitIdle`]. Unknown cookies are ignored.
    UninhibitIdle { cookie: u32 },
    /// Elevate capture UI from a portal screenshot/screencast request.
    BeginCaptureOverlay {
        #[serde(default)]
        app_id: Option<String>,
    },
    /// Tear down portal-driven capture overlay elevation.
    EndCaptureOverlay {
        #[serde(default)]
        app_id: Option<String>,
    },
    /// Native Metis screenshot UI is active (shell layer namespace `metis-screenshot`).
    BeginScreenshotOverlay,
    /// Tear down native screenshot overlay tracking.
    EndScreenshotOverlay,
    /// Inject remote-desktop pointer motion (absolute desktop coordinates).
    InjectRemotePointerAbsolute { x: f64, y: f64 },
    /// Inject remote-desktop pointer motion (relative delta in logical pixels).
    InjectRemotePointerRelative { dx: f64, dy: f64 },
    /// Inject remote-desktop pointer button (Linux evdev button code).
    InjectRemotePointerButton { button: u32, pressed: bool },
    /// Inject remote-desktop scroll delta (logical pixels).
    InjectRemotePointerScroll { dx: f64, dy: f64 },
    /// Inject remote-desktop keyboard key (evdev keycode, 8 = ESC).
    InjectRemoteKey { keycode: u32, pressed: bool },
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
    /// Reply to `ListOutputModes`: advertised modes for one output.
    OutputModes {
        modes: Vec<OutputModeInfo>,
        current: Option<OutputModeInfo>,
    },
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
    /// True fullscreen on `output` — shell hides the edge bar until `visible` is true.
    EdgeBarVisible {
        output: String,
        visible: bool,
    },
    WindowFullscreen {
        id: u32,
        fullscreen: bool,
        #[serde(default)]
        output: String,
    },
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
    /// Game or launcher session started/ended (Phase 11 gaming daemon).
    GameSession {
        active: bool,
        #[serde(default)]
        label: Option<String>,
        #[serde(default)]
        pid: Option<u32>,
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

/// A video mode (resolution + refresh) for one output.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OutputModeInfo {
    pub width: i32,
    pub height: i32,
    /// Refresh rate in millihertz (60_000 = 60 Hz), matching Smithay `output::Mode`.
    pub refresh_millihz: i32,
    #[serde(default)]
    pub preferred: bool,
}

/// A connected output, as reported to the settings app for per-display options.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OutputInfo {
    /// Compositor output name (e.g. `metis-0`). This is the key used in
    /// `wallpaper.json`'s `per_output` map and `outputs.json`.
    pub name: String,
    /// Whether this is the primary (first) output.
    #[serde(default)]
    pub primary: bool,
    /// Output position and size in global logical pixels.
    pub rect: MonitorRect,
    /// Current fractional scale (1.0 = 100%).
    #[serde(default = "default_output_scale")]
    pub scale: f64,
    /// Whether this output is currently enabled (mapped and visible to clients).
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// EDID make when known (may be empty under nested winit).
    #[serde(default)]
    pub make: String,
    /// EDID model when known.
    #[serde(default)]
    pub model: String,
    /// This output clones another when mirror mode is active.
    #[serde(default)]
    pub mirrored: bool,
    /// This output is the mirror source (duplicate mode).
    #[serde(default)]
    pub mirror_source: bool,
    /// DRM driver advertises VRR / adaptive sync on this connector.
    #[serde(default)]
    pub vrr_available: bool,
    /// VRR is currently active on the CRTC (may differ from saved pref until apply).
    #[serde(default)]
    pub vrr_active: bool,
}

fn default_output_scale() -> f64 {
    1.0
}

fn default_true() -> bool {
    true
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

/// Send one JSON command to the compositor IPC socket and read the reply line.
pub fn send_compositor_command(cmd: &CompositorCommand) -> std::io::Result<CompositorEvent> {
    use std::io::{BufRead, BufReader, Write};

    let path = ipc_socket_path();
    let mut stream = std::os::unix::net::UnixStream::connect(&path).map_err(|e| {
        std::io::Error::new(
            e.kind(),
            format!("Metis compositor not running at {}: {e}", path.display()),
        )
    })?;
    stream.set_read_timeout(Some(std::time::Duration::from_millis(400)))?;
    let payload = serde_json::to_string(cmd).map_err(std::io::Error::other)?;
    writeln!(stream, "{payload}")?;
    stream.flush()?;
    let mut reader = BufReader::new(stream);
    let mut response = String::new();
    reader.read_line(&mut response)?;
    let line = response.trim();
    if line.is_empty() {
        return Err(std::io::Error::other("empty compositor response"));
    }
    serde_json::from_str(line).map_err(|e| std::io::Error::other(e.to_string()))
}
