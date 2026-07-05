//! Desktop sharing orchestration for Metis (gnome-remote-desktop session-sharing RDP).

mod gnome_rdp;
mod host;

pub use gnome_rdp::{disable_sharing, enable_sharing, set_credentials, status_snapshot};
pub use host::{hostname, lan_addresses};

use metis_config::{load_remote_config, save_remote_config};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct RemoteStatus {
    pub available: bool,
    pub running: bool,
    pub rdp_enabled: bool,
    pub port: u16,
    pub password_set: bool,
    pub username: Option<String>,
    pub hostname: String,
    pub addresses: Vec<String>,
    pub backend: String,
    pub config_enabled: bool,
    pub error: Option<String>,
}

/// Read live status from gnome-remote-desktop and merge with `remote.json`.
pub fn status() -> RemoteStatus {
    let cfg = load_remote_config();
    let mut snap = status_snapshot();
    snap.config_enabled = cfg.enabled;
    if snap.hostname.is_empty() {
        snap.hostname = hostname();
    }
    if snap.addresses.is_empty() {
        snap.addresses = lan_addresses();
    }
    snap
}

/// Enable sharing per `remote.json` (starts headless daemon + RDP).
pub fn enable() -> Result<(), String> {
    let mut cfg = load_remote_config();
    if !gnome_rdp::grdctl_available() {
        return Err(
            "gnome-remote-desktop is not installed (install the gnome-remote-desktop package)"
                .into(),
        );
    }
    let snap = status_snapshot();
    if !snap.password_set {
        return Err("Set RDP credentials before enabling remote desktop".into());
    }
    enable_sharing()?;
    cfg.enabled = true;
    save_remote_config(&cfg).map_err(|e| e.to_string())?;
    Ok(())
}

/// Disable RDP and stop the headless daemon; clears `enabled` in config.
pub fn disable() -> Result<(), String> {
    let mut cfg = load_remote_config();
    if gnome_rdp::grdctl_available() {
        disable_sharing()?;
    }
    cfg.enabled = false;
    save_remote_config(&cfg).map_err(|e| e.to_string())?;
    Ok(())
}

/// Set RDP username/password via grdctl (headless store).
pub fn set_password(username: &str, password: &str) -> Result<(), String> {
    if username.trim().is_empty() {
        return Err("Username must not be empty".into());
    }
    if password.is_empty() {
        return Err("Password must not be empty".into());
    }
    set_credentials(username.trim(), password)
}

/// Called from metis-session when `remote.json` has enabled + auto_start.
pub fn autostart_from_config() -> Result<(), String> {
    let cfg = load_remote_config();
    if !cfg.enabled || !cfg.auto_start {
        return Ok(());
    }
    if !gnome_rdp::grdctl_available() {
        tracing::warn!("remote autostart skipped: gnome-remote-desktop not installed");
        return Ok(());
    }
    let snap = status_snapshot();
    if !snap.password_set {
        tracing::warn!("remote autostart skipped: RDP credentials not set");
        return Ok(());
    }
    enable_sharing().map_err(|e| {
        tracing::warn!(%e, "remote autostart failed");
        e
    })
}
