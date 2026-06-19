use serde::{Deserialize, Serialize};

/// Persistent clock/alarm state, stored at `~/.config/metis/clock.json`.
/// Distinct from `bar::ClockConfig`, which only holds the bar's display format.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClocksConfig {
    #[serde(default)]
    pub world_clocks: Vec<String>,
    #[serde(default)]
    pub alarms: Vec<Alarm>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alarm {
    pub id: String,
    pub hour: u8,
    pub minute: u8,
    #[serde(default)]
    pub label: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Days the alarm repeats on (0 = Sunday .. 6 = Saturday). Empty = every day.
    #[serde(default)]
    pub days: Vec<u8>,
}

fn default_true() -> bool {
    true
}

pub fn clocks_config_path() -> std::path::PathBuf {
    super::config_dir().join("clock.json")
}

/// Load clock.json, creating it on first run seeded with `seed_timezones`.
pub fn load_clocks_config(seed_timezones: &[String]) -> ClocksConfig {
    let path = clocks_config_path();
    if path.exists() {
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(cfg) = serde_json::from_str::<ClocksConfig>(&text) {
                return cfg;
            }
            tracing::warn!("clock.json parse failed — using defaults");
        }
    }
    let seeded = ClocksConfig {
        world_clocks: seed_timezones
            .iter()
            .filter(|tz| tz.as_str() != "UTC" || seed_timezones.len() == 1)
            .cloned()
            .collect(),
        alarms: Vec::new(),
    };
    let _ = save_clocks_config(&seeded);
    seeded
}

pub fn save_clocks_config(config: &ClocksConfig) -> std::io::Result<()> {
    super::ensure_config_dirs()?;
    let json = serde_json::to_string_pretty(config).map_err(std::io::Error::other)?;
    std::fs::write(clocks_config_path(), json)
}
