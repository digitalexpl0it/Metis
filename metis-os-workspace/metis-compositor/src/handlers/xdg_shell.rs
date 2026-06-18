use crate::grabs::{MoveSurfaceGrab, ResizeSurfaceGrab};
use crate::state::MetisState;
use smithay::{
    desktop::{
        PopupKind, PopupManager, Space, Window, WindowSurfaceType, find_popup_root_surface,
        get_popup_toplevel_coords, layer_map_for_output,
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
        PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
    },
};

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

    fn grab(&mut self, _surface: PopupSurface, _seat: wl_seat::WlSeat, _serial: Serial) {
        // Layer-shell tray menus use in-window dropdowns; avoid popup grabs here —
        // a partial grab stack hangs GTK clients in our compositor.
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

        if let Some(window) = self
            .space
            .elements()
            .find(|w| w.toplevel().unwrap().wl_surface() == &root)
        {
            let output = self.space.outputs().next().unwrap();
            let output_geo = self.space.output_geometry(output).unwrap();
            let window_geo = self.space.element_geometry(window).unwrap();

            let mut target = output_geo;
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
        let mut target = output_geo;
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
