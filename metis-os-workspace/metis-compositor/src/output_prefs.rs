//! Apply `outputs.json` per-output preferences (scale + enable/disable).

use std::time::{Duration, Instant};

use metis_config::{load_outputs_config, output_prefs, OutputsConfig};
use smithay::output::Output;
use smithay::output::Scale;
use smithay::utils::{Logical, Point, Rectangle, Size};

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
        let cfg = metis_config::load_outputs_config_with_fallback(&self.cached);
        tracing::debug!("reloading outputs.json");
        self.cached = cfg.clone();
        cfg
    }

    pub fn cached(&self) -> &OutputsConfig {
        &self.cached
    }

    /// Throttled re-read of `outputs.json` (~1s), mirroring `input.json`.
    pub fn maybe_refresh(&mut self) -> Option<(OutputsConfig, OutputsConfig)> {
        if self.last_check.elapsed() < Duration::from_secs(1) {
            return None;
        }
        self.last_check = Instant::now();
        let before = self.cached.clone();
        let cfg = metis_config::load_outputs_config_with_fallback(&self.cached);
        if cfg == self.cached {
            return None;
        }
        tracing::info!("outputs.json changed — reapplying output preferences");
        self.cached = cfg.clone();
        Some((before, cfg))
    }
}

/// True when the only differences between two configs are night-light fields.
pub fn is_night_light_only_change(before: &OutputsConfig, after: &OutputsConfig) -> bool {
    let night_changed = before.night_light_enabled != after.night_light_enabled
        || before.night_light_temperature != after.night_light_temperature
        || before.night_light_schedule != after.night_light_schedule
        || before.night_light_schedule_12h != after.night_light_schedule_12h;
    if !night_changed {
        return false;
    }
    let mut normalized = after.clone();
    normalized.night_light_enabled = before.night_light_enabled;
    normalized.night_light_temperature = before.night_light_temperature;
    normalized.night_light_schedule = before.night_light_schedule.clone();
    normalized.night_light_schedule_12h = before.night_light_schedule_12h;
    normalized == *before
}

/// Apply night-light field changes already loaded into `output_runtime.cached`.
pub fn refresh_night_light(state: &mut MetisState, before: &OutputsConfig) {
    let cfg = state.output_runtime.cached().clone();
    sync_night_light_schedule_state(state, &cfg);
    let vis_before = metis_config::night_light_effective(before);
    let vis_after = metis_config::night_light_effective(&cfg);
    if vis_before != vis_after || before.night_light_temperature != cfg.night_light_temperature {
        state.night_light_commit.increment();
    }
    if !vis_after {
        state.night_light_schedule_effective = None;
    }
    state.schedule_redraw();
}

fn sync_night_light_schedule_state(state: &mut MetisState, cfg: &OutputsConfig) {
    if cfg.night_light_schedule.enabled {
        state.night_light_schedule_effective =
            Some(metis_config::night_light_effective(cfg));
    } else {
        state.night_light_schedule_effective = None;
    }
}

/// Apply saved preferences to every connected output. Returns true when any
/// output scale or enabled state changed.
pub fn apply_outputs(state: &mut MetisState, cfg: &OutputsConfig) -> bool {
    let outputs: Vec<Output> = state.connected_outputs();
    let mut changed = false;
    let mut enable_changed = false;
    for output in &outputs {
        let name = output.name();
        let prefs = output_prefs(cfg, &name);
        if prefs.enabled != state.is_output_enabled(&name) {
            if state.set_output_enabled(&name, prefs.enabled) {
                enable_changed = true;
                changed = true;
            }
        }
    }
    if enable_changed {
        state.retile_after_output_prefs();
    }
    if state.mirror_mode_active() {
        if crate::mirror::apply_mirror_layout(state, cfg) {
            changed = true;
        }
    } else if apply_output_layout(state, cfg) {
        changed = true;
    }
    if crate::output_modes::apply_output_modes(state, cfg) {
        changed = true;
    }
    if crate::output_vrr::apply_output_vrrs(state, cfg) {
        changed = true;
    }
    crate::color_management::apply_color_profiles(state, cfg);
    // Upload each output's ICC vcgt calibration to its CRTC gamma ramp.
    crate::output_gamma::apply_output_gamma(state);
    if state.mirror_mode_active() {
        if crate::mirror::apply_mirror_scales(state, cfg) {
            changed = true;
        }
    } else {
        for output in state.connected_outputs() {
            if !state.is_output_enabled(&output.name()) {
                continue;
            }
            if apply_output_scale(&output, cfg) {
                changed = true;
            }
        }
    }
    if changed {
        post_output_geometry_change(state);
    }
    sync_night_light_schedule_state(state, cfg);
    state.night_light_commit.increment();
    state.schedule_redraw();
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
    state.clear_mirror_batch_cache();
    state.decorations.invalidate_all();
    state.reflow_for_bar_geometry_change();
    let (full, regions) = state.wallpaper_layout();
    state.wallpaper.set_layout(full, regions);
    state.wallpaper.start_async_decode();
}

