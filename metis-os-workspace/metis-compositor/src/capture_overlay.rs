use std::collections::HashSet;

use metis_protocol::PixelRect;
use smithay::utils::{Logical, Point};

use crate::state::MetisState;

/// Fraction of the virtual desktop a floating window must cover to be treated as
/// an interactive capture overlay (screenshot region picker, color sampler, …).
const DESKTOP_COVERAGE_RATIO: f64 = 0.85;

/// Pixel slack when comparing a window rect to [`MetisState::desktop_bounds`].
const DESKTOP_SPAN_MARGIN: i32 = 12;

#[derive(Debug, Default)]
pub(crate) struct CaptureOverlaySession {
    /// Windows elevated above ordinary clients for the duration of capture.
    pub windows: HashSet<u32>,
    /// App ids signalled by the xdg-desktop-portal before capture UI spawns.
    pub portal_app_ids: HashSet<String>,
}

impl CaptureOverlaySession {
    pub fn active(&self) -> bool {
        !self.windows.is_empty()
    }
}

fn norm_app_id(app_id: &str) -> String {
    app_id.trim().to_ascii_lowercase()
}

impl MetisState {
    pub(crate) fn capture_overlay_active(&self) -> bool {
        self.capture_overlay.active()
    }

    pub(crate) fn begin_capture_overlay_portal(&mut self, app_id: Option<String>) {
        if let Some(id) = app_id {
            self.capture_overlay
                .portal_app_ids
                .insert(norm_app_id(&id));
        }
        self.register_portal_capture_windows();
    }

    pub(crate) fn end_capture_overlay_portal(&mut self, app_id: Option<String>) {
        if let Some(id) = app_id {
            self.capture_overlay.portal_app_ids.remove(&norm_app_id(&id));
        } else {
            self.capture_overlay.portal_app_ids.clear();
        }
    }

    fn portal_wants_app(&self, app_id: Option<&str>) -> bool {
        let Some(app_id) = app_id else {
            return false;
        };
        self.capture_overlay
            .portal_app_ids
            .contains(&norm_app_id(app_id))
    }

    fn register_portal_capture_windows(&mut self) {
        let ids: Vec<u32> = self.windows.ids();
        for id in ids {
            if self.portal_wants_app(self.windows.get(id).and_then(|r| r.app_id.as_deref())) {
                let _ = self.maybe_register_capture_overlay(id);
            }
        }
    }

    pub(crate) fn unregister_capture_overlay(&mut self, id: u32) {
        self.capture_overlay.windows.remove(&id);
    }

    fn window_spans_desktop(&self, id: u32) -> bool {
        let bounds = self.desktop_bounds();
        let Some(rect) = self
            .windows
            .target_rect(id)
            .or_else(|| self.window_body_rect(id))
        else {
            return false;
        };
        rect.x <= bounds.loc.x + DESKTOP_SPAN_MARGIN
            && rect.y <= bounds.loc.y + DESKTOP_SPAN_MARGIN
            && rect.x + rect.width >= bounds.loc.x + bounds.size.w - DESKTOP_SPAN_MARGIN
            && rect.y + rect.height >= bounds.loc.y + bounds.size.h - DESKTOP_SPAN_MARGIN
    }

    fn window_covers_desktop(&self, id: u32) -> bool {
        let bounds = self.desktop_bounds();
        let area = (bounds.size.w as i64) * (bounds.size.h as i64);
        if area <= 0 {
            return false;
        }
        let Some(rect) = self
            .windows
            .target_rect(id)
            .or_else(|| self.window_body_rect(id))
        else {
            return false;
        };
        let covered = (rect.width as i64) * (rect.height as i64);
        covered as f64 / area as f64 >= DESKTOP_COVERAGE_RATIO
    }

    fn is_capture_overlay_candidate(&self, id: u32) -> bool {
        let Some(record) = self.windows.get(id) else {
            return false;
        };
        if record.fullscreen || record.maximized || self.windows.is_minimized(id) {
            return false;
        }
        if self.tile_id_for_window(id).is_some() && !self.floating.contains(&id) {
            return false;
        }
        // Never treat ordinary browsers as capture overlays — they often span the
        // monitor while maximized before our state catches up.
        if record
            .app_id
            .as_deref()
            .is_some_and(|app_id| crate::decoration_policy::id_looks_chromium_family(app_id))
        {
            return false;
        }

        if self.portal_wants_app(record.app_id.as_deref()) {
            return true;
        }

        // Floating near-fullscreen overlays (screenshot pickers). Must be floating
        // so maximized browsers are never mistaken during startup.
        if !self.floating.contains(&id) || !self.window_covers_desktop(id) {
            return false;
        }

        true
    }

