//! Steam / package / hardware detection shared by health checks and settings.

use std::fs;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SteamInstall {
    Native,
    Flatpak,
    None,
}

pub fn detect_steam() -> SteamInstall {
    if binary_in_path("steam") {
        return SteamInstall::Native;
    }
    if binary_in_path("flatpak") && flatpak_has_app("com.valvesoftware.Steam") {
        return SteamInstall::Flatpak;
    }
    SteamInstall::None
}

pub fn flatpak_has_app(app_id: &str) -> bool {
    Command::new("flatpak")
        .args(["info", app_id])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn binary_in_path(program: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| {
        let candidate = dir.join(program);
        candidate.is_file()
            && fs::metadata(&candidate)
                .map(|m| {
                    use std::os::unix::fs::PermissionsExt;
                    m.permissions().mode() & 0o111 != 0
                })
                .unwrap_or(false)
    })
}

pub fn gamemode_installed() -> bool {
    binary_in_path("gamemoderun")
}

pub fn i386_vulkan_likely_missing() -> bool {
    if Command::new("dpkg")
        .args(["-l", "mesa-vulkan-drivers:i386"])
        .output()
        .map(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).contains("ii"))
        .unwrap_or(false)
    {
        return false;
    }
    if Path::new("/usr/lib/i386-linux-gnu/libvulkan.so.1").exists()
        || Path::new("/usr/lib32/libvulkan.so.1").exists()
    {
        return false;
    }
    binary_in_path("steam") || flatpak_has_app("com.valvesoftware.Steam")
}

pub fn user_in_input_group() -> bool {
    let Ok(passwd) = fs::read_to_string("/etc/group") else {
        return true;
    };
    let user = std::env::var("USER").unwrap_or_default();
    for line in passwd.lines() {
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() >= 4 && parts[0] == "input" {
            return parts[3].split(',').any(|m| m == user);
        }
    }
    true
}

pub fn nvidia_driver_loaded() -> bool {
    Path::new("/proc/driver/nvidia").exists()
}

pub fn pipewire_or_pulse_available() -> bool {
    binary_in_path("pipewire") || binary_in_path("pulseaudio") || binary_in_path("pw-cli")
}

pub fn hybrid_gpu_summary() -> Option<String> {
    let display = metis_config::display_gpu_pci();
    metis_config::detect_hybrid_gpu(display.as_deref()).map(|h| h.discrete_label)
}
