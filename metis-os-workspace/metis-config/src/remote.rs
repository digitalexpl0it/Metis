//! Desktop sharing preferences persisted to `~/.config/metis/remote.json`.
//!
//! Credentials are never stored here — gnome-remote-desktop keeps RDP username
//! and password via `grdctl --headless`.

use serde::{Deserialize, Serialize};

/// Remote desktop backend. v1 ships GNOME RDP only; more variants later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RemoteBackend {
    #[default]
    GnomeRdp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteConfig {
    /// User wants desktop sharing active (metis-remote enable on toggle / autostart).
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub backend: RemoteBackend,
    /// Start sharing when the Metis session opens (if [`Self::enabled`]).
    #[serde(default = "default_auto_start")]
    pub auto_start: bool,
    /// UI hint for LAN-only firewall guidance (not enforced by Metis).
    #[serde(default = "default_true")]
    pub lan_only: bool,
}

fn default_auto_start() -> bool {
    true
}

fn default_true() -> bool {
    true
}

impl Default for RemoteConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            backend: RemoteBackend::default(),
            auto_start: default_auto_start(),
            lan_only: default_true(),
        }
    }
}

pub fn remote_config_path() -> std::path::PathBuf {
    super::config_dir().join("remote.json")
}

pub fn load_remote_config() -> RemoteConfig {
    let path = remote_config_path();
    if path.exists() {
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(cfg) = serde_json::from_str(&text) {
                return cfg;
            }
            tracing::warn!("remote.json parse failed — using defaults");
        }
    }
    RemoteConfig::default()
}

pub fn save_remote_config(cfg: &RemoteConfig) -> std::io::Result<()> {
    super::ensure_config_dirs()?;
    let json = serde_json::to_string_pretty(cfg).map_err(std::io::Error::other)?;
    std::fs::write(remote_config_path(), json)
}
