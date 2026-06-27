use crate::state::MetisState;
use smithay::{
    input::pointer::{
        AxisFrame, ButtonEvent, GestureHoldBeginEvent, GestureHoldEndEvent, GesturePinchBeginEvent,
        GesturePinchEndEvent, GesturePinchUpdateEvent, GestureSwipeBeginEvent, GestureSwipeEndEvent,
        GestureSwipeUpdateEvent, GrabStartData as PointerGrabStartData, MotionEvent, PointerGrab,
        PointerInnerHandle, RelativeMotionEvent,
    },
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Point},
};

/// Interactive horizontal resize of a scroll column. Unlike [`super::ResizeSurfaceGrab`]
/// this never floats the window out of the strip: each motion sets the target
/// column's width and the strip reflows so the columns to its right slide over.
pub struct ScrollResizeGrab {
    start_data: PointerGrabStartData<MetisState>,
    /// A representative window of the column being resized.
    target_window: u32,
    /// Column width (px) at grab start.
    initial_width_px: i32,
    /// Pointer x (logical) at grab start.
    start_x: f64,
}

impl ScrollResizeGrab {
    pub fn start(
        start_data: PointerGrabStartData<MetisState>,
        target_window: u32,
        initial_width_px: i32,
        start_x: f64,
    ) -> Self {
        Self {
            start_data,
            target_window,
            initial_width_px,
            start_x,
        }
    }
}

impl PointerGrab<MetisState> for ScrollResizeGrab {
    fn motion(
        &mut self,
        data: &mut MetisState,
        handle: &mut PointerInnerHandle<'_, MetisState>,
        _focus: Option<(WlSurface, Point<f64, Logical>)>,
        event: &MotionEvent,
    ) {
        // Keep the cursor moving but route no surface focus while resizing.
        handle.motion(data, None, event);
        let dx = event.location.x - self.start_x;
        let new_width = self.initial_width_px + dx.round() as i32;
        data.scroll_set_column_width_px(self.target_window, new_width);
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
            handle.unset_grab(self, data, event.serial, event.time, true);
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

    fn unset(&mut self, _data: &mut MetisState) {}
}
