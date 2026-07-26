//! RDP viewer recent hosts — `~/.config/metis/viewer.json`.
//!
//! Passwords are never stored here.

use serde::{Deserialize, Serialize};

const MAX_RECENT: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewerHost {
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub username: String,
}

fn default_port() -> u16 {
    3389
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ViewerConfig {
    #[serde(default)]
    pub recent: Vec<ViewerHost>,
}

pub fn viewer_config_path() -> std::path::PathBuf {
    super::config_dir().join("viewer.json")
}

pub fn load_viewer_config() -> ViewerConfig {
    let path = viewer_config_path();
    if path.exists() {
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(cfg) = serde_json::from_str(&text) {
                return cfg;
            }
            tracing::warn!("viewer.json parse failed — using defaults");
        }
    }
    ViewerConfig::default()
}

pub fn save_viewer_config(cfg: &ViewerConfig) -> std::io::Result<()> {
    super::ensure_config_dirs()?;
    let json = serde_json::to_string_pretty(cfg).map_err(std::io::Error::other)?;
    std::fs::write(viewer_config_path(), json)
}

/// Push `entry` to the front of recent hosts (dedupe by host+port+user).
pub fn remember_host(entry: ViewerHost) -> std::io::Result<()> {
    let mut cfg = load_viewer_config();
    cfg.recent
        .retain(|h| !(h.host == entry.host && h.port == entry.port && h.username == entry.username));
    cfg.recent.insert(0, entry);
    if cfg.recent.len() > MAX_RECENT {
        cfg.recent.truncate(MAX_RECENT);
    }
    save_viewer_config(&cfg)
}
