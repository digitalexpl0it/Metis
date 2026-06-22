use smithay::{
    backend::input::{
        AbsolutePositionEvent, Axis, ButtonState, Event, InputBackend, InputEvent,
        KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent, PointerMotionEvent,
    },
    backend::input::KeyState,
    input::{
        keyboard::{keysyms, FilterResult},
        pointer::{AxisFrame, ButtonEvent, MotionEvent},
    },
    utils::{Logical, Point, SERIAL_COUNTER, Serial},
};

use crate::focus::KeyboardFocusTarget;
use crate::state::MetisState;

impl MetisState {
    pub fn process_input_event<B: InputBackend>(&mut self, event: InputEvent<B>) {
        let mut needs_redraw = false;
        match event {
            InputEvent::Keyboard { event, .. } => {
                needs_redraw = true;
                let serial = SERIAL_COUNTER.next_serial();
                let time = Event::time_msec(&event);
                let key_state = event.state();

                self.seat.get_keyboard().unwrap().input::<(), _>(
                    self,
                    event.key_code(),
                    key_state,
                    serial,
                    time,
                    |state, modifiers, keysym| {
                        if key_state == KeyState::Pressed {
                            let sym = u32::from(keysym.modified_sym());
                            if modifiers.logo && sym == keysyms::KEY_q {
                                if let Some(id) = state.focused_window_id() {
                                    state.close_window(id);
                                }
                                return FilterResult::Intercept(());
                            }
                            if modifiers.logo && sym == keysyms::KEY_f {
                                if let Some(id) = state.focused_window_id() {
                                    if let Some(tile_id) = state.tile_id_for_window(id) {
                                        let fs = !state
                                            .windows
                                            .get(id)
                                            .map(|w| w.fullscreen)
                                            .unwrap_or(false);
                                        let mode = if fs {
                                            metis_protocol::TileMode::AppFullscreen
                                        } else {
                                            metis_protocol::TileMode::Grid
                                        };
                                        state.set_tile_mode(&tile_id, mode);
                                    } else {
                                        let fs = state
                                            .windows
                                            .get(id)
                                            .map(|w| !w.fullscreen)
                                            .unwrap_or(false);
                                        state.set_fullscreen(id, fs);
                                    }
                                }
                                return FilterResult::Intercept(());
                            }
                            if sym == keysyms::KEY_Escape {
                                if let Some(id) = state.focused_window_id() {
                                    if let Some(tile_id) = state.tile_id_for_window(id) {
                                        state.set_tile_mode(&tile_id, metis_protocol::TileMode::Grid);
                                    } else {
                                        state.set_fullscreen(id, false);
                                    }
                                }
                                return FilterResult::Intercept(());
                            }
                        }
                        FilterResult::Forward
                    },
                );
            }
            InputEvent::PointerMotion { event, .. } => {
                let pointer = self.seat.get_pointer().unwrap();
                let location = pointer.current_location() + event.delta();
                pointer.set_location(location);
                // Redraw so a client-drawn cursor follows the pointer.
                self.schedule_redraw();
                self.update_hover_cursor(location);
                if !self.should_forward_pointer_motion(location) {
                    return;
                }
                let serial = SERIAL_COUNTER.next_serial();
                let under = self.surface_under(location);
                pointer.motion(
                    self,
                    under,
                    &MotionEvent {
                        location,
                        serial,
                        time: event.time_msec(),
                    },
                );
                pointer.frame(self);
            }
            InputEvent::PointerMotionAbsolute { event, .. } => {
                let output = self.space.outputs().next().unwrap();
                let output_geo = self.space.output_geometry(output).unwrap();
                let pos = event.position_transformed(output_geo.size) + output_geo.loc.to_f64();
                let pointer = self.seat.get_pointer().unwrap();
                pointer.set_location(pos);
                // Redraw so a client-drawn cursor follows the pointer.
                self.schedule_redraw();
                self.update_hover_cursor(pos);
                if !self.should_forward_pointer_motion(pos) {
                    return;
                }
                let serial = SERIAL_COUNTER.next_serial();
                let under = self.surface_under(pos);
                pointer.motion(
                    self,
                    under,
                    &MotionEvent {
                        location: pos,
                        serial,
                        time: event.time_msec(),
                    },
                );
                pointer.frame(self);
            }
            InputEvent::PointerButton { event, .. } => {
                needs_redraw = true;
                let pointer = self.seat.get_pointer().unwrap();
                let serial = SERIAL_COUNTER.next_serial();
                let button = event.button_code();
                let button_state = event.state();
                let loc = pointer.current_location();
                let under = self.surface_under(loc);

                // Sync motion target before button so layer-shell clients receive enter/press.
                pointer.motion(
                    self,
                    under.clone(),
                    &MotionEvent {
                        location: loc,
                        serial,
                        time: event.time_msec(),
                    },
                );

                if ButtonState::Pressed == button_state {
                    // A press over the bar or one of its open popovers (e.g. the app
                    // launcher) belongs to the shell. The bar's popovers don't take a
                    // pointer grab, so without this guard a click would fall through to
                    // window resize/move chrome rendered geometrically *beneath* the
                    // popover — letting you drag a window through the open menu.
                    let on_bar_ui = self.metis_bar_ui_hit(loc);
                    if !on_bar_ui {
                        // Window resize bands sit on the outer edges/corners — check them
                        // before decorations so grabbing an edge starts a resize (and
                        // floats a tiled window out of the grid).
                        if self.handle_resize_press(loc, serial) {
                            self.schedule_redraw();
                            return;
                        }
                        // Server-side decorations (titlebar buttons / drag / border) are
                        // compositor chrome, not client surfaces — intercept before any
                        // client forwarding so close/min/max and titlebar drag work.
                        if self.handle_decoration_press(loc, serial) {
                            self.schedule_redraw();
                            return;
                        }
                    }
                    // When a popup grab is active, smithay's PopupPointerGrab already
                    // dismisses popovers on outside clicks (popup_done). Only fall back
                    // to the manual signal when no grab is in effect.
                    if !pointer.is_grabbed() && !on_bar_ui {
                        let _ = metis_protocol::write_runtime_command("close-popovers");
                    }
                    // Always move keyboard focus to whatever was clicked — including
                    // the bar's own OnDemand layer surface. Text entries inside
                    // non-grabbing bar popovers (Wi-Fi password, world-clock search)
                    // only receive keystrokes if the bar layer surface holds
                    // wl_keyboard focus; GTK then routes keys to the focused widget.
                    self.update_keyboard_focus(loc, serial);
                }

                pointer.button(
                    self,
                    &ButtonEvent {
                        button,
                        state: button_state,
                        serial,
                        time: event.time_msec(),
                    },
                );
                pointer.frame(self);
            }
            InputEvent::PointerAxis { event, .. } => {
                needs_redraw = true;
                let source = event.source();
                let horizontal_amount = event
                    .amount(Axis::Horizontal)
                    .unwrap_or_else(|| event.amount_v120(Axis::Horizontal).unwrap_or(0.0) * 15.0 / 120.);
                let vertical_amount = event
                    .amount(Axis::Vertical)
                    .unwrap_or_else(|| event.amount_v120(Axis::Vertical).unwrap_or(0.0) * 15.0 / 120.);

                let mut frame = AxisFrame::new(event.time_msec()).source(source);
                if horizontal_amount != 0.0 {
                    frame = frame.value(Axis::Horizontal, horizontal_amount);
                }
                if vertical_amount != 0.0 {
                    frame = frame.value(Axis::Vertical, vertical_amount);
                }

                let pointer = self.seat.get_pointer().unwrap();
                pointer.axis(self, frame);
                pointer.frame(self);
            }
            _ => {}
        }
        if needs_redraw {
            self.schedule_redraw();
        }
    }

    fn update_keyboard_focus(&mut self, location: Point<f64, Logical>, serial: Serial) {
        let keyboard = self.seat.get_keyboard().unwrap();
        let pointer = self.seat.get_pointer().unwrap();

        if pointer.is_grabbed() || keyboard.is_grabbed() {
            return;
        }

        if let Some(target) = self.focus_target_at(location) {
            if let KeyboardFocusTarget::Window(ref window) = target {
                self.space.raise_element(window, true);
                if let Some(toplevel) = window.toplevel() {
                    toplevel.send_pending_configure();
                }
            }
            keyboard.set_focus(self, Some(target), serial);
            return;
        }

        keyboard.set_focus(self, Option::<KeyboardFocusTarget>::None, serial);
    }
}
