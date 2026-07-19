use smithay::{
    backend::input::{
        AbsolutePositionEvent, Axis, ButtonState, Event, InputBackend, InputEvent,
        KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent, PointerMotionEvent,
        TouchCancelEvent, TouchDownEvent, TouchEvent, TouchFrameEvent, TouchMotionEvent,
        TouchUpEvent,
    },
    backend::input::KeyState,
    input::{
        keyboard::{keysyms, FilterResult},
        pointer::{AxisFrame, ButtonEvent, MotionEvent, RelativeMotionEvent},
        touch::{DownEvent, MotionEvent as TouchMotionEventWl, UpEvent},
    },
    utils::{Logical, Point, SERIAL_COUNTER, Serial},
    wayland::pointer_constraints::{with_pointer_constraint, PointerConstraint},
};

use crate::focus::KeyboardFocusTarget;
use crate::keybinds::{capture_active, keysym_to_token, mod_active};
use crate::state::MetisState;
use metis_config::KeybindAction;

/// Map a Ctrl+Alt+F<n> press to a 1-based virtual terminal number. Matches both
/// the dedicated `XF86Switch_VT_n` keysyms (when xkb is configured for them) and
/// the plain `F1..F12` keysyms (the common case under Ctrl+Alt).
fn vt_from_keysym(sym: u32) -> Option<i32> {
    const VT_BASE: u32 = 0x1008_FE01; // XF86Switch_VT_1
    if (VT_BASE..VT_BASE + 12).contains(&sym) {
        return Some((sym - VT_BASE + 1) as i32);
    }
    if (keysyms::KEY_F1..=keysyms::KEY_F12).contains(&sym) {
        return Some((sym - keysyms::KEY_F1 + 1) as i32);
    }
    None
}

