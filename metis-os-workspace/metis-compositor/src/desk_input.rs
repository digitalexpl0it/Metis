use metis_grid::{app_tile_body_rect, cell_to_pixels, PixelRect, TileKind};
use smithay::{
    backend::renderer::utils::with_renderer_surface_state,
    desktop::{layer_map_for_output, PopupManager, WindowSurfaceType},
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Point, Rectangle},
    wayland::shell::wlr_layer::Layer as WlrLayer,
};

use crate::focus::KeyboardFocusTarget;
use crate::state::MetisState;

/// True when the surface currently has a committed buffer.
///
/// NOTE: must read the *renderer* surface state, not `SurfaceAttributes.buffer`.
/// `on_commit_buffer_handler` consumes the attribute buffer on every commit, so
/// `SurfaceAttributes.current().buffer` is `None` except on the exact frame a new
/// buffer was attached — which made every bar click miss.
fn surface_has_buffer(surface: &WlSurface) -> bool {
    with_renderer_surface_state(surface, |state| state.buffer().is_some()).unwrap_or(false)
}

fn metis_bar_region_contains(
    layer: &smithay::desktop::LayerSurface,
    layers: &smithay::desktop::LayerMap,
    rel: Point<f64, Logical>,
) -> bool {
    if !surface_has_buffer(layer.wl_surface()) {
        return false;
    }
    let Some(layer_geo) = layers.layer_geometry(layer) else {
        return false;
    };
    let local = rel - layer_geo.loc.to_f64();

    if layer.bbox().to_f64().contains(local) {
        return true;
    }

    // Popovers: use client geometry, not `bbox_with_popups()`. The surface-tree
    // bbox can balloon to the full output and swallow desktop clicks — which
    // prevented "click outside to dismiss" from ever firing.
    let root = layer.wl_surface();
    for (popup, location) in PopupManager::popups_for_surface(root) {
        let geo = popup.geometry();
        if geo.size.w <= 0 || geo.size.h <= 0 {
            continue;
        }
        let origin = Point::from(location) + geo.loc;
        if Rectangle::new(origin, geo.size).to_f64().contains(local) {
            return true;
        }
    }
    false
}

