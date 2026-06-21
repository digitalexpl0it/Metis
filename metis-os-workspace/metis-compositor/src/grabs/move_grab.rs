use crate::state::MetisState;
use smithay::{
    desktop::Window,
    input::pointer::{
        AxisFrame, ButtonEvent, GestureHoldBeginEvent, GestureHoldEndEvent, GesturePinchBeginEvent,
        GesturePinchEndEvent, GesturePinchUpdateEvent, GestureSwipeBeginEvent, GestureSwipeEndEvent,
        GestureSwipeUpdateEvent, GrabStartData as PointerGrabStartData, MotionEvent, PointerGrab,
        PointerInnerHandle, RelativeMotionEvent,
    },
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Point},
};

pub struct MoveSurfaceGrab {
    pub start_data: PointerGrabStartData<MetisState>,
    pub window: Window,
    pub initial_window_location: Point<i32, Logical>,
}

impl PointerGrab<MetisState> for MoveSurfaceGrab {
    fn motion(
        &mut self,
        data: &mut MetisState,
        handle: &mut PointerInnerHandle<'_, MetisState>,
        _focus: Option<(WlSurface, Point<f64, Logical>)>,
        event: &MotionEvent,
    ) {
        handle.motion(data, None, event);
        let delta = event.location - self.start_data.location;
        let mut new_location = self.initial_window_location.to_f64() + delta;

        // Constrain dragging: the top edge stays hard-blocked just under the edge
        // bar (nothing may slide above/under the bar), but the window is free to
        // move partially off the left, right, and bottom edges — Hyprland-style —
        // as long as a grabbable slice (`MIN_VISIBLE_PX`) remains on-screen so it
        // can always be pulled back. Fully-off-screen windows (e.g. a monitor that
        // disconnected) are rescued separately in `recover_offscreen_rect`.
        if let Some(zone) = data.usable_zone() {
            let size = self.window.geometry().size;
            let gap = crate::state::BAR_GAP_PX as f64;
            let keep = crate::state::MIN_VISIBLE_PX as f64;
            // Horizontal: allow off-screen either side, keeping `keep` px within.
            let min_x = zone.x as f64 + keep - size.w as f64;
            let max_x = (zone.x + zone.width) as f64 - keep;
            // Vertical: top blocked at the bar; bottom may run off, keeping `keep`.
            // Reserve the server-side titlebar strip above the client so it stays
            // below the edge bar (the chrome is drawn above the mapped body).
            let min_y = zone.y as f64 + gap + metis_grid::APP_TILE_HEADER_PX as f64;
            let max_y = (zone.y + zone.height) as f64 - keep;
            new_location.x = new_location.x.clamp(min_x, max_x.max(min_x));
            new_location.y = new_location.y.clamp(min_y, max_y.max(min_y));
        }

        data.space
            .map_element(self.window.clone(), new_location.to_i32_round(), true);

        if let Some(id) = data
            .windows
            .id_for_surface(self.window.toplevel().unwrap().wl_surface())
        {
            let loc = new_location.to_i32_round();
            let geo = self.window.geometry();
            data.windows.set_target_rect(
                id,
                metis_grid::PixelRect {
                    x: loc.x,
                    y: loc.y,
                    width: geo.size.w,
                    height: geo.size.h,
                },
            );
        }

        // Live snap-zone preview follows the pointer (not the window), so the
        // overlay highlights the destination as the cursor approaches an edge.
        let p = event.location;
        data.snap_preview = data.snap_target_at(p.x.round() as i32, p.y.round() as i32);
        data.damaged = true;
    }

    fn relative_motion(
        &mut self,
        data: &mut MetisState,
        handle: &mut PointerInnerHandle<'_, MetisState>,
        focus: Option<(WlSurface, Point<f64, Logical>)>,
        event: &RelativeMotionEvent,
    ) {
        handle.relative_motion(data, focus, event);
    }

