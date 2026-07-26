//! Privileged helpers intended to run only under `pkexec` (Phase 15 §B).

use std::path::Path;
use std::process::Command;

/// Packages Metis may install via Polkit (onboarding + gaming health fixes).
pub const APT_ALLOWLIST: &[&str] = &[
    "gnome-remote-desktop",
    "flatpak",
    "gamemode",
    "bluez",
    "bluetooth",
    "cups",
    "system-config-printer",
    "gnome-keyring",
    "mesa-vulkan-drivers:i386",
    "pipewire-audio",
    "steam-installer",
    "nftables",
    "policykit-1-gnome",
    "mate-polkit",
];

fn require_root() -> Result<(), String> {
    let uid = Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    if uid == "0" {
        Ok(())
    } else {
        Err("this command must run as root (via pkexec)".into())
    }
}

fn package_allowed(pkg: &str) -> bool {
    APT_ALLOWLIST.iter().any(|p| *p == pkg)
}

/// `apt-get install -y -- <allowlisted packages…>` — root only.
pub fn apt_install(packages: &[String]) -> Result<(), String> {
    require_root()?;
    if packages.is_empty() {
        return Ok(());
    }
    for pkg in packages {
        if pkg.is_empty()
            || pkg.contains(['/', ' ', '\0', ';', '|', '&', '$', '`', '\n', '\r'])
            || !package_allowed(pkg)
        {
            return Err(format!(
                "package '{pkg}' is not on the Metis allowlist (refusing apt-get)"
            ));
        }
    }
    let status = Command::new("apt-get")
        .args(["install", "-y", "--"])
        .args(packages)
        .env("DEBIAN_FRONTEND", "noninteractive")
        .status()
        .map_err(|e| format!("apt-get failed: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("apt-get exited with {status}"))
    }
}

/// Validate a Unix username (no path separators / control chars).
pub fn validate_username(user: &str) -> Result<(), String> {
    if user.is_empty() || user.len() > 64 {
        return Err("invalid username".into());
    }
    if !user
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
    {
        return Err("username contains invalid characters".into());
    }
    Ok(())
}

/// `usermod -aG input <user>` — root only.
pub fn add_input_group(user: &str) -> Result<(), String> {
    require_root()?;
    validate_username(user)?;
    let status = Command::new("usermod")
        .args(["-aG", "input", user])
        .status()
        .map_err(|e| format!("usermod failed: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("usermod exited with {status}"))
    }
}

/// Prefer the packaged binary so pkexec cannot escalate a writable cwd copy.
pub fn privileged_exe() -> std::path::PathBuf {
    const INSTALLED: &str = "/usr/bin/metis-remote";
    if Path::new(INSTALLED).is_file() {
        return Path::new(INSTALLED).to_path_buf();
    }
    std::env::current_exe().unwrap_or_else(|_| Path::new("metis-remote").to_path_buf())
}
