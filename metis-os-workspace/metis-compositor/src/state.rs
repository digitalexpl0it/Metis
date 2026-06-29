use std::ffi::OsString;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use metis_grid::{cell_to_pixels, app_tile_body_rect, GridLayout, GridMetrics, MonitorRect, PixelRect, TileKind, TileModeState};
use metis_protocol::CompositorCommand;
use smithay::{
    desktop::{PopupManager, Space, Window, layer_map_for_output},
    input::{Seat, SeatState},
    reexports::{
        calloop::{EventLoop, Interest, LoopSignal, Mode, PostAction, generic::Generic},
        wayland_server::{
            Display, DisplayHandle,
            backend::{ClientData, ClientId, DisconnectReason},
        },
    },
    utils::{IsAlive, Logical, Point, Size},
    wayland::{
        compositor::{CompositorClientState, CompositorState},
        output::OutputManagerState,
        selection::{
            data_device::DataDeviceState,
            primary_selection::PrimarySelectionState,
        },
        shell::{
            wlr_layer::WlrLayerShellState,
            xdg::{decoration::XdgDecorationState, XdgShellState},
        },
        shm::ShmState,
        socket::ListeningSocketSource,
        text_input::TextInputManagerState,
    },
};

use crate::events::accept_event_subscribers;
use crate::focus::KeyboardFocusTarget;
use crate::events::EventBus;
use crate::windows::WindowRegistry;

/// Thin padding kept between the edge bar's reserved (exclusive) zone and any
/// window, so windows sit just under the bar without touching its drop shadow.
pub const BAR_GAP_PX: i32 = 2;

/// Hyprland-style uniform gap around a maximized window so it floats inside the
/// usable area (under the bar, inset from the screen edges) instead of butting up
/// against them.
pub const WINDOW_GAP_PX: i32 = 8;

/// Per-edge gaps for placing/snapping windows inside the usable zone. Edges that
/// border the edge bar use `BAR_GAP_PX`; edges that border the bare screen use
/// `WINDOW_GAP_PX`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ZoneGaps {
    pub(crate) top: i32,
    pub(crate) bottom: i32,
    pub(crate) left: i32,
    pub(crate) right: i32,
}

/// Minimum slice of a window that must remain on-screen. Used both to clamp
/// dragging (a window may slide off the left/right/bottom edges, but this much
/// stays reachable) and to decide when an off-screen window needs rescuing.
pub const MIN_VISIBLE_PX: i32 = 64;

/// How far *outside* a window edge the invisible resize grab band reaches (into
/// the gap/border around the window). Corners are where two bands overlap.
pub const RESIZE_MARGIN_PX: i32 = 12;

/// Apps that open as a centered floating window by default (rather than being
/// snapped into the tiling grid).
const CENTERED_FLOAT_APP_IDS: &[&str] = &["com.metis.Settings"];

/// Window titles that default to a centered floating window. Title fallback for
/// when GTK sets the Wayland app_id late (or not at all).
const CENTERED_FLOAT_TITLES: &[&str] = &["Metis Settings"];

/// Default size for a centered floating app when nothing is saved yet.
const DEFAULT_FLOAT_W: i32 = 900;
const DEFAULT_FLOAT_H: i32 = 660;
/// Smallest width/height (logical px) considered a valid persisted window size.
/// Guards against stale degenerate entries (e.g. a 1x1 saved by a window torn
/// down before it ever got a buffer) reopening as an unusable sliver.
const MIN_SAVED_WINDOW_PX: i32 = 120;

/// Per-output desktop state. Each output (monitor) owns an independent set of
/// virtual workspaces: its visible grid (`layout`), which workspace is showing
/// (`active_workspace`), and the hidden workspaces' app tiles (`stashed_app_tiles`).
/// Desk widget tiles (clock/weather/…) only exist on the primary output's desk.
pub struct OutputDesk {
    pub layout: GridLayout,
    /// Currently visible virtual workspace on this output (1-based).
    pub active_workspace: u32,
    /// App tiles for this output's hidden workspaces, keyed by workspace id.
    pub stashed_app_tiles: std::collections::HashMap<u32, Vec<metis_grid::GridTile>>,
    /// Per-workspace layout mode (grid vs. scroll). Absent entries fall back to the
    /// configured default; the grid tiles above remain the membership source of
    /// truth in either mode.
    pub layout_kind: std::collections::HashMap<u32, metis_grid::LayoutKind>,
    /// Per-workspace scrolling-strip arrangement, used when that workspace's
    /// `layout_kind` is `Scroll`.
    pub scroll: std::collections::HashMap<u32, metis_grid::ScrollState>,
}

pub struct MetisState {
    pub start_time: std::time::Instant,
    pub socket_name: OsString,
    pub display_handle: DisplayHandle,

    pub space: Space<Window>,
    pub loop_signal: LoopSignal,

    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    pub xdg_decoration_state: XdgDecorationState,
    pub layer_shell_state: WlrLayerShellState,
    pub shm_state: ShmState,
    pub _output_manager_state: OutputManagerState,
    pub seat_state: SeatState<MetisState>,
    pub data_device_state: DataDeviceState,
    pub primary_selection_state: PrimarySelectionState,
    pub popups: PopupManager,

    pub seat: Seat<MetisState>,

    /// XWayland shell protocol state (xwayland-surface association). Always
    /// present; the X11 window manager (`xwm`) only exists once XWayland is up.
    pub xwayland_shell_state: smithay::wayland::xwayland_shell::XWaylandShellState,
    /// Live X11 window manager, populated when the XWayland server signals ready.
    pub xwm: Option<smithay::xwayland::X11Wm>,
    /// X11 display number (e.g. `0` → `:0`) for the running XWayland server, used
    /// to set `DISPLAY` on X11 child processes.
    pub xdisplay: Option<u32>,

    pub windows: WindowRegistry,
    /// Windows the user has manually dragged out of the grid by their titlebar.
    /// They keep their free position (no grid snap-back) until closed.
    pub floating: std::collections::HashSet<u32>,
    /// Windows whose top edge meets the edge bar (maximized, or snapped left /
    /// right / top-corner), plus grid-tiled SSD windows: their titlebar auto-hides
    /// and re-appears as a translucent overlay on the client's top strip.
    pub auto_hide_titlebar: std::collections::HashSet<u32>,
    /// The auto-hide window whose titlebar is currently revealed (pointer in its
    /// top strip), or `None`. Drives both rendering and decoration clicks.
    pub revealed_titlebar: Option<u32>,
    /// Window id whose auto-hide titlebar overlay is sliding in/out (0..1 progress).
    pub titlebar_reveal_window: Option<u32>,
    /// Slide progress for [`Self::titlebar_reveal_window`]: 0 = hidden above the
    /// client, 1 = fully shown over its top strip.
    pub titlebar_reveal_progress: f32,
    last_titlebar_reveal_tick: Option<std::time::Instant>,
    /// Maximize ripple/wobble FX start times keyed by window id.
    maximize_fx_started: std::collections::HashMap<u32, std::time::Instant>,
    /// Last titlebar primary-button press for double-click maximize toggle.
    titlebar_last_click: Option<(u32, std::time::Instant)>,
    /// Maximized titlebar press waiting for drag threshold (no grab until then).
    titlebar_press_pending: Option<(u32, Point<f64, Logical>, smithay::utils::Serial)>,
    /// Minimize genie animations keyed by window id (window still mapped until done).
    minimize_genie_fx: std::collections::HashMap<u32, crate::window_fx::MinimizeGenieFx>,
    /// Persisted per-app floating geometry, so apps reopen where they were left.
    pub window_state: crate::window_state::WindowStateStore,

    /// Per-output desktops, keyed by output name. Created lazily as outputs map;
    /// the first (primary) output's desk is seeded from `desk.json` (widgets),
    /// secondary outputs get an app-only grid. See `OutputDesk`.
    pub desks: std::collections::HashMap<String, OutputDesk>,
    /// Baseline grid (columns/rows + widget tiles) loaded from `desk.json`, used to
    /// seed the primary output's desk and to size secondary (app-only) desks.
    pub default_layout: GridLayout,
    pub gutter_px: u32,
    pub tile_modes: TileModeState,
    pub monitor: MonitorRect,
    pub ipc_listener: Option<std::os::unix::net::UnixListener>,
    pub events_listener: Option<std::os::unix::net::UnixListener>,
    pub event_bus: EventBus,

    /// Spawn shell/client after the compositor is accepting connections.
    pub startup_shell: Option<String>,
    pub startup_client: Option<String>,
    pub startup_frames: u32,
    pub shell_spawned: bool,
    pub client_spawned: bool,
    pub child_processes: Vec<std::process::Child>,

    pub cursor_status: smithay::input::pointer::CursorImageStatus,
    /// Resize edge currently under the pointer (drives the host cursor shape).
    /// `None` when the pointer isn't hovering a window's resize band.
    pub hover_cursor: Option<crate::grabs::ResizeEdge>,
    /// Last app window the user brought forward (taskbar, Alt+Tab, etc.). Kept
    /// when keyboard focus moves to the edge bar so bulk layout sync does not
    /// re-raise a maximized window over the app the user just picked.
    last_focused_window: Option<u32>,
    /// Active snap-zone preview while a window is being dragged by its titlebar:
    /// the target rect (already inset) plus a short label. `None` when no drag is
    /// in progress or the pointer isn't over a snap band. Drives both the live
    /// overlay and where the window lands on drop.
    pub snap_preview: Option<(PixelRect, &'static str)>,

    pub wallpaper: crate::wallpaper::Wallpaper,
    pub blur: crate::blur::BlurRuntime,
    pub decorations: crate::decoration::DecorationRuntime,
    pub input_runtime: crate::device_input::InputRuntime,

    redraw_trigger: Option<Rc<dyn Fn()>>,
    /// When true, the next winit Redraw performs GL compositing + layer frame delivery.
    pub damaged: bool,
    /// Defer `flush_clients` until after the winit redraw handler returns (avoids reentrancy).
    pub defer_client_flush: bool,
    /// One post-configure arrange after the bar commits its first real buffer.
    last_pointer_forward: Option<(std::time::Instant, Point<f64, Logical>)>,
    /// Last known edge-bar position; used to reflow windows immediately when the
    /// bar layer commits after a settings change (not only on the blur poll).
    pub(crate) last_bar_position: metis_config::BarPosition,
    /// Last scroll-animation tick (16ms heartbeat).
    last_scroll_tick: Option<std::time::Instant>,
    /// Debounce grid/scroll toggle (`Mod+\`) so key-repeat cannot flip modes
    /// dozens of times per second and stall the compositor.
    last_layout_toggle: Option<std::time::Instant>,
    /// Resolved once at startup and reused for every spawned client — avoids
    /// blocking the event loop on `gsettings`/D-Bus during shell launch.
    client_cursor_theme: String,
    client_cursor_size: String,

    /// Monotonic clock for frame timing / cursor animation (shared by backends).
    pub clock: smithay::utils::Clock<smithay::utils::Monotonic>,
    /// Persistent identity + commit counter for the snap-zone overlay element so
    /// the damage tracker treats it as one stable element across frames.
    pub(crate) snap_overlay_id: smithay::backend::renderer::element::Id,
    pub(crate) snap_overlay_commit: smithay::backend::renderer::utils::CommitCounter,
    pub(crate) last_snap_rect: Option<PixelRect>,
    /// DRM/udev backend state (session, GPUs, per-connector surfaces). `None` in
    /// the nested winit session.
    pub udev: Option<crate::udev::UdevState>,
    /// Screen capture protocol state (ext-image-copy-capture).
    pub image_capture: crate::image_capture::ImageCaptureRuntime,
}

/// Cursor theme/size for nested clients. Never calls D-Bus — a synchronous
/// `gsettings` in `spawn_client` blocked the compositor event loop during shell
/// startup (especially after `--import-env`), which GNOME reported as
/// "Unknown is not responding".
fn resolve_client_cursor_env() -> (String, String) {
    fn cursor_icon_dirs() -> Vec<std::path::PathBuf> {
        let mut dirs = Vec::new();
        if let Ok(home) = std::env::var("HOME") {
            dirs.push(std::path::PathBuf::from(format!("{home}/.icons")));
            dirs.push(std::path::PathBuf::from(format!("{home}/.local/share/icons")));
        }
        dirs.push(std::path::PathBuf::from("/usr/share/icons"));
        dirs.push(std::path::PathBuf::from("/usr/local/share/icons"));
        dirs
    }
    fn resolve_cursor_theme(name: &str) -> Option<String> {
        fn inner(name: &str, dirs: &[std::path::PathBuf], depth: u8) -> Option<String> {
            if name.is_empty() || depth > 8 {
                return None;
            }
            if dirs.iter().any(|d| d.join(name).join("cursors").is_dir()) {
                return Some(name.to_string());
            }
            for d in dirs {
                let Ok(text) = std::fs::read_to_string(d.join(name).join("index.theme")) else {
                    continue;
                };
                for line in text.lines() {
                    if let Some(rest) = line.trim().strip_prefix("Inherits") {
                        let rest = rest.trim_start_matches([' ', '=']).trim();
                        for parent in rest.split(',') {
                            if let Some(found) = inner(parent.trim(), dirs, depth + 1) {
                                return Some(found);
                            }
                        }
                    }
                }
            }
            None
        }
        inner(name, &cursor_icon_dirs(), 0)
    }
    fn gtk_settings_value(home: &str, key: &str) -> Option<String> {
        for rel in ["gtk-4.0/settings.ini", "gtk-3.0/settings.ini"] {
            let path = std::path::Path::new(home).join(".config").join(rel);
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            for line in text.lines() {
                let line = line.trim();
                if line.starts_with('#') || !line.contains('=') {
                    continue;
                }
                let (k, v) = line.split_once('=')?;
                if k.trim() == key {
                    let v = v.trim().trim_matches('"');
                    if !v.is_empty() {
                        return Some(v.to_string());
                    }
                }
            }
        }
        None
    }

    let home = std::env::var("HOME").unwrap_or_default();
    let theme_pref = std::env::var("XCURSOR_THEME")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| gtk_settings_value(&home, "gtk-cursor-theme-name"))
        .unwrap_or_else(|| "default".into());
    let cursor_theme = resolve_cursor_theme(&theme_pref)
        .or_else(|| resolve_cursor_theme("default"))
        .or_else(|| resolve_cursor_theme("Yaru"))
        .or_else(|| resolve_cursor_theme("Adwaita"))
        .unwrap_or_else(|| "Adwaita".into());
    let cursor_size = std::env::var("XCURSOR_SIZE")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| gtk_settings_value(&home, "gtk-cursor-theme-size"))
        .unwrap_or_else(|| "24".into());
    (cursor_theme, cursor_size)
}

/// Environment shared by every client the compositor spawns (shell, settings,
/// menu launches). GTK hardening avoids portal/a11y stalls in a bare session.
fn apply_spawned_client_env(
    cmd: &mut std::process::Command,
    program: &str,
    socket: &std::ffi::OsStr,
    xdisplay: Option<u32>,
) {
    cmd.env("WAYLAND_DISPLAY", socket);
    cmd.env("METIS_SESSION", "1");
    match xdisplay {
        Some(n) => {
            cmd.env("DISPLAY", format!(":{n}"));
        }
        None => {
            cmd.env_remove("DISPLAY");
        }
    }
    cmd.env("GDK_BACKEND", "wayland");
    // Only force the Cairo GSK backend for our own shell — it avoids GL hangs on
    // some drivers during layer-shell setup. Other GTK apps should pick GL so they
    // stay responsive; do not inherit the session-wide GSK_RENDERER default.
    if program.contains("metis-shell") {
        let renderer = std::env::var("METIS_SHELL_GSK_RENDERER")
            .or_else(|_| std::env::var("GSK_RENDERER"))
            .unwrap_or_else(|_| "cairo".into());
        cmd.env("GSK_RENDERER", renderer);
    } else {
        cmd.env_remove("GSK_RENDERER");
    }
    if let Ok(runtime) = std::env::var("XDG_RUNTIME_DIR") {
        cmd.env("XDG_RUNTIME_DIR", runtime);
    }
    cmd.env("GTK_A11Y", "none");
    cmd.env("NO_AT_BRIDGE", "1");
    // Nested dev sessions run inside GNOME/KDE — disable GTK's portal proxy so
    // startup does not block on the host portal stack.
    if std::env::var_os("METIS_NESTED").is_some() {
        let gdk_debug = std::env::var("GDK_DEBUG").unwrap_or_default();
        if gdk_debug.is_empty() {
            cmd.env("GDK_DEBUG", "no-portals");
        } else if !gdk_debug.split(',').any(|p| p == "no-portals" || p == "portals") {
            cmd.env("GDK_DEBUG", format!("{gdk_debug},no-portals"));
        } else {
            cmd.env("GDK_DEBUG", gdk_debug);
        }
    }
}

impl MetisState {
    pub fn new(event_loop: &mut EventLoop<'_, MetisState>, display: Display<MetisState>) -> Self {
        let start_time = std::time::Instant::now();
        let dh = display.handle();

        let compositor_state = CompositorState::new::<MetisState>(&dh);
        let xdg_shell_state = XdgShellState::new::<MetisState>(&dh);
        let xdg_decoration_state = XdgDecorationState::new::<MetisState>(&dh);
        let layer_shell_state = WlrLayerShellState::new::<MetisState>(&dh);
        let shm_state = ShmState::new::<MetisState>(&dh, vec![]);
        let popups = PopupManager::default();
        let output_manager_state = OutputManagerState::new_with_xdg_output::<MetisState>(&dh);
        let data_device_state = DataDeviceState::new::<MetisState>(&dh);
        let primary_selection_state = PrimarySelectionState::new::<MetisState>(&dh);
        let xwayland_shell_state =
            smithay::wayland::xwayland_shell::XWaylandShellState::new::<MetisState>(&dh);
        TextInputManagerState::new::<MetisState>(&dh);

        let mut seat_state = SeatState::<MetisState>::new();
        let mut seat = seat_state.new_wl_seat(&dh, "metis");
        let input_cfg = crate::device_input::InputRuntime::initial_keyboard_config();
        let kb = &input_cfg.keyboard;
        seat.add_keyboard(
            smithay::input::keyboard::XkbConfig {
                rules: "",
                model: "",
                layout: &kb.layout,
                variant: &kb.variant,
                options: kb.merged_xkb_options(),
            },
            kb.repeat_delay_ms,
            kb.repeat_rate_hz,
        )
        .unwrap();
        seat.add_pointer();

        let space = Space::<Window>::default();
        let socket_name = Self::init_wayland_listener(display, event_loop);
        let loop_signal = event_loop.get_signal();

        let desk_path = desk_config_path();
        let grid_layout = GridLayout::load_from_path(&desk_path);
        let mut grid_layout = grid_layout;
        metis_grid::sanitize_layout(&mut grid_layout);
        let (client_cursor_theme, client_cursor_size) = resolve_client_cursor_env();
        tracing::info!(
            theme = %client_cursor_theme,
            size = %client_cursor_size,
            "client cursor theme"
        );

        Self {
            start_time,
            socket_name,
            display_handle: dh.clone(),
            space,
            loop_signal,
            compositor_state,
            xdg_shell_state,
            xdg_decoration_state,
            layer_shell_state,
            shm_state,
            _output_manager_state: output_manager_state,
            seat_state,
            data_device_state,
            primary_selection_state,
            popups,
            seat,
            xwayland_shell_state,
            xwm: None,
            xdisplay: None,
            windows: WindowRegistry::new(),
            floating: std::collections::HashSet::new(),
            auto_hide_titlebar: std::collections::HashSet::new(),
            revealed_titlebar: None,
            titlebar_reveal_window: None,
            titlebar_reveal_progress: 0.0,
            last_titlebar_reveal_tick: None,
            maximize_fx_started: std::collections::HashMap::new(),
            titlebar_last_click: None,
            titlebar_press_pending: None,
            minimize_genie_fx: std::collections::HashMap::new(),
            window_state: crate::window_state::WindowStateStore::load(),
            desks: std::collections::HashMap::new(),
            default_layout: grid_layout,
            gutter_px: 14,
            tile_modes: TileModeState::default(),
            monitor: MonitorRect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
            ipc_listener: None,
            events_listener: None,
            event_bus: EventBus::default(),
            startup_shell: None,
            startup_client: None,
            startup_frames: 0,
            shell_spawned: false,
            client_spawned: false,
            child_processes: Vec::new(),
            cursor_status: smithay::input::pointer::CursorImageStatus::default_named(),
            hover_cursor: None,
            last_focused_window: None,
            snap_preview: None,
            wallpaper: crate::wallpaper::Wallpaper::new(),
            blur: crate::blur::BlurRuntime::default(),
            decorations: crate::decoration::DecorationRuntime::default(),
            input_runtime: crate::device_input::InputRuntime::new(),
            redraw_trigger: None,
            damaged: true,
            defer_client_flush: false,
            last_pointer_forward: None,
            last_bar_position: metis_config::load_bar_config().position,
            last_scroll_tick: None,
            last_layout_toggle: None,
            client_cursor_theme,
            client_cursor_size,
            clock: smithay::utils::Clock::new(),
            snap_overlay_id: smithay::backend::renderer::element::Id::new(),
            snap_overlay_commit: smithay::backend::renderer::utils::CommitCounter::default(),
            last_snap_rect: None,
            udev: None,
            image_capture: crate::image_capture::ImageCaptureRuntime::new(&dh),
        }
    }

    pub(crate) fn process_pending_captures(&mut self, renderer: &mut smithay::backend::renderer::gles::GlesRenderer) {
        if !self.image_capture.has_pending() {
            return;
        }
        let start = self.start_time;
        crate::image_capture::finish_pending_captures(self, renderer, start);
    }

    /// Per-tick housekeeping shared by both backends: drive the startup state
    /// machine, service shell IPC, advance the debounced wallpaper decode, pick
    /// up live blur / decoration config changes, and tick scroll animations.
    pub(crate) fn xcursor_config(&self) -> (&str, u32) {
        let size = self
            .client_cursor_size
            .parse()
            .unwrap_or(24)
            .clamp(16, 96);
        (&self.client_cursor_theme, size)
    }

    /// Returns nothing; callers redraw when `self.damaged` is set. Kept off the
    /// render path so going idle can never starve shell/client spawn.
    pub fn tick_housekeeping(&mut self) {
        self.run_pending_startup();
        crate::ipc::drain_ipc(self);

        if self.wallpaper.tick_decode() {
            self.damaged = true;
        }

        let (blur_changed, bar_position_changed) = self.blur.maybe_refresh();
        if blur_changed {
            self.damaged = true;
        }
        if bar_position_changed {
            self.last_bar_position = self.blur.position;
            self.reflow_for_bar_geometry_change();
        }

        let deco = self.decorations.maybe_refresh();
        if deco.damage {
            self.damaged = true;
        }
        if deco.relayout {
            let ids: Vec<u32> = self.windows.ids();
            for id in ids {
                self.apply_window_rect(id);
            }
            self.sync_all_app_windows();
            self.refresh_all_scroll_offsets();
            self.damaged = true;
        }

        if let Some(cfg) = self.input_runtime.maybe_refresh() {
            crate::device_input::apply_keyboard(self, &cfg);
        }

        if self.tick_scroll_animations() {
            self.damaged = true;
        }

        if self.tick_titlebar_reveal_animation() {
            self.damaged = true;
        }

        if self.tick_maximize_fx() {
            self.damaged = true;
        }

        if self.tick_minimize_genie_fx() {
            self.damaged = true;
        }
    }

    /// True while the startup splash layer is on-screen (backdrop blur is deferred
    /// until it dismisses — the first blur pass is expensive).
    pub fn splash_overlay_visible(&self) -> bool {
        use smithay::desktop::layer_map_for_output;
        for out in self.space.outputs() {
            let map = layer_map_for_output(out);
            for layer in map.layers() {
                if layer.namespace() != "metis-splash" {
                    continue;
                }
                match map.layer_geometry(layer) {
                    Some(g) if g.loc.y >= 0 && g.loc.y < 16_000 => return true,
                    None => return true,
                    _ => {}
                }
            }
        }
        false
    }

    pub fn set_redraw_trigger(&mut self, trigger: Rc<dyn Fn()>) {
        self.redraw_trigger = Some(trigger);
    }

    pub fn request_redraw(&mut self) {
        if let Some(trigger) = &self.redraw_trigger {
            trigger();
        }
    }

    /// Mark the output dirty. Actual redraws are paced by the 16ms heartbeat
    /// timer in the winit backend (the nested host does not vsync-throttle us),
    /// so we only flag damage here and let the next tick coalesce it. This caps
    /// the render rate at ~60fps even under a flood of client commits.
    pub fn schedule_redraw(&mut self) {
        self.damaged = true;
    }

    pub fn flush_clients_if_pending(&mut self) {
        if self.defer_client_flush {
            self.defer_client_flush = false;
            let _ = self.display_handle.flush_clients();
        }
    }

    /// Throttle pointer motion forwarded to clients — GTK hover repaints were saturating the loop.
    pub fn should_forward_pointer_motion(&mut self, location: Point<f64, Logical>) -> bool {
        // Never throttle while a compositor grab (move/resize/scroll-resize) owns the
        // pointer — dropped motion events leave the grab stuck at its start size.
        if self.seat.get_pointer().is_some_and(|p| p.is_grabbed()) {
            return true;
        }
        if self.metis_bar_ui_hit(location) {
            return true;
        }
        const MIN_MS: u128 = 48;
        const MIN_DIST_SQ: f64 = 9.0;
        let now = std::time::Instant::now();
        if let Some((t, prev)) = self.last_pointer_forward {
            let dx = location.x - prev.x;
            let dy = location.y - prev.y;
            if now.duration_since(t).as_millis() < MIN_MS && (dx * dx + dy * dy) < MIN_DIST_SQ {
                return false;
            }
        }
        self.last_pointer_forward = Some((now, location));
        true
    }

    pub fn window_id_for_toplevel(&self, surface: &smithay::wayland::shell::xdg::ToplevelSurface) -> Option<u32> {
        self.windows.id_for_surface(surface.wl_surface())
    }

    /// True when Metis should draw server-side titlebar/border chrome for this window.
    pub(crate) fn window_uses_ssd(&self, id: u32) -> bool {
        self.windows.uses_ssd(id)
    }

    /// True when this SSD window should auto-hide its titlebar (maximize / snap /
    /// grid). All Metis-decorated windows use the slide-down hover overlay.
    pub(crate) fn should_auto_hide_titlebar(&self, id: u32) -> bool {
        self.window_uses_ssd(id)
    }

