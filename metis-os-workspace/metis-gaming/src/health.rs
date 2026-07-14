//! Gaming health checks with optional auto-fix actions.

use metis_config::load_gaming_config;

use crate::detect::{
    detect_steam, flatpak_has_app, gamemode_installed, hybrid_gpu_summary, i386_vulkan_likely_missing,
    nvidia_driver_loaded, pipewire_or_pulse_available, user_in_input_group, SteamInstall,
};
use crate::flatpak::{flatpak_steam_needs_optimize, optimize_flatpak_gaming};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthSeverity {
    Ok,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthItem {
    pub id: &'static str,
    pub label: String,
    pub severity: HealthSeverity,
    pub detail: String,
    pub fix_hint: Option<String>,
    pub auto_fixable: bool,
}

#[derive(Debug, Clone, Default)]
pub struct HealthCheck {
    pub items: Vec<HealthItem>,
}

pub fn run_health_check() -> HealthCheck {
    let cfg = load_gaming_config();
    let mut items = Vec::new();

    match detect_steam() {
        SteamInstall::Native => items.push(ok("steam", "Steam", "Installed (native)")),
        SteamInstall::Flatpak => items.push(ok("steam", "Steam", "Installed (Flatpak)")),
        SteamInstall::None => items.push(HealthItem {
            id: "steam",
            label: "Steam".into(),
            severity: HealthSeverity::Info,
            detail: "Not detected".into(),
            fix_hint: Some(
                "flatpak install -y flathub com.valvesoftware.Steam   # or: sudo apt install steam-installer"
                    .into(),
            ),
            // Prefer Flatpak when available; otherwise apt steam-installer.
            auto_fixable: true,
        }),
    }

    if flatpak_has_app("com.valvesoftware.Steam") && flatpak_steam_needs_optimize() {
        items.push(HealthItem {
            id: "flatpak_steam",
            label: "Flatpak Steam overrides".into(),
            severity: HealthSeverity::Warn,
            detail: "Gaming overrides not applied".into(),
            fix_hint: Some(
                "metis-cmd optimize-gaming   # or Settings → Gaming → Optimize now".into(),
            ),
            auto_fixable: true,
        });
    } else if flatpak_has_app("com.valvesoftware.Steam") {
        items.push(ok(
            "flatpak_steam",
            "Flatpak Steam overrides",
            "Optimized",
        ));
    }

    if i386_vulkan_likely_missing() {
        items.push(HealthItem {
            id: "vulkan_i386",
            label: "32-bit Vulkan".into(),
            severity: HealthSeverity::Error,
            detail: "Proton may fail without i386 Vulkan drivers".into(),
            fix_hint: Some("sudo apt install -y mesa-vulkan-drivers:i386".into()),
            auto_fixable: true,
        });
    } else {
        items.push(ok("vulkan_i386", "32-bit Vulkan", "OK"));
    }

    if cfg.auto_gamemode && !gamemode_installed() {
        items.push(HealthItem {
            id: "gamemode",
            label: "GameMode".into(),
            severity: HealthSeverity::Info,
            detail: "auto_gamemode on but gamemoderun missing".into(),
            fix_hint: Some("sudo apt install -y gamemode".into()),
            auto_fixable: true,
        });
    } else if gamemode_installed() {
        items.push(ok("gamemode", "GameMode", "Available"));
    }

    if !user_in_input_group() {
        items.push(HealthItem {
            id: "input_group",
            label: "Input group".into(),
            severity: HealthSeverity::Warn,
            detail: "User not in input group".into(),
            fix_hint: Some("sudo usermod -aG input $USER  (then log out and back in)".into()),
            auto_fixable: true,
        });
    } else {
        items.push(ok("input_group", "Input group", "OK"));
    }

    if let Some(label) = hybrid_gpu_summary() {
        if label.to_lowercase().contains("nvidia") && !nvidia_driver_loaded() {
            items.push(HealthItem {
                id: "nvidia_driver",
                label: "NVIDIA driver".into(),
                severity: HealthSeverity::Error,
                detail: "NVIDIA GPU without proprietary driver".into(),
                // Interactive / reboot-heavy — copy only, no blind Fix.
                fix_hint: Some("sudo ubuntu-drivers install".into()),
                auto_fixable: false,
            });
        } else {
            items.push(ok("hybrid_gpu", "Hybrid GPU", &format!("Discrete: {label}")));
        }
    } else {
        items.push(ok("hybrid_gpu", "Hybrid GPU", "Single GPU"));
    }

    if !pipewire_or_pulse_available() {
        items.push(HealthItem {
            id: "audio",
            label: "Audio".into(),
            severity: HealthSeverity::Warn,
            detail: "No PipeWire/Pulse on PATH".into(),
            fix_hint: Some("sudo apt install -y pipewire-audio".into()),
            auto_fixable: true,
        });
    } else {
        items.push(ok("audio", "Audio", "OK"));
    }

    HealthCheck { items }
}

pub fn auto_fix_item(id: &str) -> Result<String, String> {
    match id {
        "flatpak_steam" => {
            let results = optimize_flatpak_gaming(&[])?;
            Ok(results
                .iter()
                .map(|r| format!("{}: {}", r.app_id, r.message))
                .collect::<Vec<_>>()
                .join("; "))
        }
        "input_group" => add_user_to_input_group(),
        "gamemode" => pkexec_apt_install(&["gamemode"], "GameMode"),
        "vulkan_i386" => {
            pkexec_apt_install(&["mesa-vulkan-drivers:i386"], "32-bit Vulkan drivers")
        }
        "audio" => pkexec_apt_install(&["pipewire-audio"], "PipeWire audio"),
        "steam" => install_steam(),
        other => Err(format!("no auto-fix for {other}")),
    }
}

fn install_steam() -> Result<String, String> {
    match detect_steam() {
        SteamInstall::Native | SteamInstall::Flatpak => {
            return Ok("Steam is already installed".into());
        }
        SteamInstall::None => {}
    }
    if crate::detect::binary_in_path("flatpak") {
        let status = std::process::Command::new("flatpak")
            .args([
                "install",
                "-y",
                "flathub",
                "com.valvesoftware.Steam",
            ])
            .status()
            .map_err(|e| format!("failed to start flatpak: {e}"))?;
        if status.success() {
            let _ = optimize_flatpak_gaming(&[]);
            return Ok("Installed Flatpak Steam (overrides applied when needed)".into());
        }
        // Fall through to apt if Flatpak remotes aren't set up.
    }
    pkexec_apt_install(&["steam-installer"], "Steam")
}

fn pkexec_apt_install(packages: &[&str], label: &str) -> Result<String, String> {
    if packages.is_empty() {
        return Ok(format!("{label}: nothing to install"));
    }
    if !crate::detect::binary_in_path("pkexec") {
        return Err(format!(
            "pkexec not found — run: sudo apt install -y {}",
            packages.join(" ")
        ));
    }
    let status = std::process::Command::new("pkexec")
        .args(["apt-get", "install", "-y", "--"])
        .args(packages)
        .env("DEBIAN_FRONTEND", "noninteractive")
        .status()
        .map_err(|e| format!("failed to start pkexec: {e}"))?;
    if status.success() {
        Ok(format!("Installed {label}"))
    } else {
        Err(format!(
            "Could not install {label} (auth cancelled?). Run: sudo apt install -y {}",
            packages.join(" ")
        ))
    }
}

fn add_user_to_input_group() -> Result<String, String> {
    let user = std::env::var("USER").map_err(|_| "USER is not set".to_string())?;
    if user.is_empty() || user.contains(['/', ' ', '\0']) {
        return Err("refusing to modify an invalid USER".into());
    }
    if user_in_input_group() {
        return Ok("Already in the input group".into());
    }
    if !crate::detect::binary_in_path("pkexec") {
        return Err(
            "pkexec not found — run: sudo usermod -aG input $USER  (then log out)".into(),
        );
    }
    let status = std::process::Command::new("pkexec")
        .args(["usermod", "-aG", "input", &user])
        .status()
        .map_err(|e| format!("failed to start pkexec: {e}"))?;
    if status.success() {
        Ok(format!(
            "Added {user} to the input group — log out and back in for it to apply"
        ))
    } else {
        Err(
            "Could not add you to the input group (auth cancelled?). Run: sudo usermod -aG input $USER"
                .into(),
        )
    }
}

fn ok(id: &'static str, label: &str, detail: &str) -> HealthItem {
    HealthItem {
        id,
        label: label.into(),
        severity: HealthSeverity::Ok,
        detail: detail.into(),
        fix_hint: None,
        auto_fixable: false,
    }
}
