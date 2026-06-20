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
            primary_selection::{PrimarySelectionHandler, PrimarySelectionState},
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

    pub windows: WindowRegistry,

    pub grid_layout: GridLayout,
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

    pub wallpaper: crate::wallpaper::Wallpaper,
    pub blur: crate::blur::BlurRuntime,
    pub decorations: crate::decoration::DecorationRuntime,

    redraw_trigger: Option<Rc<dyn Fn()>>,
    /// When true, the next winit Redraw performs GL compositing + layer frame delivery.
    pub damaged: bool,
    /// Defer `flush_clients` until after the winit redraw handler returns (avoids reentrancy).
    pub defer_client_flush: bool,
    /// One post-configure arrange after the bar commits its first real buffer.
    pub metis_bar_geometry_seeded: bool,
    last_pointer_forward: Option<(std::time::Instant, Point<f64, Logical>)>,
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
            windows: WindowRegistry::new(),
            grid_layout,
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
            wallpaper: crate::wallpaper::Wallpaper::new(),
            blur: crate::blur::BlurRuntime::default(),
            decorations: crate::decoration::DecorationRuntime::default(),
            redraw_trigger: None,
            damaged: true,
            defer_client_flush: false,
            metis_bar_geometry_seeded: false,
            last_pointer_forward: None,
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
        self.grid_layout.tiles.iter().find_map(|t| {
            if let TileKind::App {
                window_id: Some(wid),
                ..
            } = &t.kind
            {
                if *wid == window_id {
                    return Some(t.id.clone());
                }
            }
            None
        })
    }

    /// App windows slotted in the desk grid — not free-floating or fullscreen.
    pub fn is_window_grid_managed(&self, id: u32) -> bool {
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
        cmd.env_remove("DISPLAY");
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
        if let Some(output) = self.space.outputs().next() {
            layer_map_for_output(output).arrange();
        }
    }

    pub fn grid_metrics(&self) -> GridMetrics {
        GridMetrics {
            columns: self.grid_layout.columns,
            rows: self.grid_layout.rows,
            gutter: self.gutter_px,
            monitor: self.monitor,
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
        let output = self.space.outputs().next().cloned();
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
            let output = self.space.outputs().next().unwrap();
            let geo = self.space.output_geometry(output).unwrap();
            let current = self
                .space
                .element_geometry(&record.window)
                .map(|g| PixelRect {
                    x: g.loc.x,
                    y: g.loc.y,
                    width: g.size.w,
                    height: g.size.h,
                })
                .unwrap_or(record.target_rect);
            self.windows.set_restore_rect(id, current);

            record.toplevel.with_pending_state(|state| {
                state.states.unset(xdg_toplevel::State::Fullscreen);
                state.states.set(xdg_toplevel::State::Maximized);
                state.size = Some(geo.size);
                state.fullscreen_output = None;
            });
            self.space
                .map_element(record.window.clone(), geo.loc, true);
            self.windows.set_rect(id, PixelRect {
                x: geo.loc.x,
                y: geo.loc.y,
                width: geo.size.w,
                height: geo.size.h,
            });
        } else {
            record.toplevel.with_pending_state(|state| {
                state.states.unset(xdg_toplevel::State::Maximized);
                state.size = None;
            });
            if let Some(restore) = self.windows.take_restore_rect(id) {
                self.windows.set_target_rect(id, restore);
            }
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

        if self.focused_window_id() == Some(id) {
            let serial = smithay::utils::SERIAL_COUNTER.next_serial();
            self.seat
                .get_keyboard()
                .unwrap()
                .set_focus(self, Option::<KeyboardFocusTarget>::None, serial);
        }
    }

    fn unminimize_window(&mut self, id: u32) {
        self.windows.set_minimized(id, false);
        self.apply_window_rect(id);
    }

    pub fn apply_window_rect(&mut self, id: u32) {
        let Some(record) = self.windows.get(id).cloned() else {
            return;
        };
        let Some(rect) = self
            .rect_for_window_tile(id)
            .map(app_tile_body_rect)
        else {
            // No grid slot yet — never map at the registry default (0, 0).
            return;
        };
        let mapped = self
            .space
            .elements()
            .any(|w| self.windows.id_for_window(w) == Some(id));
        if self.windows.is_minimized(id) {
            self.windows.set_minimized(id, false);
        }
        let loc = Point::from((rect.x, rect.y));
        let width = rect.width.max(1);
        let height = rect.height.max(1);
        let size = Size::from((width, height));
        if mapped
            && !self.windows.is_minimized(id)
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
    }

    /// Collect decoration specs (frame, title, focus) for every grid-tiled,
    /// mapped window. Maximized/fullscreen/minimized windows are skipped.
    pub fn decoration_specs(&self) -> Vec<crate::decoration::WindowDeco> {
        let focused = self.focused_window_id();
        let mut specs = Vec::new();
        for id in self.windows.ids() {
            if !self.is_window_grid_managed(id) {
                continue;
            }
            let Some(record) = self.windows.get(id) else {
                continue;
            };
            if !record.ready || self.windows.is_minimized(id) {
                continue;
            }
            let Some(frame) = self.rect_for_window_tile(id) else {
                continue;
            };
            specs.push(crate::decoration::WindowDeco {
                id,
                frame,
                title: record.title.clone(),
                focused: focused == Some(id),
            });
        }
        specs
    }

    /// Handle a pointer press that may land on a server-side decoration (titlebar,
    /// control buttons, or border). Returns true when the press was consumed by the
    /// decoration (so the caller must not forward it to a client surface).
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
        for spec in self.decoration_specs() {
            let frame = spec.frame;
            if !point_in_rect(x, y, frame) {
                continue;
            }
            // Inside the client body → not a decoration hit; let it pass through.
            if point_in_rect(x, y, metis_grid::app_tile_body_rect(frame)) {
                return false;
            }
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
        let initial_window_location = self
            .space
            .element_location(&window)
            .unwrap_or_default();

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
        self.grid_layout.tiles.iter().find_map(|t| {
            if let TileKind::App {
                window_id: Some(wid),
                ..
            } = &t.kind
            {
                if *wid == id {
                    return Some(cell_to_pixels(&self.grid_metrics(), &t.rect));
                }
            }
            None
        })
    }

    pub fn apply_grid_layout(&mut self, shell_layout: GridLayout, gutter_px: u32) {
        use std::collections::HashMap;

        let compositor_apps: HashMap<String, metis_grid::GridTile> = self
            .grid_layout
            .tiles
            .iter()
            .filter(|t| matches!(t.kind, TileKind::App { .. }))
            .map(|t| (t.id.clone(), t.clone()))
            .collect();

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

        self.grid_layout = merged;
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
        if self.grid_layout.tiles.iter().any(|t| t.id == tile_id) {
            return;
        }
        let class = self.windows.get(id).and_then(|r| r.app_id.clone());
        self.grid_layout.tiles.push(metis_grid::GridTile {
            id: tile_id,
            rect: default_app_tile_rect(&self.grid_layout),
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
        if let Err(err) = self.grid_layout.save_to_path(&desk_config_path()) {
            tracing::warn!(%err, "failed to persist grid layout");
        }
    }

    pub fn emit_layout_changed(&self) {
        use metis_protocol::CompositorEvent;
        self.event_bus.emit(&CompositorEvent::LayoutChanged {
            layout: self.grid_layout.clone(),
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

    pub fn handle_ipc(&mut self, cmd: CompositorCommand) -> metis_protocol::CompositorEvent {
        use metis_protocol::CompositorEvent;
        match cmd {
            CompositorCommand::Ping => CompositorEvent::Pong,
            CompositorCommand::GetMonitor => CompositorEvent::Monitor {
                rect: self.monitor,
            },
            CompositorCommand::GetLayout => CompositorEvent::LayoutChanged {
                layout: self.grid_layout.clone(),
                gutter_px: self.gutter_px,
                metrics: self.grid_metrics(),
            },
            CompositorCommand::ListWindows => CompositorEvent::WindowList {
                windows: self.windows.list(),
            },
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
                    CompositorEvent::WindowFocused { id }
                } else {
                    CompositorEvent::Error {
                        message: format!("window {id} not found"),
                    }
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
            CompositorCommand::SubscribeEvents => CompositorEvent::Pong,
            CompositorCommand::Launch { program } => {
                // Route through spawn_client so the child inherits the nested
                // Wayland env (WAYLAND_DISPLAY, GDK_BACKEND, cursor theme) and is
                // tracked for cleanup — a bare `sh -c` had no Wayland display.
                self.spawn_client(&program);
                CompositorEvent::Pong
            }
        }
    }

    pub fn register_new_window(&mut self, window: Window, title: String, app_id: Option<String>) {
        let id = self.windows.register(window, title, app_id);
        self.ensure_app_tile_for_window(id);
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
    }

    pub(crate) fn set_app_tile_display_name(&mut self, window_id: u32, title: &str, app_id: Option<&str>) {
        let display = app_display_name(app_id, title);
        let tile_id = format!("app-{window_id}");
        if let Some(tile) = self.grid_layout.tiles.iter_mut().find(|t| t.id == tile_id) {
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

    pub fn try_activate_committed_window(&mut self, surface: &smithay::reexports::wayland_server::protocol::wl_surface::WlSurface) {
        let Some(id) = self.windows.id_for_surface(surface) else {
            return;
        };
        if self.windows.is_ready(id) {
            return;
        }
        let has_buffer = smithay::wayland::compositor::with_states(surface, |states| {
            use smithay::wayland::compositor::SurfaceAttributes;
            let mut attrs = states.cached_state.get::<SurfaceAttributes>();
            attrs.current().buffer.is_some() || attrs.pending().buffer.is_some()
        });
        if has_buffer {
            self.activate_window(id);
        }
    }

    pub fn set_tile_mode(&mut self, tile_id: &str, mode: metis_protocol::TileMode) {
        use metis_protocol::TileMode;

        let window_id = self.grid_layout.tiles.iter().find_map(|t| {
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
        });

        match mode {
            TileMode::Grid => {
                if let Some(restored) = self.tile_modes.exit(tile_id) {
                    if let Some(tile) = self.grid_layout.tile_mut(tile_id) {
                        tile.rect = restored;
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
                }
            }
            TileMode::AppFullscreen => {
                self.tile_modes.enter(&self.grid_layout, tile_id, metis_grid::TileMode::AppFullscreen);
                if let Some(id) = window_id {
                    self.set_fullscreen(id, true);
                }
            }
            TileMode::Minimized => {
                self.tile_modes.enter(&self.grid_layout, tile_id, metis_grid::TileMode::Minimized);
                if let Some(id) = window_id {
                    self.minimize_window(id);
                }
            }
            TileMode::Immersive => {
                self.tile_modes
                    .enter(&self.grid_layout, tile_id, metis_grid::TileMode::Immersive);
                tracing::info!(tile_id, "immersive mode requested (shell handles chrome)");
            }
        }
    }

    pub fn on_window_destroyed(&mut self, id: u32) {
        use metis_protocol::CompositorEvent;

        self.grid_layout.tiles.retain(|t| {
            !matches!(
                &t.kind,
                TileKind::App {
                    window_id: Some(wid),
                    ..
                } if *wid == id
            )
        });
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
    metis_grid::TileRect::new(0, rows.saturating_sub(4).max(2), cols.min(6).max(4), 4)
}

#[derive(Default)]
pub struct ClientState {
    pub compositor_state: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
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
