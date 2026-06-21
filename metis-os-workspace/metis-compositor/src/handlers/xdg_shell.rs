use crate::focus::KeyboardFocusTarget;
use crate::grabs::{MoveSurfaceGrab, ResizeSurfaceGrab};
use crate::state::MetisState;
use smithay::{
    desktop::{
        PopupKeyboardGrab, PopupKind, PopupManager, PopupPointerGrab, PopupUngrabStrategy, Space,
        Window, WindowSurfaceType, find_popup_root_surface, get_popup_toplevel_coords,
        layer_map_for_output,
    },
    input::{
        Seat,
        pointer::{Focus, GrabStartData as PointerGrabStartData},
    },
    reexports::{
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_server::{
            Resource,
            protocol::{wl_seat, wl_surface::WlSurface},
        },
    },
    utils::{Rectangle, Serial},
    wayland::shell::xdg::{
        decoration::XdgDecorationHandler,
        PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
    },
};
use smithay::reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode as DecorationMode;

impl XdgShellHandler for MetisState {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        let window = Window::new_wayland_window(surface.clone());
        self.register_new_window(window, "Application".into(), None);
    }

    fn new_popup(&mut self, surface: PopupSurface, _positioner: PositionerState) {
        track_popup(self, surface);
    }

    fn reposition_request(
        &mut self,
        surface: PopupSurface,
        positioner: PositionerState,
        token: u32,
    ) {
        surface.with_pending_state(|state| {
            let geometry = positioner.get_geometry();
            state.geometry = geometry;
            state.positioner = positioner;
        });
        self.unconstrain_popup(&surface);
        surface.send_repositioned(token);
    }

    fn move_request(&mut self, surface: ToplevelSurface, seat: wl_seat::WlSeat, serial: Serial) {
        // A client-initiated move (e.g. dragging a GTK headerbar) floats the window
        // out of the grid so it follows the pointer with no snap-back.
        if let Some(id) = self.window_id_for_toplevel(&surface) {
            // A maximized (or fullscreen) window is pinned — ignore drag requests
            // so its headerbar can't be used to move it around the screen. The
            // user must unmaximize first.
            if let Some(record) = self.windows.get(id) {
                if record.maximized || record.fullscreen {
                    return;
                }
            }
            self.floating.insert(id);
        }

        let seat = Seat::from_resource(&seat).unwrap();
        let wl_surface = surface.wl_surface();

        if let Some(start_data) = check_grab(&seat, wl_surface, serial) {
            let pointer = seat.get_pointer().unwrap();
            let window = self
                .space
                .elements()
                .find(|w| w.toplevel().unwrap().wl_surface() == wl_surface)
                .unwrap()
                .clone();
            let initial_window_location = self.space.element_location(&window).unwrap();
            let grab = MoveSurfaceGrab {
                start_data,
                window,
                initial_window_location,
            };
            pointer.set_grab(self, grab, serial, Focus::Clear);
        }
    }

    fn resize_request(
        &mut self,
        surface: ToplevelSurface,
        seat: wl_seat::WlSeat,
        serial: Serial,
        edges: xdg_toplevel::ResizeEdge,
    ) {
        if let Some(id) = self.window_id_for_toplevel(&surface) {
            if self.is_window_grid_managed(id) {
                return;
            }
        }

        let seat = Seat::from_resource(&seat).unwrap();
        let wl_surface = surface.wl_surface();

        if let Some(start_data) = check_grab(&seat, wl_surface, serial) {
            let pointer = seat.get_pointer().unwrap();
            let window = self
                .space
                .elements()
                .find(|w| w.toplevel().unwrap().wl_surface() == wl_surface)
                .unwrap()
                .clone();
            let initial_window_location = self.space.element_location(&window).unwrap();
            let initial_window_size = window.geometry().size;

            surface.with_pending_state(|state| {
                state.states.set(xdg_toplevel::State::Resizing);
            });
            surface.send_pending_configure();

            let grab = ResizeSurfaceGrab::start(
                start_data,
                window,
                edges.into(),
                Rectangle::new(initial_window_location, initial_window_size),
            );
            pointer.set_grab(self, grab, serial, Focus::Clear);
        }
    }

    fn grab(&mut self, surface: PopupSurface, seat: wl_seat::WlSeat, serial: Serial) {
        // Honor xdg_popup grabs so GTK popovers that need keyboard/pointer focus
        // (text entries, dropdowns) can present and dismiss correctly. The root of
        // the grab is either an app window or one of our layer surfaces (the bar).
        let seat: Seat<MetisState> = Seat::from_resource(&seat).unwrap();
        let kind = PopupKind::Xdg(surface);
        let Some(root) = find_popup_root_surface(&kind).ok().and_then(|root| {
            self.space
                .elements()
                .find(|w| {
                    w.toplevel()
                        .map(|t| t.wl_surface() == &root)
                        .unwrap_or(false)
                })
                .cloned()
                .map(KeyboardFocusTarget::from)
                .or_else(|| {
                    self.space.outputs().find_map(|o| {
                        let map = layer_map_for_output(o);
                        map.layer_for_surface(&root, WindowSurfaceType::TOPLEVEL)
                            .cloned()
                            .map(KeyboardFocusTarget::LayerSurface)
                    })
                })
        }) else {
            return;
        };

        if let Ok(mut grab) = self.popups.grab_popup(root, kind, &seat, serial) {
            if let Some(keyboard) = seat.get_keyboard() {
                if keyboard.is_grabbed()
                    && !(keyboard.has_grab(serial)
                        || keyboard.has_grab(grab.previous_serial().unwrap_or(serial)))
                {
                    grab.ungrab(PopupUngrabStrategy::All);
                    return;
                }
                keyboard.set_focus(self, grab.current_grab(), serial);
                keyboard.set_grab(self, PopupKeyboardGrab::new(&grab), serial);
            }
            if let Some(pointer) = seat.get_pointer() {
                if pointer.is_grabbed()
                    && !(pointer.has_grab(serial)
                        || pointer.has_grab(grab.previous_serial().unwrap_or_else(|| grab.serial())))
                {
                    grab.ungrab(PopupUngrabStrategy::All);
                    return;
                }
                pointer.set_grab(self, PopupPointerGrab::new(&grab), serial, Focus::Keep);
            }
        }
    }

    fn maximize_request(&mut self, surface: ToplevelSurface) {
        if let Some(id) = self.window_id_for_toplevel(&surface) {
            self.set_maximized(id, true);
        } else if surface.is_initial_configure_sent() {
            surface.send_configure();
        }
    }

    fn unmaximize_request(&mut self, surface: ToplevelSurface) {
        if let Some(id) = self.window_id_for_toplevel(&surface) {
            self.set_maximized(id, false);
        } else if surface.is_initial_configure_sent() {
            surface.send_configure();
        }
    }

    fn minimize_request(&mut self, surface: ToplevelSurface) {
        if let Some(id) = self.window_id_for_toplevel(&surface) {
            if let Some(tile_id) = self.tile_id_for_window(id) {
                self.set_tile_mode(&tile_id, metis_protocol::TileMode::Minimized);
            } else {
                self.minimize_window(id);
            }
        }
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        let Some(id) = self.window_id_for_toplevel(&surface) else {
            return;
        };
        let ready = self.windows.is_ready(id);
        // Remember floating app geometry before the record is dropped.
        self.save_window_geometry(id);
        if let Some(record) = self.windows.unregister(id) {
            self.space.unmap_elem(&record.window);
        }
        if ready {
            self.on_window_destroyed(id);
        }
    }

    fn title_changed(&mut self, surface: ToplevelSurface) {
        self.sync_toplevel_metadata(&surface);
    }

    fn app_id_changed(&mut self, surface: ToplevelSurface) {
        self.sync_toplevel_metadata(&surface);
    }
}

