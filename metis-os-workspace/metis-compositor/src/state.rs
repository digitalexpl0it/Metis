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
pub const RESIZE_MARGIN_PX: i32 = 8;

/// How far *inside* a window edge the resize grab band reaches. Kept small so the
/// band doesn't swallow edge-hugging client controls (e.g. scrollbars) — you grab
/// the resize mostly from just outside the window instead.
pub const RESIZE_INNER_MARGIN_PX: i32 = 3;

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
    /// right / top-corner): their titlebar auto-hides and only re-appears as a
    /// translucent overlay while the pointer is in the window's top strip.
    pub auto_hide_titlebar: std::collections::HashSet<u32>,
    /// The auto-hide window whose overlay titlebar is currently revealed (pointer
    /// in its top strip), or `None`. Drives both rendering and decoration clicks.
    pub revealed_titlebar: Option<u32>,
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
    /// Active snap-zone preview while a window is being dragged by its titlebar:
    /// the target rect (already inset) plus a short label. `None` when no drag is
    /// in progress or the pointer isn't over a snap band. Drives both the live
    /// overlay and where the window lands on drop.
    pub snap_preview: Option<(PixelRect, &'static str)>,

    pub wallpaper: crate::wallpaper::Wallpaper,
    pub blur: crate::blur::BlurRuntime,
    pub decorations: crate::decoration::DecorationRuntime,

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
        seat.add_keyboard(Default::default(), 200, 25).unwrap();
        seat.add_pointer();

        let space = Space::<Window>::default();
        let socket_name = Self::init_wayland_listener(display, event_loop);
        let loop_signal = event_loop.get_signal();

        let desk_path = desk_config_path();
        let grid_layout = GridLayout::load_from_path(&desk_path);
        let mut grid_layout = grid_layout;
        metis_grid::sanitize_layout(&mut grid_layout);

        Self {
            start_time,
            socket_name,
            display_handle: dh,
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
            snap_preview: None,
            wallpaper: crate::wallpaper::Wallpaper::new(),
            blur: crate::blur::BlurRuntime::default(),
            decorations: crate::decoration::DecorationRuntime::default(),
            redraw_trigger: None,
            damaged: true,
            defer_client_flush: false,
            last_pointer_forward: None,
            last_bar_position: metis_config::load_bar_config().position,
        }
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
            .map(app_tile_body_rect)
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

        // Snaps whose top edge meets the bar (left/right halves, top corners)
        // auto-hide the titlebar like a maximized window, so the client fills the
        // whole snap footprint. Bottom snaps keep a persistent titlebar, so their
        // client is inset to the body to leave the reserved chrome strip.
        let top_touching = matches!(label, "Left half" | "Right half" | "Top-left" | "Top-right");
        let body = if top_touching {
            rect
        } else {
            app_tile_body_rect(rect)
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
        if top_touching {
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
        body = self.clamp_floating_rect(body);

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
            return;
        }

        if self.shell_spawned && !self.client_spawned && elapsed > Duration::from_millis(750) {
            if let Some(client) = self.startup_client.take() {
                self.spawn_client(&client);
            }
            self.client_spawned = true;
            self.startup_frames = 0;
        }

        // After the session client launches, keep trying until it lands in its grid slot.
        if self.client_spawned && self.startup_frames < 120 {
            self.startup_frames = self.startup_frames.saturating_add(1);
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

        cmd.env("WAYLAND_DISPLAY", &self.socket_name);
        // Marks the client as running inside a Metis session, so Metis-aware apps
        // (e.g. the Settings app) can drop their own GTK titlebar and let the
        // compositor draw the server-side chrome instead of doubling it up.
        cmd.env("METIS_SESSION", "1");
        // Point X11-only clients at our nested XWayland server (if running); GTK
        // apps still prefer Wayland via GDK_BACKEND below. Without XWayland, drop
        // DISPLAY so X11 apps don't leak onto the host X server.
        match self.xdisplay {
            Some(n) => {
                cmd.env("DISPLAY", format!(":{n}"));
            }
            None => {
                cmd.env_remove("DISPLAY");
            }
        }
        cmd.env("GDK_BACKEND", "wayland");
        cmd.env("GSK_RENDERER", std::env::var("GSK_RENDERER").unwrap_or_else(|_| "cairo".into()));
        if let Ok(runtime) = std::env::var("XDG_RUNTIME_DIR") {
            cmd.env("XDG_RUNTIME_DIR", runtime);
        }

        // Without these, GTK clients fall back to the built-in (large black) Xcursor
        // because the nested session has no settings daemon. The catch: GNOME's
        // `cursor-theme` is often "default", and the theme literally named "default"
        // IS the big black cursor — on the host it only looks right because it
        // inherits a real theme. So resolve the `Inherits=` chain to a theme that
        // actually ships cursor images (e.g. default -> DMZ-White).
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
        let gsettings_get = |key: &str| -> Option<String> {
            let out = std::process::Command::new("gsettings")
                .args(["get", "org.gnome.desktop.interface", key])
                .output()
                .ok()?;
            if !out.status.success() {
                return None;
            }
            let s = String::from_utf8_lossy(&out.stdout);
            let s = s.trim().trim_matches('\'').trim().to_string();
            if s.is_empty() {
                None
            } else {
                Some(s)
            }
        };

        let theme_pref = std::env::var("XCURSOR_THEME")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| gsettings_get("cursor-theme"))
            .unwrap_or_else(|| "default".into());
        let cursor_theme = resolve_cursor_theme(&theme_pref)
            .or_else(|| resolve_cursor_theme("default"))
            .or_else(|| resolve_cursor_theme("Yaru"))
            .or_else(|| resolve_cursor_theme("Adwaita"))
            .unwrap_or_else(|| "Adwaita".into());
        let cursor_size = std::env::var("XCURSOR_SIZE")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| gsettings_get("cursor-size"))
            .unwrap_or_else(|| "24".into());

        tracing::info!(theme = %cursor_theme, size = %cursor_size, "client cursor theme");
        cmd.env("XCURSOR_THEME", cursor_theme);
        cmd.env("XCURSOR_SIZE", cursor_size);

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
    fn output_has_bar(&self, output: &smithay::output::Output) -> bool {
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
                self.set_maximized(id, true);
            } else if self.windows.is_snapped(id) {
                self.reflow_snapped_window(id);
            } else {
                self.apply_window_rect(id);
            }
        }
        self.sync_all_app_windows();
        self.arrange_layers();
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

    /// Full (titlebar-inclusive) frames for the active scroll workspace on `key`.
    pub(crate) fn scroll_frames_for(&self, key: &str) -> Vec<(u32, PixelRect)> {
        let ws = self.active_workspace_for(key);
        let Some(desk) = self.desk(key) else {
            return Vec::new();
        };
        let Some(scroll) = desk.scroll.get(&ws) else {
            return Vec::new();
        };
        let zone = self.scroll_zone_for(key);
        scroll.layout(zone, self.gutter_px as i32)
    }

    /// Full frame for a single window when its workspace is the active scroll
    /// workspace on its output; `None` otherwise (so hidden windows stay unmapped).
    pub(crate) fn scroll_frame_for_window(&self, id: u32) -> Option<PixelRect> {
        let key = self.desk_key_for_window(id);
        let ws = self.windows.workspace(id).unwrap_or(1);
        if ws != self.active_workspace_for(&key) {
            return None;
        }
        if self.layout_kind_for(&key, ws) != metis_grid::LayoutKind::Scroll {
            return None;
        }
        self.scroll_frames_for(&key)
            .into_iter()
            .find(|(wid, _)| *wid == id)
            .map(|(_, rect)| rect)
    }

    /// Mutable scroll state for an output's workspace, creating it on demand.
    fn scroll_state_mut(&mut self, key: &str, ws: u32) -> &mut metis_grid::ScrollState {
        self.desk_mut_or_default(key)
            .scroll
            .entry(ws)
            .or_default()
    }

    /// Recompute the scroll offset for an output's active workspace so the focused
    /// column is visible.
    fn refresh_scroll_offset(&mut self, key: &str) {
        let ws = self.active_workspace_for(key);
        if self.layout_kind_for(key, ws) != metis_grid::LayoutKind::Scroll {
            return;
        }
        let zone = self.scroll_zone_for(key);
        let gutter = self.gutter_px as i32;
        if let Some(scroll) = self.desk_mut_or_default(key).scroll.get_mut(&ws) {
            scroll.scroll_x = scroll.desired_scroll_x(zone, gutter);
        }
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
        if self.layout_kind_for(key, ws) == kind {
            // Pin an explicit entry so a later default change can't silently flip it.
            self.desk_mut_or_default(key).layout_kind.insert(ws, kind);
            return;
        }
        let active = self.active_workspace_for(key);
        // App window ids for this workspace, in tile order. The active workspace's
        // tiles live in the visible layout; hidden workspaces' tiles are stashed.
        let collect_ids = |tiles: &[metis_grid::GridTile]| -> Vec<u32> {
            tiles
                .iter()
                .filter_map(|t| match &t.kind {
                    TileKind::App { window_id: Some(wid), .. } => Some(*wid),
                    _ => None,
                })
                .collect()
        };
        let app_ids: Vec<u32> = self
            .desk(key)
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
            .unwrap_or_default();

        match kind {
            metis_grid::LayoutKind::Scroll => {
                let mut scroll = metis_grid::ScrollState::new();
                for wid in &app_ids {
                    scroll.insert_window_after_focus(*wid);
                }
                if let Some(f) = self.focused_window_id() {
                    scroll.focus_window(f);
                }
                let zone = self.scroll_zone_for(key);
                let gutter = self.gutter_px as i32;
                scroll.scroll_x = scroll.desired_scroll_x(zone, gutter);
                let desk = self.desk_mut_or_default(key);
                desk.scroll.insert(ws, scroll);
                desk.layout_kind.insert(ws, kind);
            }
            metis_grid::LayoutKind::Grid => {
                let desk = self.desk_mut_or_default(key);
                desk.layout_kind.insert(ws, kind);
                desk.scroll.remove(&ws);
                if ws == active {
                    metis_grid::sanitize_layout(&mut desk.layout);
                }
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
        self.reposition_all_windows();
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
            self.refresh_scroll_offset(key);
        }
        self.reposition_all_windows();
        if let Some(f) = focused {
            self.focus_window_id(f);
        }
        self.damaged = true;
        self.request_redraw();
        self.emit_layout_changed();
    }

    /// Flip the output's active workspace between grid and scroll.
    pub fn toggle_layout_kind(&mut self, key: &str) {
        let next = match self.active_layout_kind(key) {
            metis_grid::LayoutKind::Grid => metis_grid::LayoutKind::Scroll,
            metis_grid::LayoutKind::Scroll => metis_grid::LayoutKind::Grid,
        };
        self.set_layout_kind(key, next);
    }

    /// Give a window keyboard focus and raise it (mirrors `activate_window`'s tail).
    pub fn focus_window_id(&mut self, id: u32) {
        let Some(record) = self.windows.get(id).cloned() else {
            return;
        };
        if let Some(keyboard) = self.seat.get_keyboard() {
            self.space.raise_element(&record.window, true);
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
        self.refresh_scroll_offset(&key);
        self.reposition_all_windows();
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
        GridMetrics {
            columns,
            rows,
            gutter: self.gutter_px,
            monitor: self.output_rect(output).unwrap_or(self.monitor),
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

    pub fn close_window(&mut self, id: u32) {
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
        }
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
            if !self.windows.is_snapped(id) {
                let current = self
                    .window_body_rect(id)
                    .unwrap_or(record.target_rect);
                self.windows.set_restore_rect(id, current);
            }

            // Maximize fills the usable area (below the edge bar) with a uniform
            // Hyprland-style gap on every side, so the window floats inside the
            // screen rather than butting up against the edges. Metis draws the
            // server-side titlebar + border, so the client is inset to the body
            // of that footprint (`app_tile_body_rect`). Fills the output the
            // window currently sits on, not always the primary.
            let zone = match self.output_for_window(id) {
                Some(output) => self.window_placement_zone_for(&output),
                None => self.window_placement_zone(),
            };
            // Tight gap against the edge bar on whichever screen edge it occupies,
            // Hyprland-style gaps against the bare screen edges elsewhere.
            let gaps = self.zone_edge_gaps();
            let client = PixelRect {
                x: zone.x + gaps.left,
                y: zone.y + gaps.top,
                width: (zone.width - gaps.left - gaps.right).max(1),
                height: (zone.height - gaps.top - gaps.bottom).max(1),
            };
            let client_size = Size::from((client.width.max(1), client.height.max(1)));

            record.toplevel.with_pending_state(|state| {
                state.states.unset(xdg_toplevel::State::Fullscreen);
                state.states.set(xdg_toplevel::State::Maximized);
                state.size = Some(client_size);
                state.fullscreen_output = None;
            });
            self.space
                .map_element(record.window.clone(), Point::from((client.x, client.y)), true);
            self.windows.set_rect(id, client);
            self.auto_hide_titlebar.insert(id);
            self.windows.set_snapped(id, true);
            self.reclamp_auto_hide(id);
        } else {
            record.toplevel.with_pending_state(|state| {
                state.states.unset(xdg_toplevel::State::Maximized);
                state.size = None;
            });
            if let Some(restore) = self.windows.take_restore_rect(id) {
                self.windows.set_target_rect(id, restore);
            }
            self.clear_auto_hide(id);
            self.windows.set_snapped(id, false);
            self.apply_window_rect(id);
        }

        record.toplevel.send_pending_configure();
        self.windows.set_maximized(id, enabled);
    }

    pub fn minimize_window(&mut self, id: u32) {
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
        self.restore_by_id(id);
        let Some(record) = self.windows.get(id).cloned() else {
            return;
        };
        self.space.raise_element(&record.window, true);
        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
        self.seat
            .get_keyboard()
            .unwrap()
            .set_focus(self, Some(record.window.clone().into()), serial);
        self.event_bus
            .emit(&metis_protocol::CompositorEvent::WindowFocused { id });
        self.schedule_redraw();
    }

    pub fn apply_window_rect(&mut self, id: u32) {
        let Some(record) = self.windows.get(id).cloned() else {
            return;
        };
        // Never (re)map a minimized window. Restoring goes through
        // `unminimize_window`, which clears the flag *before* calling this. Without
        // this guard a bulk `reposition_all_windows` (triggered when restoring a
        // single grid tile) would re-map and un-minimize *every* minimized window.
        if self.windows.is_minimized(id) {
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
                } else {
                    self.clamp_body_below_bar(r)
                }
            })
        } else {
            self.rect_for_window_tile(id).map(app_tile_body_rect)
        };
        let Some(rect) = rect else {
            // No grid slot yet — never map at the registry default (0, 0).
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
        if mapped
            && self
                .space
                .element_location(&record.window)
                .is_some_and(|l| l == loc)
            && record.window.geometry().size == size
            && record.target_rect == rect
        {
            return;
        }
        if self.space.elements().any(|w| self.windows.id_for_window(w) == Some(id)) {
            self.space.unmap_elem(&record.window);
        }
        self.space.map_element(record.window.clone(), loc, true);
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
                .map_element(record.window.clone(), Point::from((new_x, new_y)), true);
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
            let Some(loc) = self.space.element_location(&record.window) else {
                continue;
            };
            let size = record.window.geometry().size;
            if size.w <= 0 || size.h <= 0 {
                continue;
            }
            // Auto-hide windows (maximized / edge-snapped to the bar) draw no
            // persistent chrome. While the pointer is in the window's top strip the
            // titlebar is revealed as a translucent overlay *on top of* the client,
            // so its frame is the client rect (the titlebar sits over the top
            // `header` px) rather than the client grown by the reserved chrome.
            let auto_hide = self.auto_hide_titlebar.contains(&id);
            if auto_hide && self.revealed_titlebar != Some(id) {
                continue;
            }
            let (frame, overlay) = if auto_hide {
                (
                    PixelRect { x: loc.x, y: loc.y, width: size.w, height: size.h },
                    true,
                )
            } else {
                let border = metis_grid::app_tile_border_px();
                (
                    PixelRect {
                        x: loc.x - border,
                        y: loc.y - metis_grid::APP_TILE_HEADER_PX,
                        width: size.w + border * 2,
                        height: size.h + metis_grid::APP_TILE_HEADER_PX + border,
                    },
                    false,
                )
            };
            specs.push(crate::decoration::WindowDeco {
                id,
                frame,
                title: self.titlebar_title(id, record.app_id.as_deref(), &record.title),
                focused: focused == Some(id),
                overlay,
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
        let monitor = self.output_rect(output).unwrap_or(self.monitor);
        PixelRect {
            x: monitor.x,
            y: monitor.y,
            width: monitor.width as i32,
            height: monitor.height as i32,
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

    /// Auto-place a window if it hasn't been positioned yet. Safe to call again
    /// whenever the app_id becomes known (GTK often sets it just *after* its first
    /// buffer commit, so the initial activation may not see it). No-ops once the
    /// window is floating — i.e. already auto-placed or moved by the user.
    pub(crate) fn maybe_autoplace_window(&mut self, id: u32) {
        if self.floating.contains(&id) {
            return;
        }
        let app_id = self.windows.get(id).and_then(|r| r.app_id.clone());
        if self.place_new_window(id, app_id.as_deref()) && self.windows.is_ready(id) {
            self.apply_window_rect(id);
        }
    }

    /// Decide where a freshly-mapped window should appear (called once, on first
    /// activation). Restores saved per-app geometry, or centers apps that open
    /// floating by default. Returns true when the window was placed as floating.
    fn place_new_window(&mut self, id: u32, app_id: Option<&str>) -> bool {
        let title = self.windows.get(id).map(|r| r.title.clone());
        tracing::info!(id, ?app_id, ?title, "place_new_window: deciding placement");
        // Restore a previously saved position/size for this app (keyed by app_id).
        // Only recovered onto the primary output if the saved spot is off every
        // active monitor (e.g. a monitor that's since been disconnected).
        if let Some(app_id) = app_id {
            if let Some(saved) = self.window_state.get(app_id) {
                let saved_rect = saved.to_rect();
                // Ignore degenerate saved geometry (a stale 1x1 from a window that
                // never got a buffer) so the app falls back to a sane default.
                if saved_rect.width < MIN_SAVED_WINDOW_PX
                    || saved_rect.height < MIN_SAVED_WINDOW_PX
                {
                    self.window_state.remove(app_id);
                } else {
                    let rect = self.recover_offscreen_rect(saved_rect);
                    // Geometry saved from a snapped/maximized (auto-hide) window
                    // sits flush under the bar; restored as a floating window its
                    // titlebar would draw above the body, under the edge bar. Drop
                    // it below.
                    let rect = self.clamp_body_below_bar(rect);
                    self.floating.insert(id);
                    self.windows.set_target_rect(id, rect);
                    tracing::info!(id, "place_new_window: restored saved geometry");
                    return true;
                }
            }
        }
        // No saved state: center apps that default to floating. Match on app_id, or
        // fall back to the window title (GTK can be slow to set app_id over Wayland).
        let by_app_id = app_id.is_some_and(|a| CENTERED_FLOAT_APP_IDS.contains(&a));
        let by_title = title
            .as_deref()
            .is_some_and(|t| CENTERED_FLOAT_TITLES.contains(&t));
        if by_app_id || by_title {
            // Center the chrome footprint on the output under the cursor (falls
            // back to primary), then inset the client to its body so Metis draws
            // the server-side titlebar + border around it.
            let footprint = match self.output_under_pointer() {
                Some(output) => self.centered_rect_in(&output, DEFAULT_FLOAT_W, DEFAULT_FLOAT_H),
                None => self.centered_rect(DEFAULT_FLOAT_W, DEFAULT_FLOAT_H),
            };
            let rect = app_tile_body_rect(footprint);
            self.floating.insert(id);
            self.windows.set_target_rect(id, rect);
            tracing::info!(id, ?rect, "place_new_window: centered default-float app");
            return true;
        }
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
            if !point_in_rect(x, y, frame) {
                continue;
            }
            if spec.overlay {
                // Overlay reveal: only the top titlebar strip is interactive; the
                // rest of the frame is live client content.
                if !point_in_rect(x, y, metis_grid::app_tile_chrome_rect(frame)) {
                    return false;
                }
            } else if point_in_rect(x, y, metis_grid::app_tile_body_rect(frame)) {
                // Inside the client body → not a decoration hit; let it pass through.
                return false;
            }
            // Clicking any of a window's chrome focuses it, so the taskbar
            // highlight tracks focus immediately instead of waiting for the
            // periodic reconcile (decoration presses otherwise bypass the
            // keyboard-focus path entirely).
            self.focus_window_chrome(spec.id, serial);
            for (control, rect) in control_hitboxes(frame) {
                if !point_in_rect(x, y, rect) {
                    continue;
                }
                match control {
                    DecoControl::Close => self.close_window(spec.id),
                    DecoControl::Minimize => {
                        if let Some(tile_id) = self.tile_id_for_window(spec.id) {
                            self.set_tile_mode(&tile_id, metis_protocol::TileMode::Minimized);
                        } else {
                            self.minimize_window(spec.id);
                        }
                    }
                    DecoControl::Maximize => {
                        let maxed = self
                            .windows
                            .get(spec.id)
                            .map(|r| r.maximized)
                            .unwrap_or(false);
                        self.set_maximized(spec.id, !maxed);
                    }
                    DecoControl::Titlebar => self.start_titlebar_move(spec.id, loc, serial),
                }
                return true;
            }
            // Border (or any other decoration pixel): consume so the click does not
            // fall through to the wallpaper, but take no action (grid tiles are
            // sized by the layout, not by free resize).
            return true;
        }
        false
    }

    /// Hit-test the pointer against every mapped window's resize band. Returns the
    /// topmost window whose edge/corner (within `RESIZE_MARGIN_PX`) is under the
    /// pointer, plus the combined edge(s). Skips minimized, maximized, and
    /// fullscreen windows (resize those after restoring), and the window body.
    pub fn resize_edge_at(
        &self,
        loc: Point<f64, Logical>,
    ) -> Option<(u32, crate::grabs::ResizeEdge)> {
        use crate::grabs::ResizeEdge;

        let m = RESIZE_MARGIN_PX as f64;
        let inner = RESIZE_INNER_MARGIN_PX as f64;
        // Topmost first: Space::elements() is bottom-to-top, so reverse.
        for window in self.space.elements().rev() {
            let Some(id) = self.windows.id_for_window(window) else {
                continue;
            };
            if self.windows.is_minimized(id) {
                continue;
            }
            if let Some(record) = self.windows.get(id) {
                if record.maximized || record.fullscreen {
                    continue;
                }
            }
            let Some(geo) = self.space.element_geometry(window) else {
                continue;
            };
            // Resize against the *decorated frame* (client body grown by the
            // server-side titlebar on top and border on the sides/bottom), not the
            // inset client body — otherwise the top resize band lands at the
            // titlebar/client seam instead of the true top edge of the window.
            let border = metis_grid::app_tile_border_px() as f64;
            let header = metis_grid::APP_TILE_HEADER_PX as f64;
            let (gx, gy) = (geo.loc.x as f64 - border, geo.loc.y as f64 - header);
            let (gw, gh) = (
                geo.size.w as f64 + border * 2.0,
                geo.size.h as f64 + header + border,
            );

            // Only consider points within the band around the window: up to `m`
            // outside each edge, but only `inner` inside (so client controls that
            // hug the edge, like scrollbars, stay clickable).
            if loc.x < gx - m || loc.x > gx + gw + m || loc.y < gy - m || loc.y > gy + gh + m {
                continue;
            }
            let near_left = loc.x <= gx + inner;
            let near_right = loc.x >= gx + gw - inner;
            let near_top = loc.y <= gy + inner;
            let near_bottom = loc.y >= gy + gh - inner;

            let mut edges = ResizeEdge::empty();
            if near_left {
                edges |= ResizeEdge::LEFT;
            } else if near_right {
                edges |= ResizeEdge::RIGHT;
            }
            if near_top {
                edges |= ResizeEdge::TOP;
            } else if near_bottom {
                edges |= ResizeEdge::BOTTOM;
            }
            if edges.is_empty() {
                // Inside the window body, not on an edge — block lower windows.
                return None;
            }
            return Some((id, edges));
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
        let edge = self.resize_edge_at(loc).map(|(_, e)| e);
        if edge != self.hover_cursor {
            self.hover_cursor = edge;
            self.schedule_redraw();
        }
        self.update_titlebar_reveal(loc);
    }

    /// Reveal the auto-hide titlebar overlay for the topmost auto-hide window whose
    /// top strip (the first `APP_TILE_HEADER_PX` of its client) the pointer is in,
    /// hiding it again once the pointer drops below that strip or onto another
    /// window. Flags a redraw on change.
    fn update_titlebar_reveal(&mut self, loc: Point<f64, Logical>) {
        let (x, y) = (loc.x as i32, loc.y as i32);
        let header = metis_grid::APP_TILE_HEADER_PX;
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
                if in_x && y >= geo.loc.y && y < geo.loc.y + header {
                    revealed = Some(id);
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
    ) -> bool {
        use smithay::input::pointer::{Focus, GrabStartData};

        // A live popup/move/resize grab owns the pointer — let it run.
        if self.seat.get_pointer().is_some_and(|p| p.is_grabbed()) {
            return false;
        }

        let Some((id, edges)) = self.resize_edge_at(loc) else {
            return false;
        };
        let Some(record) = self.windows.get(id).cloned() else {
            return false;
        };
        let window = record.window.clone();
        let Some(initial_window_location) = self.space.element_location(&window) else {
            return false;
        };
        let initial_window_size = window.geometry().size;

        // Edge-resize floats the window out of the grid (no snap-back).
        self.space.raise_element(&window, true);
        self.floating.insert(id);
        // Resizing off a snapped position restores the normal floating chrome.
        self.clear_tiled_states(id);

        if let Some(keyboard) = self.seat.get_keyboard() {
            keyboard.set_focus(self, Some(window.clone().into()), serial);
        }

        let toplevel = window.toplevel().unwrap();
        toplevel.with_pending_state(|state| {
            state
                .states
                .set(smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::State::Resizing);
        });
        toplevel.send_pending_configure();

        let Some(pointer) = self.seat.get_pointer() else {
            return false;
        };
        let start_data = GrabStartData {
            focus: None,
            button: 0x110,
            location: loc,
        };
        let grab = crate::grabs::ResizeSurfaceGrab::start(
            start_data,
            window,
            edges,
            smithay::utils::Rectangle::new(initial_window_location, initial_window_size),
        );
        pointer.set_grab(self, grab, serial, Focus::Clear);
        true
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
            self.windows.set_maximized(id, false);
            let mut initial_window_location = self
                .space
                .element_location(&window)
                .unwrap_or_default();

            // A snap/maximize maps the body flush against the bar edge with no
            // reserved titlebar strip. Floating it restores a persistent titlebar
            // drawn *above* the body, so re-map clear of the bar up front.
            if self.usable_zone().is_some() {
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
        };

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
        };
        pointer.set_grab(self, grab, serial, Focus::Clear);
    }

    fn rect_for_window_tile(&self, id: u32) -> Option<PixelRect> {
        // Scrolling workspaces position from the strip, not the tile grid.
        if let Some(frame) = self.scroll_frame_for_window(id) {
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
        self.reposition_all_windows();
    }

    fn ensure_app_tiles_for_open_windows(&mut self) {
        for id in self.windows.ids() {
            self.ensure_app_tile_for_window(id);
        }
    }

    fn reposition_all_windows(&mut self) {
        let ids: Vec<u32> = self.windows.ids();
        for id in ids {
            if self.rect_for_window_tile(id).is_some() {
                self.apply_window_rect(id);
            }
        }
    }

    /// Reserve a grid slot as soon as an app registers (before its first buffer commit).
    fn ensure_app_tile_for_window(&mut self, id: u32) {
        let tile_id = format!("app-{id}");
        let key = self.desk_key_for_window(id);
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
        let ws = self.windows.workspace(id).unwrap_or(1);
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
            self.scroll_state_mut(&key, ws).insert_window_after_focus(id);
            self.refresh_scroll_offset(&key);
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
        self.refresh_scroll_offset(output_key);
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
            self.scroll_state_mut(target_key, workspace)
                .insert_window_after_focus(window_id);
            self.refresh_scroll_offset(target_key);
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
                        let clamped = self.clamp_floating_rect(rect);
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
            self.refresh_scroll_offset(target_key);
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
            self.scroll_state_mut(&key, target)
                .insert_window_after_focus(window_id);
        }
        self.refresh_scroll_offset(&key);

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
                let outputs = self
                    .space
                    .outputs()
                    .filter_map(|o| {
                        let geo = self.space.output_geometry(o)?;
                        Some(metis_protocol::OutputInfo {
                            name: o.name(),
                            rect: metis_protocol::MonitorRect {
                                x: geo.loc.x,
                                y: geo.loc.y,
                                width: geo.size.w,
                                height: geo.size.h,
                            },
                        })
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
                    let serial = smithay::utils::SERIAL_COUNTER.next_serial();
                    self.seat.get_keyboard().unwrap().set_focus(
                        self,
                        Some(record.window.clone().into()),
                        serial,
                    );
                    self.event_bus
                        .emit(&CompositorEvent::WindowFocused { id });
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
        if !self.floating.contains(&id) {
            self.place_new_window(id, app_id.as_deref());
        }
        self.apply_window_rect(id);
    }

    pub fn activate_window(&mut self, id: u32) {
        use metis_protocol::CompositorEvent;

        self.ensure_app_tile_for_window(id);
        let Some(record) = self.windows.get(id).cloned() else {
            return;
        };

        let (title, app_id) = read_toplevel_metadata(&record.toplevel);
        self.windows.set_metadata(id, title.clone(), app_id.clone());
        self.set_app_tile_display_name(id, &title, app_id.as_deref());

        let already_ready = self.windows.is_ready(id);
        // First activation: choose a placement (restore saved geometry or center
        // default-floating apps) before the window is mapped. Skip it when the
        // window was already primed (initial configure sent on first commit) so
        // we don't re-run placement.
        if !already_ready && !record.toplevel.is_initial_configure_sent() {
            self.place_new_window(id, app_id.as_deref());
        }
        self.apply_window_rect(id);

        if already_ready {
            return;
        }

        self.windows.set_ready(id, true);

        let suggested_rect = self
            .rect_for_window_tile(id)
            .map(app_tile_body_rect)
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
                if let Some(restored) = self.tile_modes.exit(tile_id) {
                    if let Some(desk) = self.desks.get_mut(&key) {
                        if let Some(tile) = desk.layout.tile_mut(tile_id) {
                            tile.rect = restored;
                        }
                    }
                    self.reposition_all_windows();
                    self.persist_layout();
                    self.emit_layout_changed();
                }
                if let Some(id) = window_id {
                    if self.windows.is_minimized(id) {
                        self.unminimize_window(id);
                    }
                    self.set_fullscreen(id, false);
                    self.set_maximized(id, false);
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

        self.floating.remove(&id);
        self.clear_auto_hide(id);
        self.remove_app_tile_everywhere(id);
        self.persist_layout();
        self.emit_layout_changed();
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
