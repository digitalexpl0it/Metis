//! Apply `outputs.json` per-output preferences (fractional scale today).

use std::time::{Duration, Instant};

use metis_config::{load_outputs_config, output_prefs, OutputsConfig};
use smithay::output::Output;
use smithay::output::Scale;

use crate::state::MetisState;

pub struct OutputRuntime {
    last_check: Instant,
    cached: OutputsConfig,
}

impl OutputRuntime {
    pub fn new() -> Self {
        Self {
            last_check: Instant::now(),
            cached: load_outputs_config(),
        }
    }

    pub fn reload_from_disk(&mut self) -> OutputsConfig {
        let cfg = load_outputs_config();
        tracing::info!("reloading outputs.json");
        self.cached = cfg.clone();
        cfg
    }

    pub fn cached(&self) -> &OutputsConfig {
        &self.cached
    }

    /// Throttled re-read of `outputs.json` (~1s), mirroring `input.json`.
    pub fn maybe_refresh(&mut self) -> Option<OutputsConfig> {
        if self.last_check.elapsed() < Duration::from_secs(1) {
            return None;
        }
        self.last_check = Instant::now();
        let cfg = load_outputs_config();
        if cfg == self.cached {
            return None;
        }
        tracing::info!("outputs.json changed — reapplying output preferences");
        self.cached = cfg.clone();
        Some(cfg)
    }
}

/// Apply saved preferences to every client-visible output. Returns true when any
/// output scale changed.
pub fn apply_outputs(state: &mut MetisState, cfg: &OutputsConfig) -> bool {
    let outputs: Vec<Output> = state
        .space
        .outputs()
        .filter(|o| o.name() != "metis-render")
        .cloned()
        .collect();
    let mut changed = false;
    for output in outputs {
        if apply_output_scale(&output, cfg) {
            changed = true;
        }
        let prefs = output_prefs(cfg, &output.name());
        if !prefs.enabled {
            tracing::debug!(
                name = %output.name(),
                "output marked disabled in outputs.json (disable/unmap not wired yet)"
            );
        }
    }
    if changed {
        post_output_geometry_change(state);
    }
    changed
}

fn apply_output_scale(output: &Output, cfg: &OutputsConfig) -> bool {
    let prefs = output_prefs(cfg, &output.name());
    let current = output.current_scale().fractional_scale();
    let next = clamp_user_scale(prefs.scale);
    if (current - next).abs() <= 0.001 {
        return false;
    }
    output.change_current_state(None, None, Some(Scale::Fractional(next)), None);
    tracing::info!(name = %output.name(), scale = next, "applied output scale");
    true
}

fn post_output_geometry_change(state: &mut MetisState) {
    state.decorations.invalidate_all();
    state.reflow_for_bar_geometry_change();
    let (full, regions) = state.wallpaper_layout();
    state.wallpaper.set_layout(full, regions);
    state.wallpaper.start_async_decode();
}

fn clamp_user_scale(raw: f64) -> f64 {
    raw.clamp(1.0, 4.0)
}

pub fn output_info_for(state: &MetisState, output: &Output, primary: Option<&str>) -> metis_protocol::OutputInfo {
    let name = output.name();
    let geo = state.space.output_geometry(output);
    let rect = geo.map(|g| metis_protocol::MonitorRect {
        x: g.loc.x,
        y: g.loc.y,
        width: g.size.w,
        height: g.size.h,
    }).unwrap_or(metis_protocol::MonitorRect {
        x: 0,
        y: 0,
        width: 0,
        height: 0,
    });
    let cfg = &state.output_runtime.cached;
    let prefs = output_prefs(cfg, &name);
    let phys = output.physical_properties();
    metis_protocol::OutputInfo {
        name,
        primary: primary.is_some_and(|p| p == output.name()),
        rect,
        scale: output.current_scale().fractional_scale(),
        enabled: prefs.enabled,
        make: phys.make,
        model: phys.model,
    }
}