impl XdgDecorationHandler for MetisState {
    fn new_decoration(&mut self, toplevel: ToplevelSurface) {
        // Metis draws server-side decorations (border + titlebar) itself, so force
        // SSD on every toplevel — GTK then omits its client-side headerbar.
        toplevel.with_pending_state(|state| {
            state.decoration_mode = Some(DecorationMode::ServerSide);
        });
        if toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
    }

    fn request_mode(&mut self, toplevel: ToplevelSurface, _mode: DecorationMode) {
        // Ignore the client's preference; we always decorate server-side.
        toplevel.with_pending_state(|state| {
            state.decoration_mode = Some(DecorationMode::ServerSide);
        });
        if toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
    }

    fn unset_mode(&mut self, toplevel: ToplevelSurface) {
        toplevel.with_pending_state(|state| {
            state.decoration_mode = Some(DecorationMode::ServerSide);
        });
        if toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
    }
}

fn check_grab(
    seat: &Seat<MetisState>,
    surface: &WlSurface,
    serial: Serial,
) -> Option<PointerGrabStartData<MetisState>> {
    let pointer = seat.get_pointer()?;
    if !pointer.has_grab(serial) {
        return None;
    }
    let start_data = pointer.grab_start_data()?;
    let (focus, _) = start_data.focus.as_ref()?;
    if !focus.id().same_client_as(&surface.id()) {
        return None;
    }
    Some(start_data)
}