    fn button(
        &mut self,
        data: &mut MetisState,
        handle: &mut PointerInnerHandle<'_, MetisState>,
        event: &ButtonEvent,
    ) {
        handle.button(data, event);
        const BTN_LEFT: u32 = 0x110;
        if !handle.current_pressed().contains(&BTN_LEFT) {
            // Capture the snap target BEFORE unset_grab: unset_grab() invokes our
            // `unset()`, which clears `snap_preview`, so reading it afterwards
            // would always be `None` (the window would never actually snap).
            let snap = data.snap_preview.take();
            handle.unset_grab(self, data, event.serial, event.time, true);
            if let Some(id) = data
                .windows
                .id_for_surface(self.window.toplevel().unwrap().wl_surface())
            {
                // Dropped in a snap zone: tile it there. Otherwise leave it where
                // the user let go (grid windows already floated on drag start).
                if let Some((rect, label)) = snap {
                    data.apply_snap(id, rect, label);
                } else {
                    data.enforce_grid_window_geometry(id);
                }
            }
            data.damaged = true;
        }
    }

    fn axis(
        &mut self,
        data: &mut MetisState,
        handle: &mut PointerInnerHandle<'_, MetisState>,
        details: AxisFrame,
    ) {
        handle.axis(data, details)
    }

    fn frame(&mut self, data: &mut MetisState, handle: &mut PointerInnerHandle<'_, MetisState>) {
        handle.frame(data);
    }

    fn gesture_swipe_begin(
        &mut self,
        data: &mut MetisState,
        handle: &mut PointerInnerHandle<'_, MetisState>,
        event: &GestureSwipeBeginEvent,
    ) {
        handle.gesture_swipe_begin(data, event)
    }

    fn gesture_swipe_update(
        &mut self,
        data: &mut MetisState,
        handle: &mut PointerInnerHandle<'_, MetisState>,
        event: &GestureSwipeUpdateEvent,
    ) {
        handle.gesture_swipe_update(data, event)
    }

    fn gesture_swipe_end(
        &mut self,
        data: &mut MetisState,
        handle: &mut PointerInnerHandle<'_, MetisState>,
        event: &GestureSwipeEndEvent,
    ) {
        handle.gesture_swipe_end(data, event)
    }

    fn gesture_pinch_begin(
        &mut self,
        data: &mut MetisState,
        handle: &mut PointerInnerHandle<'_, MetisState>,
        event: &GesturePinchBeginEvent,
    ) {
        handle.gesture_pinch_begin(data, event)
    }

    fn gesture_pinch_update(
        &mut self,
        data: &mut MetisState,
        handle: &mut PointerInnerHandle<'_, MetisState>,
        event: &GesturePinchUpdateEvent,
    ) {
        handle.gesture_pinch_update(data, event)
    }

    fn gesture_pinch_end(
        &mut self,
        data: &mut MetisState,
        handle: &mut PointerInnerHandle<'_, MetisState>,
        event: &GesturePinchEndEvent,
    ) {
        handle.gesture_pinch_end(data, event)
    }

    fn gesture_hold_begin(
        &mut self,
        data: &mut MetisState,
        handle: &mut PointerInnerHandle<'_, MetisState>,
        event: &GestureHoldBeginEvent,
    ) {
        handle.gesture_hold_begin(data, event)
    }

    fn gesture_hold_end(
        &mut self,
        data: &mut MetisState,
        handle: &mut PointerInnerHandle<'_, MetisState>,
        event: &GestureHoldEndEvent,
    ) {
        handle.gesture_hold_end(data, event)
    }

    fn start_data(&self) -> &PointerGrabStartData<MetisState> {
        &self.start_data
    }

    fn unset(&mut self, data: &mut MetisState) {
        // Clear any lingering preview if the grab ends without a normal release
        // (e.g. the window closed mid-drag), so the overlay never sticks.
        if data.snap_preview.take().is_some() {
            data.damaged = true;
        }
    }
}
