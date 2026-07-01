//! DRM video mode listing and apply (`outputs.json` mode_width/height/refresh).

use metis_config::OutputPrefs;
use metis_protocol::OutputModeInfo;
use smithay::backend::drm::output::DrmOutputRenderElements;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::output::{Mode as WlMode, Output};
use smithay::reexports::drm::control::{Mode as DrmMode, ModeTypeFlags};
use smithay::utils::Transform;

use crate::state::MetisState;
use crate::udev::SurfaceData;

pub fn wl_mode_info(mode: WlMode, preferred: bool) -> OutputModeInfo {
    OutputModeInfo {
        width: mode.size.w,
        height: mode.size.h,
        refresh_millihz: mode.refresh,
        preferred,
    }
}

pub fn drm_mode_info(mode: DrmMode, preferred: bool) -> OutputModeInfo {
    wl_mode_info(WlMode::from(mode), preferred)
}

pub fn modes_match(a: &OutputModeInfo, b: &OutputModeInfo) -> bool {
    a.width == b.width && a.height == b.height && a.refresh_millihz == b.refresh_millihz
}

pub fn pick_drm_mode_index(modes: &[DrmMode], prefs: &OutputPrefs) -> usize {
    if let (Some(w), Some(h), Some(r)) = (
        prefs.mode_width,
        prefs.mode_height,
        prefs.mode_refresh_millihz,
    ) {
        let target = OutputModeInfo {
            width: w,
            height: h,
            refresh_millihz: r,
            preferred: false,
        };
        if let Some((idx, _)) = modes
            .iter()
            .enumerate()
            .find(|(_, m)| modes_match(&drm_mode_info(**m, false), &target))
        {
            return idx;
        }
        if let Some((idx, _)) = modes.iter().enumerate().find(|(_, m)| {
            let wl = WlMode::from(**m);
            wl.size.w == w && wl.size.h == h
        }) {
            return idx;
        }
    }
    modes
        .iter()
        .position(|m| m.mode_type().contains(ModeTypeFlags::PREFERRED))
        .unwrap_or(0)
}

pub fn list_output_modes(state: &MetisState, name: &str) -> (Vec<OutputModeInfo>, Option<OutputModeInfo>) {
    if let Some(udev) = state.udev.as_ref() {
        let Some(surface) = udev.surfaces.values().find(|s| s.output.name() == name) else {
            return (Vec::new(), None);
        };
        let current = surface
            .output
            .current_mode()
            .map(|m| wl_mode_info(m, false));
        let modes = surface
            .modes
            .iter()
            .map(|m| {
                drm_mode_info(
                    *m,
                    m.mode_type().contains(ModeTypeFlags::PREFERRED),
                )
            })
            .collect();
        return (modes, current);
    }

    let Some(output) = state
        .connected_outputs()
        .into_iter()
        .find(|o| o.name() == name)
    else {
        return (Vec::new(), None);
    };
    let current = output
        .current_mode()
        .map(|m| wl_mode_info(m, true));
    let modes = current.clone().into_iter().collect::<Vec<_>>();
    (modes, current)
}

pub fn apply_output_modes(state: &mut MetisState, cfg: &metis_config::OutputsConfig) -> bool {
    let mut changed = false;
    for output in state.connected_outputs() {
        if !state.is_output_enabled(&output.name()) {
            continue;
        }
        let prefs = metis_config::output_prefs(cfg, &output.name());
        if apply_output_mode(state, &output, &prefs) {
            changed = true;
        }
    }
    changed
}

fn apply_output_mode(state: &mut MetisState, output: &Output, prefs: &OutputPrefs) -> bool {
    let Some(w) = prefs.mode_width else {
        return false;
    };
    let Some(h) = prefs.mode_height else {
        return false;
    };
    let Some(r) = prefs.mode_refresh_millihz else {
        return false;
    };
    let target = OutputModeInfo {
        width: w,
        height: h,
        refresh_millihz: r,
        preferred: false,
    };

    if let Some(current) = output.current_mode() {
        if modes_match(&wl_mode_info(current, false), &target) {
            return false;
        }
    }

    if state.udev.is_some() {
        return state.udev_apply_mode(&output.name(), target);
    }

    let wl_mode = WlMode {
        size: (w, h).into(),
        refresh: r,
    };
    output.change_current_state(Some(wl_mode), None, None, None);
    tracing::info!(name = %output.name(), ?wl_mode, "applied winit output mode (dev only)");
    true
}

impl MetisState {
    pub(crate) fn udev_apply_mode(&mut self, name: &str, target: OutputModeInfo) -> bool {
        let Some(udev) = self.udev.as_mut() else {
            return false;
        };
        let crtc = udev
            .surfaces
            .iter()
            .find(|(_, s)| s.output.name() == name && !s.user_disabled)
            .map(|(c, _)| *c);
        let Some(crtc) = crtc else {
            return false;
        };
        let Some(drm_mode) = udev
            .surfaces
            .get(&crtc)
            .and_then(|s| find_drm_mode(&s.modes, &target))
        else {
            tracing::warn!(%name, ?target, "requested mode not advertised by connector");
            return false;
        };

        let wl_mode = WlMode::from(drm_mode);
        let mut renderer = udev.renderer.take();
        let apply_result = {
            let surface = udev.surfaces.get_mut(&crtc).unwrap();
            let Some(renderer) = renderer.as_mut() else {
                tracing::error!("no renderer for mode change");
                return false;
            };
            apply_drm_mode(surface, drm_mode, renderer)
        };
        udev.renderer = renderer;

        if let Err(err) = apply_result {
            tracing::warn!(%name, ?target, ?err, "failed to apply DRM mode");
            return false;
        }

        if let Some(surface) = self.udev.as_ref().and_then(|u| u.surfaces.get(&crtc)) {
            let output = surface.output.clone();
            let pos = self
                .space
                .output_geometry(&output)
                .map(|g| g.loc)
                .unwrap_or_default();
            output.change_current_state(Some(wl_mode), Some(Transform::Normal), None, Some(pos));
        }

        tracing::info!(%name, ?target, "applied DRM output mode");
        true
    }
}

fn find_drm_mode(modes: &[DrmMode], target: &OutputModeInfo) -> Option<DrmMode> {
    modes
        .iter()
        .copied()
        .find(|m| modes_match(&drm_mode_info(*m, false), target))
        .or_else(|| {
            modes.iter().copied().find(|m| {
                let wl = WlMode::from(*m);
                wl.size.w == target.width && wl.size.h == target.height
            })
        })
}

fn apply_drm_mode(
    surface: &mut SurfaceData,
    mode: DrmMode,
    renderer: &mut GlesRenderer,
) -> Result<(), String> {
    surface
        .drm_output
        .use_mode::<GlesRenderer, crate::render::OutputStack>(
            mode,
            renderer,
            &DrmOutputRenderElements::default(),
        )
        .map_err(|err| format!("{err:?}"))
        .map(|_| {
            surface.pending = true;
        })
}