pub fn track_popup(state: &mut MetisState, surface: PopupSurface) {
    state.unconstrain_popup(&surface);
    if let Err(err) = state.popups.track_popup(PopupKind::Xdg(surface)) {
        tracing::warn!(%err, "failed to track popup");
    }
}

pub fn handle_commit(popups: &mut PopupManager, space: &Space<Window>, surface: &WlSurface) {
    if let Some(window) = space
        .elements()
        .find(|w| w.toplevel().unwrap().wl_surface() == surface)
        .cloned()
    {
        if !window.toplevel().unwrap().is_initial_configure_sent() {
            window.toplevel().unwrap().send_configure();
        }
    }

    popups.commit(surface);
    if let Some(popup) = popups.find_popup(surface) {
        if let PopupKind::Xdg(ref xdg) = popup {
            if !xdg.is_initial_configure_sent() {
                let _ = xdg.send_configure();
            }
        }
    }
}

impl MetisState {
    fn sync_toplevel_metadata(&mut self, surface: &ToplevelSurface) {
        let Some(id) = self.window_id_for_toplevel(surface) else {
            return;
        };
        let (title, app_id) = crate::state::read_toplevel_metadata(surface);
        self.windows.set_metadata(id, title.clone(), app_id.clone());
        self.set_app_tile_display_name(id, &title, app_id.as_deref());
        // GTK frequently sets app_id just after its first buffer commit, so the
        // initial activation can miss it. Now that it's known, place the window
        // (center default-floating apps / restore saved geometry) if it hasn't
        // already been positioned.
        self.maybe_autoplace_window(id);
        if self.windows.is_ready(id) {
            use metis_protocol::CompositorEvent;
            self.event_bus.emit(&CompositorEvent::WindowMetadata {
                id,
                title,
                app_id,
            });
        }
    }

    pub(crate) fn unconstrain_popup(&self, popup: &PopupSurface) {
        let kind = PopupKind::Xdg(popup.clone());
        let Ok(root) = find_popup_root_surface(&kind) else {
            return;
        };

        // Keep popovers from sliding flush against (or off) the screen edge by
        // shrinking the allowed placement area on every side.
        const SCREEN_MARGIN: i32 = 10;
        let inset = |mut r: Rectangle<i32, smithay::utils::Logical>| {
            r.loc.x += SCREEN_MARGIN;
            r.loc.y += SCREEN_MARGIN;
            r.size.w = (r.size.w - 2 * SCREEN_MARGIN).max(1);
            r.size.h = (r.size.h - 2 * SCREEN_MARGIN).max(1);
            r
        };

        if let Some(window) = self
            .space
            .elements()
            .find(|w| w.toplevel().unwrap().wl_surface() == &root)
        {
            let output = self.space.outputs().next().unwrap();
            let output_geo = self.space.output_geometry(output).unwrap();
            let window_geo = self.space.element_geometry(window).unwrap();

            let mut target = inset(output_geo);
            target.loc -= get_popup_toplevel_coords(&kind);
            target.loc -= window_geo.loc;

            popup.with_pending_state(|state| {
                state.geometry = state.positioner.get_unconstrained_geometry(target);
            });
            return;
        }

        let Some((output, layer_geo)) = self.layer_geometry_for_surface(&root) else {
            return;
        };
        let output_geo = self.space.output_geometry(&output).unwrap();
        let mut target = inset(output_geo);
        target.loc -= get_popup_toplevel_coords(&kind);
        target.loc -= layer_geo.loc;

        popup.with_pending_state(|state| {
            state.geometry = state.positioner.get_unconstrained_geometry(target);
        });
    }

    fn layer_geometry_for_surface(
        &self,
        surface: &WlSurface,
    ) -> Option<(smithay::output::Output, smithay::utils::Rectangle<i32, smithay::utils::Logical>)> {
        self.space.outputs().find_map(|output| {
            let map = layer_map_for_output(output);
            let layer = map.layer_for_surface(surface, WindowSurfaceType::TOPLEVEL)?;
            let geo = map.layer_geometry(layer)?;
            Some((output.clone(), geo))
        })
    }
}
