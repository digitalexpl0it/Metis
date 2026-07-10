//! Native Metis screenshot overlay session tracking.

use smithay::desktop::{layer_map_for_output, LayerSurface};
use smithay::utils::Serial;

use crate::focus::KeyboardFocusTarget;
use crate::state::MetisState;

#[derive(Debug, Default)]
pub(crate) struct ScreenshotOverlaySession {
    pub active: bool,
}

impl MetisState {
    pub(crate) fn begin_screenshot_overlay(&mut self) {
        self.screenshot_overlay.active = true;
        self.schedule_redraw();
    }

    pub(crate) fn end_screenshot_overlay(&mut self) {
        self.screenshot_overlay.active = false;
        self.schedule_redraw();
    }

    pub(crate) fn screenshot_overlay_active(&self) -> bool {
        self.screenshot_overlay.active
    }

    pub(crate) fn screenshot_overlay_layer(&self) -> Option<LayerSurface> {
        for output in self.space.outputs() {
            let map = layer_map_for_output(output);
            let layers: Vec<LayerSurface> = map.layers().cloned().collect();
            if let Some(layer) = layers
                .into_iter()
                .find(|l| l.namespace() == "metis-screenshot")
            {
                return Some(layer);
            }
        }
        None
    }

    pub(crate) fn focus_screenshot_overlay(&mut self, serial: Serial) {
        let Some(layer) = self.screenshot_overlay_layer() else {
            return;
        };
        if let Some(keyboard) = self.seat.get_keyboard() {
            keyboard.set_focus(self, Some(KeyboardFocusTarget::from(layer)), serial);
        }
    }
}