/// Run a configured desktop shortcut. Returns `true` when the event should be
/// intercepted (even if the action was a no-op for the current layout).
fn dispatch_keybind(state: &mut MetisState, action: KeybindAction) -> bool {
    if state.lock.locked {
        return false;
    }

    match action {
        KeybindAction::Screenshot => {
            let _ = metis_protocol::write_runtime_command("screenshot");
            true
        }
        KeybindAction::ScreenshotFull => {
            let _ = metis_protocol::write_runtime_command("screenshot instant-full");
            true
        }
        KeybindAction::ScreenshotWindow => {
            let _ = metis_protocol::write_runtime_command("screenshot window");
            true
        }
        KeybindAction::CycleWorkspacePrev => {
            let key = state
                .output_under_pointer()
                .map(|o| o.name())
                .unwrap_or_else(|| state.primary_key());
            state.cycle_workspace_routed(&key, -1);
            true
        }
        KeybindAction::CycleWorkspaceNext => {
            let key = state
                .output_under_pointer()
                .map(|o| o.name())
                .unwrap_or_else(|| state.primary_key());
            state.cycle_workspace_routed(&key, 1);
            true
        }
        KeybindAction::Workspace1
        | KeybindAction::Workspace2
        | KeybindAction::Workspace3
        | KeybindAction::Workspace4
        | KeybindAction::Workspace5
        | KeybindAction::Workspace6
        | KeybindAction::Workspace7
        | KeybindAction::Workspace8
        | KeybindAction::Workspace9 => {
            if let Some(ws) = action.workspace_number() {
                let key = state
                    .output_under_pointer()
                    .map(|o| o.name())
                    .unwrap_or_else(|| state.primary_key());
                state.switch_workspace_routed(&key, ws);
            }
            true
        }
        KeybindAction::MoveToWorkspace1
        | KeybindAction::MoveToWorkspace2
        | KeybindAction::MoveToWorkspace3
        | KeybindAction::MoveToWorkspace4
        | KeybindAction::MoveToWorkspace5
        | KeybindAction::MoveToWorkspace6
        | KeybindAction::MoveToWorkspace7
        | KeybindAction::MoveToWorkspace8
        | KeybindAction::MoveToWorkspace9 => {
            if let (Some(ws), Some(id)) = (action.workspace_number(), state.focused_window_id()) {
                state.move_window_to_workspace(id, ws);
            }
            true
        }
        KeybindAction::ScrollFocusLeft => state.scroll_focus_left(),
        KeybindAction::ScrollFocusRight => state.scroll_focus_right(),
        KeybindAction::ScrollFocusUp => state.scroll_focus_up(),
        KeybindAction::ScrollFocusDown => state.scroll_focus_down(),
        KeybindAction::ScrollMoveLeft => {
            if state.scroll_move_left() {
                true
            } else if !state.scroll_navigation_active() {
                if let Some(id) = state.focused_window_id() {
                    state.move_window_to_adjacent_output(id, -1);
                }
                true
            } else {
                false
            }
        }
        KeybindAction::ScrollMoveRight => {
            if state.scroll_move_right() {
                true
            } else if !state.scroll_navigation_active() {
                if let Some(id) = state.focused_window_id() {
                    state.move_window_to_adjacent_output(id, 1);
                }
                true
            } else {
                false
            }
        }
        KeybindAction::ScrollMoveUp => state.scroll_move_up(),
        KeybindAction::ScrollMoveDown => state.scroll_move_down(),
        KeybindAction::ScrollConsume => state.scroll_consume(),
        KeybindAction::ScrollExpel => state.scroll_expel(),
        KeybindAction::ScrollCycleWidth => state.scroll_cycle_width(),
        KeybindAction::MoveWorkspaceOutputLeft => {
            if state.workspace_mode() == metis_config::WorkspaceMode::Separate {
                let key = state
                    .output_under_pointer()
                    .map(|o| o.name())
                    .unwrap_or_else(|| state.primary_key());
                state.move_active_workspace_to_adjacent_output(&key, -1);
                true
            } else {
                false
            }
        }
        KeybindAction::MoveWorkspaceOutputRight => {
            if state.workspace_mode() == metis_config::WorkspaceMode::Separate {
                let key = state
                    .output_under_pointer()
                    .map(|o| o.name())
                    .unwrap_or_else(|| state.primary_key());
                state.move_active_workspace_to_adjacent_output(&key, 1);
                true
            } else {
                false
            }
        }
        KeybindAction::LayoutGrid => {
            let key = state
                .output_under_pointer()
                .map(|o| o.name())
                .unwrap_or_else(|| state.primary_key());
            state.enable_grid_tiling(&key);
            true
        }
        KeybindAction::LayoutFree => {
            let key = state
                .output_under_pointer()
                .map(|o| o.name())
                .unwrap_or_else(|| state.primary_key());
            state.disable_grid_tiling(&key);
            true
        }
        KeybindAction::CloseWindow => {
            if let Some(id) = state.focused_window_id() {
                state.close_window(id);
            }
            true
        }
        KeybindAction::Fullscreen => {
            if let Some(id) = state.focused_window_id() {
                let fs = state.windows.get(id).map(|w| w.fullscreen).unwrap_or(false);
                state.set_fullscreen(id, !fs, None);
            }
            true
        }
        KeybindAction::Maximize => {
            if let Some(id) = state.focused_window_id() {
                let maxed = state.windows.get(id).map(|w| w.maximized).unwrap_or(false);
                state.set_maximized(id, !maxed);
            }
            true
        }
        KeybindAction::Minimize => {
            if let Some(id) = state.focused_window_id() {
                state.minimize_by_id(id);
            }
            true
        }
        KeybindAction::Lock => {
            state.lock_session();
            true
        }
        KeybindAction::ExitFullscreenStack => {
            if let Some(id) = state.focused_window_id() {
                if state.windows.get(id).is_some_and(|w| w.fullscreen) {
                    state.set_fullscreen(id, false, None);
                } else if state.windows.get(id).is_some_and(|w| w.maximized) {
                    state.set_maximized(id, false);
                } else if let Some(tile_id) = state.tile_id_for_window(id) {
                    state.set_tile_mode(&tile_id, metis_protocol::TileMode::Grid);
                }
            }
            true
        }
    }
}

