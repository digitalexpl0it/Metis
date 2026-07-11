//! Native Metis screenshot overlay session tracking.

use smithay::desktop::{layer_map_for_output, LayerSurface};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Logical, Point, Serial};

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

    /// While the native screenshot UI is up, deliver all pointer hits to that
    /// layer — including over transparent dim regions — so apps underneath
    /// cannot receive resize/move chrome or focus.
    pub(crate) fn screenshot_overlay_surface_at(
        &self,
        pos: Point<f64, Logical>,
    ) -> Option<(WlSurface, Point<f64, Logical>)> {
        let layer = self.screenshot_overlay_layer()?;
        let output = self.space.outputs().find(|o| {
            self.space
                .output_geometry(o)
                .is_some_and(|geo| geo.contains(pos.to_i32_round()))
        })?;
        let output_geo = self.space.output_geometry(&output)?;
        let layers = layer_map_for_output(&output);
        let layer_geo = layers.layer_geometry(&layer)?;
        let rel = pos - output_geo.loc.to_f64();
        let local = rel - layer_geo.loc.to_f64();
        // Prefer a concrete subsurface under the pointer; fall back to the root
        // so transparent dim still owns the hit.
        if let Some((surface, loc)) =
            layer.surface_under(local, smithay::desktop::WindowSurfaceType::ALL)
        {
            return Some((surface, (loc + layer_geo.loc + output_geo.loc).to_f64()));
        }
        Some((layer.wl_surface().clone(), pos))
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