    /// Detect portal or geometry-based capture overlays, elevate them, and span
    /// the desktop when the portal signalled the requesting app.
    pub(crate) fn maybe_register_capture_overlay(&mut self, id: u32) -> bool {
        if !self.is_capture_overlay_candidate(id) {
            return false;
        }

        let portal_app = self.portal_wants_app(
            self.windows
                .get(id)
                .and_then(|r| r.app_id.as_deref()),
        );
        let needs_prepare = portal_app || !self.window_spans_desktop(id);
        if needs_prepare {
            self.prepare_capture_overlay_window(id);
        } else {
            self.remove_app_tile_everywhere(id);
            self.floating.insert(id);
            self.windows.set_placement_chosen(id, true);
            self.clear_auto_hide(id);
        }

        self.capture_overlay.windows.insert(id);
        self.enforce_capture_overlay_stacking();
        self.focus_window_id(id);
        tracing::info!(
            id,
            portal_app,
            "capture overlay registered"
        );
        true
    }

    /// Span the virtual desktop, raise above clients, and focus for capture.
    fn prepare_capture_overlay_window(&mut self, id: u32) {
        use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
        use smithay::utils::{Logical, Point, Size};

        self.remove_app_tile_everywhere(id);
        self.floating.insert(id);
        self.windows.set_placement_chosen(id, true);
        self.clear_auto_hide(id);
        self.windows.set_maximized(id, false);
        self.windows.set_fullscreen(id, false);

        let bounds = self.desktop_bounds();
        let rect = PixelRect {
            x: bounds.loc.x,
            y: bounds.loc.y,
            width: bounds.size.w.max(1),
            height: bounds.size.h.max(1),
        };
        self.windows.set_target_rect(id, rect);

        let Some(record) = self.windows.get(id).cloned() else {
            return;
        };

        let loc = Point::<i32, Logical>::from((rect.x, rect.y));
        let size = Size::<i32, Logical>::from((rect.width, rect.height));
        record.toplevel.with_pending_state(|state| {
            state.states.unset(xdg_toplevel::State::Maximized);
            state.states.unset(xdg_toplevel::State::Fullscreen);
            state.states.unset(xdg_toplevel::State::TiledLeft);
            state.states.unset(xdg_toplevel::State::TiledRight);
            state.states.unset(xdg_toplevel::State::TiledTop);
            state.states.unset(xdg_toplevel::State::TiledBottom);
            state.size = Some(size);
            state.fullscreen_output = None;
        });

        let mapped = self
            .space
            .elements()
            .any(|w| self.windows.id_for_window(w) == Some(id));
        if mapped {
            self.space.relocate_element(&record.window, loc);
        } else {
            self.space.map_element(record.window.clone(), loc, true);
        }
        record.toplevel.send_pending_configure();
        self.focus_window_id(id);
        self.schedule_redraw();
    }

    pub(crate) fn enforce_capture_overlay_stacking(&mut self) {
        if self.capture_overlay.windows.is_empty() {
            return;
        }
        for id in self.capture_overlay.windows.iter().copied().collect::<Vec<_>>() {
            let Some(record) = self.windows.get(id).cloned() else {
                continue;
            };
            self.space.raise_element(&record.window, false);
        }
        self.schedule_redraw();
    }

    pub(crate) fn window_is_capture_overlay(&self, id: u32) -> bool {
        self.capture_overlay.windows.contains(&id)
    }

    /// Topmost mapped capture-overlay window in the desktop stack.
    pub(crate) fn top_capture_overlay_window(
        &self,
    ) -> Option<smithay::desktop::Window> {
        self.space
            .elements()
            .rev()
            .find(|window| {
                self.windows
                    .id_for_window(window)
                    .is_some_and(|id| self.window_is_capture_overlay(id))
            })
            .cloned()
    }

    /// While capture is active, deliver pointer/focus to the overlay regardless of
    /// whether the hit lands on a transparent or unmapped sub-region.
    pub(crate) fn capture_overlay_surface_at(
        &self,
        pos: Point<f64, Logical>,
    ) -> Option<(
        smithay::reexports::wayland_server::protocol::wl_surface::WlSurface,
        Point<f64, Logical>,
    )> {
        if !self.desktop_bounds().contains(pos.to_i32_round()) {
            return None;
        }
        let window = self.top_capture_overlay_window()?;
        self.window_surface_for(pos, &window)
    }

    pub(crate) fn sync_toplevel_fullscreen_from_client(&mut self, id: u32) {
        let Some(record) = self.windows.get(id).cloned() else {
            return;
        };
        if !record.fullscreen {
            return;
        }
        use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;

        // Do not infer exit while a fullscreen configure is still in flight —
        // committed state lags until the client acks our configure.
        let pending_fullscreen = record.toplevel.with_pending_state(|state| {
            state.states.contains(xdg_toplevel::State::Fullscreen)
        });
        if pending_fullscreen {
            return;
        }

        let client_fullscreen = record.toplevel.with_committed_state(|state| {
            state
                .map(|s| s.states.contains(xdg_toplevel::State::Fullscreen))
                .unwrap_or(false)
        });
        if client_fullscreen {
            return;
        }

        // Only act after the client has acked at least one configure; otherwise
        // `last_acked` is empty on first commits and we would spuriously exit.
        let has_acked = record.toplevel.with_committed_state(|state| state.is_some());
        if !has_acked {
            return;
        }

        tracing::info!(id, "client dropped fullscreen without unfullscreen_request");
        self.set_fullscreen(id, false, None);
    }
}