    /// True when Metis should render or hit-test server-side window chrome.
    pub(crate) fn should_draw_metis_ssd(&self, id: u32) -> bool {
        if !self.window_uses_ssd(id) {
            return false;
        }
        let Some(record) = self.windows.get(id) else {
            return false;
        };
        let negotiated_mode = read_toplevel_decoration_mode(&record.toplevel);
        !crate::decoration_policy::defer_ssd_paint(
            record.app_id.as_deref(),
            negotiated_mode,
            record.decoration_bound,
        )
    }

    /// Client surface rect within a tile footprint — body inset for SSD, full tile for CSD.
    pub(crate) fn tile_client_rect(&self, id: u32, full: PixelRect) -> PixelRect {
        if !self.should_draw_metis_ssd(id) {
            return full;
        }
        if self.auto_hide_titlebar.contains(&id) {
            metis_grid::app_tile_auto_hide_body_rect(full)
        } else {
            app_tile_body_rect(full)
        }
    }

    /// SSD client placement for a tile footprint: auto-hide windows fill the
    /// footprint; others keep a persistent titlebar inset.
    fn ssd_client_rect(&self, id: u32, full: PixelRect) -> PixelRect {
        if self.should_auto_hide_titlebar(id) {
            metis_grid::app_tile_auto_hide_body_rect(full)
        } else {
            app_tile_body_rect(full)
        }
    }

    /// Reconcile `uses_ssd` with xdg-decoration negotiation and app-id heuristics.
    pub(crate) fn refresh_window_decoration_mode(&mut self, id: u32) {
        let Some(record) = self.windows.get(id).cloned() else {
            return;
        };
        let was_draw = self.should_draw_metis_ssd(id);
        let negotiated_mode = read_toplevel_decoration_mode(&record.toplevel);
        let uses_ssd = crate::decoration_policy::resolve_uses_ssd(
            record.app_id.as_deref(),
            negotiated_mode,
        );
        let mode_changed = uses_ssd != record.uses_ssd;
        if mode_changed {
            tracing::info!(
                id,
                uses_ssd,
                app_id = ?record.app_id,
                ?negotiated_mode,
                decoration_negotiated = record.decoration_negotiated,
                "window decoration policy updated"
            );
            if !uses_ssd {
                self.clear_auto_hide(id);
            }
        }
        self.windows.set_uses_ssd(id, uses_ssd);
        if self.should_auto_hide_titlebar(id)
            && self
                .windows
                .get(id)
                .is_some_and(|r| r.maximized || r.snapped)
        {
            self.auto_hide_titlebar.insert(id);
        }
        if record.decoration_bound || record.decoration_negotiated {
            self.push_preferred_decoration_mode(&record.toplevel, uses_ssd);
        }
        let now_draw = self.should_draw_metis_ssd(id);
        if mode_changed || was_draw != now_draw {
            self.apply_window_rect(id);
            self.schedule_redraw();
        }
    }

    fn push_preferred_decoration_mode(
        &self,
        toplevel: &smithay::wayland::shell::xdg::ToplevelSurface,
        uses_ssd: bool,
    ) {
        let mode = crate::decoration_policy::grant_decoration_mode(uses_ssd);
        toplevel.with_pending_state(|state| {
            state.decoration_mode = Some(mode);
        });
        if toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
    }

    pub fn tile_id_for_window(&self, window_id: u32) -> Option<String> {
        self.find_app_tile(window_id).map(|(_, t)| t.id)
    }

    /// App windows slotted in the desk grid — not free-floating or fullscreen.
    pub fn is_window_grid_managed(&self, id: u32) -> bool {
        if self.floating.contains(&id) {
            return false;
        }
        if self.tile_id_for_window(id).is_none() {
            return false;
        }
        let Some(record) = self.windows.get(id) else {
            return false;
        };
        !record.fullscreen && !record.maximized && !self.windows.is_minimized(id)
    }

    /// Snap a grid-managed window back to its tile body if the client moved or resized it.
    pub fn enforce_grid_window_geometry(&mut self, id: u32) {
        if !self.is_window_grid_managed(id) {
            return;
        }
        let Some(expected) = self
            .rect_for_window_tile(id)
            .map(|full| self.tile_client_rect(id, full))
        else {
            return;
        };
        let Some(record) = self.windows.get(id).cloned() else {
            return;
        };
        let drifted = self
            .space
            .element_location(&record.window)
            .is_none_or(|loc| loc.x != expected.x || loc.y != expected.y)
            || record.window.geometry().size
                != smithay::utils::Size::from((expected.width, expected.height))
            || record.target_rect != expected;
        if drifted {
            self.apply_window_rect(id);
        }
    }

    /// Compute the snap-zone target for a pointer at global-logical (`x`, `y`),
    /// in pixel space against the usable area (so the top edge maximizes below
    /// the bar). Returns the final *client* rect (gaps already applied to match
    /// the maximize look) + label, or `None` when the pointer isn't near an edge.
    pub fn snap_target_at(&self, x: i32, y: i32) -> Option<(PixelRect, &'static str)> {
        // Snap against the output the pointer is over, so dragging a window to a
        // secondary monitor's edge tiles it on *that* monitor.
        let place = match self.output_at(Point::from((x, y))) {
            Some(output) => self.window_placement_zone_for(&output),
            None => self.window_placement_zone(),
        };
        let pos = metis_config::load_bar_config().position;

        // Snap geometry is computed in `place` (beside the bar). When the pointer
        // hugs the physical monitor edge over an overlay bar, project it onto the
        // nearest `place` edge so left/right/bottom snaps still fire.
        let mut sx = x;
        let mut sy = y;
        match pos {
            metis_config::BarPosition::Left if x < place.x => sx = place.x,
            metis_config::BarPosition::Right if x > place.x + place.width => {
                sx = place.x + place.width;
            }
            metis_config::BarPosition::Bottom if y > place.y + place.height => {
                sy = place.y + place.height;
            }
            _ => {}
        }

        let (raw, label) = metis_grid::pixel_snap_target(sx, sy, place)?;
        let gaps = self.zone_edge_gaps();
        Some((snap_client_rect(raw, place, gaps), label))
    }

    /// Drop a window into a snap zone. The "Maximize" zone routes through the real
    /// `set_maximized` so it's pixel-identical to the titlebar maximize button.
    /// Half / quarter zones float the window and mark it *tiled* (all four edges)
    /// so GTK squares its corners and drops its drop-shadow, filling the snapped
    /// rect exactly — otherwise the leftover CSD shadow makes the padding look
    /// uneven from edge to edge.
    pub fn apply_snap(&mut self, id: u32, rect: PixelRect, label: &str) {
        use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;

        if label == "Maximize" {
            self.set_maximized(id, true);
            return;
        }

        // Re-home desk membership to the monitor this snap targets *before*
        // applying geometry. Doing this after the snap (via `maybe_adopt`) used
        // to run `clamp_floating_rect`, adding a spurious titlebar inset on
        // auto-hide edge snaps dragged across outputs.
        let snap_center = Point::from((rect.x + rect.width / 2, rect.y + rect.height / 2));
        if let Some(output) = self.output_at(snap_center) {
            let key = output.name();
            if key != self.desk_key_for_window(id) {
                self.move_window_to_output_inner(id, &key, false);
            }
        }

        self.capture_pre_snap_geometry(id);

        let Some(record) = self.windows.get(id).cloned() else {
            return;
        };
        self.space.raise_element(&record.window, true);
        self.floating.insert(id);
        self.windows.set_maximized(id, false);

        // Snaps whose top edge meets the bar auto-hide the titlebar as a hover
        // overlay on the client's top strip.
        let top_touching = matches!(label, "Left half" | "Right half" | "Top-left" | "Top-right");
        let uses_ssd = self.window_uses_ssd(id);
        let body = if uses_ssd {
            if top_touching && self.should_auto_hide_titlebar(id) {
                metis_grid::app_tile_auto_hide_body_rect(rect)
            } else {
                app_tile_body_rect(rect)
            }
        } else {
            rect
        };
        let size = Size::from((body.width.max(1), body.height.max(1)));
        record.toplevel.with_pending_state(|state| {
            state.states.unset(xdg_toplevel::State::Maximized);
            state.states.unset(xdg_toplevel::State::Fullscreen);
            state.states.set(xdg_toplevel::State::TiledLeft);
            state.states.set(xdg_toplevel::State::TiledRight);
            state.states.set(xdg_toplevel::State::TiledTop);
            state.states.set(xdg_toplevel::State::TiledBottom);
            state.size = Some(size);
        });
        self.space
            .map_element(record.window.clone(), Point::from((body.x, body.y)), true);
        record.toplevel.send_pending_configure();
        self.windows.set_target_rect(id, body);
        if uses_ssd && top_touching && self.should_auto_hide_titlebar(id) {
            self.auto_hide_titlebar.insert(id);
        } else {
            self.clear_auto_hide(id);
        }
        self.reclamp_auto_hide(id);
        self.windows.set_snapped(id, true);
        self.save_window_geometry(id);
        tracing::info!(id, ?rect, label, "snap: window snapped to zone");
    }

    /// Clear the tiled states a snap applied, so a window pulled off a snapped
    /// position regains its normal floating chrome (GTK rounded corners + drop
    /// shadow). `send_pending_configure` is a no-op when nothing actually changed.
    fn clear_tiled_states(&mut self, id: u32) {
        use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::State;
        let Some(record) = self.windows.get(id).cloned() else {
            return;
        };
        record.toplevel.with_pending_state(|state| {
            state.states.unset(State::TiledLeft);
            state.states.unset(State::TiledRight);
            state.states.unset(State::TiledTop);
            state.states.unset(State::TiledBottom);
        });
        record.toplevel.send_pending_configure();
    }

    /// Drop a window's auto-hide-titlebar state (e.g. on unmaximize, unsnap,
    /// minimize, fullscreen, or close), clearing the reveal if it was showing.
    pub fn clear_auto_hide(&mut self, id: u32) {
        self.auto_hide_titlebar.remove(&id);
        if self.revealed_titlebar == Some(id) {
            self.revealed_titlebar = None;
        }
        if self.titlebar_reveal_window == Some(id) {
            self.titlebar_reveal_window = None;
            self.titlebar_reveal_progress = 0.0;
        }
    }

    /// Live client-surface (body) geometry for a mapped window.
    fn window_body_rect(&self, id: u32) -> Option<PixelRect> {
        let record = self.windows.get(id)?;
        let loc = self.space.element_location(&record.window)?;
        let size = record.window.geometry().size;
        Some(PixelRect {
            x: loc.x,
            y: loc.y,
            width: size.w.max(1),
            height: size.h.max(1),
        })
    }

    /// Remember a floating window's size/position before the first snap in a chain.
    fn capture_pre_snap_geometry(&mut self, id: u32) {
        if self.windows.is_snapped(id) {
            return;
        }
        let Some(body) = self.window_body_rect(id) else {
            return;
        };
        self.windows.set_restore_rect(id, body);
    }

    /// Pull a snapped/maximized window back to its pre-snap floating size when the
    /// user starts dragging it by the titlebar. Keeps the grab point under the
    /// pointer so the window doesn't jump.
    fn restore_floating_from_snap(
        &mut self,
        id: u32,
        pointer: Point<f64, Logical>,
    ) -> Point<i32, Logical> {
        use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;

        let Some(record) = self.windows.get(id).cloned() else {
            return Point::default();
        };

        let current = self
            .window_body_rect(id)
            .unwrap_or(record.target_rect);
        let restore = self
            .windows
            .take_restore_rect(id)
            .unwrap_or(current);

        self.clear_auto_hide(id);
        self.windows.set_maximized(id, false);
        self.windows.set_snapped(id, false);

        let rel_x = if current.width > 0 {
            (pointer.x - current.x as f64) / current.width as f64
        } else {
            0.5
        };
        let rel_y = if current.height > 0 {
            (pointer.y - current.y as f64) / current.height as f64
        } else {
            0.0
        };
        let rel_x = rel_x.clamp(0.0, 1.0);
        let rel_y = rel_y.clamp(0.0, 1.0);

        let mut body = PixelRect {
            x: (pointer.x - rel_x * restore.width as f64).round() as i32,
            y: (pointer.y - rel_y * restore.height as f64).round() as i32,
            width: restore.width.max(1),
            height: restore.height.max(1),
        };
        body = self.clamp_floating_rect_for(id, body);

        record.toplevel.with_pending_state(|state| {
            state.states.unset(xdg_toplevel::State::Maximized);
            state.states.unset(xdg_toplevel::State::Fullscreen);
            state.states.unset(xdg_toplevel::State::TiledLeft);
            state.states.unset(xdg_toplevel::State::TiledRight);
            state.states.unset(xdg_toplevel::State::TiledTop);
            state.states.unset(xdg_toplevel::State::TiledBottom);
            state.size = Some(Size::from((body.width, body.height)));
            state.fullscreen_output = None;
        });
        let loc = Point::from((body.x, body.y));
        self.space
            .map_element(record.window.clone(), loc, true);
        record.toplevel.send_pending_configure();
        self.windows.set_target_rect(id, body);
        self.schedule_redraw();
        loc
    }

    pub fn queue_startup(&mut self, shell: Option<String>, client: Option<String>) {
        self.startup_shell = shell;
        self.startup_client = client;
    }

    pub fn run_pending_startup(&mut self) {
        let elapsed = self.start_time.elapsed();

        if !self.shell_spawned && elapsed > Duration::from_millis(250) {
            if std::env::var("METIS_NO_SHELL").is_err() {
                if let Some(shell) = self.startup_shell.take() {
                    self.spawn_client(&shell);
                }
            } else {
                self.startup_shell = None;
            }
            self.shell_spawned = true;
        }

        if self.shell_spawned && !self.client_spawned && elapsed > Duration::from_millis(750) {
            if let Some(client) = self.startup_client.take() {
                self.spawn_client(&client);
                // Only poll grid placement when an explicit `-c` client was requested.
                self.startup_frames = 120;
            }
            self.client_spawned = true;
        }

        if self.startup_frames > 0 {
            self.startup_frames -= 1;
            self.sync_all_app_windows();
        }
    }

    pub fn spawn_client(&mut self, program: &str) {
        // Metis binaries (shell, settings) live alongside the compositor in the
        // cargo target dir, which is usually not on PATH. Resolve a bare program
        // name to its sibling-of-current-exe absolute path so `Launch` works.
        fn resolve_sibling_program(program: &str) -> String {
            let (bin, rest) = match program.split_once(' ') {
                Some((b, r)) => (b, Some(r)),
                None => (program, None),
            };
            if bin.contains('/') {
                return program.to_string();
            }
            if let Ok(exe) = std::env::current_exe() {
                if let Some(dir) = exe.parent() {
                    let candidate = dir.join(bin);
                    if candidate.is_file() {
                        let p = candidate.display().to_string();
                        return match rest {
                            Some(r) => format!("{p} {r}"),
                            None => p,
                        };
                    }
                }
            }
            program.to_string()
        }
        let program = resolve_sibling_program(program);
        let program = program.as_str();

        let mut cmd = if program.contains(' ') {
            let mut c = std::process::Command::new("sh");
            c.arg("-lc").arg(program);
            c
        } else {
            std::process::Command::new(program)
        };

        apply_spawned_client_env(&mut cmd, program, &self.socket_name, self.xdisplay);
        cmd.env("XCURSOR_THEME", &self.client_cursor_theme);
        cmd.env("XCURSOR_SIZE", &self.client_cursor_size);

        match cmd.spawn() {
            Ok(child) => {
                tracing::info!(
                    program,
                    pid = child.id(),
                    wayland_display = ?self.socket_name,
                    "spawned client"
                );
                self.child_processes.push(child);
            }
            Err(err) => tracing::warn!(program, %err, "failed to spawn client"),
        }
    }

