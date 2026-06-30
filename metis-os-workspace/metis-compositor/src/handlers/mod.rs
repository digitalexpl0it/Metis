mod compositor;
mod layer_shell;
mod xdg_shell;

pub use layer_shell::handle_layer_commit;

use std::os::unix::io::OwnedFd;

use crate::clipboard::{write_selection_to_fd, MetisSelectionUserData};
use crate::state::MetisState;

use smithay::input::dnd::{DnDGrab, DndGrabHandler, GrabType, Source};
use smithay::input::pointer::Focus;
use smithay::input::{Seat, SeatHandler, SeatState};
use smithay::reexports::wayland_server::Resource;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Logical, Point, Serial};
use smithay::wayland::output::OutputHandler;
use smithay::wayland::selection::{SelectionHandler, SelectionSource, SelectionTarget};
use smithay::wayland::selection::primary_selection::{
    PrimarySelectionHandler, PrimarySelectionState, set_primary_focus,
};
use smithay::wayland::seat::WaylandFocus;
use smithay::wayland::selection::data_device::{
    DataDeviceHandler, DataDeviceState, WaylandDndGrabHandler, set_data_device_focus,
};

use crate::focus::KeyboardFocusTarget;

impl SeatHandler for MetisState {
    type KeyboardFocus = KeyboardFocusTarget;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Self> {
        &mut self.seat_state
    }

    fn cursor_image(
        &mut self,
        _seat: &Seat<Self>,
        image: smithay::input::pointer::CursorImageStatus,
    ) {
        self.cursor_status = image;
    }

    fn focus_changed(&mut self, seat: &Seat<Self>, focused: Option<&KeyboardFocusTarget>) {
        let dh = &self.display_handle;
        let client = focused
            .and_then(|target| target.wl_surface())
            .and_then(|surface| dh.get_client(surface.id()).ok());
        set_data_device_focus(dh, seat, client.clone());
        set_primary_focus(dh, seat, client);
    }
}

impl MetisState {
    /// Point the seat's clipboard + primary-selection devices at the client under
    /// `loc` so paste (right/middle-click, Shift+Insert) works even when keyboard
    /// focus was previously elsewhere.
    pub(crate) fn sync_selection_focus_at(&mut self, loc: Point<f64, Logical>) {
        let under = self.pointer_target_at(loc);
        self.sync_selection_focus_from_target(&under);
    }

    /// Align data-device and primary-selection focus with an already-resolved
    /// pointer target (avoids a second hit-test that can disagree with motion).
    pub(crate) fn sync_selection_focus_from_target(
        &mut self,
        under: &Option<(WlSurface, Point<f64, Logical>)>,
    ) {
        let dh = &self.display_handle;
        let client = under
            .as_ref()
            .and_then(|(surface, _)| dh.get_client(surface.id()).ok());
        set_data_device_focus(dh, &self.seat, client.clone());
        set_primary_focus(dh, &self.seat, client);
    }
}

impl SelectionHandler for MetisState {
    type SelectionUserData = MetisSelectionUserData;

    fn new_selection(
        &mut self,
        ty: SelectionTarget,
        source: Option<SelectionSource>,
        _seat: Seat<Self>,
    ) {
        if ty == SelectionTarget::Clipboard {
            if let Some(ref source) = source {
                self.queue_clipboard_capture(source.mime_types());
            }
        }
        if let Some(xwm) = self.xwm.as_mut() {
            if let Err(err) = xwm.new_selection(ty, source.map(|s| s.mime_types())) {
                tracing::warn!(?err, ?ty, "failed to mirror Wayland selection to XWayland");
            }
        }
    }

    fn send_selection(
        &mut self,
        ty: SelectionTarget,
        mime_type: String,
        fd: OwnedFd,
        _seat: Seat<Self>,
        user_data: &Self::SelectionUserData,
    ) {
        if let Some(offer) = user_data.offer.as_ref() {
            if offer.mime == mime_type {
                write_selection_to_fd(fd, offer);
                return;
            }
        }
        if let Some(xwm) = self.xwm.as_mut() {
            if let Err(err) = xwm.send_selection(ty, mime_type, fd) {
                tracing::warn!(?err, ?ty, "failed to send Wayland selection to XWayland");
            }
        }
    }
}

impl PrimarySelectionHandler for MetisState {
    fn primary_selection_state(&mut self) -> &mut PrimarySelectionState {
        &mut self.primary_selection_state
    }
}

impl DataDeviceHandler for MetisState {
    fn data_device_state(&mut self) -> &mut DataDeviceState {
        &mut self.data_device_state
    }
}

impl DndGrabHandler for MetisState {}
impl WaylandDndGrabHandler for MetisState {
    fn dnd_requested<S: Source>(
        &mut self,
        source: S,
        icon: Option<smithay::reexports::wayland_server::protocol::wl_surface::WlSurface>,
        seat: Seat<Self>,
        serial: Serial,
        type_: GrabType,
    ) {
        match type_ {
            GrabType::Pointer => {
                let ptr = seat.get_pointer().unwrap();
                let start_data = ptr.grab_start_data().unwrap();
                let grab = DnDGrab::new_pointer(&self.display_handle, start_data, source, seat);
                ptr.set_grab(self, grab, serial, Focus::Keep);
            }
            GrabType::Touch => {
                source.cancel();
            }
        }
        let _ = icon;
    }
}

impl OutputHandler for MetisState {}

smithay::delegate_dispatch2!(MetisState);
