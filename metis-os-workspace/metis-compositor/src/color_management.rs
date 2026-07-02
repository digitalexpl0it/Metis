//! Per-output ICC profile paths from `outputs.json`.
//!
//! Clients will eventually receive profiles through `wp_color_management_v1`
//! (Smithay handler still pending). Today we validate saved paths on apply so
//! Settings can persist choices ahead of the compositor colour pipeline.

use metis_config::{output_prefs, OutputsConfig};

use crate::state::MetisState;

pub fn apply_color_profiles(state: &MetisState, cfg: &OutputsConfig) {
    for output in state.connected_outputs() {
        let name = output.name();
        let prefs = output_prefs(cfg, &name);
        let Some(ref path) = prefs.color_profile else {
            continue;
        };
        if std::path::Path::new(path).is_file() {
            tracing::info!(
                %name,
                profile = %path,
                "ICC profile configured (wp_color_management apply pending)"
            );
        } else {
            tracing::warn!(%name, profile = %path, "ICC profile path not found");
        }
    }
}
