//! XWayland integration: spawns the `Xwayland` server, runs an X11 window
//! manager, and maps X11 toplevels/override-redirect surfaces into the same
//! `Space<Window>` used for native Wayland clients.
//!
//! Scope note: X11 windows are treated as floating, centered surfaces. They are
//! intentionally kept out of Metis's tiling grid and window registry (which are
//! built around `xdg_toplevel`), so they are not snapped, decorated with Metis
//! server-side titlebars, or tracked for IPC. This is enough to run X11-only and
//! D-Bus-activated apps inside a nested session; richer integration can come
//! later.

use smithay::{
    desktop::Window,
    reexports::calloop::LoopHandle,
    utils::{Logical, Point, Rectangle, SERIAL_COUNTER, Size},
    wayland::xwayland_shell::{XWaylandShellHandler, XWaylandShellState},
    xwayland::{
        X11Surface, X11Wm, XWayland, XWaylandEvent, XwmHandler,
        xwm::{Reorder, ResizeEdge as X11ResizeEdge, XwmId},
    },
};

use crate::focus::KeyboardFocusTarget;
use crate::state::MetisState;

impl MetisState {
    /// Spawn the XWayland server and, once it is ready, start the X11 window
    /// manager. Best-effort: if `Xwayland` is missing or fails to start we log a
    /// warning and continue with a Wayland-only session.
    pub fn start_xwayland(&mut self, loop_handle: LoopHandle<'static, MetisState>) {
        use std::process::Stdio;

        let (xwayland, client) = match XWayland::spawn(
            &self.display_handle,
            None,
            std::iter::empty::<(String, String)>(),
            true,
            Stdio::null(),
            Stdio::null(),
            |_| (),
        ) {
            Ok(pair) => pair,
            Err(err) => {
                tracing::warn!(%err, "could not spawn XWayland — X11 apps will be unavailable");
                return;
            }
        };

        let dh = self.display_handle.clone();
        let wm_handle = loop_handle.clone();
        let inserted = loop_handle.insert_source(xwayland, move |event, _, state| match event {
            XWaylandEvent::Ready {
                x11_socket,
                display_number,
            } => {
                match X11Wm::start_wm(wm_handle.clone(), &dh, x11_socket, client.clone()) {
                    Ok(wm) => {
                        state.xwm = Some(wm);
                        state.xdisplay = Some(display_number);
                        // Make the X11 display discoverable to processes the
                        // compositor spawns later (spawn_client also sets it).
                        unsafe {
                            std::env::set_var("DISPLAY", format!(":{display_number}"));
                        }
                        tracing::info!(display = display_number, "XWayland ready — X11 apps supported");
                    }
                    Err(err) => {
                        tracing::error!(%err, "failed to start the X11 window manager");
                    }
                }
            }
            XWaylandEvent::Error => {
                tracing::warn!("XWayland crashed during startup");
            }
        });

        if let Err(err) = inserted {
            tracing::error!(%err, "failed to insert XWayland source into the event loop");
        }
    }

    /// Find the `Space` element backing a given X11 surface, if it is mapped.
    fn x11_element(&self, surface: &X11Surface) -> Option<Window> {
        self.space
            .elements()
            .find(|w| w.x11_surface() == Some(surface))
            .cloned()
    }

    /// Center a window of `size` within the primary output's logical area,
    /// falling back to the configured monitor rect when no output exists yet.
    fn centered_loc(&self, size: Size<i32, Logical>) -> Point<i32, Logical> {
        let area = self
            .space
            .outputs()
            .next()
            .and_then(|o| self.space.output_geometry(o))
            .unwrap_or_else(|| {
                Rectangle::new(
                    (self.monitor.x, self.monitor.y).into(),
                    (self.monitor.width, self.monitor.height).into(),
                )
            });
        let x = area.loc.x + (area.size.w - size.w).max(0) / 2;
        let y = area.loc.y + (area.size.h - size.h).max(0) / 2;
        (x, y).into()
    }