    pub fn kill_spawned_clients(&mut self) {
        for mut child in self.child_processes.drain(..) {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    fn init_wayland_listener(
        display: Display<MetisState>,
        event_loop: &mut EventLoop<'_, MetisState>,
    ) -> OsString {
        let listening_socket = ListeningSocketSource::new_auto().unwrap();
        let socket_name = listening_socket.socket_name().to_os_string();
        let loop_handle = event_loop.handle();

        loop_handle
            .insert_source(listening_socket, move |client_stream, _, state| {
                state
                    .display_handle
                    .insert_client(client_stream, Arc::new(ClientState::default()))
                    .unwrap();
            })
            .expect("Failed to init the wayland event source.");

        loop_handle
            .insert_source(
                Generic::new(display, Interest::READ, Mode::Level),
                |_, display, state| {
                    state.run_pending_startup();
                    if let Err(err) = unsafe { display.get_mut().dispatch_clients(state) } {
                        tracing::error!(?err, "wayland dispatch failed");
                    }
                    // Configure events are queued during dispatch; clients block until flushed.
                    let _ = state.display_handle.flush_clients();
                    if let Some(ref listener) = state.events_listener {
                        accept_event_subscribers(listener, &state.event_bus);
                    }
                    Ok(PostAction::Continue)
                },
            )
            .unwrap();

        socket_name
    }

    pub fn send_layer_frames(&self, output: &smithay::output::Output, time: Duration) {
        let layers: Vec<_> = layer_map_for_output(output).layers().cloned().collect();
        let throttle = Duration::from_millis(16);
        for layer in layers {
            layer.send_frame(output, time, Some(throttle), |_, _| Some(output.clone()));
        }
    }

    pub fn arrange_layers(&self) {
        for output in self.space.outputs() {
            layer_map_for_output(output).arrange();
        }
    }

    // --- Output registry helpers ------------------------------------------------
    //
    // The single source of truth for output geometry is the smithay `Space`. These
    // helpers centralize "which output" decisions so the per-output refactor (Phase
    // 3) only has to change the chokepoints below rather than ~15 scattered
    // `space.outputs().next()` call sites. With one output they are equivalent to
    // the old primary-only behavior.

    /// The primary (first-registered) output, if any output is mapped yet.
    pub fn primary_output(&self) -> Option<smithay::output::Output> {
        self.space.outputs().next().cloned()
    }

    /// Global logical geometry of `output` as a `MonitorRect`.
    pub fn output_rect(&self, output: &smithay::output::Output) -> Option<MonitorRect> {
        self.space.output_geometry(output).map(|g| MonitorRect {
            x: g.loc.x,
            y: g.loc.y,
            width: g.size.w,
            height: g.size.h,
        })
    }

    /// Bounding rectangle of every output — the whole virtual desktop — in global
    /// logical coords. Falls back to the cached monitor before any output maps.
    /// Used for absolute-pointer mapping and cross-output window dragging.
    /// Clamp a pointer position to the union of output geometries so relative
    /// (libinput) motion can never leave the visible desktop.
    pub fn clamp_to_desktop(
        &self,
        p: Point<f64, Logical>,
    ) -> Point<f64, Logical> {
        let b = self.desktop_bounds();
        let max_x = (b.loc.x + b.size.w - 1).max(b.loc.x) as f64;
        let max_y = (b.loc.y + b.size.h - 1).max(b.loc.y) as f64;
        Point::from((
            p.x.clamp(b.loc.x as f64, max_x),
            p.y.clamp(b.loc.y as f64, max_y),
        ))
    }

    pub fn desktop_bounds(&self) -> smithay::utils::Rectangle<i32, Logical> {
        let mut bounds: Option<smithay::utils::Rectangle<i32, Logical>> = None;
        for o in self.space.outputs() {
            if let Some(g) = self.space.output_geometry(o) {
                bounds = Some(match bounds {
                    Some(b) => b.merge(g),
                    None => g,
                });
            }
        }
        bounds.unwrap_or_else(|| {
            smithay::utils::Rectangle::new(
                Point::from((self.monitor.x, self.monitor.y)),
                Size::from((self.monitor.width, self.monitor.height)),
            )
        })
    }

    /// The output whose logical geometry contains `point` (global logical
    /// coords), falling back to the primary output when the point is off every
    /// output. Used to route placement, snapping, and maximize to the monitor a
    /// window or the cursor is actually on.
    pub fn output_at(&self, point: Point<i32, Logical>) -> Option<smithay::output::Output> {
        self.space
            .outputs()
            .find(|o| {
                self.space
                    .output_geometry(o)
                    .is_some_and(|g| g.contains(point))
            })
            .cloned()
            .or_else(|| self.primary_output())
    }

    /// The output currently under the pointer, falling back to primary.
    pub fn output_under_pointer(&self) -> Option<smithay::output::Output> {
        match self.seat.get_pointer() {
            Some(p) => {
                let loc = p.current_location();
                self.output_at(Point::from((loc.x.round() as i32, loc.y.round() as i32)))
            }
            None => self.primary_output(),
        }
    }

    /// The output a window `id` sits on, decided by its center point (live
    /// geometry preferred, else its target rect), falling back to primary.
    pub fn output_for_window(&self, id: u32) -> Option<smithay::output::Output> {
        let rect = self
            .window_body_rect(id)
            .or_else(|| self.windows.target_rect(id))?;
        let center = Point::from((rect.x + rect.width / 2, rect.y + rect.height / 2));
        self.output_at(center)
    }

    /// True when `output` carries a Metis edge bar layer surface. Overlay bars
    /// (bottom/left/right) don't set a layer-shell exclusive zone, so window
    /// placement reserves their strip manually — but only on the outputs that
    /// actually show a bar (e.g. not on secondaries in "primary display only").
    pub(crate) fn output_has_bar(&self, output: &smithay::output::Output) -> bool {
        layer_map_for_output(output)
            .layers()
            .any(|l| l.namespace() == "metis-bar")
    }

    /// Re-apply window geometry after the edge bar moves between reserved
    /// (top) and overlay (bottom/left/right) modes.
    pub fn reflow_for_bar_geometry_change(&mut self) {
        let ids: Vec<u32> = self.windows.ids();
        for id in ids {
            if self.windows.is_minimized(id) {
                continue;
            }
            if self.windows.get(id).is_some_and(|r| r.maximized) {
                self.reapply_maximized_geometry(id);
            } else if self.windows.is_snapped(id) {
                self.reflow_snapped_window(id);
            } else {
                self.apply_window_rect(id);
            }
        }
        self.sync_all_app_windows();
        self.refresh_all_scroll_offsets();
        self.arrange_layers();
        self.restore_focus_stacking();
        self.schedule_redraw();
    }

    /// Re-clamp a snapped (non-maximized) window into the new placement zone after
    /// the edge bar moves.
    fn reflow_snapped_window(&mut self, id: u32) {
        let Some(mut rect) = self.windows.target_rect(id) else {
            return;
        };
        rect = self.clamp_rect_on_screen(rect);
        self.windows.set_target_rect(id, rect);
        self.apply_window_rect(id);
        self.reclamp_auto_hide(id);
    }

    /// Called when the bar layer commits; reflows windows if `bar.json` position changed.
    pub(crate) fn on_bar_layer_committed(&mut self) {
        let pos = metis_config::load_bar_config().position;
        if pos == self.last_bar_position {
            return;
        }
        tracing::info!(?pos, "edge bar position changed — reflowing windows");
        self.last_bar_position = pos;
        self.blur.position = pos;
        self.reflow_for_bar_geometry_change();
    }

    // --- Per-output desk helpers -----------------------------------------------

    /// The output with the given name, if mapped.
    pub fn output_by_name(&self, name: &str) -> Option<smithay::output::Output> {
        self.space.outputs().find(|o| o.name() == name).cloned()
    }

    /// Desk key (output name) of the primary output, falling back to any existing
    /// desk, then the empty string before any output/desk exists.
    pub fn primary_key(&self) -> String {
        self.primary_output()
            .map(|o| o.name())
            .or_else(|| self.desks.keys().next().cloned())
            .unwrap_or_default()
    }

    /// Desk for an output key, if it exists.
    pub fn desk(&self, key: &str) -> Option<&OutputDesk> {
        self.desks.get(key)
    }

    /// Desk for an output key, creating it on demand. The first desk created is
    /// the primary (seeded with widgets from `desk.json`); later ones are app-only.
    pub fn desk_mut_or_default(&mut self, key: &str) -> &mut OutputDesk {
        if !self.desks.contains_key(key) {
            let is_primary = self.desks.is_empty();
            let mut layout = self.default_layout.clone();
            if !is_primary {
                layout.tiles.retain(|t| matches!(t.kind, TileKind::App { .. }));
            }
            self.desks.insert(
                key.to_string(),
                OutputDesk {
                    layout,
                    active_workspace: 1,
                    stashed_app_tiles: std::collections::HashMap::new(),
                    layout_kind: std::collections::HashMap::new(),
                    scroll: std::collections::HashMap::new(),
                },
            );
        }
        self.desks.get_mut(key).unwrap()
    }

    /// Ensure a desk exists for `output` (called when an output is mapped).
    pub fn ensure_desk_for_output(&mut self, output: &smithay::output::Output) {
        let key = output.name();
        let _ = self.desk_mut_or_default(&key);
    }

    /// Desk key (output name) a window belongs to. Prefers its assigned `output`,
    /// then the output under its geometry, then the primary.
    pub fn desk_key_for_window(&self, id: u32) -> String {
        if let Some(name) = self.windows.output_name(id) {
            if !name.is_empty() && self.desks.contains_key(&name) {
                return name;
            }
        }
        self.output_for_window(id)
            .map(|o| o.name())
            .unwrap_or_else(|| self.primary_key())
    }

    /// Active workspace on the given output key (defaults to 1).
    pub fn active_workspace_for(&self, key: &str) -> u32 {
        self.desk(key).map(|d| d.active_workspace).unwrap_or(1)
    }

    // --- Scrolling layout -----------------------------------------------------

    /// Layout mode new workspaces start in (from `bar.json`).
    pub fn default_layout_kind(&self) -> metis_grid::LayoutKind {
        match metis_config::load_bar_config().default_layout {
            metis_config::DefaultLayout::Free => metis_grid::LayoutKind::Free,
            metis_config::DefaultLayout::Grid => metis_grid::LayoutKind::Grid,
            metis_config::DefaultLayout::Scroll => metis_grid::LayoutKind::Scroll,
        }
    }

    /// Layout mode of a specific workspace on an output (falls back to the default).
    pub fn layout_kind_for(&self, key: &str, ws: u32) -> metis_grid::LayoutKind {
        self.desk(key)
            .and_then(|d| d.layout_kind.get(&ws).copied())
            .unwrap_or_else(|| self.default_layout_kind())
    }

    /// Layout mode of the output's currently-active workspace.
    pub fn active_layout_kind(&self, key: &str) -> metis_grid::LayoutKind {
        self.layout_kind_for(key, self.active_workspace_for(key))
    }

    /// The bar-excluded usable zone for an output key, used as the scroll viewport.
    fn scroll_zone_for(&self, key: &str) -> PixelRect {
        match self.output_by_name(key) {
            Some(o) => self.window_placement_zone_for(&o),
            None => self.window_placement_zone(),
        }
    }

    /// Full (titlebar-inclusive) frames for the active scroll workspace on `key`,
    /// using the animated viewport offset (visual position during easing).
    pub(crate) fn scroll_frames_for(&self, key: &str) -> Vec<(u32, PixelRect)> {
        self.scroll_frames_at(key, false)
    }

    /// Full frames at the scroll target offset — where client surfaces are mapped.
    fn scroll_frames_placed_for(&self, key: &str) -> Vec<(u32, PixelRect)> {
        self.scroll_frames_at(key, true)
    }

    fn scroll_frames_at(&self, key: &str, placed: bool) -> Vec<(u32, PixelRect)> {
        let ws = self.active_workspace_for(key);
        if self.layout_kind_for(key, ws) != metis_grid::LayoutKind::Scroll {
            return Vec::new();
        }
        let Some(scroll) = self.desk(key).and_then(|d| d.scroll.get(&ws)) else {
            return Vec::new();
        };
        let zone = self.scroll_zone_for(key);
        let gutter = self.gutter_px as i32;
        if placed {
            scroll.layout_placed(zone, gutter)
        } else {
            scroll.layout(zone, gutter)
        }
    }

    /// Full frame for a single window when its workspace is the active scroll
    /// workspace on its output; `None` otherwise (for decorations / hit-testing).
    pub(crate) fn scroll_frame_for_window(&self, id: u32) -> Option<PixelRect> {
        let key = self.desk_key_for_window(id);
        self.scroll_frames_for(&key)
            .into_iter()
            .find(|(wid, _)| *wid == id)
            .map(|(_, rect)| rect)
    }

    /// Mapped (target-offset) frame for scroll-managed window placement.
    fn scroll_frame_placed_for_window(&self, id: u32) -> Option<PixelRect> {
        let key = self.desk_key_for_window(id);
        self.scroll_frames_placed_for(&key)
            .into_iter()
            .find(|(wid, _)| *wid == id)
            .map(|(_, rect)| rect)
    }

    /// Render-time X offset for a scroll-managed window while the viewport eases.
    pub(crate) fn scroll_render_nudge(&self, id: u32) -> i32 {
        if !self.is_active_scroll_window(id) {
            return 0;
        }
        let key = self.desk_key_for_window(id);
        let ws = self.active_workspace_for(&key);
        let Some(scroll) = self.desk(&key).and_then(|d| d.scroll.get(&ws)) else {
            return 0;
        };
        scroll.scroll_x_target - scroll.scroll_x
    }

    /// Mutable scroll state for an output's workspace, creating it on demand.
    fn scroll_state_mut(&mut self, key: &str, ws: u32) -> &mut metis_grid::ScrollState {
        self.desk_mut_or_default(key)
            .scroll
            .entry(ws)
            .or_default()
    }

    /// Recompute the scroll offset for an output's active workspace so the focused
    /// column is visible. When `animate` is true the viewport eases toward the
    /// target via render-time translation (no per-frame client reconfigure).
    fn refresh_scroll_offset(&mut self, key: &str, animate: bool) {
        let ws = self.active_workspace_for(key);
        if self.layout_kind_for(key, ws) != metis_grid::LayoutKind::Scroll {
            return;
        }
        let zone = self.scroll_zone_for(key);
        let gutter = self.gutter_px as i32;
        if let Some(scroll) = self.desk_mut_or_default(key).scroll.get_mut(&ws) {
            let target = scroll.desired_scroll_x(zone, gutter);
            scroll.set_scroll_target(target, zone, gutter);
            if !animate {
                scroll.snap_scroll();
            }
        }
    }

    /// Update the scroll viewport and remap windows to the target strip layout.
    fn apply_scroll_viewport(&mut self, key: &str, animate: bool) {
        self.refresh_scroll_offset(key, animate);
        self.reposition_scroll_windows();
        if animate {
            self.damaged = true;
            self.request_redraw();
        }
    }

    /// Re-snap every active scroll workspace after the usable zone changes.
    pub(crate) fn refresh_all_scroll_offsets(&mut self) {
        let keys: Vec<String> = self.desks.keys().cloned().collect();
        for key in keys {
            let ws = self.active_workspace_for(&key);
            if self.layout_kind_for(&key, ws) == metis_grid::LayoutKind::Scroll {
                self.refresh_scroll_offset(&key, false);
            }
        }
    }

    /// Advance scroll-strip animations on every output; returns true while any strip
    /// is still easing toward its target.
    pub fn tick_scroll_animations(&mut self) -> bool {
        let now = std::time::Instant::now();
        let dt = self
            .last_scroll_tick
            .map(|t| now.duration_since(t).as_secs_f32())
            .unwrap_or(0.016);
        self.last_scroll_tick = Some(now);

        let keys: Vec<String> = self.desks.keys().cloned().collect();
        let mut moved = false;
        for key in keys {
            let ws = self.active_workspace_for(&key);
            if self.layout_kind_for(&key, ws) != metis_grid::LayoutKind::Scroll {
                continue;
            }
            if let Some(scroll) = self.desk_mut_or_default(&key).scroll.get_mut(&ws) {
                if scroll.scroll_x != scroll.scroll_x_target {
                    moved |= scroll.advance_scroll_animation(dt);
                }
            }
        }
        if moved {
            self.request_redraw();
        }
        moved
    }

    /// Advance auto-hide titlebar slide animations. Returns true while a reveal or
    /// hide is still in progress.
    pub fn tick_titlebar_reveal_animation(&mut self) -> bool {
        const DURATION_SECS: f32 = 0.2;

        let now = std::time::Instant::now();
        let dt = self
            .last_titlebar_reveal_tick
            .map(|t| now.duration_since(t).as_secs_f32())
            .unwrap_or(0.016);
        self.last_titlebar_reveal_tick = Some(now);

        let target = if self.revealed_titlebar.is_some() {
            1.0
        } else {
            0.0
        };

        if let Some(id) = self.revealed_titlebar {
            self.titlebar_reveal_window = Some(id);
        }

        if self.titlebar_reveal_window.is_none() {
            return false;
        }

        let before = self.titlebar_reveal_progress;
        if (before - target).abs() < 0.001 {
            self.titlebar_reveal_progress = target;
            if target <= 0.0 {
                self.titlebar_reveal_window = None;
            }
            return false;
        }

        let step = dt / DURATION_SECS;
        self.titlebar_reveal_progress = if !crate::window_fx::animations_enabled() {
            target
        } else if target > before {
            (before + step).min(target)
        } else {
            (before - step).max(target)
        };

        if self.titlebar_reveal_progress <= 0.0 {
            self.titlebar_reveal_window = None;
        }

        if (self.titlebar_reveal_progress - before).abs() > f32::EPSILON {
            self.request_redraw();
            true
        } else {
            false
        }
    }

    /// Per-edge outward ripple (top, right, bottom, left) in logical px during the
    /// post-maximize wobble. Zero when the effect has finished.
    pub(crate) fn maximize_wobble_offset(&self, id: u32) -> (i32, i32) {
        const DURATION_SECS: f32 = 0.55;
        const AMP_PX: f32 = 14.0;
        const FREQ_HZ: f32 = 5.5;

        let Some(start) = self.maximize_fx_started.get(&id) else {
            return (0, 0);
        };
        let elapsed = start.elapsed().as_secs_f32();
        if elapsed >= DURATION_SECS {
            return (0, 0);
        }
        let decay = (1.0 - elapsed / DURATION_SECS).powi(2);
        let amp = AMP_PX * decay;
        let phase = elapsed * FREQ_HZ * std::f32::consts::TAU;
        (
            (amp * phase.sin()).round() as i32,
            (amp * (phase * 1.37).sin()).round() as i32,
        )
    }

    fn window_base_location(&self, id: u32) -> Option<Point<i32, Logical>> {
        let record = self.windows.get(id)?;
        if record.maximized {
            return self
                .maximized_client_geometry(id)
                .map(|(client, _)| Point::from((client.x, client.y)));
        }
        Some(Point::from((record.target_rect.x, record.target_rect.y)))
    }

    fn apply_maximize_wobble(&mut self, id: u32) {
        let (dx, dy) = self.maximize_wobble_offset(id);
        if dx == 0 && dy == 0 && !self.maximize_fx_started.contains_key(&id) {
            return;
        }
        let Some(base) = self.window_base_location(id) else {
            return;
        };
        let Some(record) = self.windows.get(id).cloned() else {
            return;
        };
        if !self
            .space
            .elements()
            .any(|w| self.windows.id_for_window(w) == Some(id))
        {
            return;
        }
        self.space.relocate_element(
            &record.window,
            Point::from((base.x + dx, base.y + dy)),
        );
        self.schedule_redraw();
    }

    fn snap_maximize_wobble(&mut self, id: u32) {
        let Some(base) = self.window_base_location(id) else {
            return;
        };
        let Some(record) = self.windows.get(id).cloned() else {
            return;
        };
        if self
            .space
            .elements()
            .any(|w| self.windows.id_for_window(w) == Some(id))
        {
            self.space.relocate_element(&record.window, base);
            self.schedule_redraw();
        }
    }

    fn start_maximize_fx(&mut self, id: u32) {
        if !crate::window_fx::animations_enabled() {
            return;
        }
        self.maximize_fx_started
            .insert(id, std::time::Instant::now());
        self.apply_maximize_wobble(id);
    }

    /// Advance post-maximize wobble animations. Returns true while any are active.
    pub fn tick_maximize_fx(&mut self) -> bool {
        const DURATION_SECS: f32 = 0.55;
        let active_ids: Vec<u32> = self.maximize_fx_started.keys().copied().collect();
        for id in active_ids {
            self.apply_maximize_wobble(id);
        }
        let ended: Vec<u32> = self
            .maximize_fx_started
            .iter()
            .filter(|(_, t)| t.elapsed().as_secs_f32() >= DURATION_SECS)
            .map(|(id, _)| *id)
            .collect();
        for id in ended {
            self.maximize_fx_started.remove(&id);
            self.snap_maximize_wobble(id);
        }
        !self.maximize_fx_started.is_empty()
    }

    pub(crate) fn window_uses_compact_overlay(&self, id: u32) -> bool {
        self.windows
            .get(id)
            .and_then(|r| r.app_id.as_deref())
            .is_some_and(crate::decoration_policy::id_uses_compact_overlay)
    }

    fn minimize_visual_bounds(&self, id: u32) -> Option<PixelRect> {
        let record = self.windows.get(id)?;
        let loc = self.space.element_location(&record.window)?;
        if let Some(frame) = self.ssd_frame_for_mapped_window(id, &record.window) {
            return Some(frame);
        }
        let geo = record.window.geometry();
        Some(PixelRect {
            x: loc.x,
            y: loc.y,
            width: geo.size.w.max(1),
            height: geo.size.h.max(1),
        })
    }

    fn genie_minimize_target(&self, anchor: &PixelRect, id: u32) -> Option<Point<i32, Logical>> {
        let output = self
            .output_for_window(id)
            .or_else(|| self.primary_output())?;
        if !self.output_has_bar(&output) {
            return None;
        }
        let cfg = metis_config::load_bar_config();
        let margin = cfg.margin_top as i32;
        let half = cfg.height as i32 / 2;
        let zone = self.placement_zone_for(&output);
        let usable = self.usable_zone_for(&output).unwrap_or(zone);
        let cx = (anchor.x + anchor.width / 2).clamp(zone.x + 40, zone.x + zone.width - 40);
        let cy = anchor.y + anchor.height / 2;
        Some(match cfg.position {
            metis_config::BarPosition::Top => Point::from((cx, usable.y - margin - half)),
            metis_config::BarPosition::Bottom => {
                Point::from((cx, zone.y + zone.height - margin - half))
            }
            metis_config::BarPosition::Left => {
                Point::from((zone.x + margin + half, cy))
            }
            metis_config::BarPosition::Right => {
                Point::from((zone.x + zone.width - margin - half, cy))
            }
        })
    }

    fn begin_minimize_genie(&mut self, id: u32) -> bool {
        if self.windows.is_minimized(id) {
            return false;
        }
        let anchor = if self.windows.get(id).is_some_and(|r| r.maximized) {
            self.maximized_client_geometry(id).map(|(c, _)| c)
        } else {
            self.minimize_visual_bounds(id)
        };
        let Some(anchor) = anchor else {
            return false;
        };
        let Some(target) = self.genie_minimize_target(&anchor, id) else {
            return false;
        };
        self.minimize_genie_fx.insert(
            id,
            crate::window_fx::MinimizeGenieFx {
                started: std::time::Instant::now(),
                anchor,
                target,
            },
        );
        self.schedule_redraw();
        true
    }

    fn tick_minimize_genie_fx(&mut self) -> bool {
        let finished: Vec<u32> = self
            .minimize_genie_fx
            .iter()
            .filter(|(_, fx)| fx.finished())
            .map(|(id, _)| *id)
            .collect();
        for id in finished {
            self.minimize_genie_fx.remove(&id);
            self.minimize_window_now(id);
        }
        !self.minimize_genie_fx.is_empty()
    }

    /// Render clip + alpha for an in-flight minimize genie, if any.
    pub(crate) fn minimize_genie_render(&self, id: u32) -> Option<(PixelRect, f32)> {
        self.minimize_genie_fx.get(&id).map(|fx| fx.frame())
    }

    pub(crate) fn is_minimize_genie_active(&self, id: u32) -> bool {
        self.minimize_genie_fx.contains_key(&id)
    }

    /// Window ids slotted on a specific (output, workspace), in tile order.
    fn app_ids_for_workspace(&self, key: &str, ws: u32) -> Vec<u32> {
        let active = self.active_workspace_for(key);
        let collect_ids = |tiles: &[metis_grid::GridTile]| -> Vec<u32> {
            tiles
                .iter()
                .filter_map(|t| match &t.kind {
                    TileKind::App { window_id: Some(wid), .. } => Some(*wid),
                    _ => None,
                })
                .collect()
        };
        self.desk(key)
            .map(|d| {
                if ws == active {
                    collect_ids(&d.layout.tiles)
                } else {
                    d.stashed_app_tiles
                        .get(&ws)
                        .map(|t| collect_ids(t))
                        .unwrap_or_default()
                }
            })
            .unwrap_or_default()
    }

    /// App windows positioned by the scroll strip (excludes free-floating clients).
    fn scroll_managed_app_ids(&self, key: &str, ws: u32) -> Vec<u32> {
        self.app_ids_for_workspace(key, ws)
            .into_iter()
            .filter(|id| !self.floating.contains(id))
            .collect()
    }

    /// True when the scroll strip lists exactly the workspace's app windows.
    fn scroll_strip_matches(app_ids: &[u32], scroll: &metis_grid::ScrollState) -> bool {
        use std::collections::HashSet;
        let strip: HashSet<u32> = scroll
            .columns
            .iter()
            .flat_map(|c| c.windows.iter().copied())
            .collect();
        let tiles: HashSet<u32> = app_ids.iter().copied().collect();
        strip == tiles
    }

    /// Window ids on a specific (output, workspace).
    fn window_ids_on_workspace(&self, key: &str, ws: u32) -> Vec<u32> {
        self.windows
            .ids()
            .into_iter()
            .filter(|id| {
                self.desk_key_for_window(*id) == key
                    && self.windows.workspace(*id).unwrap_or(1) == ws
            })
            .collect()
    }

    /// True when a window belongs to the active workspace on its output and may
    /// be mapped (minimized windows are handled separately).
    fn window_visible_on_desktop(&self, id: u32) -> bool {
        let key = self.desk_key_for_window(id);
        let ws = self.windows.workspace(id).unwrap_or(1);
        ws == self.active_workspace_for(&key)
    }

    /// Best-effort client body rect for a mapped or placed window.
    fn current_window_body_rect(&self, id: u32) -> Option<PixelRect> {
        let record = self.windows.get(id)?;
        if let Some(loc) = self.space.element_location(&record.window) {
            let size = record.window.geometry().size;
            if size.w > 0 && size.h > 0 {
                return Some(PixelRect {
                    x: loc.x,
                    y: loc.y,
                    width: size.w,
                    height: size.h,
                });
            }
        }
        self.windows
            .target_rect(id)
            .or_else(|| {
                self.rect_for_window_tile(id)
                    .map(|full| self.tile_client_rect(id, full))
            })
    }

    /// Drop grid/scroll management for a workspace — windows keep their on-screen
    /// geometry and float freely.
    fn release_workspace_to_free(&mut self, key: &str, ws: u32) {
        for id in self.window_ids_on_workspace(key, ws) {
            if let Some(body) = self.current_window_body_rect(id) {
                self.windows.set_target_rect(id, body);
            }
            self.floating.insert(id);
            self.auto_hide_titlebar.remove(&id);
            self.save_window_geometry(id);
        }
        let active = self.active_workspace_for(key);
        let desk = self.desk_mut_or_default(key);
        if ws == active {
            desk
                .layout
                .tiles
                .retain(|t| !matches!(t.kind, TileKind::App { .. }));
        } else {
            desk.stashed_app_tiles.remove(&ws);
        }
        desk.scroll.remove(&ws);
    }

    /// Pull every window on a workspace into the grid and reserve tiles.
    fn adopt_workspace_to_grid(&mut self, key: &str, ws: u32) {
        for id in self.window_ids_on_workspace(key, ws) {
            self.floating.remove(&id);
            self.ensure_app_tile_for_window(id);
        }
    }

    /// Pull every window on a workspace into the scroll strip.
    fn adopt_workspace_to_scroll(&mut self, key: &str, ws: u32) {
        for id in self.window_ids_on_workspace(key, ws) {
            self.floating.remove(&id);
            self.ensure_app_tile_for_window(id);
        }
        self.seed_scroll_state(key, ws);
    }

    /// Build or refresh the scroll strip for a workspace from its app tiles.
    fn seed_scroll_state(&mut self, key: &str, ws: u32) {
        let app_ids = self.scroll_managed_app_ids(key, ws);
        let focused = self.focused_window_id();
        let zone = self.scroll_zone_for(key);
        let gutter = self.gutter_px as i32;
        let desk = self.desk_mut_or_default(key);
        let needs_rebuild = desk.scroll.get(&ws).is_none_or(|s| {
            (s.columns.is_empty() && !app_ids.is_empty())
                || !Self::scroll_strip_matches(&app_ids, s)
        });
        if needs_rebuild {
            let mut scroll = metis_grid::ScrollState::new();
            for wid in &app_ids {
                scroll.insert_window_after_focus(*wid);
            }
            if let Some(f) = focused {
                scroll.focus_window(f);
            }
            desk.scroll.insert(ws, scroll);
        }
        if let Some(scroll) = desk.scroll.get_mut(&ws) {
            let target = scroll.desired_scroll_x(zone, gutter);
            scroll.set_scroll_target(target, zone, gutter);
            scroll.snap_scroll();
        }
    }

    /// Re-position only the windows on active scroll workspaces (used during
    /// viewport animation so we don't reconfigure every client every frame).
    pub(crate) fn reposition_scroll_windows(&mut self) {
        let keys: Vec<String> = self.desks.keys().cloned().collect();
        for key in keys {
            let ws = self.active_workspace_for(&key);
            if self.layout_kind_for(&key, ws) != metis_grid::LayoutKind::Scroll {
                continue;
            }
            for id in self.scroll_managed_app_ids(&key, ws) {
                self.apply_window_rect(id);
            }
        }
    }

    /// True when `id` belongs to the active scroll workspace on its output.
    fn is_active_scroll_window(&self, id: u32) -> bool {
        let key = self.desk_key_for_window(id);
        let ws = self.windows.workspace(id).unwrap_or(1);
        ws == self.active_workspace_for(&key)
            && self.layout_kind_for(&key, ws) == metis_grid::LayoutKind::Scroll
    }

    /// Physical-space clip rect for a scroll-managed window so its column can
    /// scroll off its own display's edge (carousel) without bleeding onto an
    /// adjacent output. Returns `None` for windows that aren't scroll-managed —
    /// those must not be clipped (e.g. a floating window dragged across outputs).
    pub(crate) fn scroll_window_clip(
        &self,
        id: u32,
        scale: impl Into<smithay::utils::Scale<f64>>,
    ) -> Option<smithay::utils::Rectangle<i32, smithay::utils::Physical>> {
        if !self.is_active_scroll_window(id) {
            return None;
        }
        let key = self.desk_key_for_window(id);
        let output = self.output_by_name(&key)?;
        let geo = self.space.output_geometry(&output)?;
        Some(geo.to_physical_precise_round(scale))
    }

    /// Resolve a border drag on scroll window `id` to the column it should resize.
    /// The right edge grows this window's column; the left edge grows the previous
    /// column (the shared border). Returns a representative window of the target
    /// column plus that column's current pixel width, or `None` when the drag isn't
    /// a horizontal resize of a scroll column (e.g. left edge of the first column).
    pub(crate) fn scroll_resize_target(
        &self,
        id: u32,
        edges: crate::grabs::ResizeEdge,
    ) -> Option<(u32, i32)> {
        use crate::grabs::ResizeEdge;
        if !self.is_active_scroll_window(id) {
            return None;
        }
        let key = self.desk_key_for_window(id);
        let ws = self.active_workspace_for(&key);
        let scroll = self.desk(&key)?.scroll.get(&ws)?;
        let ci = scroll.column_index_of(id)?;
        let target_ci = if edges.contains(ResizeEdge::RIGHT) {
            ci
        } else if edges.contains(ResizeEdge::LEFT) {
            ci.checked_sub(1)?
        } else {
            return None;
        };
        let zone = self.scroll_zone_for(&key);
        let target_window = *scroll.columns.get(target_ci)?.windows.first()?;
        Some((target_window, scroll.column_width_px(target_ci, zone)))
    }

    /// Set the pixel width of the scroll column holding `target_window` and reflow
    /// the strip so the columns to its right slide over to make room. Driven live
    /// from [`crate::grabs::ScrollResizeGrab`] during a mouse resize.
    pub(crate) fn scroll_set_column_width_px(&mut self, target_window: u32, width_px: i32) {
        let key = self.desk_key_for_window(target_window);
        let ws = self.active_workspace_for(&key);
        let zone = self.scroll_zone_for(&key);
        if let Some(scroll) = self.desk_mut_or_default(&key).scroll.get_mut(&ws) {
            if !scroll.set_column_width_px_for(target_window, width_px, zone) {
                return;
            }
        }
        self.refresh_scroll_offset(&key, false);
        self.reposition_scroll_windows();
        self.damaged = true;
        self.request_redraw();
    }

    /// Drop a window from every output's scroll state (used on destroy / move).
    fn remove_from_scroll_everywhere(&mut self, id: u32) {
        for desk in self.desks.values_mut() {
            for scroll in desk.scroll.values_mut() {
                scroll.remove_window(id);
            }
        }
    }

    /// Set the layout mode of a specific (output, workspace) without repositioning.
    /// Entering scroll seeds the strip from that workspace's app tiles (visible or
    /// stashed); leaving scroll drops the strip and de-overlaps the visible grid.
    fn set_layout_kind_on(&mut self, key: &str, ws: u32, kind: metis_grid::LayoutKind) {
        let active = self.active_workspace_for(key);
        if self.layout_kind_for(key, ws) == kind {
            // Pin an explicit entry so a later default change can't silently flip it.
            self.desk_mut_or_default(key).layout_kind.insert(ws, kind);
            // Still sync backing state. When bar.json already matches the target
            // (e.g. SetDefaultLayout after saving the dropdown), the early return
            // used to skip seeding scroll strips entirely.
            match kind {
                metis_grid::LayoutKind::Scroll => self.seed_scroll_state(key, ws),
                metis_grid::LayoutKind::Grid => {
                    let desk = self.desk_mut_or_default(key);
                    desk.scroll.remove(&ws);
                    if ws == active {
                        metis_grid::sanitize_layout(&mut desk.layout);
                    }
                }
                metis_grid::LayoutKind::Free => {
                    self.desk_mut_or_default(key).scroll.remove(&ws);
                }
            }
            return;
        }

        match kind {
            metis_grid::LayoutKind::Scroll => {
                self.seed_scroll_state(key, ws);
                self.desk_mut_or_default(key).layout_kind.insert(ws, kind);
            }
            metis_grid::LayoutKind::Grid => {
                let desk = self.desk_mut_or_default(key);
                desk.layout_kind.insert(ws, kind);
                desk.scroll.remove(&ws);
                if ws == active {
                    metis_grid::sanitize_layout(&mut desk.layout);
                }
            }
            metis_grid::LayoutKind::Free => {
                let desk = self.desk_mut_or_default(key);
                desk.layout_kind.insert(ws, kind);
                desk.scroll.remove(&ws);
            }
        }
    }

    /// Set the layout mode of an output's active workspace and apply it live.
    pub fn set_layout_kind(&mut self, key: &str, kind: metis_grid::LayoutKind) {
        let ws = self.active_workspace_for(key);
        if self.layout_kind_for(key, ws) == kind {
            return;
        }
        let focused = self.focused_window_id();
        self.set_layout_kind_on(key, ws, kind);
        match kind {
            metis_grid::LayoutKind::Grid => {
                self.adopt_workspace_to_grid(key, ws);
                self.auto_reflow_grid_apps(key, focused, false);
            }
            metis_grid::LayoutKind::Scroll => {
                self.adopt_workspace_to_scroll(key, ws);
                self.refresh_scroll_offset(key, false);
                self.reposition_scroll_windows();
            }
            metis_grid::LayoutKind::Free => {
                self.release_workspace_to_free(key, ws);
                self.reposition_all_windows();
                self.persist_layout();
            }
        }
        if let Some(f) = focused {
            self.focus_window_id(f);
        }
        self.damaged = true;
        self.request_redraw();
        self.emit_layout_changed();
    }

    /// Apply a layout mode to every workspace on every output at once, so the
    /// settings "New workspace layout" default behaves as a live global on/off.
    pub fn set_layout_kind_all(&mut self, kind: metis_grid::LayoutKind) {
        let count = self.workspace_count();
        let keys: Vec<String> = self.desks.keys().cloned().collect();
        let focused = self.focused_window_id();
        for key in &keys {
            for ws in 1..=count {
                self.set_layout_kind_on(key, ws, kind);
            }
            match kind {
                metis_grid::LayoutKind::Free => {
                    for ws in 1..=count {
                        self.release_workspace_to_free(key, ws);
                    }
                }
                metis_grid::LayoutKind::Grid => {
                    for ws in 1..=count {
                        self.adopt_workspace_to_grid(key, ws);
                    }
                    self.auto_reflow_grid_apps(key, focused, false);
                }
                metis_grid::LayoutKind::Scroll => {
                    for ws in 1..=count {
                        self.adopt_workspace_to_scroll(key, ws);
                    }
                    self.refresh_scroll_offset(key, false);
                }
            }
        }
        self.reposition_all_windows();
        if let Some(f) = focused {
            self.focus_window_id(f);
        }
        self.damaged = true;
        self.request_redraw();
        self.emit_layout_changed();
    }

    /// Turn on grid tiling for the active workspace under `key`.
    pub fn enable_grid_tiling(&mut self, key: &str) {
        const DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(500);
        let now = std::time::Instant::now();
        if self
            .last_layout_toggle
            .is_some_and(|t| now.duration_since(t) < DEBOUNCE)
        {
            return;
        }
        self.last_layout_toggle = Some(now);
        if self.active_layout_kind(key) == metis_grid::LayoutKind::Grid {
            return;
        }
        tracing::info!(output = key, "enable_grid_tiling");
        self.set_layout_kind(key, metis_grid::LayoutKind::Grid);
    }

    /// Return the active workspace to a normal floating desktop (grid/scroll off).
    pub fn disable_grid_tiling(&mut self, key: &str) {
        const DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(500);
        let now = std::time::Instant::now();
        if self
            .last_layout_toggle
            .is_some_and(|t| now.duration_since(t) < DEBOUNCE)
        {
            return;
        }
        self.last_layout_toggle = Some(now);
        if self.active_layout_kind(key) == metis_grid::LayoutKind::Free {
            return;
        }
        tracing::info!(output = key, "disable_grid_tiling");
        self.set_layout_kind(key, metis_grid::LayoutKind::Free);
    }

    /// Cycle the active workspace: free desktop → grid tiling → scrolling.
    pub fn toggle_layout_kind(&mut self, key: &str) {
        const DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(500);
        let now = std::time::Instant::now();
        if self
            .last_layout_toggle
            .is_some_and(|t| now.duration_since(t) < DEBOUNCE)
        {
            return;
        }
        self.last_layout_toggle = Some(now);

        let next = match self.active_layout_kind(key) {
            metis_grid::LayoutKind::Free => metis_grid::LayoutKind::Grid,
            metis_grid::LayoutKind::Grid => metis_grid::LayoutKind::Scroll,
            metis_grid::LayoutKind::Scroll => metis_grid::LayoutKind::Free,
        };
        tracing::info!(output = key, ?next, "toggle_layout_kind");
        self.set_layout_kind(key, next);
    }

    /// Give a window keyboard focus and raise it (mirrors `activate_window`'s tail).
    pub fn focus_window_id(&mut self, id: u32) {
        let Some(record) = self.windows.get(id).cloned() else {
            return;
        };
        self.note_window_focus(id);
        self.space.raise_element(&record.window, true);
        if self.focused_window_id() == Some(id) {
            return;
        }
        if let Some(keyboard) = self.seat.get_keyboard() {
            let serial = smithay::utils::SERIAL_COUNTER.next_serial();
            keyboard.set_focus(self, Some(record.window.clone().into()), serial);
            self.event_bus
                .emit(&metis_protocol::CompositorEvent::WindowFocused { id });
        }
    }

    /// Apply a scroll action to the output under the pointer's active scroll
    /// workspace, then reposition and refocus. No-op unless that workspace is in
    /// scroll mode.
    fn with_active_scroll<F: FnOnce(&mut metis_grid::ScrollState)>(&mut self, f: F) -> bool {
        let key = self
            .output_under_pointer()
            .map(|o| o.name())
            .unwrap_or_else(|| self.primary_key());
        let ws = self.active_workspace_for(&key);
        if self.layout_kind_for(&key, ws) != metis_grid::LayoutKind::Scroll {
            return false;
        }
        {
            let scroll = self.scroll_state_mut(&key, ws);
            f(scroll);
        }
        self.apply_scroll_viewport(&key, true);
        let focused = self
            .desk(&key)
            .and_then(|d| d.scroll.get(&ws))
            .and_then(|s| s.focused_window());
        if let Some(f) = focused {
            self.focus_window_id(f);
        }
        self.damaged = true;
        self.request_redraw();
        true
    }

    /// Keep the scroll strip's focused column aligned with keyboard focus.
    pub fn sync_scroll_focus_for_window(&mut self, id: u32) {
        let key = self.desk_key_for_window(id);
        let ws = self.windows.workspace(id).unwrap_or(1);
        if ws != self.active_workspace_for(&key) {
            return;
        }
        if self.layout_kind_for(&key, ws) != metis_grid::LayoutKind::Scroll {
            return;
        }
        let changed = self
            .desk_mut_or_default(&key)
            .scroll
            .get_mut(&ws)
            .map(|scroll| {
                let before = scroll.focused_window();
                scroll.focus_window(id);
                before != scroll.focused_window()
            })
            .unwrap_or(false);
        if changed {
            self.apply_scroll_viewport(&key, true);
        }
    }

    pub fn scroll_focus_left(&mut self) -> bool {
        self.with_active_scroll(|s| s.focus_left())
    }
    pub fn scroll_focus_right(&mut self) -> bool {
        self.with_active_scroll(|s| s.focus_right())
    }
    pub fn scroll_focus_up(&mut self) -> bool {
        self.with_active_scroll(|s| s.focus_up())
    }
    pub fn scroll_focus_down(&mut self) -> bool {
        self.with_active_scroll(|s| s.focus_down())
    }
    pub fn scroll_move_left(&mut self) -> bool {
        self.with_active_scroll(|s| s.move_column_left())
    }
    pub fn scroll_move_right(&mut self) -> bool {
        self.with_active_scroll(|s| s.move_column_right())
    }
    pub fn scroll_move_up(&mut self) -> bool {
        self.with_active_scroll(|s| s.move_window_up())
    }
    pub fn scroll_move_down(&mut self) -> bool {
        self.with_active_scroll(|s| s.move_window_down())
    }
    pub fn scroll_consume(&mut self) -> bool {
        self.with_active_scroll(|s| s.consume_into_prev())
    }
    pub fn scroll_expel(&mut self) -> bool {
        self.with_active_scroll(|s| s.expel_to_new_column())
    }
    pub fn scroll_cycle_width(&mut self) -> bool {
        self.with_active_scroll(|s| s.cycle_focus_width())
    }

    /// Grid metrics (columns/rows/gutter + monitor rect) for a specific output.
    pub fn grid_metrics_for(&self, output: &smithay::output::Output) -> GridMetrics {
        let key = output.name();
        let (columns, rows) = self
            .desk(&key)
            .map(|d| (d.layout.columns, d.layout.rows))
            .unwrap_or((self.default_layout.columns, self.default_layout.rows));
        let zone = self.grid_placement_zone_for(output);
        GridMetrics {
            columns,
            rows,
            gutter: self.gutter_px,
            monitor: MonitorRect {
                x: zone.x,
                y: zone.y,
                width: zone.width,
                height: zone.height,
            },
        }
    }

    /// Usable desktop band for grid tiling on `output` (below/ beside the edge bar).
    fn grid_placement_zone_for(&self, output: &smithay::output::Output) -> PixelRect {
        let mut zone = self.window_placement_zone_for(output);
        if !self.output_has_bar(output) {
            return zone;
        }
        let Some(output_geo) = self.output_rect(output) else {
            return zone;
        };
        let reserve = Self::bar_reserved_px();
        let gaps = self.zone_edge_gaps();
        match metis_config::load_bar_config().position {
            metis_config::BarPosition::Top => {
                let min_y = output_geo.y + reserve + gaps.top;
                if zone.y < min_y {
                    let delta = min_y - zone.y;
                    zone.y = min_y;
                    zone.height = (zone.height - delta).max(1);
                }
            }
            metis_config::BarPosition::Bottom => {
                zone.height = (zone.height - gaps.bottom).max(1);
            }
            metis_config::BarPosition::Left => {
                let min_x = output_geo.x + reserve + gaps.left;
                if zone.x < min_x {
                    let delta = min_x - zone.x;
                    zone.x = min_x;
                    zone.width = (zone.width - delta).max(1);
                }
            }
            metis_config::BarPosition::Right => {
                zone.width = (zone.width - gaps.right).max(1);
            }
        }
        zone
    }

    /// Hide persistent titlebars for grid-tiled windows; reveal on hover.
    fn sync_grid_titlebar_chrome(&mut self, output_key: &str) {
        let ws = self.active_workspace_for(output_key);
        if self.layout_kind_for(output_key, ws) != metis_grid::LayoutKind::Grid {
            return;
        }
        for id in self.window_ids_on_workspace(output_key, ws) {
            if self.tile_id_for_window(id).is_some()
                && !self.floating.contains(&id)
                && self.should_auto_hide_titlebar(id)
            {
                self.auto_hide_titlebar.insert(id);
            } else {
                self.auto_hide_titlebar.remove(&id);
            }
        }
    }

    /// Grid metrics for the primary output (back-compat for output-agnostic call sites).
    pub fn grid_metrics(&self) -> GridMetrics {
        match self.primary_output() {
            Some(o) => self.grid_metrics_for(&o),
            None => GridMetrics {
                columns: self.default_layout.columns,
                rows: self.default_layout.rows,
                gutter: self.gutter_px,
                monitor: self.monitor,
            },
        }
    }

    /// Find the app tile for `window_id` across all outputs' visible layouts,
    /// returning its output key and a clone of the tile.
    pub fn find_app_tile(&self, window_id: u32) -> Option<(String, metis_grid::GridTile)> {
        for (key, desk) in &self.desks {
            for tile in &desk.layout.tiles {
                if let TileKind::App { window_id: Some(wid), .. } = &tile.kind {
                    if *wid == window_id {
                        return Some((key.clone(), tile.clone()));
                    }
                }
            }
        }
        None
    }

    /// Output key whose visible layout currently contains `tile_id`.
    pub fn desk_key_for_tile(&self, tile_id: &str) -> Option<String> {
        self.desks.iter().find_map(|(key, desk)| {
            desk.layout
                .tiles
                .iter()
                .any(|t| t.id == tile_id)
                .then(|| key.clone())
        })
    }

    /// Drop app tiles whose window no longer exists and dedupe multiple tiles for
    /// the same live window (stale `desk.json` entries otherwise block reflow).
    fn prune_stale_app_tiles(&mut self, output_key: &str) {
        use std::collections::{HashMap, HashSet};

        let live: HashSet<u32> = self.windows.ids().into_iter().collect();
        let desk = self.desk_mut_or_default(output_key);

        let prune_list = |tiles: &mut Vec<metis_grid::GridTile>| {
            tiles.retain(|t| match &t.kind {
                TileKind::App { window_id: Some(wid), .. } => live.contains(wid),
                TileKind::App { window_id: None, .. } => false,
                _ => true,
            });
            let mut keep: HashMap<u32, String> = HashMap::new();
            for t in tiles.iter() {
                let TileKind::App {
                    window_id: Some(wid),
                    ..
                } = &t.kind
                else {
                    continue;
                };
                let canonical = format!("app-{wid}");
                keep.entry(*wid).or_insert_with(|| t.id.clone());
                if t.id == canonical {
                    keep.insert(*wid, canonical);
                }
            }
            tiles.retain(|t| match &t.kind {
                TileKind::App { window_id: Some(wid), .. } => keep.get(wid) == Some(&t.id),
                _ => true,
            });
        };

        prune_list(&mut desk.layout.tiles);
        for tiles in desk.stashed_app_tiles.values_mut() {
            prune_list(tiles);
        }
    }

    /// Drop a window's app tile from every desk (visible and stashed).
    fn remove_app_tile_everywhere(&mut self, window_id: u32) {
        let matches_window = |t: &metis_grid::GridTile| {
            matches!(&t.kind, TileKind::App { window_id: Some(wid), .. } if *wid == window_id)
        };
        for desk in self.desks.values_mut() {
            desk.layout.tiles.retain(|t| !matches_window(t));
            for tiles in desk.stashed_app_tiles.values_mut() {
                tiles.retain(|t| !matches_window(t));
            }
            for scroll in desk.scroll.values_mut() {
                scroll.remove_window(window_id);
            }
        }
    }

    pub fn focused_window_id(&self) -> Option<u32> {
        let focus = self.seat.get_keyboard()?.current_focus()?;
        match focus {
            KeyboardFocusTarget::Window(window) => {
                self.windows.id_for_surface(window.toplevel()?.wl_surface())
            }
            _ => None,
        }
    }

    pub(crate) fn note_window_focus(&mut self, id: u32) {
        self.last_focused_window = Some(id);
    }

    /// Window the user last brought forward (taskbar, Alt+Tab path, etc.), falling
    /// back to live keyboard focus. Taskbar picks beat transient bar-layer focus.
    fn preferred_stacking_window(&self) -> Option<u32> {
        self.last_focused_window.or(self.focused_window_id())
    }

    fn raise_stacking_window(&mut self, id: u32, activate: bool) {
        let Some(record) = self.windows.get(id).cloned() else {
            return;
        };
        self.space.raise_element(&record.window, activate);
    }

    pub fn close_window(&mut self, id: u32) {
        self.maximize_fx_started.remove(&id);
        self.minimize_genie_fx.remove(&id);
        if let Some(record) = self.windows.get(id).cloned() {
            record.toplevel.send_close();
        }
    }

    pub fn set_fullscreen(&mut self, id: u32, enabled: bool) {
        use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
        use smithay::reexports::wayland_server::Resource;

        let Some(record) = self.windows.get(id).cloned() else {
            return;
        };
        let output = self.primary_output();
        let wl_surface = record.toplevel.wl_surface().clone();

        if enabled {
            if let Some(output) = output {
                let geo = self.space.output_geometry(&output).unwrap();
                let wl_output = self
                    .display_handle
                    .get_client(wl_surface.id())
                    .ok()
                    .and_then(|client| output.client_outputs(&client).next());
                record.toplevel.with_pending_state(|state| {
                    state.states.set(xdg_toplevel::State::Fullscreen);
                    state.size = Some(geo.size);
                    state.fullscreen_output = wl_output;
                });
                self.space
                    .map_element(record.window.clone(), geo.loc, true);
            }
        } else {
            record.toplevel.with_pending_state(|state| {
                state.states.unset(xdg_toplevel::State::Fullscreen);
                state.fullscreen_output = None;
            });
            self.apply_window_rect(id);
        }

        record.toplevel.send_pending_configure();
        self.windows.set_fullscreen(id, enabled);
        if enabled {
            self.windows.set_maximized(id, false);
            self.clear_auto_hide(id);
            self.focus_window_id(id);
        }
        self.schedule_redraw();
    }

    pub fn set_maximized(&mut self, id: u32, enabled: bool) {
        use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;

        let Some(record) = self.windows.get(id).cloned() else {
            return;
        };

        if self.windows.is_minimized(id) {
            self.unminimize_window(id);
        }

        if enabled {
            // Mark maximized before any nested layout/configure work so bulk
            // `apply_window_rect` passes cannot reposition this window back into
            // its grid tile mid-transition.
            self.windows.set_maximized(id, true);

            let current = self
                .window_body_rect(id)
                .unwrap_or(record.target_rect);
            self.windows.set_restore_rect(id, current);

            let Some((client, client_size)) = self.maximized_client_geometry(id) else {
                return;
            };

            record.toplevel.with_pending_state(|state| {
                state.states.unset(xdg_toplevel::State::Fullscreen);
                state.states.set(xdg_toplevel::State::Maximized);
                state.size = Some(client_size);
                state.fullscreen_output = None;
            });
            self.space
                .map_element(record.window.clone(), Point::from((client.x, client.y)), true);
            self.windows.set_rect(id, client);
            if self.should_auto_hide_titlebar(id) {
                self.auto_hide_titlebar.insert(id);
                self.reclamp_auto_hide(id);
            } else {
                self.clear_auto_hide(id);
            }
            self.windows.set_snapped(id, true);
            self.start_maximize_fx(id);
        } else {
            self.demote_maximized(id);
            // Match the maximize path: remapping with `activate: true` forces the
            // freshly restored window above any neighbor that kept a stale
            // full-screen stack slot after `relocate_element`-only demotion.
            if let Some(record) = self.windows.get(id).cloned() {
                if let Some(loc) = self.space.element_location(&record.window) {
                    self.space.map_element(record.window.clone(), loc, true);
                }
            }
        }

        record.toplevel.send_pending_configure();
        self.focus_window_id(id);
        self.restore_focus_stacking();
        self.schedule_redraw();
    }

    /// Client body rect + configure for a window already marked maximized. Does
    /// not change focus or re-raise unless the caller does so afterward.
    fn reapply_maximized_geometry(&mut self, id: u32) {
        use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;

        let Some(record) = self.windows.get(id).cloned() else {
            return;
        };
        if !record.maximized {
            return;
        }
        if self.maximize_fx_started.contains_key(&id) {
            return;
        }
        let Some((client, client_size)) = self.maximized_client_geometry(id) else {
            return;
        };
        record.toplevel.with_pending_state(|state| {
            state.states.unset(xdg_toplevel::State::Fullscreen);
            state.states.set(xdg_toplevel::State::Maximized);
            state.size = Some(client_size);
            state.fullscreen_output = None;
        });
        let loc = Point::from((client.x, client.y));
        let mapped = self
            .space
            .elements()
            .any(|w| self.windows.id_for_window(w) == Some(id));
        if mapped {
            self.space.relocate_element(&record.window, loc);
        } else {
            self.space.map_element(record.window.clone(), loc, false);
        }
        self.windows.set_rect(id, client);
        if self.should_auto_hide_titlebar(id) {
            self.auto_hide_titlebar.insert(id);
            self.reclamp_auto_hide(id);
        } else {
            self.clear_auto_hide(id);
        }
        record.toplevel.send_pending_configure();
    }

    /// Usable-zone footprint for a maximized window on its current output.
    fn maximized_client_geometry(&self, id: u32) -> Option<(PixelRect, Size<i32, Logical>)> {
        let zone = match self.output_for_window(id) {
            Some(output) => self.window_placement_zone_for(&output),
            None => self.window_placement_zone(),
        };
        let gaps = self.zone_edge_gaps();
        let full = PixelRect {
            x: zone.x + gaps.left,
            y: zone.y + gaps.top,
            width: (zone.width - gaps.left - gaps.right).max(1),
            height: (zone.height - gaps.top - gaps.bottom).max(1),
        };
        let client = if self.window_uses_ssd(id) {
            self.ssd_client_rect(id, full)
        } else {
            full
        };
        let client_size = Size::from((client.width.max(1), client.height.max(1)));
        Some((client, client_size))
    }

    /// Drop a window out of maximized mode without stealing focus (internal).
    fn demote_maximized(&mut self, id: u32) {
        use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;

        let Some(record) = self.windows.get(id).cloned() else {
            return;
        };
        if !record.maximized {
            return;
        }
        // Clear before `apply_window_rect` — while `maximized` is still true that
        // path returns immediately and the window stays at its maximized map origin
        // (often tucked under the edge bar once chrome is restored).
        self.windows.set_maximized(id, false);
        record.toplevel.with_pending_state(|state| {
            state.states.unset(xdg_toplevel::State::Maximized);
            state.size = None;
        });
        self.clear_tiled_states(id);
        self.clear_auto_hide(id);
        self.windows.set_snapped(id, false);
        if self.tile_id_for_window(id).is_some() {
            // Grid apps return to their tile instead of staying ad-hoc floating.
            self.floating.remove(&id);
        } else {
            self.floating.insert(id);
            if let Some(restore) = self.windows.take_restore_rect(id) {
                let restore = self.recover_offscreen_rect(restore);
                let restore = if self.should_draw_metis_ssd(id) {
                    self.clamp_body_below_bar(restore)
                } else {
                    self.clamp_floating_rect_for(id, restore)
                };
                self.windows.set_target_rect(id, restore);
            }
        }
        self.apply_window_rect(id);
        self.start_maximize_fx(id);
    }

    pub fn minimize_window(&mut self, id: u32) {
        if self.minimize_genie_fx.contains_key(&id) {
            return;
        }
        if crate::window_fx::animations_enabled() && self.begin_minimize_genie(id) {
            return;
        }
        self.minimize_window_now(id);
    }

    fn minimize_window_now(&mut self, id: u32) {
        use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;

        let Some(record) = self.windows.get(id).cloned() else {
            return;
        };

        record.toplevel.with_pending_state(|state| {
            state.states.unset(xdg_toplevel::State::Maximized);
            state.states.unset(xdg_toplevel::State::Fullscreen);
            state.size = None;
            state.fullscreen_output = None;
        });
        record.toplevel.send_pending_configure();

        self.space.unmap_elem(&record.window);
        self.windows.set_minimized(id, true);
        self.windows.set_maximized(id, false);
        self.windows.set_fullscreen(id, false);
        self.clear_auto_hide(id);

        if self.focused_window_id() == Some(id) {
            let serial = smithay::utils::SERIAL_COUNTER.next_serial();
            self.seat
                .get_keyboard()
                .unwrap()
                .set_focus(self, Option::<KeyboardFocusTarget>::None, serial);
        }
        self.event_bus.emit(&metis_protocol::CompositorEvent::WindowMinimized {
            id,
            minimized: true,
        });
    }

    fn unminimize_window(&mut self, id: u32) {
        self.windows.set_minimized(id, false);
        self.apply_window_rect(id);
        if self.preferred_stacking_window() == Some(id) {
            self.raise_stacking_window(id, true);
        }
        self.event_bus.emit(&metis_protocol::CompositorEvent::WindowMinimized {
            id,
            minimized: false,
        });
    }

    /// Minimize a window by id, routing grid tiles through `set_tile_mode` (so the
    /// tile's mode stays consistent) and floating windows directly. Mirrors the
    /// decoration minimize button.
    pub fn minimize_by_id(&mut self, id: u32) {
        if let Some(tile_id) = self.tile_id_for_window(id) {
            self.set_tile_mode(&tile_id, metis_protocol::TileMode::Minimized);
        } else {
            self.minimize_window(id);
        }
    }

    /// Restore a window by id (grid tiles back to Grid mode, floating windows via
    /// `unminimize_window`).
    pub fn restore_by_id(&mut self, id: u32) {
        if !self.windows.is_minimized(id) {
            return;
        }
        if let Some(tile_id) = self.tile_id_for_window(id) {
            self.set_tile_mode(&tile_id, metis_protocol::TileMode::Grid);
        } else {
            self.unminimize_window(id);
        }
    }

    /// Bring a window to the foreground: restore if minimized, raise, and focus.
    pub fn activate_window_by_id(&mut self, id: u32) {
        self.note_window_focus(id);
        let key = self.desk_key_for_window(id);
        let ws = self.windows.workspace(id).unwrap_or(1);
        if ws != self.active_workspace_for(&key) {
            self.switch_workspace(&key, ws);
        }
        self.restore_by_id(id);
        self.ensure_app_tile_for_window(id);
        self.remap_window_for_desktop(id);
        let Some(record) = self.windows.get(id).cloned() else {
            return;
        };
        self.raise_stacking_window(id, true);
        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
        self.seat
            .get_keyboard()
            .unwrap()
            .set_focus(self, Some(record.window.clone().into()), serial);
        self.event_bus
            .emit(&metis_protocol::CompositorEvent::WindowFocused { id });
        self.restore_focus_stacking();
        self.schedule_redraw();
    }

    /// Map, unmap, or refresh a window for the active workspace on its output.
    /// Grid tiles, floating geometry, maximize, and fullscreen each have their
    /// own placement path; hidden workspaces always unmap.
    fn remap_window_for_desktop(&mut self, id: u32) {
        let Some(record) = self.windows.get(id).cloned() else {
            return;
        };
        if self.windows.is_minimized(id) {
            return;
        }
        if !self.window_visible_on_desktop(id) {
            if self
                .space
                .elements()
                .any(|w| self.windows.id_for_window(w) == Some(id))
            {
                self.space.unmap_elem(&record.window);
                self.schedule_redraw();
            }
            return;
        }
        if record.maximized {
            self.reapply_maximized_geometry(id);
            return;
        }
        if record.fullscreen {
            self.reapply_fullscreen_geometry(id);
            return;
        }
        self.apply_window_rect(id);
    }

    fn reapply_fullscreen_geometry(&mut self, id: u32) {
        use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
        use smithay::reexports::wayland_server::Resource;

        let Some(record) = self.windows.get(id).cloned() else {
            return;
        };
        if !record.fullscreen {
            return;
        }
        let Some(output) = self
            .output_for_window(id)
            .or_else(|| self.primary_output())
        else {
            return;
        };
        let Some(geo) = self.space.output_geometry(&output) else {
            return;
        };
        let wl_surface = record.toplevel.wl_surface().clone();
        let wl_output = self
            .display_handle
            .get_client(wl_surface.id())
            .ok()
            .and_then(|client| output.client_outputs(&client).next());
        record.toplevel.with_pending_state(|state| {
            state.states.set(xdg_toplevel::State::Fullscreen);
            state.size = Some(geo.size);
            state.fullscreen_output = wl_output;
        });
        let mapped = self
            .space
            .elements()
            .any(|w| self.windows.id_for_window(w) == Some(id));
        if mapped {
            self.space.relocate_element(&record.window, geo.loc);
        } else {
            self.space
                .map_element(record.window.clone(), geo.loc, false);
        }
        record.toplevel.send_pending_configure();
        self.schedule_redraw();
    }

    pub fn apply_window_rect(&mut self, id: u32) {
        let Some(record) = self.windows.get(id).cloned() else {
            return;
        };
        if let Some(toplevel) = record.window.toplevel() {
            if crate::grabs::resize_grab::surface_is_interactively_resizing(toplevel.wl_surface()) {
                return;
            }
        }
        // Never (re)map a minimized window. Restoring goes through
        // `unminimize_window`, which clears the flag *before* calling this. Without
        // this guard a bulk `reposition_all_windows` (triggered when restoring a
        // single grid tile) would re-map and un-minimize *every* minimized window.
        if self.windows.is_minimized(id) {
            return;
        }
        if !self.window_visible_on_desktop(id) {
            if self
                .space
                .elements()
                .any(|w| self.windows.id_for_window(w) == Some(id))
            {
                self.space.unmap_elem(&record.window);
                self.schedule_redraw();
            }
            return;
        }
        // Maximized / fullscreen geometry is owned by `set_maximized` /
        // `set_fullscreen` / `reapply_*`. Bulk tile passes must not snap these
        // back to their grid slot.
        if record.maximized || record.fullscreen {
            return;
        }
        // Floating windows keep their free geometry (only recovered if they'd
        // land off every active output); grid windows snap to their tile.
        let rect = if self.floating.contains(&id) {
            // Auto-hide (snapped/maximized) windows map flush under the bar; only
            // ordinary floating windows reserve the titlebar strip above the body.
            let auto_hide = self.auto_hide_titlebar.contains(&id);
            self.windows.target_rect(id).map(|r| {
                let r = self.recover_offscreen_rect(r);
                if auto_hide {
                    r
                } else if self.should_draw_metis_ssd(id) {
                    self.clamp_body_below_bar(r)
                } else {
                    self.clamp_floating_rect_for(id, r)
                }
            })
        } else {
            self.rect_for_window_tile(id).and_then(|full| {
                let body = self.tile_client_rect(id, full);
                if self.is_active_scroll_window(id) {
                    let key = self.desk_key_for_window(id);
                    let zone = self.scroll_zone_for(&key);
                    if !body.intersects(&zone) {
                        return None;
                    }
                }
                Some(body)
            })
        };
        let Some(rect) = rect else {
            if self
                .space
                .elements()
                .any(|w| self.windows.id_for_window(w) == Some(id))
            {
                self.space.unmap_elem(&record.window);
                self.schedule_redraw();
            }
            return;
        };
        let mapped = self
            .space
            .elements()
            .any(|w| self.windows.id_for_window(w) == Some(id));
        let loc = Point::from((rect.x, rect.y));
        let width = rect.width.max(1);
        let height = rect.height.max(1);
        let size = Size::from((width, height));
        if mapped {
            let prev_loc = self.space.element_location(&record.window);
            let unchanged = prev_loc == Some(loc)
                && record.target_rect == rect
                && (record.window.geometry().size == size
                    || self.floating.contains(&id)
                    || self.is_active_scroll_window(id));
            if unchanged {
                return;
            }
            // `map_element` always inserts at the top of the stack, even with
            // `activate: false`, so routine layout sync must relocate in place
            // instead of unmap/remap — otherwise a grid reflow raises every
            // repositioned window above a maximized or focused one.
            if prev_loc != Some(loc) {
                self.space.relocate_element(&record.window, loc);
            }
            record.toplevel.with_pending_state(|state| {
                state.size = Some(size);
            });
            record.toplevel.send_pending_configure();
            self.windows.set_target_rect(id, rect);
            self.reclamp_auto_hide(id);
            self.schedule_redraw();
            return;
        }
        // First map — insert without stealing keyboard activation.
        self.space.map_element(record.window.clone(), loc, false);
        record.toplevel.with_pending_state(|state| {
            state.size = Some(size);
        });
        record.toplevel.send_pending_configure();
        self.windows.set_target_rect(id, rect);
        // An auto-hide (maximized / edge-snapped) window may refuse to shrink to
        // its footprint; re-anchor it so the screen-edge gap survives.
        self.reclamp_auto_hide(id);
    }

    /// Keep an auto-hide (maximized / edge-snapped) window pinned to its snapped
    /// edge so the screen-edge gap survives even when the client refuses to
    /// shrink to its footprint (e.g. an app whose minimum width is wider than the
    /// snap zone on a small display). The footprint (`target_rect`) encodes the
    /// desired gaps; if the committed size is larger we re-anchor the window to
    /// the edge the footprint hugs so the overflow spills toward screen center
    /// instead of off the screen edge.
    pub fn reclamp_auto_hide(&mut self, id: u32) {
        if !self.auto_hide_titlebar.contains(&id) {
            return;
        }
        // Post-maximize wobble temporarily offsets the map origin; reclamp would
        // snap it back every client commit and kill the animation.
        if self.maximize_fx_started.contains_key(&id) {
            return;
        }
        let Some(record) = self.windows.get(id).cloned() else {
            return;
        };
        let Some(foot) = self.windows.target_rect(id) else {
            return;
        };
        let Some(loc) = self.space.element_location(&record.window) else {
            return;
        };
        let size = record.window.geometry().size;
        if size.w <= 0 || size.h <= 0 {
            return;
        }
        // Anchor against the output the window sits on, not always the primary —
        // otherwise an auto-hide (maximized / edge-snapped) window on a secondary
        // monitor gets dragged back toward the primary output's zone.
        let zone = match self.output_for_window(id) {
            Some(output) => self.window_placement_zone_for(&output),
            None => self.window_placement_zone(),
        };
        let gaps = self.zone_edge_gaps();
        let pos = metis_config::load_bar_config().position;
        let y_anchor = match pos {
            metis_config::BarPosition::Bottom => Some(BarEdgeAnchor::Max),
            metis_config::BarPosition::Top => Some(BarEdgeAnchor::Min),
            _ => None,
        };
        let x_anchor = match pos {
            metis_config::BarPosition::Left => Some(BarEdgeAnchor::Min),
            metis_config::BarPosition::Right => Some(BarEdgeAnchor::Max),
            _ => None,
        };
        let new_x = anchor_axis(
            foot.x,
            foot.width,
            zone.x,
            zone.width,
            size.w,
            gaps.left,
            gaps.right,
            x_anchor,
        );
        let new_y = anchor_axis(
            foot.y,
            foot.height,
            zone.y,
            zone.height,
            size.h,
            gaps.top,
            gaps.bottom,
            y_anchor,
        );
        if new_x != loc.x || new_y != loc.y {
            self.space
                .relocate_element(&record.window, Point::from((new_x, new_y)));
            self.schedule_redraw();
        }
    }

    /// Collect decoration specs (frame, title, focus) for every mapped, ready
    /// window that should be decorated. The frame is derived from the window's
    /// *actual* mapped geometry (so chrome tracks tiled, floating, and maximized
    /// windows alike). Fullscreen and minimized windows are skipped.
    pub fn decoration_specs(&self) -> Vec<crate::decoration::WindowDeco> {
        let focused = self.focused_window_id();
        let mut specs = Vec::new();
        for id in self.windows.ids() {
            let Some(record) = self.windows.get(id) else {
                continue;
            };
            if record.fullscreen || self.windows.is_minimized(id) {
                continue;
            }
            if self.is_minimize_genie_active(id) {
                continue;
            }
            if !self.should_draw_metis_ssd(id) {
                continue;
            }
            // Gate on the window actually being mapped in the space with real
            // geometry rather than the `ready` flag: floating windows can be mapped
            // by `reposition_all_windows` without ever flipping `ready` (the
            // commit-time activation's buffer check is unreliable — see the note in
            // `handlers::compositor::commit`). A window that's in the space with a
            // positive-size buffer is renderable, so it gets chrome.
            //
            // Every non-fullscreen window — tiled, floating, maximized, or snapped —
            // is mapped at its inner *body* rect (placement insets the client by the
            // titlebar + border), so Metis draws the same server-side chrome around
            // all of them. The decoration frame is the body grown by the titlebar
            // (top) and border (sides/bottom).
            let size = record.window.geometry().size;
            if size.w <= 0 || size.h <= 0 {
                continue;
            }
            let Some(loc) = self.space.element_location(&record.window) else {
                continue;
            };
            let auto_hide = self.auto_hide_titlebar.contains(&id);
            let show_overlay_titlebar = auto_hide
                && self.titlebar_reveal_window == Some(id)
                && self.titlebar_reveal_progress > 0.0;
            if auto_hide && !show_overlay_titlebar {
                continue;
            }
            let overlay_compact = auto_hide && self.window_uses_compact_overlay(id);
            let (frame, overlay) = if auto_hide {
                (
                    PixelRect {
                        x: loc.x,
                        y: loc.y,
                        width: size.w,
                        height: size.h,
                    },
                    true,
                )
            } else if let Some(frame) =
                self.ssd_frame_for_mapped_window(id, &record.window)
            {
                (frame, false)
            } else {
                continue;
            };
            specs.push(crate::decoration::WindowDeco {
                id,
                frame,
                title: if overlay_compact {
                    String::new()
                } else {
                    self.titlebar_title(id, record.app_id.as_deref(), &record.title)
                },
                focused: focused == Some(id) || self.revealed_titlebar == Some(id),
                overlay,
                overlay_reveal: if overlay {
                    self.titlebar_reveal_progress
                } else {
                    1.0
                },
                overlay_compact,
            });
        }
        specs
    }

    /// Title to draw in a window's titlebar. When more than one window of the same
    /// app is open, a 1-based ordinal (by ascending window id) is appended — e.g.
    /// "Alacritty (2)" — matching the number the dock's window picker shows, so the
    /// two can be visually correlated.
    fn titlebar_title(&self, id: u32, app_id: Option<&str>, title: &str) -> String {
        let Some(app_id) = app_id else {
            return title.to_string();
        };
        let mut same: Vec<u32> = self
            .windows
            .ids()
            .into_iter()
            .filter(|&oid| {
                self.windows
                    .get(oid)
                    .is_some_and(|r| r.app_id.as_deref() == Some(app_id))
            })
            .collect();
        if same.len() <= 1 {
            return title.to_string();
        }
        same.sort_unstable();
        match same.iter().position(|&x| x == id) {
            Some(p) => format!("{} ({})", title, p + 1),
            None => title.to_string(),
        }
    }

    /// Gaps for maximize/snap/clamp: `BAR_GAP_PX` on the edge where the bar sits,
    /// `WINDOW_GAP_PX` everywhere else.
    pub(crate) fn zone_edge_gaps(&self) -> ZoneGaps {
        let pos = metis_config::load_bar_config().position;
        let g = WINDOW_GAP_PX;
        let bar = BAR_GAP_PX;
        ZoneGaps {
            top: if matches!(pos, metis_config::BarPosition::Top) {
                bar
            } else {
                g
            },
            bottom: if matches!(pos, metis_config::BarPosition::Bottom) {
                bar
            } else {
                g
            },
            left: if matches!(pos, metis_config::BarPosition::Left) {
                bar
            } else {
                g
            },
            right: if matches!(pos, metis_config::BarPosition::Right) {
                bar
            } else {
                g
            },
        }
    }

    /// Pixels an overlay bar occupies along its anchored edge for window placement:
    /// the configured edge distance plus the visible body. This mirrors the top
    /// bar's layer-shell exclusive zone (`margin + body`) so every edge reserves
    /// exactly the *visible* strip. The transparent shadow pad above the pill is
    /// intentionally not reserved — the window tucks right up to the visible pill
    /// (it sits below the pill's top edge, so a translucent bar never shows the
    /// client through it), with a `BAR_GAP_PX` breathing gap added separately by
    /// `zone_edge_gaps`.
    fn bar_reserved_px() -> i32 {
        let cfg = metis_config::load_bar_config();
        cfg.margin_top as i32 + cfg.height as i32
    }

    /// Region for maximize, snap, and clamp. Top bar shrinks via layer-shell
    /// exclusive zone; other edges overlay but this zone excludes the bar strip
    /// so snapped/tiled windows sit beside/above it (floating windows may still
    /// slide underneath).
    pub(crate) fn window_placement_zone(&self) -> PixelRect {
        match self.primary_output() {
            Some(output) => self.window_placement_zone_for(&output),
            None => self.placement_zone(),
        }
    }

    /// Like [`window_placement_zone`](Self::window_placement_zone) but for a
    /// specific `output`, so snap/maximize/placement target the monitor a window
    /// (or the cursor) is on. The overlay-bar strip is only reserved on outputs
    /// that actually show a bar.
    pub(crate) fn window_placement_zone_for(
        &self,
        output: &smithay::output::Output,
    ) -> PixelRect {
        let mut zone = self.placement_zone_for(output);
        if !self.output_has_bar(output) {
            return zone;
        }
        let reserve = Self::bar_reserved_px();
        match metis_config::load_bar_config().position {
            metis_config::BarPosition::Bottom => {
                zone.height = (zone.height - reserve).max(1);
            }
            metis_config::BarPosition::Left => {
                zone.x += reserve;
                zone.width = (zone.width - reserve).max(1);
            }
            metis_config::BarPosition::Right => {
                zone.width = (zone.width - reserve).max(1);
            }
            metis_config::BarPosition::Top => {}
        }
        zone
    }

    /// The output area not covered by exclusive layer-shell zones, in global logical coordinates.
    ///
    /// `BAR_GAP_PX` is the thin padding kept between the edge bar and any window so
    /// the bar's drop shadow has breathing room and nothing visually touches it.
    pub fn usable_zone(&self) -> Option<PixelRect> {
        let output = self.primary_output()?;
        self.usable_zone_for(&output)
    }

    /// The usable area of a specific `output` (its geometry minus that output's
    /// exclusive layer-shell zones), in global logical coordinates.
    pub fn usable_zone_for(&self, output: &smithay::output::Output) -> Option<PixelRect> {
        let zone = layer_map_for_output(output).non_exclusive_zone();
        let origin = self.space.output_geometry(output)?.loc;
        Some(PixelRect {
            x: zone.loc.x + origin.x,
            y: zone.loc.y + origin.y,
            width: zone.size.w,
            height: zone.size.h,
        })
    }

    /// The usable area, falling back to the full output if the bar zone isn't
    /// known yet, and finally to the configured monitor size. Always returns the
    /// main (first) output, so off-screen windows are recovered onto it.
    fn placement_zone(&self) -> PixelRect {
        match self.primary_output() {
            Some(output) => self.placement_zone_for(&output),
            None => {
                let monitor = self.monitor;
                PixelRect {
                    x: monitor.x,
                    y: monitor.y,
                    width: monitor.width as i32,
                    height: monitor.height as i32,
                }
            }
        }
    }

    /// Usable area of `output`, falling back to its full geometry if the bar
    /// zone isn't known yet, and finally to the configured monitor size.
    fn placement_zone_for(&self, output: &smithay::output::Output) -> PixelRect {
        if let Some(zone) = self.usable_zone_for(output) {
            return zone;
        }
        let mut zone = {
            let monitor = self.output_rect(output).unwrap_or(self.monitor);
            PixelRect {
                x: monitor.x,
                y: monitor.y,
                width: monitor.width as i32,
                height: monitor.height as i32,
            }
        };
        // Layer-shell exclusive zone may not be committed yet at startup. For a
        // top bar, reserve the visible strip so maximize/placement never tuck
        // windows under the bar while waiting for the first bar configure.
        if self.output_has_bar(output)
            && matches!(
                metis_config::load_bar_config().position,
                metis_config::BarPosition::Top
            )
        {
            let reserve = Self::bar_reserved_px();
            zone.y += reserve;
            zone.height = (zone.height - reserve).max(1);
        }
        zone
    }

    /// Output a window was opened on (assigned at registration from the pointer).
    fn launch_output_for(&self, id: u32) -> Option<smithay::output::Output> {
        self.windows
            .output_name(id)
            .and_then(|name| self.output_by_name(&name))
            .or_else(|| self.output_under_pointer())
            .or_else(|| self.primary_output())
    }

    /// Center a client rect for a window, accounting for SSD chrome insets.
    fn centered_body_for_window(&self, id: u32, body_w: i32, body_h: i32) -> PixelRect {
        if !self.should_draw_metis_ssd(id) {
            let rect = match self.launch_output_for(id) {
                Some(output) => self.centered_rect_in(&output, body_w, body_h),
                None => self.centered_rect(body_w, body_h),
            };
            return self.clamp_floating_rect_for(id, rect);
        }
        let border = metis_grid::app_tile_border_px() as i32;
        let header = metis_grid::APP_TILE_HEADER_PX;
        let footprint_w = body_w + border * 2;
        let footprint_h = body_h + header + border;
        let footprint = match self.launch_output_for(id) {
            Some(output) => self.centered_rect_in(&output, footprint_w, footprint_h),
            None => self.centered_rect(footprint_w, footprint_h),
        };
        self.clamp_body_below_bar(app_tile_body_rect(footprint))
    }

    /// Restore a saved client rect, keeping position and size when possible.
    fn restore_body_for_window(&self, id: u32, saved: PixelRect) -> PixelRect {
        let rect = self.recover_offscreen_rect(saved);
        if self.should_draw_metis_ssd(id) {
            self.clamp_floating_rect(rect)
        } else {
            self.clamp_floating_rect_for(id, rect)
        }
    }

    /// A rect of `width`x`height` centered in the primary output's usable area.
    fn centered_rect(&self, width: i32, height: i32) -> PixelRect {
        self.centered_rect_in_zone(self.placement_zone(), width, height)
    }

    /// A rect of `width`x`height` centered in `output`'s usable area.
    fn centered_rect_in(
        &self,
        output: &smithay::output::Output,
        width: i32,
        height: i32,
    ) -> PixelRect {
        self.centered_rect_in_zone(self.placement_zone_for(output), width, height)
    }

    /// A rect of `width`x`height` centered in `zone` (clamped to fit).
    fn centered_rect_in_zone(&self, zone: PixelRect, width: i32, height: i32) -> PixelRect {
        let w = width.min((zone.width - WINDOW_GAP_PX * 2).max(1)).max(1);
        let h = height.min((zone.height - WINDOW_GAP_PX * 2).max(1)).max(1);
        PixelRect {
            x: zone.x + (zone.width - w) / 2,
            y: zone.y + (zone.height - h).max(0) / 2,
            width: w,
            height: h,
        }
    }

    /// True when `rect` is visible on at least one active output — i.e. it
    /// overlaps some monitor by a grabbable amount. A window on a secondary
    /// monitor counts as on-screen; only a window that lies off *every* output is
    /// considered lost. The minimum overlap ensures the titlebar stays reachable.
    fn rect_visible_on_any_output(&self, rect: PixelRect) -> bool {
        // Require a chunk at least this big (incl. the titlebar) on some output.
        const MIN_VISIBLE: i32 = MIN_VISIBLE_PX;
        for output in self.space.outputs() {
            let Some(g) = self.space.output_geometry(output) else {
                continue;
            };
            let left = rect.x.max(g.loc.x);
            let right = (rect.x + rect.width).min(g.loc.x + g.size.w);
            let top = rect.y.max(g.loc.y);
            let bottom = (rect.y + rect.height).min(g.loc.y + g.size.h);
            let overlap_w = (right - left).min(rect.width);
            let overlap_h = (bottom - top).min(rect.height);
            if overlap_w >= MIN_VISIBLE.min(rect.width) && overlap_h >= MIN_VISIBLE.min(rect.height) {
                return true;
            }
        }
        false
    }

    /// Keep a window reachable: if `rect` lies off every active output (e.g. it
    /// was saved on a monitor that's no longer connected), pull it back onto the
    /// primary output. Windows already visible on *some* monitor — including a
    /// secondary one in a multi-monitor setup — are left exactly where they are.
    pub fn recover_offscreen_rect(&self, rect: PixelRect) -> PixelRect {
        if self.rect_visible_on_any_output(rect) {
            return rect;
        }
        self.clamp_rect_on_screen(rect)
    }

    /// Force `rect` to be fully visible on the main output: cap its size to the
    /// usable area and shift its origin so the whole window is on-screen (under
    /// the bar). Used to recover a window that's off every active output.
    pub fn clamp_rect_on_screen(&self, rect: PixelRect) -> PixelRect {
        let zone = self.window_placement_zone();
        let gaps = self.zone_edge_gaps();
        let width = rect.width.clamp(1, zone.width.max(1));
        let height = rect.height.clamp(1, zone.height.max(1));
        let min_x = zone.x + gaps.left;
        let min_y = zone.y + gaps.top;
        let max_x = (zone.x + zone.width - width - gaps.right).max(min_x);
        let max_y = (zone.y + zone.height - height - gaps.bottom).max(min_y);
        PixelRect {
            x: rect.x.clamp(min_x, max_x),
            y: rect.y.clamp(min_y, max_y),
            width,
            height,
        }
    }

    /// Keep a floating window's body clear of the top edge bar (exclusive zone).
    /// Bottom/left/right bars overlay the desktop — floating windows may pass under them.
    pub fn clamp_body_below_bar(&self, mut rect: PixelRect) -> PixelRect {
        let pos = metis_config::load_bar_config().position;
        if !matches!(pos, metis_config::BarPosition::Top) {
            return rect;
        }
        let gaps = self.zone_edge_gaps();
        let header = metis_grid::APP_TILE_HEADER_PX;
        let zone = self.placement_zone();
        let min_y = zone.y + gaps.top + header;
        if rect.y < min_y {
            rect.y = min_y;
        }
        rect
    }

    /// Keep a floating window on-screen. Overlay edge bars do not inset the bounds
    /// (windows may slide underneath); only the top bar reserves space for SSD windows.
    fn clamp_floating_rect_for(&self, id: u32, rect: PixelRect) -> PixelRect {
        if self.should_draw_metis_ssd(id) {
            self.clamp_floating_rect(rect)
        } else {
            self.clamp_floating_rect_no_header(rect)
        }
    }

    /// Like [`Self::clamp_floating_rect`] but without reserving space for Metis SSD chrome.
    fn clamp_floating_rect_no_header(&self, rect: PixelRect) -> PixelRect {
        let center = Point::from((rect.x + rect.width / 2, rect.y + rect.height / 2));
        let zone = match self.output_at(center) {
            Some(output) => self.placement_zone_for(&output),
            None => self.placement_zone(),
        };
        let g = WINDOW_GAP_PX;
        let width = rect.width.clamp(1, (zone.width - g * 2).max(1));
        let height = rect.height.clamp(1, (zone.height - g * 2).max(1));
        let min_x = zone.x + g;
        let min_y = zone.y + g;
        let max_x = (zone.x + zone.width - width - g).max(min_x);
        let max_y = (zone.y + zone.height - height - g).max(min_y);
        PixelRect {
            x: rect.x.clamp(min_x, max_x),
            y: rect.y.clamp(min_y, max_y),
            width,
            height,
        }
    }

    /// Keep a floating window on-screen. Overlay edge bars do not inset the bounds
    /// (windows may slide underneath); only the top bar reserves space.
    fn clamp_floating_rect(&self, rect: PixelRect) -> PixelRect {
        // Clamp within the output the window mostly sits on (by its center), so a
        // floating window on a secondary monitor isn't yanked back to primary.
        let center = Point::from((rect.x + rect.width / 2, rect.y + rect.height / 2));
        let zone = match self.output_at(center) {
            Some(output) => self.placement_zone_for(&output),
            None => self.placement_zone(),
        };
        let g = WINDOW_GAP_PX;
        let pos = metis_config::load_bar_config().position;
        let gaps = self.zone_edge_gaps();
        let width = rect.width.clamp(1, (zone.width - g * 2).max(1));
        let height = rect.height.clamp(1, (zone.height - g * 2).max(1));
        let min_x = zone.x + g;
        let min_y = match pos {
            metis_config::BarPosition::Top => zone.y + gaps.top + metis_grid::APP_TILE_HEADER_PX,
            _ => zone.y + g,
        };
        let max_x = (zone.x + zone.width - width - g).max(min_x);
        let max_y = (zone.y + zone.height - height - g).max(min_y);
        PixelRect {
            x: rect.x.clamp(min_x, max_x),
            y: rect.y.clamp(min_y, max_y),
            width,
            height,
        }
    }

    /// Auto-place a window if it hasn't been finally positioned yet. Safe to call
    /// again whenever the app_id becomes known (GTK often sets it just *after* its
    /// first buffer commit, so the initial activation may not see it). No-ops once
    /// placement is locked in (`placement_chosen`) — i.e. positioned with a known
    /// app_id, or moved/resized by the user.
    pub(crate) fn maybe_autoplace_window(&mut self, id: u32) {
        // `placement_chosen` is the authoritative "we're done positioning" flag.
        // A free window may already be in `floating` with a provisional centered
        // rect (placed before its app_id was known) — that must still be allowed to
        // re-run here so the saved geometry can be restored once app_id arrives.
        if self.windows.placement_chosen(id) {
            return;
        }
        let app_id = self.windows.get(id).and_then(|r| r.app_id.clone());
        if self.place_new_window(id, app_id.as_deref()) && self.windows.is_ready(id) {
            self.apply_window_rect(id);
        }
    }

    /// Decide where a freshly-mapped window should appear (once per window).
    /// Grid workspaces tile; free and scroll workspaces center floating windows
    /// (saved size when the app was opened before, default size on first launch).
    /// Returns true when the window was placed as floating.
    fn place_new_window(&mut self, id: u32, app_id: Option<&str>) -> bool {
        if self.windows.placement_chosen(id) {
            return self.floating.contains(&id);
        }

        let title = self.windows.get(id).map(|r| r.title.clone());
        let key = self.desk_key_for_window(id);
        let ws = self.windows.workspace(id).unwrap_or(1);
        let kind = self.layout_kind_for(&key, ws);

        tracing::info!(id, ?app_id, ?title, ?kind, "place_new_window: deciding placement");

        // Settings and similar always open centered floating regardless of layout.
        let by_app_id = app_id.is_some_and(|a| CENTERED_FLOAT_APP_IDS.contains(&a));
        let by_title = title
            .as_deref()
            .is_some_and(|t| CENTERED_FLOAT_TITLES.contains(&t));
        if by_app_id || by_title {
            let rect = self.centered_body_for_window(id, DEFAULT_FLOAT_W, DEFAULT_FLOAT_H);
            self.floating.insert(id);
            self.windows.set_target_rect(id, rect);
            self.windows.set_placement_chosen(id, true);
            tracing::info!(id, ?rect, "place_new_window: centered default-float app");
            return true;
        }

        // Grid layout: tile via the desk — never auto-center from saved geometry.
        if kind == metis_grid::LayoutKind::Grid {
            self.windows.set_placement_chosen(id, true);
            return false;
        }

        // Free desktop: restore saved geometry when known, else centered default.
        if kind == metis_grid::LayoutKind::Free {
            // Free windows must be floating to map at all (apply_window_rect unmaps
            // non-floating free windows), so claim it up front on every path.
            self.floating.insert(id);
            if let Some(app_id) = app_id {
                if let Some(saved) = self.window_state.get(app_id) {
                    let saved_rect = saved.to_rect();
                    if saved_rect.width >= MIN_SAVED_WINDOW_PX
                        && saved_rect.height >= MIN_SAVED_WINDOW_PX
                    {
                        let rect = self.restore_body_for_window(id, saved_rect);
                        self.windows.set_target_rect(id, rect);
                        self.windows.set_placement_chosen(id, true);
                        tracing::info!(id, ?rect, "place_new_window: restored saved geometry");
                        return true;
                    }
                    self.window_state.remove(app_id);
                }
                // app_id known but nothing saved: first launch, center and lock.
                let rect = self.centered_body_for_window(id, DEFAULT_FLOAT_W, DEFAULT_FLOAT_H);
                self.windows.set_target_rect(id, rect);
                self.windows.set_placement_chosen(id, true);
                tracing::info!(id, "place_new_window: free desktop centered on launch output");
                return true;
            }
            // app_id not set yet (GTK usually assigns it just after the first
            // commit). Give the window a provisional centered rect so it maps, but
            // do NOT lock placement — a later pass, once the app_id is known, must
            // still be able to restore the saved geometry instead of leaving the
            // window stuck centered at the default size.
            if self.windows.target_rect(id).is_none() {
                let rect = self.centered_body_for_window(id, DEFAULT_FLOAT_W, DEFAULT_FLOAT_H);
                self.windows.set_target_rect(id, rect);
            }
            tracing::info!(
                id,
                "place_new_window: free desktop provisional center (awaiting app_id)"
            );
            return true;
        }

        // Scroll layout: the window belongs to the strip, not a free float. Like
        // the grid branch, just mark placement decided and let the scroll strip own
        // it — `ensure_app_tile_for_window` adds it to the strip (seed_scroll_state)
        // and `apply_window_rect` positions it from its scroll frame
        // (`rect_for_window_tile` → `scroll_frame_for_window`). Floating it here
        // would exclude it from `scroll_managed_app_ids` (which filters out floating
        // windows), leaving a centered window stranded on top of the strip. Column
        // widths are presets (⅓/½/⅔/full), so saved pixel geometry doesn't apply.
        self.windows.set_placement_chosen(id, true);
        tracing::info!(id, ?kind, "place_new_window: scroll strip-managed");
        false
    }

    /// Persist a floating window's current on-screen geometry under its app_id,
    /// so it reopens in the same place next time. No-op for grid-tiled windows
    /// (their position is derived from the grid) or windows without an app_id.
    pub(crate) fn save_window_geometry(&mut self, id: u32) {
        if !self.floating.contains(&id) {
            return;
        }
        let Some(record) = self.windows.get(id) else {
            return;
        };
        let Some(app_id) = record.app_id.clone() else {
            return;
        };
        // Prefer the live mapped geometry (captures user resizes); for a maximized
        // or snapped window save its pre-snap rect so it reopens at a sane float size.
        let rect = if record.maximized || record.snapped {
            record.restore_rect.unwrap_or(record.target_rect)
        } else if let Some(loc) = self.space.element_location(&record.window) {
            let size = record.window.geometry().size;
            PixelRect {
                x: loc.x,
                y: loc.y,
                width: size.w.max(1),
                height: size.h.max(1),
            }
        } else {
            record.target_rect
        };
        // Never persist a degenerate size (e.g. a window torn down before it ever
        // got a real buffer would save 1x1, which then reopens as an unusable
        // sliver). Keep the previous good value instead.
        if rect.width < MIN_SAVED_WINDOW_PX || rect.height < MIN_SAVED_WINDOW_PX {
            return;
        }
        self.window_state
            .set(&app_id, crate::window_state::SavedGeometry::from_rect(rect));
    }

    /// Handle a pointer press that may land on a server-side decoration (titlebar,
    /// control buttons, or border). Returns true when the press was consumed by the
    /// decoration (so the caller must not forward it to a client surface).
    /// Give keyboard focus to a window because its server-side chrome was
    /// clicked, and report it to the shell. No-op when already focused.
    fn focus_window_chrome(&mut self, id: u32, serial: smithay::utils::Serial) {
        let Some(record) = self.windows.get(id).cloned() else {
            return;
        };
        // Always raise: clicking any chrome must bring the window to the front,
        // even when it already holds keyboard focus (it can still be stacked
        // behind another window after a raise of its neighbor).
        self.note_window_focus(id);
        self.space.raise_element(&record.window, true);
        self.schedule_redraw();
        if self.focused_window_id() == Some(id) {
            return;
        }
        if let Some(keyboard) = self.seat.get_keyboard() {
            keyboard.set_focus(self, Some(record.window.clone().into()), serial);
        }
        self.event_bus
            .emit(&metis_protocol::CompositorEvent::WindowFocused { id });
    }

    pub fn handle_decoration_press(
        &mut self,
        loc: Point<f64, Logical>,
        serial: smithay::utils::Serial,
        button: u32,
    ) -> bool {
        use crate::decoration::{control_hitboxes, DecoControl};
        use crate::desk_input::point_in_rect;

        // A live popup/move/resize grab owns the pointer — let it run.
        if self
            .seat
            .get_pointer()
            .is_some_and(|p| p.is_grabbed())
        {
            return false;
        }
        if self.metis_bar_ui_hit(loc) {
            return false;
        }

        let (x, y) = (loc.x as i32, loc.y as i32);
        // Hit-test chrome in stacking order, topmost first, so a covered window's
        // titlebar/border can never catch a press that lands within the frame of a
        // window stacked in front of it (the front window owns that point). Revealed
        // overlay titlebars (auto-hide) always win — they float above all clients.
        let mut z: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
        for (i, window) in self.space.elements().enumerate() {
            if let Some(id) = self.windows.id_for_window(window) {
                z.insert(id, i);
            }
        }
        let specs = self.decoration_specs();
        let mut ordered: Vec<&crate::decoration::WindowDeco> = specs.iter().collect();
        ordered.sort_by_key(|s| {
            // overlay first (false < true); then highest stacking index (topmost).
            (!s.overlay, std::cmp::Reverse(z.get(&s.id).copied().unwrap_or(0)))
        });
        for spec in ordered {
            let frame = spec.frame;
            if spec.overlay {
                let chrome = crate::decoration::overlay_chrome_rect(
                    spec.frame,
                    spec.overlay_reveal,
                    spec.overlay_compact,
                );
                // Overlay chrome can sit above the client rect while sliding in.
                if !point_in_rect(x, y, chrome) {
                    continue;
                }
            } else {
                if !point_in_rect(x, y, frame) {
                    continue;
                }
                if point_in_rect(x, y, metis_grid::app_tile_body_rect(frame)) {
                    // Inside the client body → not a decoration hit; let it pass through.
                    return false;
                }
            }
            // Clicking any of a window's chrome focuses it, so the taskbar
            // highlight tracks focus immediately instead of waiting for the
            // periodic reconcile (decoration presses otherwise bypass the
            // keyboard-focus path entirely).
            self.focus_window_chrome(spec.id, serial);
            // Prefer edge resize over titlebar drag when the click lands on a
            // border strip (corners overlap both regions). Skipped for overlay
            // reveals — only the titlebar strip is interactive there.
            if !spec.overlay {
                let edges = self.resize_edges_for_point(loc, spec.frame);
                if !edges.is_empty() {
                    return self.start_edge_resize(spec.id, edges, loc, serial, button);
                }
            }
            let hit_frame = if spec.overlay {
                let chrome = crate::decoration::overlay_chrome_rect(
                    spec.frame,
                    spec.overlay_reveal,
                    spec.overlay_compact,
                );
                PixelRect {
                    x: spec.frame.x,
                    y: chrome.y,
                    width: spec.frame.width,
                    height: spec.frame.height,
                }
            } else {
                frame
            };
            for (control, rect) in control_hitboxes(hit_frame, spec.overlay_compact) {
                if !point_in_rect(x, y, rect) {
                    continue;
                }
                match control {
                    DecoControl::Close => self.close_window(spec.id),
                    DecoControl::Minimize => {
                        if self.windows.get(spec.id).is_some_and(|r| r.maximized) {
                            self.set_maximized(spec.id, false);
                        }
                        if let Some(tile_id) = self.tile_id_for_window(spec.id) {
                            self.set_tile_mode(&tile_id, metis_protocol::TileMode::Minimized);
                        } else {
                            self.minimize_window(spec.id);
                        }
                    }
                    DecoControl::Maximize => {
                        self.titlebar_press_pending = None;
                        let maxed = self
                            .windows
                            .get(spec.id)
                            .map(|r| r.maximized)
                            .unwrap_or(false);
                        self.set_maximized(spec.id, !maxed);
                    }
                    DecoControl::Titlebar => {
                        if self.titlebar_double_click_toggle(spec.id) {
                            self.titlebar_press_pending = None;
                            return true;
                        }
                        if self.windows.get(spec.id).is_some_and(|r| r.maximized) {
                            self.titlebar_press_pending = Some((spec.id, loc, serial));
                            return true;
                        }
                        self.start_titlebar_move(spec.id, loc, serial);
                    }
                }
                return true;
            }
        }
        false
    }

    /// Which resize edge(s) the pointer is over within `frame`'s border strips and
    /// outer grab halo. Returns empty when the pointer is in the interior body.
    fn resize_edges_for_point(
        &self,
        loc: Point<f64, Logical>,
        frame: PixelRect,
    ) -> crate::grabs::ResizeEdge {
        use crate::desk_input::point_in_rect;
        use crate::grabs::ResizeEdge;

        let (x, y) = (loc.x as i32, loc.y as i32);
        let m = RESIZE_MARGIN_PX;
        let corner = m + metis_grid::app_tile_border_px().max(1);

        // m-wide band straddling each frame edge (inside + outside the drawn border).
        // Each side is clamped to the frame extent (+ halo) on the perpendicular axis
        // so left/right strips do not extend infinitely above/below the window.
        let y_lo = frame.y - m;
        let y_hi = frame.y + frame.height + m;
        let x_lo = frame.x - m;
        let x_hi = frame.x + frame.width + m;
        let on_left = x >= frame.x - m && x < frame.x + m && y >= y_lo && y < y_hi;
        let on_right =
            x >= frame.x + frame.width - m && x < frame.x + frame.width + m && y >= y_lo && y < y_hi;
        let on_top = y >= frame.y - m && y < frame.y + m && x >= x_lo && x < x_hi;
        let on_bottom = y >= frame.y + frame.height - m
            && y < frame.y + frame.height + m
            && x >= x_lo
            && x < x_hi;
        if !on_left && !on_right && !on_top && !on_bottom {
            return ResizeEdge::empty();
        }

        let mut edges = ResizeEdge::empty();
        if on_left {
            edges |= ResizeEdge::LEFT;
        }
        if on_right {
            edges |= ResizeEdge::RIGHT;
        }
        if on_bottom {
            edges |= ResizeEdge::BOTTOM;
        }
        if on_top {
            edges |= ResizeEdge::TOP;
        }

        // Titlebar centre is for dragging; keep corner resize bits only.
        let titlebar = metis_grid::app_tile_chrome_rect(frame);
        if point_in_rect(x, y, titlebar) {
            let in_left_corner = x < frame.x + corner;
            let in_right_corner = x >= frame.x + frame.width - corner;
            if !in_left_corner && !in_right_corner {
                edges.remove(ResizeEdge::TOP);
            }
        }

        if edges.is_empty() {
            return ResizeEdge::empty();
        }
        edges
    }

    /// Server-side decoration frame for a mapped window, when chrome should be
    /// drawn or hit-tested. `None` for minimized/fullscreen windows and for
    /// auto-hide windows whose titlebar is not revealed.
    pub(crate) fn ssd_frame_for_mapped_window(
        &self,
        id: u32,
        window: &smithay::desktop::Window,
    ) -> Option<PixelRect> {
        if !self.should_draw_metis_ssd(id) {
            return None;
        }
        let record = self.windows.get(id)?;
        if record.fullscreen || self.windows.is_minimized(id) {
            return None;
        }
        let loc = self.space.element_location(window)?;
        let size = window.geometry().size;
        if size.w <= 0 || size.h <= 0 {
            return None;
        }
        if self.auto_hide_titlebar.contains(&id) && self.revealed_titlebar != Some(id) {
            return None;
        }
        if self.auto_hide_titlebar.contains(&id) {
            return Some(PixelRect {
                x: loc.x,
                y: loc.y,
                width: size.w,
                height: size.h,
            });
        }
        let border = metis_grid::app_tile_border_px();
        Some(PixelRect {
            x: loc.x - border,
            y: loc.y - metis_grid::APP_TILE_HEADER_PX,
            width: size.w + border * 2,
            height: size.h + metis_grid::APP_TILE_HEADER_PX + border,
        })
    }

    /// Hit-test the pointer against every mapped window's resize band. Returns the
    /// topmost window whose edge/corner is under the pointer, plus the combined
    /// edge(s). Skips minimized, maximized, and fullscreen windows.
    pub fn resize_edge_at(
        &self,
        loc: Point<f64, Logical>,
    ) -> Option<(u32, crate::grabs::ResizeEdge)> {
        use crate::desk_input::point_in_rect;

        if self.metis_bar_ui_hit(loc) {
            return None;
        }
        let (x, y) = (loc.x as i32, loc.y as i32);
        // Walk mapped windows top-to-bottom so the frontmost window owns edge hits.
        for window in self.space.elements().rev() {
            let Some(id) = self.windows.id_for_window(window) else {
                continue;
            };
            if self.windows.is_minimized(id) {
                continue;
            }
            if self.windows.get(id).is_some_and(|r| r.maximized || r.fullscreen) {
                continue;
            }
            let Some(frame) = self.ssd_frame_for_mapped_window(id, window) else {
                continue;
            };
            let edges = self.resize_edges_for_point(loc, frame);
            if !edges.is_empty() {
                return Some((id, edges));
            }
            if point_in_rect(x, y, frame) {
                return None;
            }
        }
        None
    }

    /// Update the hovered resize edge from the pointer position so the host cursor
    /// can show the matching directional arrow. No-op while a grab owns the pointer
    /// (the active move/resize keeps its cursor). Flags a redraw on change.
    pub fn update_hover_cursor(&mut self, loc: Point<f64, Logical>) {
        if self.seat.get_pointer().is_some_and(|p| p.is_grabbed()) {
            return;
        }
        // Window chrome must not react while the pointer is over the edge bar or
        // its popovers — otherwise titlebars below the bar's transparent shadow
        // pad show hover/reveal state when interacting with bar widgets.
        if self.metis_bar_ui_hit(loc) {
            if self.hover_cursor.is_some() {
                self.hover_cursor = None;
                self.schedule_redraw();
            }
            // Still reveal auto-hide titlebars when the pointer is over the edge bar
            // strip above a maximized window (the strip overlaps the client top).
            self.update_titlebar_reveal(loc);
            self.tick_titlebar_press_pending(loc);
            return;
        }
        let edge = self.resize_edge_at(loc).map(|(_, e)| e);
        if edge != self.hover_cursor {
            self.hover_cursor = edge;
            self.schedule_redraw();
        }
        self.update_titlebar_reveal(loc);
        self.tick_titlebar_press_pending(loc);
    }

    fn tick_titlebar_press_pending(&mut self, loc: Point<f64, Logical>) {
        let Some((id, start, serial)) = self.titlebar_press_pending else {
            return;
        };
        if self.seat.get_pointer().is_some_and(|p| p.is_grabbed()) {
            return;
        }
        let dx = loc.x - start.x;
        let dy = loc.y - start.y;
        if dx * dx + dy * dy >= 25.0 {
            self.titlebar_press_pending = None;
            self.start_titlebar_move(id, loc, serial);
        }
    }

    pub fn clear_titlebar_press_pending(&mut self) {
        self.titlebar_press_pending = None;
    }

    fn auto_hide_reveal_hit(
        &self,
        id: u32,
        geo: &smithay::utils::Rectangle<i32, Logical>,
        x: i32,
        y: i32,
    ) -> bool {
        use crate::decoration::overlay_chrome_rect;
        use crate::desk_input::point_in_rect;

        const STICKY_PAD_PX: i32 = 16;
        let header = metis_grid::APP_TILE_HEADER_PX;
        let in_x = x >= geo.loc.x && x < geo.loc.x + geo.size.w;
        if !in_x {
            return false;
        }
        let compact = self.window_uses_compact_overlay(id);
        let frame = PixelRect {
            x: geo.loc.x,
            y: geo.loc.y,
            width: geo.size.w,
            height: geo.size.h,
        };

        if self.titlebar_reveal_window == Some(id) {
            let chrome = overlay_chrome_rect(frame, self.titlebar_reveal_progress, compact);
            return point_in_rect(
                x,
                y,
                PixelRect {
                    x: chrome.x,
                    y: chrome.y - 4,
                    width: chrome.width,
                    height: chrome.height + STICKY_PAD_PX + 4,
                },
            );
        }

        if compact {
            let strip_w = metis_grid::OVERLAY_CONTROLS_WIDTH_PX.min(geo.size.w.max(1));
            return y >= geo.loc.y
                && y < geo.loc.y + header
                && x >= geo.loc.x + geo.size.w - strip_w;
        }

        // Maximized windows sit flush under the edge bar; the bar's shadow pad
        // overlaps the client top and blocks the thin client-side trigger strip.
        // Treat horizontal pointer-over-window in the bar strip as a reveal too.
        if self.windows.get(id).is_some_and(|r| r.maximized) {
            if let Some(output) = self.output_for_window(id) {
                if let Some(output_geo) = self.space.output_geometry(&output) {
                    let strip = Self::bar_config_strip_rect(&output_geo);
                    if point_in_rect(x, y, strip) {
                        return true;
                    }
                }
            }
        }

        y >= geo.loc.y && y < geo.loc.y + header
    }

    /// Reveal the auto-hide titlebar overlay for the topmost auto-hide window whose
    /// pointer is in the reveal trigger or sticky chrome zone.
    fn update_titlebar_reveal(&mut self, loc: Point<f64, Logical>) {
        let (x, y) = (loc.x as i32, loc.y as i32);
        let mut revealed = None;
        // Topmost first: `Space::elements()` is bottom-to-top, so reverse.
        for window in self.space.elements().rev() {
            let Some(geo) = self.space.element_geometry(window) else {
                continue;
            };
            let in_x = x >= geo.loc.x && x < geo.loc.x + geo.size.w;
            let in_window = in_x && y >= geo.loc.y && y < geo.loc.y + geo.size.h;
            let Some(id) = self.windows.id_for_window(window) else {
                continue;
            };
            if self.auto_hide_titlebar.contains(&id) {
                if self.auto_hide_reveal_hit(id, &geo, x, y) {
                    revealed = Some(id);
                    break;
                }
                if in_window {
                    break;
                }
            } else if in_window {
                // A normal window occludes anything beneath it at this point.
                break;
            }
        }
        if revealed != self.revealed_titlebar {
            self.revealed_titlebar = revealed;
            if let Some(id) = revealed {
                self.titlebar_reveal_window = Some(id);
            }
            let _ = self.tick_titlebar_reveal_animation();
            // Hover only reveals chrome; keyboard focus stays on the window the
            // user picked until they click its titlebar (see `focus_window_chrome`).
            // Calling `focus_window_id` here re-raised a stale maximized neighbor
            // when the pointer lingered at the top edge after unmaximize.
            self.schedule_redraw();
        }
    }

    /// Handle a pointer press that may land on a window's resize band. On a hit,
    /// floats the window out of the grid and starts an interactive resize grab.
    /// Returns true when the press was consumed.
    pub fn handle_resize_press(
        &mut self,
        loc: Point<f64, Logical>,
        serial: smithay::utils::Serial,
        button: u32,
    ) -> bool {
        if self.metis_bar_ui_hit(loc) {
            return false;
        }
        if self.seat.get_pointer().is_some_and(|p| p.is_grabbed()) {
            return false;
        }
        let Some((id, edges)) = self.resize_edge_at(loc) else {
            return false;
        };
        if self.is_active_scroll_window(id) {
            if self.start_scroll_resize(id, edges, loc, serial) {
                return true;
            }
            // Fall through — vertical edges (or scroll-target miss) use normal resize.
        }
        self.start_edge_resize(id, edges, loc, serial, button)
    }

    /// Begin an interactive edge resize for a normal (non-scroll-column) window.
    fn start_edge_resize(
        &mut self,
        id: u32,
        edges: crate::grabs::ResizeEdge,
        loc: Point<f64, Logical>,
        serial: smithay::utils::Serial,
        button: u32,
    ) -> bool {
        use smithay::input::pointer::{Focus, GrabStartData};

        let Some(record) = self.windows.get(id).cloned() else {
            return false;
        };
        let window = record.window.clone();
        let Some(initial_window_location) = self.space.element_location(&window) else {
            return false;
        };
        let initial_window_size = window.geometry().size;

        self.space.raise_element(&window, true);
        self.floating.insert(id);
        self.clear_tiled_states(id);

        if let Some(keyboard) = self.seat.get_keyboard() {
            keyboard.set_focus(self, Some(window.clone().into()), serial);
        }

        let toplevel = window.toplevel().unwrap();
        toplevel.with_pending_state(|state| {
            state.states.set(
                smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::State::Resizing,
            );
            state.size = Some(initial_window_size);
        });
        toplevel.send_pending_configure();

        let Some(pointer) = self.seat.get_pointer() else {
            return false;
        };
        let start_data = GrabStartData {
            focus: None,
            button,
            location: loc,
        };
        let grab = crate::grabs::ResizeSurfaceGrab::start(
            start_data,
            window,
            edges,
            smithay::utils::Rectangle::new(initial_window_location, initial_window_size),
        );
        self.hover_cursor = Some(edges);
        self.schedule_redraw();
        pointer.set_grab(self, grab, serial, Focus::Clear);
        true
    }

    /// Begin a horizontal resize of a scroll column from a left/right border drag.
    /// The grab adjusts the target column's width live and reflows the strip.
    fn start_scroll_resize(
        &mut self,
        id: u32,
        edges: crate::grabs::ResizeEdge,
        loc: Point<f64, Logical>,
        serial: smithay::utils::Serial,
    ) -> bool {
        use crate::grabs::ResizeEdge;
        use smithay::input::pointer::{Focus, GrabStartData};

        if !edges.intersects(ResizeEdge::LEFT | ResizeEdge::RIGHT) {
            return false;
        }
        let Some((target_window, initial_width_px)) = self.scroll_resize_target(id, edges) else {
            return false;
        };
        let Some(pointer) = self.seat.get_pointer() else {
            return false;
        };
        if pointer.is_grabbed() {
            return false;
        }
        // Focus the window the user grabbed so the resize reads as acting on it.
        if let Some(record) = self.windows.get(id).cloned() {
            self.space.raise_element(&record.window, true);
            if let Some(keyboard) = self.seat.get_keyboard() {
                keyboard.set_focus(self, Some(record.window.into()), serial);
            }
        }
        let start_data = GrabStartData {
            focus: None,
            button: 0x110,
            location: loc,
        };
        let grab = crate::grabs::ScrollResizeGrab::start(
            start_data,
            target_window,
            initial_width_px,
            loc.x,
        );
        self.hover_cursor = Some(edges);
        self.schedule_redraw();
        pointer.set_grab(self, grab, serial, Focus::Clear);
        true
    }

    /// Double-click the titlebar (anywhere outside the traffic-light buttons) to
    /// toggle maximize. The first click of a pair may start a brief move grab; the
    /// second press within the interval toggles without dragging.
    fn titlebar_double_click_toggle(&mut self, id: u32) -> bool {
        const INTERVAL: std::time::Duration = std::time::Duration::from_millis(400);
        let now = std::time::Instant::now();
        if let Some((prev_id, prev)) = self.titlebar_last_click {
            if prev_id == id && now.duration_since(prev) <= INTERVAL {
                self.titlebar_last_click = None;
                let maxed = self
                    .windows
                    .get(id)
                    .map(|r| r.maximized)
                    .unwrap_or(false);
                self.set_maximized(id, !maxed);
                return true;
            }
        }
        self.titlebar_last_click = Some((id, now));
        false
    }

    /// Unmaximize only after the user actually drags the titlebar (not on click).
    pub fn unmaximize_for_titlebar_drag(&mut self, id: u32) {
        if self.windows.get(id).is_some_and(|r| r.maximized) {
            self.set_maximized(id, false);
        }
    }

    fn start_titlebar_move(
        &mut self,
        id: u32,
        loc: Point<f64, Logical>,
        serial: smithay::utils::Serial,
    ) {
        use smithay::input::pointer::{Focus, GrabStartData};

        let Some(record) = self.windows.get(id).cloned() else {
            return;
        };
        let window = record.window.clone();
        self.space.raise_element(&window, true);
        // Manual titlebar drag floats the window out of the grid (no snap-back).
        self.floating.insert(id);
        self.clear_tiled_states(id);

        let initial_window_location = if self.windows.is_snapped(id) {
            self.restore_floating_from_snap(id, loc)
        } else {
            let was_maximized = self.windows.get(id).is_some_and(|r| r.maximized);
            if !was_maximized {
                let mut initial_window_location = self
                    .space
                    .element_location(&window)
                    .unwrap_or_default();

                // SSD windows reserve a titlebar strip above the body when floating;
                // CSD windows keep their own chrome and need no extra inset.
                if self.usable_zone().is_some() && self.should_draw_metis_ssd(id) {
                    let rect = metis_grid::PixelRect {
                        x: initial_window_location.x,
                        y: initial_window_location.y,
                        width: window.geometry().size.w,
                        height: window.geometry().size.h,
                    };
                    let clamped = self.clamp_body_below_bar(rect);
                    if clamped.y != initial_window_location.y || clamped.x != initial_window_location.x {
                        initial_window_location.x = clamped.x;
                        initial_window_location.y = clamped.y;
                        self.space
                            .map_element(window.clone(), initial_window_location, true);
                        self.windows.set_target_rect(id, clamped);
                    }
                }
                initial_window_location
            } else {
                self.space.element_location(&window).unwrap_or_default()
            }
        };

        let pending_maximized_demote = self.windows.get(id).is_some_and(|r| r.maximized)
            && !self.windows.is_snapped(id);

        // Focus the window so keyboard input follows the titlebar grab.
        if let Some(keyboard) = self.seat.get_keyboard() {
            keyboard.set_focus(self, Some(window.clone().into()), serial);
        }

        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let start_data = GrabStartData {
            focus: None,
            button: 0x110,
            location: loc,
        };
        let grab = crate::grabs::MoveSurfaceGrab {
            start_data,
            window,
            initial_window_location,
            drag_active: false,
            pending_maximized_demote,
        };
        pointer.set_grab(self, grab, serial, Focus::Clear);
    }

    fn rect_for_window_tile(&self, id: u32) -> Option<PixelRect> {
        if self.floating.contains(&id) {
            return None;
        }
        // Scrolling workspaces position from the strip, not the tile grid.
        if let Some(frame) = self.scroll_frame_placed_for_window(id) {
            return Some(frame);
        }
        let key = self.desk_key_for_window(id);
        let desk = self.desk(&key)?;
        let tile = desk.layout.tiles.iter().find(|t| {
            matches!(&t.kind, TileKind::App { window_id: Some(wid), .. } if *wid == id)
        })?;
        let metrics = match self.output_by_name(&key) {
            Some(o) => self.grid_metrics_for(&o),
            None => self.grid_metrics(),
        };
        Some(cell_to_pixels(&metrics, &tile.rect))
    }

    pub fn apply_grid_layout(&mut self, shell_layout: GridLayout, gutter_px: u32) {
        use std::collections::HashMap;

        // The shell desk editor is dormant; this path applies to the primary desk.
        let key = self.primary_key();
        let compositor_apps: HashMap<String, metis_grid::GridTile> = self
            .desk(&key)
            .map(|d| {
                d.layout
                    .tiles
                    .iter()
                    .filter(|t| matches!(t.kind, TileKind::App { .. }))
                    .map(|t| (t.id.clone(), t.clone()))
                    .collect()
            })
            .unwrap_or_default();

        let mut merged = shell_layout;
        for tile in &mut merged.tiles {
            let TileKind::App { window_id, class } = &mut tile.kind else {
                continue;
            };
            let Some(existing) = compositor_apps.get(&tile.id) else {
                continue;
            };
            let TileKind::App {
                window_id: existing_wid,
                class: existing_class,
            } = &existing.kind
            else {
                continue;
            };
            if window_id.is_none() {
                *window_id = existing_wid.clone();
            }
            if class.as_deref().unwrap_or("").is_empty() {
                *class = existing_class.clone();
            }
        }

        for app in compositor_apps.values() {
            if !merged.tiles.iter().any(|t| t.id == app.id) {
                merged.tiles.push(app.clone());
            }
        }

        self.desk_mut_or_default(&key).layout = merged;
        self.gutter_px = gutter_px;
        self.ensure_app_tiles_for_open_windows();
        self.sync_grid_titlebar_chrome(&key);
        self.reposition_all_windows();
    }

    fn ensure_app_tiles_for_open_windows(&mut self) {
        for id in self.windows.ids() {
            self.ensure_app_tile_for_window(id);
        }
    }

    pub(crate) fn reposition_all_windows(&mut self) {
        for id in self.windows.ids() {
            self.remap_window_for_desktop(id);
        }
        self.restore_focus_stacking();
    }

    /// After bulk layout sync, put the user-focused window back on top without
    /// toggling xdg activation state on neighbors.
    fn restore_focus_stacking(&mut self) {
        let Some(id) = self.preferred_stacking_window() else {
            return;
        };
        self.raise_stacking_window(id, false);
    }

    /// Keep the window the user picked above neighbors while the pointer is over
    /// its chrome/body. Uses the window's frame geometry, not `element_under`, so
    /// a neighbor stacked too high during minimize/maximize restore cannot block
    /// the raise when the cursor reaches the chosen app.
    pub(crate) fn maintain_focus_stacking(&mut self, loc: Point<f64, Logical>) {
        use crate::desk_input::point_in_rect;

        let Some(preferred) = self.preferred_stacking_window() else {
            return;
        };
        let Some(record) = self.windows.get(preferred).cloned() else {
            return;
        };
        if record.maximized || record.fullscreen || self.windows.is_minimized(preferred) {
            return;
        }
        let frame = self
            .ssd_frame_for_mapped_window(preferred, &record.window)
            .or_else(|| {
                let loc = self.space.element_location(&record.window)?;
                let size = record.window.geometry().size;
                Some(PixelRect {
                    x: loc.x,
                    y: loc.y,
                    width: size.w.max(1),
                    height: size.h.max(1),
                })
            });
        let Some(frame) = frame else {
            return;
        };
        if !point_in_rect(loc.x as i32, loc.y as i32, frame) {
            return;
        }
        self.raise_stacking_window(preferred, false);
        if self.focused_window_id() != Some(preferred) {
            let pointer_ok = self
                .seat
                .get_pointer()
                .is_none_or(|p| !p.is_grabbed());
            let keyboard_ok = self
                .seat
                .get_keyboard()
                .is_none_or(|k| !k.is_grabbed());
            if pointer_ok && keyboard_ok {
                if let Some(keyboard) = self.seat.get_keyboard() {
                    let serial = smithay::utils::SERIAL_COUNTER.next_serial();
                    keyboard.set_focus(self, Some(record.window.into()), serial);
                }
            }
        }
    }

    /// Reserve a grid slot as soon as an app registers (before its first buffer commit).
    fn ensure_app_tile_for_window(&mut self, id: u32) {
        let key = self.desk_key_for_window(id);
        let ws = self.windows.workspace(id).unwrap_or(1);
        if self.layout_kind_for(&key, ws) == metis_grid::LayoutKind::Free {
            return;
        }

        let tile_id = format!("app-{id}");
        // Already present, visible or stashed on this output's desk?
        if let Some(desk) = self.desk(&key) {
            if desk.layout.tiles.iter().any(|t| t.id == tile_id)
                || desk
                    .stashed_app_tiles
                    .values()
                    .any(|tiles| tiles.iter().any(|t| t.id == tile_id))
            {
                return;
            }
        }
        let class = self.windows.get(id).and_then(|r| r.app_id.clone());
        let active = self.active_workspace_for(&key);
        let desk = self.desk_mut_or_default(&key);
        let tile = metis_grid::GridTile {
            id: tile_id,
            rect: default_app_tile_rect(&desk.layout),
            kind: TileKind::App {
                window_id: Some(id),
                class,
            },
            glow: "cool".into(),
            pinned: false,
            min_w: None,
            max_w: None,
            min_h: None,
            max_h: None,
        };
        if ws == active {
            desk.layout.tiles.push(tile);
        } else {
            desk.stashed_app_tiles.entry(ws).or_default().push(tile);
        }
        // Mirror membership into the scroll strip when this workspace scrolls.
        if self.layout_kind_for(&key, ws) == metis_grid::LayoutKind::Scroll {
            self.seed_scroll_state(&key, ws);
            self.refresh_scroll_offset(&key, false);
            // Slide the windows already in the strip into their new frames — a fresh
            // column shifts every column to its right, so the prior window must move
            // instead of the newcomer just painting on top of it.
            self.reposition_scroll_windows();
        } else {
            self.auto_reflow_grid_apps(&key, Some(id), true);
        }
    }

    /// Split the active grid workspace among grid-managed app windows.
    fn auto_reflow_grid_apps(
        &mut self,
        output_key: &str,
        focus_window_id: Option<u32>,
        emit: bool,
    ) {
        let ws = self.active_workspace_for(output_key);
        if self.layout_kind_for(output_key, ws) != metis_grid::LayoutKind::Grid {
            return;
        }

        self.prune_stale_app_tiles(output_key);

        let include: Vec<String> = self
            .desk(output_key)
            .map(|desk| {
                desk.layout
                    .tiles
                    .iter()
                    .filter_map(|t| {
                        if let TileKind::App {
                            window_id: Some(wid),
                            ..
                        } = &t.kind
                        {
                            self.is_window_grid_managed(*wid).then(|| t.id.clone())
                        } else {
                            None
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        if include.is_empty() {
            return;
        }

        let focus_tile_id = focus_window_id.and_then(|id| self.tile_id_for_window(id));

        let desk = self.desk_mut_or_default(output_key);
        metis_grid::sanitize_layout(&mut desk.layout);
        let focus = focus_tile_id.as_deref();
        if let Err(err) =
            metis_grid::auto_tile_apps(&mut desk.layout, focus, &include)
        {
            tracing::warn!(%err, output = output_key, "auto_tile_apps failed after sanitize; retrying");
            metis_grid::sanitize_layout(&mut desk.layout);
            if let Err(err) =
                metis_grid::auto_tile_apps(&mut desk.layout, focus, &include)
            {
                tracing::warn!(%err, output = output_key, "auto_tile_apps failed after retry");
            }
        }
        self.sync_grid_titlebar_chrome(output_key);
        self.reposition_all_windows();
        self.persist_layout();
        if emit {
            self.emit_layout_changed();
        }
    }

    pub fn sync_all_app_windows(&mut self) {
        for id in self.windows.ids() {
            self.ensure_app_tile_for_window(id);
        }
        self.try_activate_all_pending();
        self.reposition_all_windows();
    }

    pub fn try_activate_all_pending(&mut self) {
        let pending: Vec<_> = self
            .windows
            .ids()
            .into_iter()
            .filter(|id| !self.windows.is_ready(*id))
            .filter_map(|id| {
                self.windows
                    .get(id)
                    .map(|record| record.toplevel.wl_surface().clone())
            })
            .collect();
        for surface in pending {
            self.try_activate_committed_window(&surface);
        }
    }

    fn persist_layout(&mut self) {
        // Persist the primary desk's layout (its widget positions) to `desk.json`.
        let key = self.primary_key();
        if let Some(desk) = self.desk(&key) {
            if let Err(err) = desk.layout.save_to_path(&desk_config_path()) {
                tracing::warn!(%err, "failed to persist grid layout");
            }
        }
    }

    pub fn emit_layout_changed(&self) {
        use metis_protocol::CompositorEvent;
        let key = self.primary_key();
        let layout = self
            .desk(&key)
            .map(|d| d.layout.clone())
            .unwrap_or_else(|| self.default_layout.clone());
        self.event_bus.emit(&CompositorEvent::LayoutChanged {
            layout,
            gutter_px: self.gutter_px,
            metrics: self.grid_metrics(),
        });
    }

    pub fn emit_monitor_changed(&self) {
        use metis_protocol::CompositorEvent;
        self.event_bus.emit(&CompositorEvent::MonitorChanged {
            rect: self.monitor,
        });
    }

    pub fn emit_workspace_changed(&self, output_key: &str) {
        use metis_protocol::CompositorEvent;
        self.event_bus.emit(&CompositorEvent::WorkspaceChanged {
            output: output_key.to_string(),
            active: self.active_workspace_for(output_key),
            count: self.workspace_count(),
        });
    }

    /// Configured number of virtual workspaces (clamped to a sane 1..=12).
    pub fn workspace_count(&self) -> u32 {
        metis_config::load_bar_config().workspace_count.clamp(1, 12)
    }

    /// Configured multi-monitor workspace behavior (independent vs. linked).
    pub fn workspace_mode(&self) -> metis_config::WorkspaceMode {
        metis_config::load_bar_config().workspace_mode
    }

    /// Switch workspace honoring the configured multi-monitor mode. In `Separate`
    /// only `requested_output` changes; in `Linked` every output switches to the
    /// same workspace at once (each emits its own `WorkspaceChanged`).
    pub fn switch_workspace_routed(&mut self, requested_output: &str, target: u32) {
        if self.workspace_mode() == metis_config::WorkspaceMode::Linked {
            let keys: Vec<String> = self.space.outputs().map(|o| o.name()).collect();
            if keys.is_empty() {
                self.switch_workspace(requested_output, target);
            } else {
                for key in keys {
                    self.switch_workspace(&key, target);
                }
            }
        } else {
            self.switch_workspace(requested_output, target);
        }
    }

    /// Show a different virtual workspace on a single output. Stashes that
    /// output's visible app tiles (and unmaps their windows), then restores the
    /// target workspace's tiles and remaps its windows. Other outputs and the
    /// desk widget tiles are untouched.
    pub fn switch_workspace(&mut self, output_key: &str, target: u32) {
        let target = target.clamp(1, self.workspace_count());
        let current = self.active_workspace_for(output_key);
        if target == current {
            return;
        }

        if self.layout_kind_for(output_key, current) == metis_grid::LayoutKind::Free {
            for id in self.window_ids_on_workspace(output_key, current) {
                if let Some(record) = self.windows.get(id).cloned() {
                    self.space.unmap_elem(&record.window);
                }
            }
            self.desk_mut_or_default(output_key).active_workspace = target;
            for id in self.window_ids_on_workspace(output_key, target) {
                self.ensure_app_tile_for_window(id);
                self.remap_window_for_desktop(id);
            }
            self.focus_topmost_on_active_workspace();
            self.emit_workspace_changed(output_key);
            return;
        }

        // Pull this output's app tiles out of its live grid and remember them.
        let mut stashed: Vec<metis_grid::GridTile> = Vec::new();
        {
            let desk = self.desk_mut_or_default(output_key);
            desk.layout.tiles.retain(|t| {
                if matches!(t.kind, TileKind::App { .. }) {
                    stashed.push(t.clone());
                    false
                } else {
                    true
                }
            });
        }
        // Hide the windows that just left the visible workspace.
        for tile in &stashed {
            if let TileKind::App { window_id: Some(wid), .. } = &tile.kind {
                if let Some(record) = self.windows.get(*wid).cloned() {
                    self.space.unmap_elem(&record.window);
                }
            }
        }
        {
            let desk = self.desk_mut_or_default(output_key);
            desk.stashed_app_tiles.insert(current, stashed);
            desk.active_workspace = target;
            // Restore the target workspace's app tiles.
            if let Some(tiles) = desk.stashed_app_tiles.remove(&target) {
                desk.layout.tiles.extend(tiles);
            }
        }
        self.refresh_scroll_offset(output_key, false);
        if self.layout_kind_for(output_key, target) == metis_grid::LayoutKind::Grid {
            self.auto_reflow_grid_apps(output_key, self.last_focused_window.or(self.focused_window_id()), false);
        }
        for id in self.window_ids_on_workspace(output_key, target) {
            self.ensure_app_tile_for_window(id);
        }
        self.reposition_all_windows();
        self.focus_topmost_on_active_workspace();

        self.damaged = true;
        self.request_redraw();
        self.emit_layout_changed();
        self.emit_workspace_changed(output_key);
    }

    /// Output names sorted left-to-right (then top-to-bottom) for adjacent-monitor
    /// navigation.
    fn output_keys_left_to_right(&self) -> Vec<String> {
        let mut outputs: Vec<_> = self.space.outputs().collect();
        outputs.sort_by_key(|o| {
            self.space
                .output_geometry(o)
                .map(|g| (g.loc.x, g.loc.y))
                .unwrap_or((0, 0))
        });
        outputs.into_iter().map(|o| o.name()).collect()
    }

    fn adjacent_output_key(&self, from: &str, direction: i32) -> Option<String> {
        let keys = self.output_keys_left_to_right();
        let idx = keys.iter().position(|k| k == from)?;
        let next = idx as i32 + direction;
        if next < 0 || next >= keys.len() as i32 {
            return None;
        }
        Some(keys[next as usize].clone())
    }

    /// Remove a window's app tile from one output desk (visible layout or stash).
    fn take_app_tile_from_desk(
        &mut self,
        desk_key: &str,
        window_id: u32,
        workspace: u32,
    ) -> Option<metis_grid::GridTile> {
        let tile_id = format!("app-{window_id}");
        let desk = self.desk_mut_or_default(desk_key);
        if let Some(pos) = desk.layout.tiles.iter().position(|t| t.id == tile_id) {
            return Some(desk.layout.tiles.remove(pos));
        }
        if let Some(tiles) = desk.stashed_app_tiles.get_mut(&workspace) {
            if let Some(pos) = tiles.iter().position(|t| t.id == tile_id) {
                return Some(tiles.remove(pos));
            }
        }
        for tiles in desk.stashed_app_tiles.values_mut() {
            if let Some(pos) = tiles.iter().position(|t| t.id == tile_id) {
                return Some(tiles.remove(pos));
            }
        }
        None
    }

    fn remove_window_from_desk_scroll(&mut self, desk_key: &str, window_id: u32, workspace: u32) {
        if let Some(desk) = self.desks.get_mut(desk_key) {
            if let Some(scroll) = desk.scroll.get_mut(&workspace) {
                scroll.remove_window(window_id);
            }
        }
    }

    /// Move a window to another output, keeping its workspace number. Desk tiles
    /// and scroll membership follow the window; visibility follows the destination
    /// output's active workspace.
    pub fn move_window_to_output(&mut self, window_id: u32, target_key: &str) {
        self.move_window_to_output_inner(window_id, target_key, true);
    }

    /// Like [`move_window_to_output`](Self::move_window_to_output) but optionally
    /// skips geometry clamp/reposition (used when a snap immediately follows).
    fn move_window_to_output_inner(
        &mut self,
        window_id: u32,
        target_key: &str,
        reposition: bool,
    ) {
        if target_key.is_empty() {
            return;
        }
        if self.output_by_name(target_key).is_none() && !self.desks.contains_key(target_key) {
            return;
        }
        self.desk_mut_or_default(target_key);

        let source_key = self.desk_key_for_window(window_id);
        if source_key == target_key {
            return;
        }

        let workspace = self.windows.workspace(window_id).unwrap_or(1);
        let source_active = self.active_workspace_for(&source_key);
        let target_active = self.active_workspace_for(target_key);
        let was_visible = workspace == source_active;
        let will_be_visible = workspace == target_active;

        let mut tile = self.take_app_tile_from_desk(&source_key, window_id, workspace);
        self.remove_window_from_desk_scroll(&source_key, window_id, workspace);

        if tile.is_none() {
            let class = self.windows.get(window_id).and_then(|r| r.app_id.clone());
            tile = Some(metis_grid::GridTile {
                id: format!("app-{window_id}"),
                rect: default_app_tile_rect(&self.desk(target_key).map(|d| &d.layout).unwrap_or(&self.default_layout)),
                kind: TileKind::App {
                    window_id: Some(window_id),
                    class,
                },
                glow: "cool".into(),
                pinned: false,
                min_w: None,
                max_w: None,
                min_h: None,
                max_h: None,
            });
        }

        self.windows
            .set_output(window_id, target_key.to_string());

        if let Some(tile) = tile {
            let desk = self.desk_mut_or_default(target_key);
            if will_be_visible {
                desk.layout.tiles.push(tile);
            } else {
                desk.stashed_app_tiles
                    .entry(workspace)
                    .or_default()
                    .push(tile);
            }
        }

        if self.layout_kind_for(target_key, workspace) == metis_grid::LayoutKind::Scroll {
            self.seed_scroll_state(target_key, workspace);
            self.refresh_scroll_offset(target_key, false);
        }

        if was_visible && !will_be_visible {
            if let Some(record) = self.windows.get(window_id).cloned() {
                self.space.unmap_elem(&record.window);
            }
            self.focus_topmost_on_active_workspace();
        } else if will_be_visible && reposition {
            if self.floating.contains(&window_id) {
                // Auto-hide / snapped windows keep their footprint — never apply
                // the ordinary floating titlebar inset (`APP_TILE_HEADER_PX`).
                if !self.auto_hide_titlebar.contains(&window_id)
                    && !self.windows.is_snapped(window_id)
                {
                    if let Some(rect) = self.windows.target_rect(window_id) {
                        let clamped = self.clamp_floating_rect_for(window_id, rect);
                        if clamped != rect {
                            self.windows.set_target_rect(window_id, clamped);
                        }
                    }
                }
                self.apply_window_rect(window_id);
            } else {
                self.apply_window_rect(window_id);
            }
            self.focus_window_id(window_id);
        }

        self.damaged = true;
        self.request_redraw();
        self.emit_layout_changed();
        self.emit_workspace_changed(&source_key);
        self.emit_workspace_changed(target_key);
    }

    /// If a window's center sits on a different output than its assigned desk,
    /// re-home it there. Called after a drag-drop or snap on another monitor.
    pub fn maybe_adopt_window_output(&mut self, window_id: u32) {
        let Some(target) = self.output_for_window(window_id).map(|o| o.name()) else {
            return;
        };
        if target == self.desk_key_for_window(window_id) {
            return;
        }
        self.move_window_to_output(window_id, &target);
    }

    /// Move the focused window one output to the left (`direction` = -1) or right (+1).
    pub fn move_window_to_adjacent_output(&mut self, window_id: u32, direction: i32) {
        let from = self.desk_key_for_window(window_id);
        let Some(target) = self.adjacent_output_key(&from, direction) else {
            return;
        };
        self.move_window_to_output(window_id, &target);
    }

    /// Move every window on `workspace` from `source_key` to `target_key` (keeping
    /// the same workspace number). Layout mode and scroll state for that workspace
    /// move with the windows. Only valid in independent per-output workspace mode.
    pub fn move_workspace_to_output(
        &mut self,
        source_key: &str,
        workspace: u32,
        target_key: &str,
    ) {
        if source_key.is_empty() || target_key.is_empty() || source_key == target_key {
            return;
        }
        if self.workspace_mode() != metis_config::WorkspaceMode::Separate {
            return;
        }
        if self.output_by_name(target_key).is_none() && !self.desks.contains_key(target_key) {
            return;
        }
        self.desk_mut_or_default(target_key);

        let ws = workspace.clamp(1, self.workspace_count());
        let source_active = self.active_workspace_for(source_key);
        let target_active = self.active_workspace_for(target_key);
        let was_visible_on_source = ws == source_active;
        let will_be_visible_on_target = ws == target_active;

        let window_ids: Vec<u32> = self
            .windows
            .ids()
            .into_iter()
            .filter(|&id| {
                self.desk_key_for_window(id) == source_key
                    && self.windows.workspace(id) == Some(ws)
            })
            .collect();

        let (kind, scroll, mut tiles) = {
            let desk = self.desk_mut_or_default(source_key);
            let kind = desk.layout_kind.remove(&ws);
            let scroll = desk.scroll.remove(&ws);
            let mut tiles = desk.stashed_app_tiles.remove(&ws).unwrap_or_default();
            if was_visible_on_source {
                let on_ws: std::collections::HashSet<u32> = window_ids.iter().copied().collect();
                desk.layout.tiles.retain(|t| {
                    if let TileKind::App { window_id: Some(wid), .. } = &t.kind {
                        if on_ws.contains(wid) {
                            tiles.push(t.clone());
                            return false;
                        }
                    }
                    true
                });
            }
            (kind, scroll, tiles)
        };

        let default_layout = self
            .desk(target_key)
            .map(|d| &d.layout)
            .unwrap_or(&self.default_layout);
        for &id in &window_ids {
            let tile_id = format!("app-{id}");
            if tiles.iter().any(|t| t.id == tile_id) {
                continue;
            }
            let class = self.windows.get(id).and_then(|r| r.app_id.clone());
            tiles.push(metis_grid::GridTile {
                id: tile_id,
                rect: default_app_tile_rect(default_layout),
                kind: TileKind::App {
                    window_id: Some(id),
                    class,
                },
                glow: "cool".into(),
                pinned: false,
                min_w: None,
                max_w: None,
                min_h: None,
                max_h: None,
            });
        }

        if was_visible_on_source {
            for &id in &window_ids {
                if let Some(record) = self.windows.get(id).cloned() {
                    self.space.unmap_elem(&record.window);
                }
            }
        }

        for &id in &window_ids {
            self.windows.set_output(id, target_key.to_string());
        }

        {
            let desk = self.desk_mut_or_default(target_key);
            if let Some(k) = kind {
                desk.layout_kind.insert(ws, k);
            }
            if let Some(s) = scroll {
                desk.scroll.insert(ws, s);
            }
            if will_be_visible_on_target {
                desk.layout.tiles.extend(tiles);
            } else {
                desk.stashed_app_tiles.entry(ws).or_default().extend(tiles);
            }
        }

        if will_be_visible_on_target {
            self.refresh_scroll_offset(target_key, false);
            for &id in &window_ids {
                if !self.windows.is_minimized(id) {
                    self.apply_window_rect(id);
                }
            }
            self.focus_topmost_on_active_workspace();
        } else if was_visible_on_source {
            self.focus_topmost_on_active_workspace();
        }

        self.damaged = true;
        self.request_redraw();
        self.emit_layout_changed();
        self.emit_workspace_changed(source_key);
        self.emit_workspace_changed(target_key);
    }

    /// Move the active workspace on `source_key` one output left/right.
    pub fn move_active_workspace_to_adjacent_output(&mut self, source_key: &str, direction: i32) {
        let ws = self.active_workspace_for(source_key);
        let Some(target) = self.adjacent_output_key(source_key, direction) else {
            return;
        };
        self.move_workspace_to_output(source_key, ws, &target);
    }

    /// True when the workspace under the pointer uses the scrolling layout (so
    /// Super+Shift+arrow is reserved for scroll navigation).
    pub fn scroll_navigation_active(&self) -> bool {
        let key = self
            .output_under_pointer()
            .map(|o| o.name())
            .unwrap_or_else(|| self.primary_key());
        self.active_layout_kind(&key) == metis_grid::LayoutKind::Scroll
    }

    /// Move a window to another workspace on its own output. When it leaves or
    /// joins that output's visible workspace its tile is stashed/restored and the
    /// window is hidden/shown.
    pub fn move_window_to_workspace(&mut self, window_id: u32, target: u32) {
        let target = target.clamp(1, self.workspace_count());
        let Some(current) = self.windows.workspace(window_id) else {
            return;
        };
        if target == current {
            return;
        }
        let key = self.desk_key_for_window(window_id);
        self.windows.set_workspace(window_id, target);
        let tile_id = format!("app-{window_id}");
        let active = self.active_workspace_for(&key);

        if current == active {
            // Leaving the visible workspace: stash its tile and hide it.
            let mut moved: Vec<metis_grid::GridTile> = Vec::new();
            {
                let desk = self.desk_mut_or_default(&key);
                desk.layout.tiles.retain(|t| {
                    if t.id == tile_id {
                        moved.push(t.clone());
                        false
                    } else {
                        true
                    }
                });
            }
            if let Some(record) = self.windows.get(window_id).cloned() {
                self.space.unmap_elem(&record.window);
            }
            self.desk_mut_or_default(&key)
                .stashed_app_tiles
                .entry(target)
                .or_default()
                .extend(moved);
            self.reposition_all_windows();
            self.focus_topmost_on_active_workspace();
        } else if target == active {
            // Joining the visible workspace: pull its tile back into the grid.
            let desk = self.desk_mut_or_default(&key);
            if let Some(tiles) = desk.stashed_app_tiles.get_mut(&current) {
                if let Some(pos) = tiles.iter().position(|t| t.id == tile_id) {
                    let tile = tiles.remove(pos);
                    desk.layout.tiles.push(tile);
                }
            }
            self.reposition_all_windows();
        } else {
            // Hidden-to-hidden: just relocate the stashed tile.
            let desk = self.desk_mut_or_default(&key);
            let tile = desk.stashed_app_tiles.get_mut(&current).and_then(|tiles| {
                tiles
                    .iter()
                    .position(|t| t.id == tile_id)
                    .map(|pos| tiles.remove(pos))
            });
            if let Some(tile) = tile {
                desk.stashed_app_tiles.entry(target).or_default().push(tile);
            }
        }

        // Keep the scroll strips in sync: drop from the source workspace, add to
        // the target if it scrolls.
        self.remove_from_scroll_everywhere(window_id);
        if self.layout_kind_for(&key, target) == metis_grid::LayoutKind::Scroll {
            self.seed_scroll_state(&key, target);
        }
        self.refresh_scroll_offset(&key, false);

        self.damaged = true;
        self.request_redraw();
        self.emit_layout_changed();
        // Nudge the shell to reconcile its window cache so per-output/per-workspace
        // dock filtering reflects the move promptly (the active workspace itself is
        // unchanged; this just carries the refresh).
        self.emit_workspace_changed(&key);
    }

    /// Give keyboard focus to the topmost mapped window on its output's active
    /// workspace, or clear focus if no eligible window is visible.
    fn focus_topmost_on_active_workspace(&mut self) {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        // `space.elements()` is bottom-to-top; the last match is the topmost.
        let ordered: Vec<Window> = self.space.elements().cloned().collect();
        let candidate = ordered.into_iter().rev().find_map(|w| {
            let id = self.windows.id_for_window(&w)?;
            let key = self.desk_key_for_window(id);
            let on_active = self.windows.workspace(id) == Some(self.active_workspace_for(&key));
            if on_active && !self.windows.is_minimized(id) {
                Some((id, w))
            } else {
                None
            }
        });
        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
        match candidate {
            Some((id, window)) => {
                self.space.raise_element(&window, true);
                keyboard.set_focus(self, Some(window.into()), serial);
                self.event_bus
                    .emit(&metis_protocol::CompositorEvent::WindowFocused { id });
            }
            None => {
                keyboard.set_focus(self, Option::<KeyboardFocusTarget>::None, serial);
            }
        }
    }

    pub fn handle_ipc(&mut self, cmd: CompositorCommand) -> metis_protocol::CompositorEvent {
        use metis_protocol::CompositorEvent;
        match cmd {
            CompositorCommand::Ping => CompositorEvent::Pong,
            CompositorCommand::GetMonitor => CompositorEvent::Monitor {
                rect: self.monitor,
            },
            CompositorCommand::ListOutputs => {
                let primary = self.primary_output().map(|o| o.name());
                let mut entries: Vec<(String, MonitorRect)> = self
                    .space
                    .outputs()
                    .filter_map(|o| {
                        let geo = self.space.output_geometry(o)?;
                        Some((
                            o.name(),
                            MonitorRect {
                                x: geo.loc.x,
                                y: geo.loc.y,
                                width: geo.size.w,
                                height: geo.size.h,
                            },
                        ))
                    })
                    .collect();
                entries.sort_by_key(|(_, rect)| (rect.x, rect.y));
                let outputs = entries
                    .into_iter()
                    .map(|(name, rect)| metis_protocol::OutputInfo {
                        name: name.clone(),
                        primary: primary.as_deref() == Some(name.as_str()),
                        rect,
                    })
                    .collect();
                CompositorEvent::OutputList { outputs }
            }
            CompositorCommand::GetLayout => {
                let key = self.primary_key();
                let layout = self
                    .desk(&key)
                    .map(|d| d.layout.clone())
                    .unwrap_or_else(|| self.default_layout.clone());
                CompositorEvent::LayoutChanged {
                    layout,
                    gutter_px: self.gutter_px,
                    metrics: self.grid_metrics(),
                }
            }
            CompositorCommand::ListWindows => {
                let focused = self.focused_window_id();
                let mut windows = self.windows.list();
                if let Some(fid) = focused {
                    for w in &mut windows {
                        w.focused = w.id == fid;
                    }
                }
                CompositorEvent::WindowList { windows }
            }
            CompositorCommand::MoveWindow { id, rect } => {
                self.windows.set_target_rect(id, rect);
                self.apply_window_rect(id);
                CompositorEvent::LayoutApplied
            }
            CompositorCommand::CloseWindow { id } => {
                self.close_window(id);
                CompositorEvent::WindowClosed { id }
            }
            CompositorCommand::FocusWindow { id } => {
                if let Some(record) = self.windows.get(id).cloned() {
                    self.note_window_focus(id);
                    self.space.raise_element(&record.window, true);
                    let serial = smithay::utils::SERIAL_COUNTER.next_serial();
                    self.seat.get_keyboard().unwrap().set_focus(
                        self,
                        Some(record.window.clone().into()),
                        serial,
                    );
                    self.event_bus
                        .emit(&CompositorEvent::WindowFocused { id });
                    self.schedule_redraw();
                    CompositorEvent::WindowFocused { id }
                } else {
                    CompositorEvent::Error {
                        message: format!("window {id} not found"),
                    }
                }
            }
            CompositorCommand::SetMinimized { id, minimized } => {
                if self.windows.get(id).is_none() {
                    CompositorEvent::Error {
                        message: format!("window {id} not found"),
                    }
                } else {
                    if minimized {
                        self.minimize_by_id(id);
                    } else {
                        self.activate_window_by_id(id);
                    }
                    CompositorEvent::LayoutApplied
                }
            }
            CompositorCommand::ActivateWindow { id } => {
                if self.windows.get(id).is_none() {
                    CompositorEvent::Error {
                        message: format!("window {id} not found"),
                    }
                } else {
                    self.activate_window_by_id(id);
                    CompositorEvent::WindowFocused { id }
                }
            }
            CompositorCommand::SetFullscreen { id, enabled } => {
                self.set_fullscreen(id, enabled);
                CompositorEvent::LayoutApplied
            }
            CompositorCommand::ApplyLayout { layout, gutter_px } => {
                self.apply_grid_layout(layout, gutter_px);
                CompositorEvent::LayoutApplied
            }
            CompositorCommand::SetTileMode { tile_id, mode } => {
                self.set_tile_mode(&tile_id, mode);
                CompositorEvent::LayoutApplied
            }
            CompositorCommand::SwitchWorkspace { output, id } => {
                let key = output.filter(|o| !o.is_empty()).unwrap_or_else(|| {
                    self.output_under_pointer()
                        .map(|o| o.name())
                        .unwrap_or_else(|| self.primary_key())
                });
                self.switch_workspace_routed(&key, id);
                CompositorEvent::WorkspaceChanged {
                    output: key.clone(),
                    active: self.active_workspace_for(&key),
                    count: self.workspace_count(),
                }
            }
            CompositorCommand::MoveWindowToWorkspace { window_id, workspace } => {
                if self.windows.get(window_id).is_none() {
                    CompositorEvent::Error {
                        message: format!("window {window_id} not found"),
                    }
                } else {
                    self.move_window_to_workspace(window_id, workspace);
                    CompositorEvent::LayoutApplied
                }
            }
            CompositorCommand::MoveWindowToOutput { window_id, output } => {
                if self.windows.get(window_id).is_none() {
                    CompositorEvent::Error {
                        message: format!("window {window_id} not found"),
                    }
                } else {
                    let key = output.filter(|o| !o.is_empty()).unwrap_or_else(|| {
                        self.output_under_pointer()
                            .map(|o| o.name())
                            .unwrap_or_else(|| self.primary_key())
                    });
                    self.move_window_to_output(window_id, &key);
                    CompositorEvent::LayoutApplied
                }
            }
            CompositorCommand::MoveWorkspaceToOutput {
                output,
                workspace,
                target_output,
            } => {
                if target_output.is_empty() {
                    CompositorEvent::Error {
                        message: "target_output is required".into(),
                    }
                } else if self.workspace_mode() != metis_config::WorkspaceMode::Separate {
                    CompositorEvent::Error {
                        message: "MoveWorkspaceToOutput requires independent per-output workspaces"
                            .into(),
                    }
                } else {
                    let source = output.filter(|o| !o.is_empty()).unwrap_or_else(|| {
                        self.output_under_pointer()
                            .map(|o| o.name())
                            .unwrap_or_else(|| self.primary_key())
                    });
                    let ws = workspace
                        .unwrap_or_else(|| self.active_workspace_for(&source));
                    self.move_workspace_to_output(&source, ws, &target_output);
                    CompositorEvent::LayoutApplied
                }
            }
            CompositorCommand::SetWorkspaceLayout { output, workspace, kind } => {
                let key = output.filter(|o| !o.is_empty()).unwrap_or_else(|| {
                    self.output_under_pointer()
                        .map(|o| o.name())
                        .unwrap_or_else(|| self.primary_key())
                });
                // A specific non-active workspace is set quietly (it's hidden);
                // otherwise act on the output's active workspace (rebuilds the
                // strip + repositions live).
                match workspace {
                    Some(ws) if ws != self.active_workspace_for(&key) => {
                        self.set_layout_kind_on(&key, ws, kind);
                    }
                    _ => self.set_layout_kind(&key, kind),
                }
                CompositorEvent::LayoutApplied
            }
            CompositorCommand::SetDefaultLayout { kind } => {
                self.set_layout_kind_all(kind);
                CompositorEvent::LayoutApplied
            }
            CompositorCommand::SubscribeEvents => CompositorEvent::Pong,
            CompositorCommand::Launch { program } => {
                // Route through spawn_client so the child inherits the nested
                // Wayland env (WAYLAND_DISPLAY, GDK_BACKEND, cursor theme) and is
                // tracked for cleanup — a bare `sh -c` had no Wayland display.
                self.spawn_client(&program);
                CompositorEvent::Pong
            }
            CompositorCommand::EndSession => {
                tracing::info!("EndSession requested — stopping compositor event loop");
                self.loop_signal.stop();
                CompositorEvent::Pong
            }
            CompositorCommand::ApplyBackground => {
                self.wallpaper.apply_config();
                let (full, regions) = self.wallpaper_layout();
                self.wallpaper.set_layout(full, regions);
                self.wallpaper.start_async_decode();
                self.damaged = true;
                self.request_redraw();
                CompositorEvent::Pong
            }
            CompositorCommand::ReloadInput => {
                let cfg = self.input_runtime.reload_from_disk();
                crate::device_input::apply_keyboard(self, &cfg);
                CompositorEvent::Pong
            }
        }
    }

    /// The wallpaper layout: the whole virtual desktop's physical size plus one
    /// region per output (global physical origin + size). The wallpaper composes
    /// a single framebuffer-sized texture by cover-cropping each output's image
    /// into its region, so every monitor is filled independently.
    pub fn wallpaper_layout(
        &self,
    ) -> (
        smithay::utils::Size<i32, smithay::utils::Physical>,
        Vec<crate::wallpaper::OutputRegion>,
    ) {
        let bounds = self.desktop_bounds();
        let full = smithay::utils::Size::from((bounds.size.w, bounds.size.h)).to_physical(1);
        let regions = self
            .space
            .outputs()
            .filter_map(|o| {
                let geo = self.space.output_geometry(o)?;
                Some(crate::wallpaper::OutputRegion {
                    name: o.name(),
                    origin: (geo.loc - bounds.loc).to_physical(1),
                    size: geo.size.to_physical(1),
                })
            })
            .collect();
        (full, regions)
    }

    pub fn register_new_window(&mut self, window: Window, title: String, app_id: Option<String>) {
        let id = self.windows.register(window, title, app_id);
        // New windows open on the output under the cursor, joining that output's
        // currently-visible workspace.
        let key = self
            .output_under_pointer()
            .map(|o| o.name())
            .unwrap_or_else(|| self.primary_key());
        self.windows.set_output(id, key.clone());
        self.windows.set_workspace(id, self.active_workspace_for(&key));
        self.ensure_app_tile_for_window(id);
    }

    /// Send the initial xdg configure as soon as the client makes its first
    /// commit, rather than waiting for a later layout/placement pass.
    ///
    /// A Wayland client cannot attach its first buffer until it has acked the
    /// initial configure, so deferring it stalls the window's first paint. With
    /// the old behavior a toplevel only got configured as a side effect of an
    /// unrelated layout pass — terminals like foot/alacritty/kitty could hang
    /// for many seconds, or forever if nothing else happened. Priming the
    /// configure here decouples client startup from Metis's layout passes.
    ///
    /// The configure carries the real placement size (saved geometry / grid
    /// tile) so the window opens at its final size instead of a placeholder.
    pub fn ensure_initial_configure(&mut self, id: u32) {
        let Some(record) = self.windows.get(id).cloned() else {
            return;
        };
        if record.toplevel.is_initial_configure_sent() {
            return;
        }
        // Make sure metadata + placement are decided before the configure goes
        // out, so the size is correct on the very first map.
        let (title, app_id) = read_toplevel_metadata(&record.toplevel);
        self.windows.set_metadata(id, title.clone(), app_id.clone());
        self.set_app_tile_display_name(id, &title, app_id.as_deref());
        if !self.floating.contains(&id) && !self.windows.placement_chosen(id) {
            self.place_new_window(id, app_id.as_deref());
        }
        self.refresh_window_decoration_mode(id);
        self.apply_window_rect(id);
    }

    pub fn activate_window(&mut self, id: u32) {
        use metis_protocol::CompositorEvent;

        // Before tile reflow/reposition — `ensure_app_tile_for_window` can call
        // `reposition_all_windows`, whose stacking restore must not fall back to a
        // maximized neighbor while this window is still being mapped.
        self.note_window_focus(id);

        let key = self.desk_key_for_window(id);
        let ws = self.windows.workspace(id).unwrap_or(1);
        let kind = self.layout_kind_for(&key, ws);
        if kind == metis_grid::LayoutKind::Free {
            // Grid tiles must not drive placement while the workspace is floating.
            self.remove_app_tile_everywhere(id);
        }

        self.ensure_app_tile_for_window(id);
        let Some(record) = self.windows.get(id).cloned() else {
            return;
        };

        let (title, app_id) = read_toplevel_metadata(&record.toplevel);
        self.windows.set_metadata(id, title.clone(), app_id.clone());
        self.set_app_tile_display_name(id, &title, app_id.as_deref());
        self.refresh_window_decoration_mode(id);

        let already_ready = self.windows.is_ready(id);
        // Choose placement before the first map whenever the window is not yet
        // floating (e.g. app_id arrived after the initial configure).
        if kind == metis_grid::LayoutKind::Free {
            self.place_new_window(id, app_id.as_deref());
        } else if !self.floating.contains(&id) && !self.windows.placement_chosen(id) {
            self.place_new_window(id, app_id.as_deref());
        }
        self.apply_window_rect(id);

        if already_ready {
            return;
        }

        self.windows.set_ready(id, true);

        let suggested_rect = self
            .windows
            .target_rect(id)
            .or_else(|| {
                self.rect_for_window_tile(id)
                    .map(|full| self.tile_client_rect(id, full))
            })
            .unwrap_or(PixelRect {
                x: 0,
                y: 0,
                width: 800,
                height: 600,
            });

        self.persist_layout();
        self.emit_layout_changed();
        self.event_bus.emit(&CompositorEvent::WindowOpened {
            id,
            title,
            app_id,
            suggested_rect,
        });

        // A freshly mapped window becomes the active one: raise it, give it
        // keyboard focus, and report the focus to the shell. Without this the
        // taskbar starts with no focused window, so the first click on a dock
        // icon only re-focuses the (already visible) app instead of minimizing
        // it, forcing a wasted first click.
        if let Some(keyboard) = self.seat.get_keyboard() {
            self.space.raise_element(&record.window, true);
            let serial = smithay::utils::SERIAL_COUNTER.next_serial();
            keyboard.set_focus(self, Some(record.window.clone().into()), serial);
            self.event_bus
                .emit(&CompositorEvent::WindowFocused { id });
        }
    }

    pub(crate) fn set_app_tile_display_name(&mut self, window_id: u32, title: &str, app_id: Option<&str>) {
        let display = app_display_name(app_id, title);
        let tile_id = format!("app-{window_id}");
        let key = self.desk_key_for_window(window_id);
        if let Some(desk) = self.desks.get_mut(&key) {
            if let Some(tile) = desk.layout.tiles.iter_mut().find(|t| t.id == tile_id) {
                if let TileKind::App {
                    window_id: wid,
                    class,
                } = &mut tile.kind
                {
                    *wid = Some(window_id);
                    *class = Some(display);
                }
            }
        }
    }

    pub fn try_activate_committed_window(&mut self, surface: &smithay::reexports::wayland_server::protocol::wl_surface::WlSurface) {
        let Some(id) = self.windows.id_for_surface(surface) else {
            return;
        };
        if self.windows.is_ready(id) {
            return;
        }
        // Must read the *renderer* surface state, not `SurfaceAttributes.buffer`:
        // `on_commit_buffer_handler` (run at the top of the commit handler) consumes
        // the attribute buffer, so `SurfaceAttributes.current().buffer` is `None`
        // except on the exact frame a buffer was attached. That made activation
        // (and therefore `WindowOpened` + the `ready` flag) effectively never fire.
        let has_buffer =
            smithay::backend::renderer::utils::with_renderer_surface_state(surface, |state| {
                state.buffer().is_some()
            })
            .unwrap_or(false);
        if has_buffer {
            self.activate_window(id);
        }
    }

    pub fn set_tile_mode(&mut self, tile_id: &str, mode: metis_protocol::TileMode) {
        use metis_protocol::TileMode;

        let key = self
            .desk_key_for_tile(tile_id)
            .unwrap_or_else(|| self.primary_key());
        let window_id = self.desk(&key).and_then(|d| {
            d.layout.tiles.iter().find_map(|t| {
                if t.id != tile_id {
                    return None;
                }
                if let TileKind::App {
                    window_id: Some(wid),
                    ..
                } = &t.kind
                {
                    Some(*wid)
                } else {
                    None
                }
            })
        });

        match mode {
            TileMode::Grid => {
                let layout_restored = self.tile_modes.exit(tile_id);
                if let Some(restored) = layout_restored {
                    if let Some(desk) = self.desks.get_mut(&key) {
                        if let Some(tile) = desk.layout.tile_mut(tile_id) {
                            tile.rect = restored;
                        }
                    }
                }
                if let Some(id) = window_id {
                    if self.windows.is_minimized(id) {
                        self.unminimize_window(id);
                    }
                    self.set_fullscreen(id, false);
                    self.set_maximized(id, false);
                }
                if layout_restored.is_some() {
                    self.reposition_all_windows();
                    self.persist_layout();
                    self.emit_layout_changed();
                }
            }
            TileMode::AppFullscreen => {
                if let Some(layout) = self.desk(&key).map(|d| d.layout.clone()) {
                    self.tile_modes
                        .enter(&layout, tile_id, metis_grid::TileMode::AppFullscreen);
                }
                if let Some(id) = window_id {
                    self.set_maximized(id, true);
                }
            }
            TileMode::Minimized => {
                if let Some(layout) = self.desk(&key).map(|d| d.layout.clone()) {
                    self.tile_modes
                        .enter(&layout, tile_id, metis_grid::TileMode::Minimized);
                }
                if let Some(id) = window_id {
                    self.minimize_window(id);
                }
            }
            TileMode::Immersive => {
                if let Some(layout) = self.desk(&key).map(|d| d.layout.clone()) {
                    self.tile_modes
                        .enter(&layout, tile_id, metis_grid::TileMode::Immersive);
                }
                tracing::info!(tile_id, "immersive mode requested (shell handles chrome)");
            }
        }
    }

    pub fn on_window_destroyed(&mut self, id: u32) {
        use metis_protocol::CompositorEvent;

        if self.last_focused_window == Some(id) {
            self.last_focused_window = None;
        }
        self.save_window_geometry(id);
        self.floating.remove(&id);
        self.clear_auto_hide(id);
        let desk_key = self.desk_key_for_window(id);
        self.remove_app_tile_everywhere(id);
        self.auto_reflow_grid_apps(&desk_key, self.focused_window_id(), false);
        // Grid reflow above is a no-op on scroll workspaces; re-snap the offset and
        // slide the surviving columns over to close the gap the closed window left.
        self.refresh_all_scroll_offsets();
        self.reposition_scroll_windows();
        self.persist_layout();
        self.event_bus.emit(&CompositorEvent::WindowClosed { id });
    }

    pub fn cleanup_destroyed_windows(&mut self) {
        // Only drop registry entries whose Wayland resources are actually gone.
        // Unmapped windows (minimized, pending first commit) remain alive and must
        // not be treated as destroyed just because they are absent from the space.
        let stale: Vec<u32> = self
            .windows
            .ids()
            .into_iter()
            .filter(|id| {
                self.windows
                    .get(*id)
                    .is_some_and(|record| !record.window.alive())
            })
            .collect();

        for id in stale {
            // Remember floating app geometry before the record is dropped.
            self.save_window_geometry(id);
            if let Some(record) = self.windows.unregister(id) {
                self.space.unmap_elem(&record.window);
            }
            self.on_window_destroyed(id);
        }
    }
}

fn default_app_tile_rect(layout: &GridLayout) -> metis_grid::TileRect {
    let rows = layout.rows.max(8);
    let cols = layout.columns.max(12);
    // Open new apps as a large, centered tile rather than a small bottom-left
    // cell, so a freshly launched window is immediately usable.
    let w = (cols * 2 / 3).clamp(4, cols);
    let h = (rows * 2 / 3).clamp(3, rows);
    let col = (cols - w) / 2;
    let row = (rows - h) / 2;
    metis_grid::TileRect::new(col, row, w, h)
}

/// When re-anchoring an oversized client, keep this placement-zone edge flush
/// with the footprint (overflow spills toward the interior).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BarEdgeAnchor {
    Min,
    Max,
}

/// Pick a window origin on one axis that preserves the footprint's screen-edge
/// gap when the client is larger than the footprint. Honors the footprint origin
/// when the client fits; otherwise anchors to whichever screen edge the footprint
/// hugs (so the overflow grows toward the opposite, interior side).
fn anchor_axis(
    foot_min: i32,
    foot_size: i32,
    zone_min: i32,
    zone_size: i32,
    actual: i32,
    gap_min: i32,
    gap_max: i32,
    bar_edge: Option<BarEdgeAnchor>,
) -> i32 {
    if actual <= foot_size {
        return foot_min;
    }
    let foot_max = foot_min + foot_size;
    let zone_max = zone_min + zone_size;
    let touches_min = foot_min - zone_min <= gap_min;
    let touches_max = zone_max - foot_max <= gap_max;

    // Maximize hugs both zone edges — keep the bar-adjacent side fixed so any
    // overflow spills away from the edge bar instead of underneath it.
    if touches_min && touches_max {
        return match bar_edge {
            Some(BarEdgeAnchor::Max) => foot_max - actual,
            Some(BarEdgeAnchor::Min) | None => foot_min,
        };
    }
    if touches_max && !touches_min {
        foot_max - actual
    } else {
        foot_min
    }
}

/// Apply maximize-consistent edge gaps to a raw snap region. Sides on the
/// usable-zone boundary use `gaps` (bar-adjacent edges use `BAR_GAP_PX`, bare
/// screen edges use `WINDOW_GAP_PX`); interior split lines get half `WINDOW_GAP_PX`.
fn snap_client_rect(raw: PixelRect, zone: PixelRect, gaps: ZoneGaps) -> PixelRect {
    let half = WINDOW_GAP_PX / 2;
    let touches_left = raw.x <= zone.x;
    let touches_right = raw.x + raw.width >= zone.x + zone.width;
    let touches_top = raw.y <= zone.y;
    let touches_bottom = raw.y + raw.height >= zone.y + zone.height;

    let l = if touches_left { gaps.left } else { half };
    let r = if touches_right { gaps.right } else { half };
    let t = if touches_top { gaps.top } else { half };
    let b = if touches_bottom { gaps.bottom } else { half };

    PixelRect {
        x: raw.x + l,
        y: raw.y + t,
        width: (raw.width - l - r).max(1),
        height: (raw.height - t - b).max(1),
    }
}

#[derive(Default)]
pub struct ClientState {
    pub compositor_state: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, client_id: ClientId, reason: DisconnectReason) {
        // A protocol error means a client (e.g. the shell's gtk4-layer-shell)
        // sent something invalid and was force-disconnected; surface the exact
        // object/code/message so these are diagnosable instead of silent.
        match reason {
            DisconnectReason::ProtocolError(err) => tracing::error!(
                ?client_id,
                object = %err.object_interface,
                code = err.code,
                message = %err.message,
                "client disconnected: protocol error"
            ),
            other => tracing::info!(?client_id, ?other, "client disconnected"),
        }
    }
}

pub fn desk_config_path() -> std::path::PathBuf {
    directories::ProjectDirs::from("com", "metis", "metis")
        .map(|dirs| dirs.config_dir().join("desk.json"))
        .unwrap_or_else(|| {
            std::env::var("HOME")
                .map(|h| std::path::PathBuf::from(h).join(".config/metis/desk.json"))
                .unwrap_or_else(|_| std::path::PathBuf::from(".config/metis/desk.json"))
        })
}

pub(crate) fn read_toplevel_metadata(
    surface: &smithay::wayland::shell::xdg::ToplevelSurface,
) -> (String, Option<String>) {
    use smithay::wayland::compositor::with_states;
    use smithay::wayland::shell::xdg::XdgToplevelSurfaceData;

    with_states(surface.wl_surface(), |states| {
        let Some(data) = states.data_map.get::<XdgToplevelSurfaceData>() else {
            return ("Application".into(), None);
        };
        let Ok(role) = data.lock() else {
            return ("Application".into(), None);
        };
        (
            role.title
                .clone()
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| "Application".into()),
            role.app_id.clone().filter(|id| !id.is_empty()),
        )
    })
}

pub(crate) fn read_toplevel_decoration_mode(
    surface: &smithay::wayland::shell::xdg::ToplevelSurface,
) -> Option<smithay::reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode> {
    surface.with_committed_state(|state| state.and_then(|s| s.decoration_mode))
}

fn app_display_name(app_id: Option<&str>, title: &str) -> String {
    if let Some(id) = app_id.filter(|s| !s.is_empty()) {
        return id.to_string();
    }
    let trimmed = title.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("application") {
        return "App".into();
    }
    trimmed.to_string()
}
