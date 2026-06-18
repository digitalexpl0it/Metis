use std::borrow::Cow;

use smithay::{
    backend::input::KeyState,
    desktop::{LayerSurface, PopupKind, Window},
    input::{
        Seat,
        keyboard::{KeyboardTarget, KeysymHandle, ModifiersState},
    },
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{IsAlive, Serial},
    wayland::seat::WaylandFocus,
};

use crate::state::MetisState;

#[derive(Debug, Clone, PartialEq)]
pub enum KeyboardFocusTarget {
    Window(Window),
    LayerSurface(LayerSurface),
    Popup(PopupKind),
}

impl IsAlive for KeyboardFocusTarget {
    fn alive(&self) -> bool {
        match self {
            Self::Window(w) => w.alive(),
            Self::LayerSurface(l) => l.alive(),
            Self::Popup(p) => p.alive(),
        }
    }
}

impl KeyboardFocusTarget {
    fn inner_keyboard_target(&self) -> &dyn KeyboardTarget<MetisState> {
        match self {
            Self::Window(w) => w.toplevel().unwrap().wl_surface(),
            Self::LayerSurface(l) => l.wl_surface(),
            Self::Popup(p) => p.wl_surface(),
        }
    }
}

impl WaylandFocus for KeyboardFocusTarget {
    fn wl_surface(&self) -> Option<Cow<'_, WlSurface>> {
        match self {
            Self::Window(w) => w.wl_surface(),
            Self::LayerSurface(l) => Some(Cow::Borrowed(l.wl_surface())),
            Self::Popup(p) => Some(Cow::Borrowed(p.wl_surface())),
        }
    }
}

impl KeyboardTarget<MetisState> for KeyboardFocusTarget {
    fn enter(
        &self,
        seat: &Seat<MetisState>,
        data: &mut MetisState,
        keys: Vec<KeysymHandle<'_>>,
        serial: Serial,
    ) {
        self.inner_keyboard_target().enter(seat, data, keys, serial)
    }

    fn leave(&self, seat: &Seat<MetisState>, data: &mut MetisState, serial: Serial) {
        self.inner_keyboard_target().leave(seat, data, serial)
    }

    fn key(
        &self,
        seat: &Seat<MetisState>,
        data: &mut MetisState,
        key: KeysymHandle<'_>,
        state: KeyState,
        serial: Serial,
        time: u32,
    ) {
        self.inner_keyboard_target()
            .key(seat, data, key, state, serial, time)
    }

    fn modifiers(
        &self,
        seat: &Seat<MetisState>,
        data: &mut MetisState,
        modifiers: ModifiersState,
        serial: Serial,
    ) {
        self.inner_keyboard_target()
            .modifiers(seat, data, modifiers, serial)
    }
}

impl From<Window> for KeyboardFocusTarget {
    fn from(w: Window) -> Self {
        Self::Window(w)
    }
}

impl From<LayerSurface> for KeyboardFocusTarget {
    fn from(l: LayerSurface) -> Self {
        Self::LayerSurface(l)
    }
}

impl From<PopupKind> for KeyboardFocusTarget {
    fn from(p: PopupKind) -> Self {
        Self::Popup(p)
    }
}

impl From<KeyboardFocusTarget> for WlSurface {
    fn from(target: KeyboardFocusTarget) -> Self {
        match target {
            KeyboardFocusTarget::Window(w) => w
                .wl_surface()
                .map(|surface| surface.into_owned())
                .unwrap_or_else(|| w.toplevel().unwrap().wl_surface().clone()),
            KeyboardFocusTarget::LayerSurface(l) => l.wl_surface().clone(),
            KeyboardFocusTarget::Popup(p) => p.wl_surface().clone(),
        }
    }
}
