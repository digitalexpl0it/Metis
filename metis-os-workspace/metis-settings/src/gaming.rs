//! Gaming diagnostics — connected input devices and session hints.

use std::fs;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputDeviceKind {
    Gamepad,
    Touchscreen,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputDevice {
    pub name: String,
    pub kind: InputDeviceKind,
    pub sysfs_path: Option<String>,
    pub vendor: Option<String>,
    pub product: Option<String>,
    pub handlers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SteamInstall {
    Native,
    Flatpak,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GamingSnapshot {
    pub gamepads: Vec<InputDevice>,
    pub touchscreens: Vec<InputDevice>,
    pub steam: SteamInstall,
    pub gpu_hint: String,
}

pub fn load_snapshot() -> GamingSnapshot {
    let devices = parse_input_devices();
    let gamepads: Vec<_> = devices
        .iter()
        .filter(|d| d.kind == InputDeviceKind::Gamepad)
        .cloned()
        .collect();
    let touchscreens: Vec<_> = devices
        .iter()
        .filter(|d| d.kind == InputDeviceKind::Touchscreen)
        .cloned()
        .collect();
    GamingSnapshot {
        gamepads,
        touchscreens,
        steam: detect_steam(),
        gpu_hint: gpu_hint_label(),
    }
}

fn gpu_hint_label() -> String {
    if let Ok(v) = std::env::var("METIS_GAME_GPU") {
        return format!("METIS_GAME_GPU={v}");
    }
    if std::env::var_os("METIS_NO_CLIENT_GPU").is_some() {
        return "METIS_NO_CLIENT_GPU=1".into();
    }
    let cfg = metis_config::load_gaming_config();
    let hybrid = metis_config::detect_hybrid_gpu(metis_config::display_gpu_pci().as_deref())
        .map(|h| h.discrete_label)
        .unwrap_or_else(|| "single GPU".into());
    format!(
        "Mode: {:?} · {} · compositor auto-offload for games",
        cfg.graphics_mode, hybrid
    )
}

fn detect_steam() -> SteamInstall {
    if on_path("steam") {
        return SteamInstall::Native;
    }
    if on_path("flatpak")
        && flatpak_has_app("com.valvesoftware.Steam")
    {
        return SteamInstall::Flatpak;
    }
    SteamInstall::None
}

fn flatpak_has_app(app_id: &str) -> bool {
    std::process::Command::new("flatpak")
        .args(["info", app_id])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn on_path(program: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| {
        let candidate = dir.join(program);
        candidate.is_file()
            && std::fs::metadata(&candidate)
                .map(|m| {
                    use std::os::unix::fs::PermissionsExt;
                    m.permissions().mode() & 0o111 != 0
                })
                .unwrap_or(false)
    })
}

fn parse_input_devices() -> Vec<InputDevice> {
    let Ok(raw) = fs::read_to_string("/proc/bus/input/devices") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for block in raw.split("\n\n") {
        if block.trim().is_empty() {
            continue;
        }
        let mut name = None;
        let mut sysfs = None;
        let mut vendor = None;
        let mut product = None;
        let mut handlers = Vec::new();
        let mut is_gamepad = false;
        let mut is_touch = false;

        for line in block.lines() {
            if let Some(rest) = line.strip_prefix("N: Name=\"") {
                name = Some(rest.trim_end_matches('"').to_string());
            } else if let Some(rest) = line.strip_prefix("S: Sysfs=") {
                sysfs = Some(rest.to_string());
            } else if let Some(rest) = line.strip_prefix("P: Phys=") {
                if sysfs.is_none() {
                    sysfs = Some(rest.to_string());
                }
            } else if let Some(rest) = line.strip_prefix("I: Bus=") {
                let parts: Vec<&str> = rest.split_whitespace().collect();
                for part in parts {
                    if let Some(v) = part.strip_prefix("Vendor=") {
                        vendor = Some(v.to_string());
                    } else if let Some(p) = part.strip_prefix("Product=") {
                        product = Some(p.to_string());
                    }
                }
            } else if let Some(rest) = line.strip_prefix("H: Handlers=") {
                handlers = rest
                    .split_whitespace()
                    .map(str::to_string)
                    .collect();
            }
        }

        let Some(name) = name else {
            continue;
        };
        let name_lc = name.to_lowercase();
        let handler_has_js = handlers.iter().any(|h| h.starts_with("js"));
        is_gamepad = handler_has_js
            || name_lc.contains("gamepad")
            || name_lc.contains("joystick")
            || (name_lc.contains("controller") && !name_lc.contains("steam controller"))
            || name_lc.contains("x-box")
            || name_lc.contains("xbox")
            || name_lc.contains("playstation")
            || name_lc.contains("dualshock")
            || name_lc.contains("switch pro");
        is_touch = name_lc.contains("touchscreen")
            || (name_lc.contains("touch") && !name_lc.contains("touchpad"));
        let kind = if is_gamepad {
            InputDeviceKind::Gamepad
        } else if is_touch {
            InputDeviceKind::Touchscreen
        } else {
            continue;
        };
        out.push(InputDevice {
            name,
            kind,
            sysfs_path: sysfs,
            vendor,
            product,
            handlers,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_is_ok() {
        let _ = parse_input_devices();
    }
}
