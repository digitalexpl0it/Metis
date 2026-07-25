//! Desktop sharing orchestration for Metis (gnome-remote-desktop session-sharing RDP).

mod firewall;
mod gnome_rdp;
mod host;

pub use firewall::FirewallStatus;
pub use gnome_rdp::{
    disable_sharing, enable_sharing, pause_sharing, resume_sharing, set_credentials,
    status_snapshot,
};
pub use host::{hostname, lan_addresses};

use metis_config::{load_remote_config, save_remote_config};
use zeroize::Zeroize;

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
    /// From `remote.json` — Metis should restrict TCP 3389 to private ranges.
    #[serde(default = "default_true")]
    pub lan_only: bool,
    /// Whether LAN-only firewall rules appear applied on this host.
    #[serde(default)]
    pub firewall_applied: bool,
    /// `nft`, `ufw`, or empty.
    #[serde(default)]
    pub firewall_backend: String,
    pub error: Option<String>,
}

fn default_true() -> bool {
    true
}

/// Read live status from gnome-remote-desktop and merge with `remote.json`.
pub fn status() -> RemoteStatus {
    let cfg = load_remote_config();
    let fw = firewall::status();
    let mut snap = status_snapshot();
    snap.config_enabled = cfg.enabled;
    snap.lan_only = cfg.lan_only;
    snap.firewall_applied = fw.applied;
    snap.firewall_backend = fw.backend;
    if snap.hostname.is_empty() {
        snap.hostname = hostname();
    }
    if snap.addresses.is_empty() {
        snap.addresses = lan_addresses();
    }
    if snap.config_enabled
        && snap.lan_only
        && !snap.firewall_applied
        && snap.error.is_none()
    {
        snap.error = Some(
            "LAN-only firewall is not applied — RDP may be reachable beyond the LAN \
             (install nftables or ufw, or approve the pkexec prompt)"
                .into(),
        );
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
    if cfg.lan_only {
        if let Err(err) = firewall::apply() {
            // Keep sharing on; Settings toggle stays enabled. `status()` surfaces
            // `firewall_applied: false` so the UI can warn honestly.
            tracing::warn!(%err, "LAN-only firewall apply failed — RDP may be reachable beyond the LAN");
        }
    } else {
        let _ = firewall::clear();
    }
    Ok(())
}

/// Disable RDP and stop the headless daemon; clears `enabled` in config.
pub fn disable() -> Result<(), String> {
    let mut cfg = load_remote_config();
    if gnome_rdp::grdctl_available() {
        disable_sharing()?;
    }
    let _ = firewall::clear();
    cfg.enabled = false;
    save_remote_config(&cfg).map_err(|e| e.to_string())?;
    Ok(())
}

/// Pause RDP listen while keeping `remote.json.enabled` (used on session lock).
pub fn pause() -> Result<(), String> {
    if !gnome_rdp::grdctl_available() {
        return Ok(());
    }
    pause_sharing()
}

/// Resume RDP if config still wants sharing (used on session unlock).
pub fn resume() -> Result<(), String> {
    let cfg = load_remote_config();
    if !cfg.enabled || !gnome_rdp::grdctl_available() {
        return Ok(());
    }
    let snap = status_snapshot();
    if !snap.password_set {
        return Ok(());
    }
    resume_sharing()?;
    if cfg.lan_only {
        let _ = firewall::apply();
    }
    Ok(())
}

/// Persist and optionally re-apply LAN-only firewall when sharing is active.
pub fn set_lan_only(lan_only: bool) -> Result<(), String> {
    let mut cfg = load_remote_config();
    cfg.lan_only = lan_only;
    save_remote_config(&cfg).map_err(|e| e.to_string())?;
    if cfg.enabled {
        if lan_only {
            firewall::apply().map(|_| ())?;
        } else {
            firewall::clear().map(|_| ())?;
        }
    } else if !lan_only {
        let _ = firewall::clear();
    }
    Ok(())
}

pub fn firewall_apply() -> Result<FirewallStatus, String> {
    firewall::apply()
}

pub fn firewall_clear() -> Result<FirewallStatus, String> {
    firewall::clear()
}

pub fn firewall_status() -> FirewallStatus {
    firewall::status()
}

pub fn firewall_apply_as_root() -> Result<FirewallStatus, String> {
    firewall::apply_as_root()
}

pub fn firewall_clear_as_root() -> Result<FirewallStatus, String> {
    firewall::clear_as_root()
}

/// Set RDP username/password via grdctl (headless store).
pub fn set_password(username: &str, password: &str) -> Result<(), String> {
    if username.trim().is_empty() {
        return Err("Username must not be empty".into());
    }
    if password.is_empty() {
        return Err("Password must not be empty".into());
    }
    let mut owned = password.to_string();
    let result = set_credentials(username.trim(), &owned);
    owned.zeroize();
    result
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
    })?;
    if cfg.lan_only {
        if let Err(err) = firewall::apply() {
            tracing::warn!(%err, "remote autostart: LAN-only firewall apply failed");
        }
    }
    Ok(())
}