    fn focus_x11(&mut self, window: &Window) {
        if let Some(keyboard) = self.seat.get_keyboard() {
            keyboard.set_focus(
                self,
                Some(KeyboardFocusTarget::Window(window.clone())),
                SERIAL_COUNTER.next_serial(),
            );
        }
    }
}

impl XWaylandShellHandler for MetisState {
    fn xwayland_shell_state(&mut self) -> &mut XWaylandShellState {
        &mut self.xwayland_shell_state
    }
}

impl XwmHandler for MetisState {
    fn xwm_state(&mut self, _xwm: XwmId) -> &mut X11Wm {
        self.xwm.as_mut().expect("xwm called without a running X11Wm")
    }

    fn new_window(&mut self, _xwm: XwmId, _window: X11Surface) {}
    fn new_override_redirect_window(&mut self, _xwm: XwmId, _window: X11Surface) {}

    fn map_window_request(&mut self, _xwm: XwmId, window: X11Surface) {
        if let Err(err) = window.set_mapped(true) {
            tracing::warn!(%err, "failed to map X11 window");
            return;
        }
        let elem = Window::new_x11_window(window.clone());
        // Map once at the origin so the space can resolve the element's natural
        // size, then re-map centered.
        self.space.map_element(elem.clone(), (0, 0), true);
        let size = self
            .space
            .element_bbox(&elem)
            .map(|bbox| bbox.size)
            .unwrap_or_else(|| window.geometry().size);
        let loc = self.centered_loc(size);
        self.space.map_element(elem.clone(), loc, true);
        let _ = window.configure(Rectangle::new(loc, size));
        let _ = window.set_activated(true);
        self.focus_x11(&elem);
        self.schedule_redraw();
    }

    fn mapped_override_redirect_window(&mut self, _xwm: XwmId, window: X11Surface) {
        let loc = window.geometry().loc;
        let elem = Window::new_x11_window(window);
        self.space.map_element(elem, loc, true);
        self.schedule_redraw();
    }

    fn unmapped_window(&mut self, _xwm: XwmId, window: X11Surface) {
        if let Some(elem) = self.x11_element(&window) {
            self.space.unmap_elem(&elem);
        }
        if !window.is_override_redirect() {
            let _ = window.set_mapped(false);
        }
        self.schedule_redraw();
    }

    fn destroyed_window(&mut self, _xwm: XwmId, _window: X11Surface) {
        self.schedule_redraw();
    }

    fn configure_request(
        &mut self,
        _xwm: XwmId,
        window: X11Surface,
        _x: Option<i32>,
        _y: Option<i32>,
        w: Option<u32>,
        h: Option<u32>,
        _reorder: Option<Reorder>,
    ) {
        // Honor client size requests, but keep placement under our control.
        let mut geo = window.geometry();
        if let Some(w) = w {
            geo.size.w = w as i32;
        }
        if let Some(h) = h {
            geo.size.h = h as i32;
        }
        let _ = window.configure(geo);
    }

    fn configure_notify(
        &mut self,
        _xwm: XwmId,
        window: X11Surface,
        geometry: Rectangle<i32, Logical>,
        _above: Option<u32>,
    ) {
        let Some(elem) = self.x11_element(&window) else {
            return;
        };
        self.space.map_element(elem, geometry.loc, false);
        self.schedule_redraw();
    }

    fn resize_request(
        &mut self,
        _xwm: XwmId,
        _window: X11Surface,
        _button: u32,
        _edges: X11ResizeEdge,
    ) {
        // Interactive resize for X11 windows is not wired into Metis's grab
        // machinery yet; the window keeps its current size.
    }

    fn move_request(&mut self, _xwm: XwmId, window: X11Surface, _button: u32) {
        // No interactive move grab yet; at least raise + focus the window so the
        // user can interact with it.
        if let Some(elem) = self.x11_element(&window) {
            self.space.raise_element(&elem, true);
            self.focus_x11(&elem);
        }
    }
}
