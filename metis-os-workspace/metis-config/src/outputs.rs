use serde::{Deserialize, Serialize};

/// Per-output display preferences. Keys match compositor output names (`metis-0`, …).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputPrefs {
    /// UI scale factor (1.0 = 100%). Applied by the compositor when supported.
    #[serde(default = "default_scale")]
    pub scale: f64,
    /// Whether this output is enabled (future compositor hook).
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Night-light warm shift enabled on this output (Phase 5 precursor).
    #[serde(default)]
    pub night_light: bool,
}

fn default_scale() -> f64 {
    1.0
}

fn default_true() -> bool {
    true
}

impl Default for OutputPrefs {
    fn default() -> Self {
        Self {
            scale: default_scale(),
            enabled: true,
            night_light: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct OutputsConfig {
    /// Per-output overrides keyed by compositor output name.
    #[serde(default)]
    pub outputs: std::collections::HashMap<String, OutputPrefs>,
    /// Global night-light schedule (future): enabled + warm temperature.
    #[serde(default)]
    pub night_light_enabled: bool,
    /// Colour temperature in kelvin when night light is on (e.g. 4000).
    #[serde(default = "default_night_temp")]
    pub night_light_temperature: u32,
}

fn default_night_temp() -> u32 {
    4000
}

pub fn outputs_config_path() -> std::path::PathBuf {
    super::config_dir().join("outputs.json")
}

pub fn load_outputs_config() -> OutputsConfig {
    let path = outputs_config_path();
    if path.exists() {
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(cfg) = serde_json::from_str(&text) {
                return cfg;
            }
        }
    }
    OutputsConfig::default()
}

pub fn save_outputs_config(cfg: &OutputsConfig) -> std::io::Result<()> {
    super::ensure_config_dirs()?;
    let json = serde_json::to_string_pretty(cfg).map_err(std::io::Error::other)?;
    std::fs::write(outputs_config_path(), json)
}

/// Merge saved prefs for `name`, creating defaults when missing.
pub fn output_prefs(cfg: &OutputsConfig, name: &str) -> OutputPrefs {
    cfg.outputs.get(name).cloned().unwrap_or_default()
}
