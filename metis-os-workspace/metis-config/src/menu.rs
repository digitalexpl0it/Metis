use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Persistent app-menu state, stored at `~/.config/metis/menu.json`.
///
/// `pinned` is an ordered list of `.desktop` ids shown in the menu's pinned grid.
/// `launch_counts` tracks how often each app id has been launched from the menu,
/// driving the "Frequent Apps" ordering.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MenuConfig {
    #[serde(default)]
    pub pinned: Vec<String>,
    #[serde(default)]
    pub launch_counts: HashMap<String, u32>,
}

pub fn menu_config_path() -> std::path::PathBuf {
    super::config_dir().join("menu.json")
}

pub fn load_menu_config() -> MenuConfig {
    let path = menu_config_path();
    if path.exists() {
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(cfg) = serde_json::from_str::<MenuConfig>(&text) {
                return cfg;
            }
            tracing::warn!("menu.json parse failed — using defaults");
        }
    }
    MenuConfig::default()
}

pub fn save_menu_config(config: &MenuConfig) -> std::io::Result<()> {
    super::ensure_config_dirs()?;
    let json = serde_json::to_string_pretty(config).map_err(std::io::Error::other)?;
    std::fs::write(menu_config_path(), json)
}