impl MetisState {
    pub fn process_input_event<B: InputBackend>(&mut self, event: InputEvent<B>) {
        // Any hardware input counts as activity: wake a blanked screen and
        // restart the idle countdown before dispatching the event.
        self.idle_notify_activity();

        // While the session is locked, no pointer input reaches clients — motion
        // still moves the (compositor-drawn) cursor and repaints, but buttons and
        // scroll are swallowed. Keyboard events fall through to the filter below,
        // which routes them into the password buffer and never forwards them.
        if self.lock.locked && !matches!(event, InputEvent::Keyboard { .. }) {
            if let Some(pointer) = self.seat.get_pointer() {
                match &event {
                    InputEvent::PointerMotion { event: e, .. } => {
                        let loc = self.clamp_to_desktop(pointer.current_location() + e.delta());
                        pointer.set_location(loc);
                        self.lock_update_hover(loc);
                    }
                    InputEvent::PointerMotionAbsolute { event: e, .. } => {
                        let bounds = self.desktop_bounds();
                        let pos = e.position_transformed(bounds.size) + bounds.loc.to_f64();
                        pointer.set_location(pos);
                        self.lock_update_hover(pos);
                    }
                    InputEvent::PointerButton { event: e, .. } => {
                        // Left-press on a power control (suspend/restart/shutdown) fires it.
                        if e.state() == ButtonState::Pressed && e.button_code() == 0x110 {
                            let loc = pointer.current_location();
                            self.lock_pointer_click(loc);
                        }
                    }
                    InputEvent::TouchDown { event: e, .. } => {
                        if let Some(loc) = self.touch_location_transformed(e) {
                            self.lock_update_hover(loc);
                            self.lock_pointer_click(loc);
                        }
                    }
                    InputEvent::TouchMotion { event: e, .. } => {
                        if let Some(loc) = self.touch_location_transformed(e) {
                            self.lock_update_hover(loc);
                        }
                    }
                    _ => {}
                }
            }
            self.schedule_redraw();
            return;
        }

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
                        // Locked: capture every key into the password field and
                        // never forward it to a client. Ctrl+Alt VT-switch / quit
                        // escape hatches on the DRM backend stay live so a wedged
                        // lock screen can always be recovered.
                        if state.lock.locked {
                            if key_state == KeyState::Pressed {
                                let sym = u32::from(keysym.modified_sym());
                                if state.is_drm_backend() && modifiers.ctrl && modifiers.alt {
                                    if sym == keysyms::KEY_BackSpace {
                                        state.drm_quit();
                                        return FilterResult::Intercept(());
                                    }
                                    let vt_sym = keysym
                                        .raw_latin_sym_or_raw_current_sym()
                                        .map(u32::from)
                                        .unwrap_or(sym);
                                    if let Some(vt) =
                                        vt_from_keysym(sym).or_else(|| vt_from_keysym(vt_sym))
                                    {
                                        state.drm_change_vt(vt);
                                        return FilterResult::Intercept(());
                                    }
                                }
                                match sym {
                                    keysyms::KEY_Return | keysyms::KEY_KP_Enter => {
                                        state.lock_submit()
                                    }
                                    keysyms::KEY_BackSpace => state.lock_backspace(),
                                    keysyms::KEY_Escape => state.lock_clear_input(),
                                    _ => {
                                        if let Some(c) = keysym.modified_sym().key_char() {
                                            if !c.is_control() {
                                                state.lock_push_char(c);
                                            }
                                        }
                                    }
                                }
                            }
                            return FilterResult::Intercept(());
                        }
                        if key_state == KeyState::Pressed {
                            let sym = u32::from(keysym.modified_sym());
                            if state.screenshot_overlay_active()
                                && sym == keysyms::KEY_Escape
                                && !mod_active(&state.keybinds, modifiers)
                            {
                                let _ = metis_protocol::write_runtime_command("dismiss-screenshot");
                                return FilterResult::Intercept(());
                            }
                            // Use the layout's raw Latin sym so Mod+Shift+<n>
                            // (whose modified sym is punctuation) still maps to a digit.
                            let digit_sym = keysym
                                .raw_latin_sym_or_raw_current_sym()
                                .map(u32::from)
                                .unwrap_or(sym);
                            // Standalone-session escape hatches (DRM backend only):
                            // Ctrl+Alt+F<n> switches VT, Ctrl+Alt+Backspace quits
                            // back to the greeter. Always reserved — not user-rebindable.
                            if state.is_drm_backend() && modifiers.ctrl && modifiers.alt {
                                if sym == keysyms::KEY_BackSpace {
                                    state.drm_quit();
                                    return FilterResult::Intercept(());
                                }
                                let vt_sym = keysym
                                    .raw_latin_sym_or_raw_current_sym()
                                    .map(u32::from)
                                    .unwrap_or(sym);
                                if let Some(vt) = vt_from_keysym(sym).or_else(|| vt_from_keysym(vt_sym)) {
                                    state.drm_change_vt(vt);
                                    return FilterResult::Intercept(());
                                }
                            }
                            // Settings shortcut capture: do not fire global actions.
                            if capture_active() {
                                return FilterResult::Forward;
                            }
                            if let Some(token) = keysym_to_token(sym, digit_sym) {
                                if let Some(action) = state.keybinds.lookup(modifiers, &token) {
                                    if dispatch_keybind(state, action) {
                                        return FilterResult::Intercept(());
                                    }
                                }
                            }
                            // Trace bare Esc forwarded to a game (usually opens pause menu).
                            if sym == keysyms::KEY_Escape
                                && !mod_active(&state.keybinds, modifiers)
                                && !state.lock.locked
                            {
                                if let Some(id) = state.focused_window_id() {
                                    let app_id = state
                                        .windows
                                        .get(id)
                                        .and_then(|r| r.app_id.clone());
                                    if app_id.as_deref().is_some_and(|a| {
                                        a.starts_with("steam_app_") || a.contains(".exe")
                                    }) {
                                        tracing::info!(
                                            id,
                                            ?app_id,
                                            "game-pointer: Esc forwarded to game"
                                        );
                                    }
                                }
                            }
                        }
                        FilterResult::Forward
                    },
                );
            }
            InputEvent::PointerMotion { event, .. } => {
                let pointer = self.seat.get_pointer().unwrap();
                let current = pointer.current_location();
                // Surface under the *current* position drives constraint checks:
                // when the pointer is locked it never moves, so the target can't
                // change from raw motion.
                let under = self.pointer_target_at(current);

                if let Some((surface, _)) = under.as_ref() {
                    self.sync_pointer_constraint_phase(surface, &pointer);
                }

                // Pointer constraints (games: mouse-look lock / region confinement).
                let mut pointer_locked = false;
                let mut pointer_confined = false;
                let mut confine_region = None;
                if let Some((surface, surface_loc)) = under.as_ref() {
                    with_pointer_constraint(surface, &pointer, |constraint| {
                        let Some(constraint) = constraint else { return };
                        if !constraint.is_active() {
                            return;
                        }
                        // A region-limited constraint only applies while the
                        // pointer sits inside that region.
                        if !constraint.region().is_none_or(|region| {
                            region.contains((current - *surface_loc).to_i32_round())
                        }) {
                            return;
                        }
                        match &*constraint {
                            PointerConstraint::Locked(_) => pointer_locked = true,
                            PointerConstraint::Confined(confine) => {
                                pointer_confined = true;
                                confine_region = confine.region().cloned();
                            }
                        }
                    });
                }

                // Raw, unclamped delta always goes out as relative motion — this is
                // the signal games use for camera "look".
                pointer.relative_motion(
                    self,
                    under.clone(),
                    &RelativeMotionEvent {
                        delta: event.delta(),
                        delta_unaccel: event.delta_unaccel(),
                        utime: event.time(),
                    },
                );

                if pointer_locked {
                    // Protocol lock: relative only, cursor stays put.
                    self.cursor_hint_click_valid = false;
                    pointer.frame(self);
                    return;
                }

                // Proton/XWayland FPS-style capture: keep absolute pointer at the
                // window centre while still sending relative deltas. Absolute-only
                // look clamps at the edge; skipping absolute entirely breaks X11
                // MotionNotify. Warping absolute to centre (and relative for look)
                // matches gamescope --force-grab-cursor and stops click-aim snaps.
                let over_game = under.as_ref().is_some_and(|(surface, _)| {
                    self.windows
                        .id_for_surface(surface)
                        .is_some_and(|id| self.window_is_running_game(id))
                });
                let game_menu = under
                    .as_ref()
                    .is_some_and(|(surface, _)| self.game_surface_in_menu_mode(surface));
                if over_game && !game_menu {
                    self.cursor_hint_click_valid = false;
                    if let Some((surface, _)) = under.as_ref() {
                        if let Some(anchor) = self.game_pointer_anchor(surface) {
                            pointer.set_location(anchor);
                            let serial = SERIAL_COUNTER.next_serial();
                            pointer.motion(
                                self,
                                under.clone(),
                                &MotionEvent {
                                    location: anchor,
                                    serial,
                                    time: event.time_msec(),
                                },
                            );
                        }
                    }
                    pointer.frame(self);
                    return;
                }

                // Relative motion (libinput) can run off-screen; clamp to the
                // union of output geometries so the cursor stays reachable.
                let location = self.clamp_to_desktop(current + event.delta());

                // Confined: reject moves that would leave the surface or its region.
                if pointer_confined {
                    if let Some((surface, surface_loc)) = under.as_ref() {
                        let new_under = self.pointer_target_at(location);
                        let same_surface =
                            new_under.as_ref().map(|(s, _)| s) == Some(surface);
                        let in_region = confine_region.as_ref().is_none_or(|region| {
                            region.contains((location - *surface_loc).to_i32_round())
                        });
                        if !same_surface || !in_region {
                            pointer.frame(self);
                            return;
                        }
                    }
                }

                pointer.set_location(location);
                // Redraw so a client-drawn cursor follows the pointer.
                self.schedule_redraw();
                self.update_hover_cursor(location);
                self.enforce_capture_overlay_stacking();
                self.maintain_focus_stacking(location);
                let serial = SERIAL_COUNTER.next_serial();
                let new_under = self.pointer_target_at(location);
                let forward = self.should_forward_pointer_motion(location);
                if forward {
                    pointer.motion(
                        self,
                        new_under.clone(),
                        &MotionEvent {
                            location,
                            serial,
                            time: event.time_msec(),
                        },
                    );
                }
                // Always frame so `relative_motion` flushes even when absolute
                // motion is throttled (desktop GTK hover path).
                pointer.frame(self);

                // Arm a not-yet-active constraint once the pointer enters its
                // region (games commonly request the lock before grabbing focus).
                // Never re-arm a lock the client deactivated for a pause menu —
                // see `maybe_arm_pointer_constraint`.
                if let Some((surface, surface_loc)) = new_under {
                    self.maybe_arm_pointer_constraint(
                        &surface,
                        &pointer,
                        location,
                        surface_loc,
                    );
                }
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
                self.enforce_capture_overlay_stacking();
                self.maintain_focus_stacking(pos);
                let serial = SERIAL_COUNTER.next_serial();
                let under = self.pointer_target_at(pos);
                if self.should_forward_pointer_motion(pos) {
                    pointer.motion(
                        self,
                        under,
                        &MotionEvent {
                            location: pos,
                            serial,
                            time: event.time_msec(),
                        },
                    );
                }
                pointer.frame(self);
            }
            InputEvent::PointerButton { event, .. } => {
                needs_redraw = true;
                let pointer = self.seat.get_pointer().unwrap();
                let serial = SERIAL_COUNTER.next_serial();
                let button = event.button_code();
                let button_state = event.state();
                let raw_loc = pointer.current_location();
                let under = self.pointer_target_at(raw_loc);
                let loc = self.effective_pointer_delivery_loc(&pointer, under.as_ref());
                let remapped_via_hint = loc != raw_loc;
                let pointer_locked = under
                    .as_ref()
                    .is_some_and(|(surface, _)| self.pointer_locked_on_surface(surface, &pointer));
                let over_game = under.as_ref().is_some_and(|(surface, _)| {
                    self.windows
                        .id_for_surface(surface)
                        .is_some_and(|id| self.window_is_running_game(id))
                });
                let game_menu = under
                    .as_ref()
                    .is_some_and(|(surface, _)| self.game_surface_in_menu_mode(surface));
                // Gameplay capture keeps the pointer at the window centre so a
                // click cannot inherit a drifted edge position as aim/look.
                let mut loc = loc;
                if over_game && !game_menu && !remapped_via_hint {
                    if let Some((surface, _)) = under.as_ref() {
                        if let Some(anchor) = self.game_pointer_anchor(surface) {
                            pointer.set_location(anchor);
                            loc = anchor;
                        }
                    }
                }
                if remapped_via_hint {
                    if let Some((surface, _)) = under.as_ref() {
                        self.trace_game_pointer(
                            surface,
                            &pointer,
                            "click remapped via cursor position hint",
                            Some(loc),
                        );
                    }
                }

                if let Some((surface, _)) = under.as_ref() {
                    self.sync_pointer_constraint_phase(surface, &pointer);
                }

                if ButtonState::Pressed == button_state {
                    if let Some((surface, _)) = under.as_ref() {
                        self.trace_game_pointer(
                            surface,
                            &pointer,
                            "pointer button press",
                            Some(loc),
                        );
                    }
                }

                // Locked: no absolute unless menu-hint remap. Gameplay warp: send
                // absolute at centre. Desktop: normal absolute at pointer.
                let send_absolute = remapped_via_hint || !pointer_locked;
                if send_absolute {
                    pointer.motion(
                        self,
                        under.clone(),
                        &MotionEvent {
                            location: loc,
                            serial,
                            time: event.time_msec(),
                        },
                    );
                } else if button_state == ButtonState::Pressed {
                    if let Some((surface, _)) = under.as_ref() {
                        self.trace_game_pointer(
                            surface,
                            &pointer,
                            "locked click without absolute motion",
                            Some(raw_loc),
                        );
                    }
                }

                if ButtonState::Pressed == button_state {
                    const BTN_LEFT: u32 = 0x110;
                    const BTN_MIDDLE: u32 = 0x112;
                    const BTN_RIGHT: u32 = 0x111;
                    if self.capture_overlay_active() {
                        self.enforce_capture_overlay_stacking();
                    }
                    let paste_button = button == BTN_MIDDLE || button == BTN_RIGHT;
                    // A press over the bar or one of its open popovers (e.g. the app
                    // launcher) belongs to the shell. The bar's popovers don't take a
                    // pointer grab, so without this guard a click would fall through to
                    // window resize/move chrome rendered geometrically *beneath* the
                    // popover — letting you drag a window through the open menu.
                    let on_bar_ui = self.metis_bar_ui_hit(loc);
                    let on_nc = self.metis_notification_center_hit(loc);
                    // Any press outside the Notification Center dismisses it (and
                    // bar popovers) — including presses on the edge bar. Presses
                    // on the NC panel itself must not dismiss.
                    if !pointer.is_grabbed() && !on_nc {
                        if !on_bar_ui || self.notification_center_mapped() {
                            let _ = metis_protocol::write_runtime_command("close-popovers");
                        }
                    }
                    // Terminals (kitty, foot, …) use right/middle-click paste and
                    // context menus against the surface under the pointer — align
                    // clipboard + primary-selection focus before any chrome handler
                    // can short-circuit the press path.
                    if paste_button && !on_bar_ui {
                        self.sync_selection_focus_from_target(&under);
                    }
                    let mut chrome_press = false;
                    if !on_bar_ui
                        && button == BTN_LEFT
                        && !self.capture_overlay_active()
                        && !self.screenshot_overlay_active()
                    {
                        chrome_press = self.handle_resize_press(loc, serial, button)
                            || self.handle_decoration_press(loc, serial, button);
                        if chrome_press {
                            self.schedule_redraw();
                        }
                    }
                    if !chrome_press {
                        self.update_keyboard_focus(loc, serial);
                        if !paste_button {
                            self.sync_selection_focus_from_target(&under);
                        }
                    }
                } else if button_state == ButtonState::Released && button == 0x110 {
                    self.clear_titlebar_press_pending();
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
                if let Some((surface, _)) = under {
                    self.suppress_spurious_pointer_lock(&surface, &pointer);
                    if button_state == ButtonState::Pressed {
                        self.trace_game_pointer(
                            &surface,
                            &pointer,
                            "after pointer button (post-suppress)",
                            Some(loc),
                        );
                    }
                }
                pointer.frame(self);
                if remapped_via_hint {
                    // Clients are never notified of set_location — this only
                    // rewinds the compositor lock anchor after the synthetic
                    // delivery motion above.
                    pointer.set_location(raw_loc);
                }
            }
            InputEvent::PointerAxis { event, .. } => {
                needs_redraw = true;
                let source = event.source();
                let mult = self.input_runtime.scroll_multiplier();
                let horizontal_amount = event
                    .amount(Axis::Horizontal)
                    .unwrap_or_else(|| event.amount_v120(Axis::Horizontal).unwrap_or(0.0) * 15.0 / 120.)
                    * mult;
                let vertical_amount = event
                    .amount(Axis::Vertical)
                    .unwrap_or_else(|| event.amount_v120(Axis::Vertical).unwrap_or(0.0) * 15.0 / 120.)
                    * mult;

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
            InputEvent::TouchDown { event, .. } => {
                needs_redraw = true;
                self.on_touch_down::<B>(event);
            }
            InputEvent::TouchUp { event, .. } => {
                needs_redraw = true;
                self.on_touch_up::<B>(event);
            }
            InputEvent::TouchMotion { event, .. } => {
                needs_redraw = true;
                self.on_touch_motion::<B>(event);
            }
            InputEvent::TouchFrame { event, .. } => {
                self.on_touch_frame::<B>(event);
            }
            InputEvent::TouchCancel { event, .. } => {
                needs_redraw = true;
                self.on_touch_cancel::<B>(event);
            }
            _ => {}
        }
        if needs_redraw {
            self.schedule_redraw();
        }
    }

    /// Lazily add a `wl_touch` device when the first touchscreen appears.
    pub fn ensure_touch_device(&mut self) {
        if self.seat.get_touch().is_none() {
            self.seat.add_touch();
            tracing::info!("touchscreen detected — wl_touch enabled on seat");
        }
    }

    fn touch_location_transformed<B: InputBackend, E: AbsolutePositionEvent<B>>(
        &self,
        evt: &E,
    ) -> Option<Point<f64, Logical>> {
        let bounds = self.desktop_bounds();
        Some(evt.position_transformed(bounds.size) + bounds.loc.to_f64())
    }

    fn on_touch_down<B: InputBackend>(&mut self, evt: B::TouchDownEvent) {
        self.ensure_touch_device();
        let Some(handle) = self.seat.get_touch() else {
            return;
        };
        let Some(loc) = self.touch_location_transformed(&evt) else {
            return;
        };
        let serial = SERIAL_COUNTER.next_serial();
        self.update_keyboard_focus(loc, serial);
        let under = self.pointer_target_at(loc);
        handle.down(
            self,
            under,
            &DownEvent {
                slot: evt.slot(),
                location: loc,
                serial,
                time: evt.time_msec(),
            },
        );
    }

    fn on_touch_up<B: InputBackend>(&mut self, evt: B::TouchUpEvent) {
        self.ensure_touch_device();
        let Some(handle) = self.seat.get_touch() else {
            return;
        };
        let serial = SERIAL_COUNTER.next_serial();
        handle.up(
            self,
            &UpEvent {
                slot: evt.slot(),
                serial,
                time: evt.time_msec(),
            },
        );
    }

    fn on_touch_motion<B: InputBackend>(&mut self, evt: B::TouchMotionEvent) {
        self.ensure_touch_device();
        let Some(handle) = self.seat.get_touch() else {
            return;
        };
        let Some(loc) = self.touch_location_transformed(&evt) else {
            return;
        };
        let under = self.pointer_target_at(loc);
        handle.motion(
            self,
            under,
            &TouchMotionEventWl {
                slot: evt.slot(),
                location: loc,
                time: evt.time_msec(),
            },
        );
    }

    fn on_touch_frame<B: InputBackend>(&mut self, _evt: B::TouchFrameEvent) {
        if let Some(handle) = self.seat.get_touch() {
            handle.frame(self);
        }
    }

    fn on_touch_cancel<B: InputBackend>(&mut self, _evt: B::TouchCancelEvent) {
        if let Some(handle) = self.seat.get_touch() {
            handle.cancel(self);
        }
    }

    fn update_keyboard_focus(&mut self, location: Point<f64, Logical>, serial: Serial) {
        let keyboard = self.seat.get_keyboard().unwrap();
        let pointer = self.seat.get_pointer().unwrap();

        if pointer.is_grabbed() || keyboard.is_grabbed() {
            return;
        }

        if self.screenshot_overlay_active() {
            self.focus_screenshot_overlay(serial);
            return;
        }

        if self.capture_overlay_active() {
            if let Some(window) = self.top_capture_overlay_window() {
                self.space.raise_element(&window, false);
                if let Some(id) = self.windows.id_for_window(&window) {
                    self.note_window_focus(id);
                }
                keyboard.set_focus(self, Some(window.into()), serial);
            }
            return;
        }

        if let Some(target) = self.focus_target_at(location) {
            if let KeyboardFocusTarget::Window(ref window) = target {
                // Keyboard focus for X11 windows now routes through `X11Surface`,
                // whose `enter` sets X input focus (`XSetInputFocus`) and sends
                // `WM_TAKE_FOCUS`. Re-entering the *same* already-focused window on
                // every pointer click resets the client's in-game UI state: menu
                // items stop opening their dialogs (settings panels never appear),
                // and the game repositions its cursor as if focus changed — the
                // "mouse jumps from the menu on the left to the middle-top where
                // the dialog should be" report during Proton gameplay.
                if self.windows.id_for_window(&window) == self.focused_window_id() {
                    return;
                }
                self.space.raise_element(&window, true);
                if let Some(toplevel) = window.toplevel() {
                    toplevel.send_pending_configure();
                }
                // Tell the shell (taskbar) which window now has focus — focus
                // changes are otherwise only reported as a reply to FocusWindow.
                if let Some(id) = self.windows.id_for_window(&window) {
                    self.note_window_focus(id);
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
