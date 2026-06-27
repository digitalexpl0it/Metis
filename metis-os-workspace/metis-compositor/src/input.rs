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
use crate::keybinds::mod_active;
use crate::state::MetisState;

/// Map a number-row keysym (1..9) to a 1-based workspace id, for Mod+<n> bindings.
fn workspace_from_keysym(sym: u32) -> Option<u32> {
    match sym {
        keysyms::KEY_1 => Some(1),
        keysyms::KEY_2 => Some(2),
        keysyms::KEY_3 => Some(3),
        keysyms::KEY_4 => Some(4),
        keysyms::KEY_5 => Some(5),
        keysyms::KEY_6 => Some(6),
        keysyms::KEY_7 => Some(7),
        keysyms::KEY_8 => Some(8),
        keysyms::KEY_9 => Some(9),
        _ => None,
    }
}

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
                            // Use the layout's raw Latin sym so Mod+Shift+<n>
                            // (whose modified sym is punctuation) still maps to a digit.
                            let digit_sym = keysym
                                .raw_latin_sym_or_raw_current_sym()
                                .map(u32::from)
                                .unwrap_or(sym);
                            if mod_active(modifiers) {
                                if let Some(ws) = workspace_from_keysym(digit_sym) {
                                    if modifiers.shift {
                                        if let Some(id) = state.focused_window_id() {
                                            state.move_window_to_workspace(id, ws);
                                        }
                                    } else {
                                        // Target the output under the pointer; in
                                        // linked mode this switches every output to
                                        // the same workspace at once.
                                        let key = state
                                            .output_under_pointer()
                                            .map(|o| o.name())
                                            .unwrap_or_else(|| state.primary_key());
                                        state.switch_workspace_routed(&key, ws);
                                    }
                                    return FilterResult::Intercept(());
                                }
                                // Scrolling-layout navigation. Each helper only acts
                                // (and reports `true`) when the workspace under the
                                // pointer is in scroll mode, so these keys still
                                // forward to apps on grid workspaces.
                                let handled = match sym {
                                    keysyms::KEY_Left if modifiers.shift && !modifiers.ctrl => {
                                        state.scroll_move_left()
                                    }
                                    keysyms::KEY_Right if modifiers.shift && !modifiers.ctrl => {
                                        state.scroll_move_right()
                                    }
                                    keysyms::KEY_Up if modifiers.shift && !modifiers.ctrl => {
                                        state.scroll_move_up()
                                    }
                                    keysyms::KEY_Down if modifiers.shift && !modifiers.ctrl => {
                                        state.scroll_move_down()
                                    }
                                    keysyms::KEY_Left => state.scroll_focus_left(),
                                    keysyms::KEY_Right => state.scroll_focus_right(),
                                    keysyms::KEY_Up => state.scroll_focus_up(),
                                    keysyms::KEY_Down => state.scroll_focus_down(),
                                    keysyms::KEY_comma => state.scroll_consume(),
                                    keysyms::KEY_period => state.scroll_expel(),
                                    keysyms::KEY_minus | keysyms::KEY_equal => {
                                        state.scroll_cycle_width()
                                    }
                                    _ => false,
                                };
                                if handled {
                                    return FilterResult::Intercept(());
                                }
                                // Cross-output move on grid workspaces (scroll mode
                                // reserves Super+Shift+arrows for column/window moves).
                                if modifiers.shift
                                    && !modifiers.ctrl
                                    && !state.scroll_navigation_active()
                                {
                                    let cross = match sym {
                                        keysyms::KEY_Left => state
                                            .focused_window_id()
                                            .map(|id| {
                                                state.move_window_to_adjacent_output(id, -1);
                                            }),
                                        keysyms::KEY_Right => state
                                            .focused_window_id()
                                            .map(|id| {
                                                state.move_window_to_adjacent_output(id, 1);
                                            }),
                                        _ => None,
                                    };
                                    if cross.is_some() {
                                        return FilterResult::Intercept(());
                                    }
                                }
                                // Move the active workspace to an adjacent output
                                // (independent per-output mode only).
                                if modifiers.ctrl
                                    && modifiers.shift
                                    && state.workspace_mode()
                                        == metis_config::WorkspaceMode::Separate
                                {
                                    let key = state
                                        .output_under_pointer()
                                        .map(|o| o.name())
                                        .unwrap_or_else(|| state.primary_key());
                                    let moved = matches!(
                                        sym,
                                        keysyms::KEY_Left | keysyms::KEY_Right
                                    ) && {
                                        match sym {
                                            keysyms::KEY_Left => {
                                                state.move_active_workspace_to_adjacent_output(
                                                    &key, -1,
                                                );
                                                true
                                            }
                                            keysyms::KEY_Right => {
                                                state.move_active_workspace_to_adjacent_output(
                                                    &key, 1,
                                                );
                                                true
                                            }
                                            _ => false,
                                        }
                                    };
                                    if moved {
                                        return FilterResult::Intercept(());
                                    }
                                }
                                // Alt+/ turns grid tiling on; Alt+\ returns to free desktop.
                                if mod_active(modifiers) && sym == keysyms::KEY_slash {
                                    let key = state
                                        .output_under_pointer()
                                        .map(|o| o.name())
                                        .unwrap_or_else(|| state.primary_key());
                                    state.enable_grid_tiling(&key);
                                    return FilterResult::Intercept(());
                                }
                                if mod_active(modifiers) && sym == keysyms::KEY_backslash {
                                    let key = state
                                        .output_under_pointer()
                                        .map(|o| o.name())
                                        .unwrap_or_else(|| state.primary_key());
                                    state.disable_grid_tiling(&key);
                                    return FilterResult::Intercept(());
                                }
                            }
                            if mod_active(modifiers) && sym == keysyms::KEY_q {
                                if let Some(id) = state.focused_window_id() {
                                    state.close_window(id);
                                }
                                return FilterResult::Intercept(());
                            }
                            if mod_active(modifiers) && sym == keysyms::KEY_f {
                                if let Some(id) = state.focused_window_id() {
                                    let maxed = state
                                        .windows
                                        .get(id)
                                        .map(|w| w.maximized)
                                        .unwrap_or(false);
                                    state.set_maximized(id, !maxed);
                                }
                                return FilterResult::Intercept(());
                            }
                            if sym == keysyms::KEY_Escape {
                                if let Some(id) = state.focused_window_id() {
                                    if state
                                        .windows
                                        .get(id)
                                        .is_some_and(|w| w.fullscreen)
                                    {
                                        state.set_fullscreen(id, false);
                                    } else if state
                                        .windows
                                        .get(id)
                                        .is_some_and(|w| w.maximized)
                                    {
                                        state.set_maximized(id, false);
                                    } else if let Some(tile_id) =
                                        state.tile_id_for_window(id)
                                    {
                                        state.set_tile_mode(
                                            &tile_id,
                                            metis_protocol::TileMode::Grid,
                                        );
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
                // The absolute position is normalized to the whole winit window, so
                // map it across the full virtual desktop (all outputs), not just the
                // first one — otherwise multi-output sessions compress the cursor
                // into the primary output's (now smaller) rect.
                let bounds = self.desktop_bounds();
                let pos = event.position_transformed(bounds.size) + bounds.loc.to_f64();
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
                    // Any press off the bar/popover dismisses open popovers — do this
                    // FIRST so it still fires when the press is consumed by compositor
                    // chrome (resize band or server-side titlebar) and returns early.
                    // Without this, clicking another window's titlebar left the start
                    // menu open while every other off-bar click closed it.
                    //
                    // When a popup grab is active, smithay's PopupPointerGrab already
                    // dismisses popovers on outside clicks (popup_done); only fall back
                    // to the manual signal when no grab is in effect.
                    if !pointer.is_grabbed() && !on_bar_ui {
                        let _ = metis_protocol::write_runtime_command("close-popovers");
                    }
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
                // Tell the shell (taskbar) which window now has focus — focus
                // changes are otherwise only reported as a reply to FocusWindow.
                if let Some(id) = self.windows.id_for_window(window) {
                    self.sync_scroll_focus_for_window(id);
                    self.event_bus
                        .emit(&metis_protocol::CompositorEvent::WindowFocused { id });
                }
            }
            keyboard.set_focus(self, Some(target), serial);
            return;
        }

        keyboard.set_focus(self, Option::<KeyboardFocusTarget>::None, serial);
    }
}