fn clamp_user_scale(raw: f64) -> f64 {
    raw.clamp(1.0, 4.0)
}

/// Reposition outputs from saved `layout_x` / `layout_y` in `outputs.json`.
/// With a single active display the desktop always stays at the origin — saved
/// layout offsets are only meaningful when two or more outputs are enabled.
pub fn apply_output_layout(state: &mut MetisState, cfg: &OutputsConfig) -> bool {
    let active: Vec<Output> = state
        .connected_outputs()
        .into_iter()
        .filter(|o| state.is_output_enabled(&o.name()))
        .collect();

    if active.len() < 2 {
        let mut changed = false;
        let origin = Point::from((0i32, 0i32));
        for output in active {
            let current = state.space.output_geometry(&output).map(|g| g.loc);
            if current == Some(origin) {
                continue;
            }
            output.change_current_state(None, None, None, Some(origin));
            state.space.map_output(&output, origin);
            tracing::info!(name = %output.name(), "reset single output to origin");
            changed = true;
        }
        return changed;
    }

    let mut changed = false;
    for output in active {
        let prefs = output_prefs(cfg, &output.name());
        let Some(x) = prefs.layout_x else { continue };
        let Some(y) = prefs.layout_y else { continue };
        let pos = Point::from((x, y));
        let current = state.space.output_geometry(&output).map(|g| g.loc);
        if current == Some(pos) {
            continue;
        }
        output.change_current_state(None, None, None, Some(pos));
        state.space.map_output(&output, pos);
        tracing::info!(name = %output.name(), ?pos, "applied output layout position");
        changed = true;
    }
    changed
}

/// Default left-to-right placement for a newly connected output.
pub fn auto_output_position(state: &MetisState) -> Point<i32, Logical> {
    let x: i32 = state
        .connected_outputs()
        .iter()
        .filter_map(|o| state.space.output_geometry(o))
        .map(|g| g.loc.x + g.size.w)
        .max()
        .unwrap_or(0);
    Point::from((x, 0))
}

pub fn output_position_for_connect(
    state: &MetisState,
    cfg: &OutputsConfig,
    name: &str,
) -> Point<i32, Logical> {
    let prefs = output_prefs(cfg, name);
    if let (Some(x), Some(y)) = (prefs.layout_x, prefs.layout_y) {
        Point::from((x, y))
    } else {
        auto_output_position(state)
    }
}

pub(crate) fn output_geometry(state: &MetisState, output: &Output) -> Option<Rectangle<i32, Logical>> {
    state.space.output_geometry(output).or_else(|| {
        output.current_mode().map(|mode| {
            Rectangle::new(Point::from((0, 0)), Size::from((mode.size.w, mode.size.h)))
        })
    })
}

pub fn output_info_for(
    state: &MetisState,
    output: &Output,
    primary: Option<&str>,
    mirror_source: Option<&str>,
) -> metis_protocol::OutputInfo {
    let name = output.name();
    let geo = output_geometry(state, output);
    let rect = geo
        .map(|g| metis_protocol::MonitorRect {
            x: g.loc.x,
            y: g.loc.y,
            width: g.size.w,
            height: g.size.h,
        })
        .unwrap_or(metis_protocol::MonitorRect {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        });
    let cfg = &state.output_runtime.cached;
    let prefs = output_prefs(cfg, &name);
    let phys = output.physical_properties();
    let active = state.is_output_enabled(&name);
    let mirror_active = state.mirror_mode_active();
    let is_mirror_source = mirror_active && mirror_source.is_some_and(|s| s == name);
    let is_mirrored = mirror_active
        && !is_mirror_source
        && active
        && prefs.enabled;
    let vrr_support = crate::output_vrr::query_vrr_support(state, &name);
    let vrr_active = crate::output_vrr::query_vrr_active(state, &name);
    metis_protocol::OutputInfo {
        name,
        primary: primary.is_some_and(|p| p == output.name()),
        rect,
        scale: output.current_scale().fractional_scale(),
        enabled: active && prefs.enabled,
        make: phys.make,
        model: phys.model,
        mirrored: is_mirrored,
        mirror_source: is_mirror_source,
        vrr_available: crate::output_vrr::vrr_available(vrr_support),
        vrr_active,
    }
}
