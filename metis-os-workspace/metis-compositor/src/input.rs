use smithay::{
    backend::input::{
        AbsolutePositionEvent, Axis, ButtonState, Event, InputBackend, InputEvent,
        KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent, PointerMotionEvent,
    },
    backend::input::KeyState,
    input::{
        keyboard::{keysyms, FilterResult},
        pointer::{AxisFrame, ButtonEvent, MotionEvent, RelativeMotionEvent},
    },
    utils::{Logical, Point, SERIAL_COUNTER, Serial},
    wayland::pointer_constraints::{with_pointer_constraint, PointerConstraint},
};

use crate::focus::KeyboardFocusTarget;
use crate::keybinds::mod_active;
use crate::state::MetisState;

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
                            // Use the layout's raw Latin sym so Mod+Shift+<n>
                            // (whose modified sym is punctuation) still maps to a digit.
                            let digit_sym = keysym
                                .raw_latin_sym_or_raw_current_sym()
                                .map(u32::from)
                                .unwrap_or(sym);
                            // Standalone-session escape hatches (DRM backend only):
                            // Ctrl+Alt+F<n> switches VT, Ctrl+Alt+Backspace quits
                            // back to the greeter. Checked first so they always win.
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
                            // Super+Alt+←/→ cycles workspaces in order (wraps at
                            // 1..=count). Always Super+Alt regardless of METIS_MOD.
                            if modifiers.logo
                                && modifiers.alt
                                && !modifiers.shift
                                && !modifiers.ctrl
                            {
                                let key = state
                                    .output_under_pointer()
                                    .map(|o| o.name())
                                    .unwrap_or_else(|| state.primary_key());
                                let cycled = match sym {
                                    keysyms::KEY_Left => {
                                        state.cycle_workspace_routed(&key, -1);
                                        true
                                    }
                                    keysyms::KEY_Right => {
                                        state.cycle_workspace_routed(&key, 1);
                                        true
                                    }
                                    _ => false,
                                };
                                if cycled {
                                    return FilterResult::Intercept(());
                                }
                            }
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
                            // Super+Shift+F force-toggles *true fullscreen* on the
                            // focused window regardless of whether the client asked
                            // for it — a reliable rescue for games that only offer
                            // "windowed" (e.g. Hytale) or that never issue a
                            // fullscreen request. Checked before the plain Super+F
                            // maximize bind so Shift disambiguates.
                            if mod_active(modifiers)
                                && modifiers.shift
                                && sym == keysyms::KEY_f
                            {
                                if let Some(id) = state.focused_window_id() {
                                    let fs = state
                                        .windows
                                        .get(id)
                                        .map(|w| w.fullscreen)
                                        .unwrap_or(false);
                                    state.set_fullscreen(id, !fs, None);
                                }
                                return FilterResult::Intercept(());
                            }
                            if mod_active(modifiers)
                                && !modifiers.shift
                                && sym == keysyms::KEY_f
                            {
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
                            if mod_active(modifiers) && sym == keysyms::KEY_m {
                                if let Some(id) = state.focused_window_id() {
                                    state.minimize_by_id(id);
                                }
                                return FilterResult::Intercept(());
                            }
                            if mod_active(modifiers) && sym == keysyms::KEY_l {
                                state.lock_session();
                                return FilterResult::Intercept(());
                            }
                            // Escape hatch out of fullscreen / maximize / tiling is
                            // bound to *Super+Esc*. Bare `Esc` MUST fall through to
                            // the focused client: games open their in-game menu with
                            // it, and apps use it for cancel/close — a compositor that
                            // swallows bare Esc breaks both.
                            if mod_active(modifiers) && sym == keysyms::KEY_Escape {
                                if let Some(id) = state.focused_window_id() {
                                    if state
                                        .windows
                                        .get(id)
                                        .is_some_and(|w| w.fullscreen)
                                    {
                                        state.set_fullscreen(id, false, None);
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
                            // Trace bare Esc forwarded to a game (usually opens pause menu).
                            if sym == keysyms::KEY_Escape
                                && !mod_active(modifiers)
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
                    // Locked: the cursor stays put; deliver relative motion only.
                    // Do not schedule_redraw — nothing on screen changes and the
                    // game repaints from its own commits; repainting here on every
                    // mouse poll was saturating the compositor during mouse-look.
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
                if !self.should_forward_pointer_motion(location) {
                    return;
                }
                let serial = SERIAL_COUNTER.next_serial();
                let new_under = self.pointer_target_at(location);
                pointer.motion(
                    self,
                    new_under.clone(),
                    &MotionEvent {
                        location,
                        serial,
                        time: event.time_msec(),
                    },
                );
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
                if !self.should_forward_pointer_motion(pos) {
                    return;
                }
                let serial = SERIAL_COUNTER.next_serial();
                let under = self.pointer_target_at(pos);
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
                let raw_loc = pointer.current_location();
                let under = self.pointer_target_at(raw_loc);
                let loc = self.effective_pointer_delivery_loc(&pointer, under.as_ref());
                if loc != raw_loc {
                    pointer.set_location(loc);
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
                    // Any press off the bar/popover dismisses open popovers — do this
                    // FIRST so it still fires when the press is consumed by compositor
                    // chrome (resize band or server-side titlebar) and returns early.
                    if !pointer.is_grabbed() && !on_bar_ui {
                        let _ = metis_protocol::write_runtime_command("close-popovers");
                    }
                    // Terminals (kitty, foot, …) use right/middle-click paste and
                    // context menus against the surface under the pointer — align
                    // clipboard + primary-selection focus before any chrome handler
                    // can short-circuit the press path.
                    if paste_button && !on_bar_ui {
                        self.sync_selection_focus_from_target(&under);
                    }
                    let mut chrome_press = false;
                    if !on_bar_ui && button == BTN_LEFT && !self.capture_overlay_active() {
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