fn layer_accepts_pointer(
    layer: &smithay::desktop::LayerSurface,
    layers: &smithay::desktop::LayerMap,
    rel: Point<f64, Logical>,
) -> bool {
    if layer.namespace().starts_with("metis-bar") {
        return metis_bar_region_contains(layer, layers, rel);
    }
    if !surface_has_buffer(layer.wl_surface()) {
        return false;
    }
    let layer_loc = layers.layer_geometry(layer).unwrap().loc;
    layer
        .surface_under(rel - layer_loc.to_f64(), WindowSurfaceType::ALL)
        .is_some()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeskHit {
    WidgetTile { tile_id: String },
    AppHeader { tile_id: String, window_id: u32 },
    AppBody { window_id: u32 },
    Gutter,
    Empty,
}

pub fn point_in_rect(x: i32, y: i32, rect: PixelRect) -> bool {
    x >= rect.x && x < rect.x + rect.width && y >= rect.y && y < rect.y + rect.height
}

impl MetisState {
    pub fn classify_hit(&self, x: i32, y: i32) -> DeskHit {
        let (metrics, key) = match self.output_at(Point::from((x, y))) {
            Some(o) => (self.grid_metrics_for(&o), o.name()),
            None => (self.grid_metrics(), self.primary_key()),
        };
        let Some(desk) = self.desk(&key) else {
            return DeskHit::Empty;
        };

        for tile in &desk.layout.tiles {
            let full = cell_to_pixels(&metrics, &tile.rect);
            if !point_in_rect(x, y, full) {
                continue;
            }
            match &tile.kind {
                TileKind::Widget { .. } => {
                    return DeskHit::WidgetTile {
                        tile_id: tile.id.clone(),
                    };
                }
                TileKind::App {
                    window_id: Some(wid),
                    ..
                } => {
                    if point_in_rect(x, y, app_tile_body_rect(full)) {
                        return DeskHit::AppBody { window_id: *wid };
                    }
                    return DeskHit::AppHeader {
                        tile_id: tile.id.clone(),
                        window_id: *wid,
                    };
                }
                TileKind::App { window_id: None, .. } => {
                    return DeskHit::AppHeader {
                        tile_id: tile.id.clone(),
                        window_id: 0,
                    };
                }
            }
        }

        if self.is_gutter_hit(x, y) {
            return DeskHit::Gutter;
        }

        DeskHit::Empty
    }

    fn is_gutter_hit(&self, x: i32, y: i32) -> bool {
        let metrics = match self.output_at(Point::from((x, y))) {
            Some(o) => self.grid_metrics_for(&o),
            None => self.grid_metrics(),
        };
        let gutter = metrics.gutter as i32;
        if gutter <= 0 {
            return false;
        }
        let cols = metrics.columns.max(1) as i32;
        let rows = metrics.rows.max(1) as i32;
        let usable_w = metrics.monitor.width - gutter * (cols + 1);
        let usable_h = metrics.monitor.height - gutter * (rows + 1);
        let cell_w = usable_w / cols;
        let cell_h = usable_h / rows;

        for col in 1..cols {
            let gx = metrics.monitor.x + gutter + col * (cell_w + gutter) - gutter / 2;
            if (x - gx).abs() <= gutter {
                return true;
            }
        }
        for row in 1..rows {
            let gy = metrics.monitor.y + gutter + row * (cell_h + gutter) - gutter / 2;
            if (y - gy).abs() <= gutter {
                return true;
            }
        }
        false
    }

    /// True when the monitor-space point is inside an app tile body (below the header chrome).
    pub fn app_body_hit(&self, x: i32, y: i32, window_id: u32) -> bool {
        matches!(self.classify_hit(x, y), DeskHit::AppBody { window_id: wid } if wid == window_id)
    }

    pub fn app_tile_body_rect(&self, window_id: u32) -> Option<PixelRect> {
        let key = self.desk_key_for_window(window_id);
        let metrics = match self.output_by_name(&key) {
            Some(o) => self.grid_metrics_for(&o),
            None => self.grid_metrics(),
        };
        let desk = self.desk(&key)?;
        for tile in &desk.layout.tiles {
            let TileKind::App {
                window_id: Some(wid),
                ..
            } = &tile.kind
            else {
                continue;
            };
            if *wid != window_id {
                continue;
            }
            let full = cell_to_pixels(&metrics, &tile.rect);
            return Some(app_tile_body_rect(full));
        }
        None
    }

    pub fn window_id_at(&self, pos: Point<f64, Logical>) -> Option<u32> {
        self.space
            .element_under(pos)
            .and_then(|(window, _)| self.windows.id_for_window(&window))
    }

    pub fn window_surface_at(
        &self,
        pos: Point<f64, Logical>,
    ) -> Option<(WlSurface, Point<f64, Logical>)> {
        let (window, location) = self.space.element_under(pos)?;
        window
            .surface_under(pos - location.to_f64(), WindowSurfaceType::ALL)
            .map(|(surface, loc)| (surface, (loc + location).to_f64()))
    }

    fn layer_surface_at(
        &self,
        layer: &smithay::desktop::LayerSurface,
        layers: &smithay::desktop::LayerMap,
        rel: Point<f64, Logical>,
        output_geo: smithay::utils::Rectangle<i32, Logical>,
    ) -> Option<(WlSurface, Point<f64, Logical>)> {
        if layer.namespace().starts_with("metis-bar") {
            return metis_bar_layer_surface_at(layer, layers, rel, output_geo);
        }
        let layer_loc = layers.layer_geometry(layer).unwrap().loc;
        layer
            .surface_under(rel - layer_loc.to_f64(), WindowSurfaceType::ALL)
            .map(|(surface, loc)| (surface, (loc + layer_loc + output_geo.loc).to_f64()))
    }

    /// True when a pointer press should dismiss open bar popovers.
    ///
    /// Any press that does not land on the bar chrome or its popover (i.e. bare
    /// desktop OR an app window) dismisses. Bar-local clicks are handled by the
    /// shell itself (GTK), which avoids a race with the poll-based dismiss signal.
    pub fn should_dismiss_bar_popovers(&self, pos: Point<f64, Logical>) -> bool {
        !self.metis_bar_ui_hit(pos)
    }

    /// Bar chrome or an attached dropdown panel below the bar strip.
    pub(crate) fn metis_bar_ui_hit(&self, pos: Point<f64, Logical>) -> bool {
        let Some(output) = self.space.outputs().find(|o| {
            self.space
                .output_geometry(o)
                .is_some_and(|geo| geo.contains(pos.to_i32_round()))
        }) else {
            return false;
        };
        let output_geo = self.space.output_geometry(output).unwrap();
        let rel = pos - output_geo.loc.to_f64();
        let layers = layer_map_for_output(output);

        for layer in layers
            .layers()
            .filter(|layer| layer.namespace().starts_with("metis-bar"))
        {
            if layer_accepts_pointer(layer, &layers, rel) {
                return true;
            }
        }
        false
    }

    /// Route pointer hits: app bodies pass through, then layer-shell UI, then windows.
    pub fn surface_under(&self, pos: Point<f64, Logical>) -> Option<(WlSurface, Point<f64, Logical>)> {
        let output = self.space.outputs().find(|o| {
            self.space
                .output_geometry(o)
                .is_some_and(|geo| geo.contains(pos.to_i32_round()))
        })?;
        let output_geo = self.space.output_geometry(output).unwrap();
        let rel = pos - output_geo.loc.to_f64();
        let layers = layer_map_for_output(output);
        let (x, y) = (pos.x as i32, pos.y as i32);

        // The bar strip and its popovers (e.g. the start menu) take priority over
        // everything below — including the desk-grid app-body passthrough. Without
        // this, an app window stacked behind an open popover would win pointer focus,
        // so scroll/clicks over the menu went to the window instead of the menu.
        // (When the window was moved away the passthrough missed and it "worked".)
        //
        // NOTE: must NOT call `self.metis_bar_ui_hit()` here — it re-locks this
        // output's layer map, which we already hold via `layers`, deadlocking the
        // compositor thread. Use the held guard directly.
        for layer in layers
            .layers()
            .filter(|layer| layer.namespace().starts_with("metis-bar"))
        {
            if !surface_has_buffer(layer.wl_surface()) {
                continue;
            }
            if !metis_bar_region_contains(layer, &layers, rel) {
                continue;
            }
            if let Some(hit) = self.layer_surface_at(layer, &layers, rel, output_geo) {
                return Some(hit);
            }
            // Over a bar region (strip or popover) with no concrete input surface —
            // a transparent gutter. Block fallthrough so nothing below grabs focus.
            return None;
        }

        if let DeskHit::AppBody { window_id } = self.classify_hit(x, y) {
            if self.window_id_at(pos) == Some(window_id) {
                if let Some(hit) = self.window_surface_at(pos) {
                    return Some(hit);
                }
            }
        }

        for layer_kind in [WlrLayer::Overlay, WlrLayer::Top] {
            if let Some(layer) = layers.layer_under(layer_kind, rel) {
                if !surface_has_buffer(layer.wl_surface()) {
                    continue;
                }
                if layer.namespace().starts_with("metis-bar") {
                    // Handled by the priority pass above.
                    continue;
                }
                if let Some(hit) = self.layer_surface_at(layer, &layers, rel, output_geo) {
                    return Some(hit);
                }
            }
        }

        if let Some(hit) = self.window_surface_at(pos) {
            return Some(hit);
        }

        for layer_kind in [WlrLayer::Bottom, WlrLayer::Background] {
            if let Some(layer) = layers.layer_under(layer_kind, rel) {
                if !surface_has_buffer(layer.wl_surface()) {
                    continue;
                }
                if let Some(hit) = self.layer_surface_at(layer, &layers, rel, output_geo) {
                    return Some(hit);
                }
            }
        }

        None
    }

    /// Keyboard focus follows the same desk/app passthrough rules as pointer routing.
    pub fn focus_target_at(&self, location: Point<f64, Logical>) -> Option<KeyboardFocusTarget> {
        let output = self
            .space
            .outputs()
            .find(|o| {
                self.space
                    .output_geometry(o)
                    .is_some_and(|geo| geo.contains(location.to_i32_round()))
            })
            .cloned()?;

        let output_geo = self.space.output_geometry(&output).unwrap();
        let rel = location - output_geo.loc.to_f64();
        let layers = layer_map_for_output(&output);
        let (x, y) = (location.x as i32, location.y as i32);

        for layer_kind in [WlrLayer::Overlay] {
            if let Some(layer) = layers.layer_under(layer_kind, rel) {
                if layer.can_receive_keyboard_focus() {
                    let layer_loc = layers.layer_geometry(layer).unwrap().loc;
                    if layer
                        .surface_under(rel - layer_loc.to_f64(), WindowSurfaceType::ALL)
                        .is_some()
                    {
                        return Some(layer.clone().into());
                    }
                }
            }
        }

        // NOTE: must NOT call `self.metis_bar_ui_hit()` here — it re-locks this
        // output's layer map, which we already hold via `layers`, deadlocking the
        // compositor thread on click. Inline the check against the held guard.
        for layer in layers
            .layers()
            .filter(|layer| layer.namespace().starts_with("metis-bar"))
        {
            if layer_accepts_pointer(layer, &layers, rel) {
                return Some(layer.clone().into());
            }
        }

        match self.classify_hit(x, y) {
            DeskHit::AppBody { window_id } => {
                if self.window_id_at(location) == Some(window_id) {
                    if let Some((window, _)) = self.space.element_under(location) {
                        return Some(window.clone().into());
                    }
                }
            }
            DeskHit::WidgetTile { .. } | DeskHit::AppHeader { .. } => {
                for layer_kind in [WlrLayer::Top] {
                    if let Some(layer) = layers.layer_under(layer_kind, rel) {
                        if layer.can_receive_keyboard_focus() {
                            let layer_loc = layers.layer_geometry(layer).unwrap().loc;
                            if layer
                                .surface_under(rel - layer_loc.to_f64(), WindowSurfaceType::ALL)
                                .is_some()
                            {
                                return Some(layer.clone().into());
                            }
                        }
                    }
                }
            }
            DeskHit::Gutter | DeskHit::Empty => {}
        }

        if let Some((window, _)) = self.space.element_under(location) {
            return Some(window.clone().into());
        }

        for layer_kind in [WlrLayer::Bottom, WlrLayer::Background] {
            if let Some(layer) = layers.layer_under(layer_kind, rel) {
                if layer.can_receive_keyboard_focus() {
                    let layer_loc = layers.layer_geometry(layer).unwrap().loc;
                    if layer
                        .surface_under(rel - layer_loc.to_f64(), WindowSurfaceType::ALL)
                        .is_some()
                    {
                        return Some(layer.clone().into());
                    }
                }
            }
        }

        None
    }
}

/// Resolve a pointer target on the bar layer, including transparent popover gutters.
fn metis_bar_layer_surface_at(
    layer: &smithay::desktop::LayerSurface,
    layers: &smithay::desktop::LayerMap,
    rel: Point<f64, Logical>,
    output_geo: smithay::utils::Rectangle<i32, Logical>,
) -> Option<(WlSurface, Point<f64, Logical>)> {
    let layer_loc = layers.layer_geometry(layer)?.loc;
    let local = rel - layer_loc.to_f64();

    if let Some((surface, loc)) = layer.surface_under(local, WindowSurfaceType::ALL) {
        return Some((surface, (loc + layer_loc + output_geo.loc).to_f64()));
    }

    if !metis_bar_region_contains(layer, layers, rel) {
        return None;
    }

    // Transparent popover gutters have no input region — deliver to the popup whose
    // geometry contains the point, using its global origin (smithay's focus tuple
    // expects the surface origin, not the cursor position).
    let root = layer.wl_surface();
    for (popup, location) in PopupManager::popups_for_surface(root) {
        let geo = popup.geometry();
        if geo.size.w <= 0 || geo.size.h <= 0 {
            continue;
        }
        let origin = Point::from(location) + geo.loc;
        if Rectangle::new(origin, geo.size).to_f64().contains(local) {
            let popup_origin_global = (Point::from(location) + layer_loc + output_geo.loc).to_f64();
            return Some((popup.wl_surface().clone(), popup_origin_global));
        }
    }

    None
}
