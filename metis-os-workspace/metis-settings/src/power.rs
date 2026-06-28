//! Battery status, power profiles, and idle/lid preferences.

use metis_config::{LidCloseAction, PowerConfig, PowerProfile};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct BatteryInfo {
    pub present: bool,
    pub percent: Option<u8>,
    pub charging: bool,
    pub status: String,
    /// Time to empty/full as reported by sysfs or upower (best-effort).
    pub time_label: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PowerSnapshot {
    pub battery: BatteryInfo,
    pub profile: PowerProfile,
    pub profiles_available: Vec<PowerProfile>,
    pub config: PowerConfig,
}

pub fn load_snapshot() -> PowerSnapshot {
    let config = metis_config::load_power_config();
    let (profile, profiles_available) = read_power_profile();
    PowerSnapshot {
        battery: read_battery(),
        profile,
        profiles_available,
        config,
    }
}

pub fn save_config(cfg: &PowerConfig) {
    if let Err(err) = metis_config::save_power_config(cfg) {
        tracing::warn!(%err, "failed to save power.json");
    }
    apply_profile(cfg.profile);
    apply_idle_settings(cfg);
}

pub fn apply_profile(profile: PowerProfile) {
    let name = match profile {
        PowerProfile::Balanced => "balanced",
        PowerProfile::PowerSaver => "power-saver",
        PowerProfile::Performance => "performance",
    };
    let _ = std::process::Command::new("powerprofilesctl")
        .args(["set", name])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

fn apply_idle_settings(cfg: &PowerConfig) {
    // Best-effort logind idle settings via busctl (no extra crate).
    let blank_us = cfg.blank_after_minutes.saturating_mul(60) * 1_000_000;
    let _ = busctl_set(
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
        "SetIdleAction",
        &format!("u 3 {blank_us}"),
    );
    let suspend_us = cfg.suspend_after_minutes.saturating_mul(60) * 1_000_000;
    let _ = busctl_set(
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
        "SetIdleAction",
        &format!("u 4 {suspend_us}"),
    );
    let lid = match cfg.lid_close {
        LidCloseAction::Suspend => "suspend",
        LidCloseAction::Ignore => "ignore",
        LidCloseAction::Hibernate => "hibernate",
        LidCloseAction::PowerOff => "poweroff",
    };
    let _ = busctl_set(
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
        "SetHandleLidSwitch",
        &format!("s {lid}"),
    );
}

fn busctl_set(dest: &str, path: &str, iface: &str, method: &str, args: &str) -> bool {
    std::process::Command::new("busctl")
        .args(["call", dest, path, iface, method, args])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn read_power_profile() -> (PowerProfile, Vec<PowerProfile>) {
    let output = std::process::Command::new("powerprofilesctl")
        .arg("get")
        .output()
        .ok();
    let text = output
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_lowercase())
        .unwrap_or_default();
    let profile = match text.as_str() {
        "power-saver" => PowerProfile::PowerSaver,
        "performance" => PowerProfile::Performance,
        _ => PowerProfile::Balanced,
    };
    let list_out = std::process::Command::new("powerprofilesctl")
        .args(["list"])
        .output()
        .ok();
    let mut available = Vec::new();
    if let Some(o) = list_out {
        for line in String::from_utf8_lossy(&o.stdout).lines() {
            let key = line.split(':').next().unwrap_or("").trim().to_lowercase();
            match key.as_str() {
                "power-saver" => available.push(PowerProfile::PowerSaver),
                "balanced" => available.push(PowerProfile::Balanced),
                "performance" => available.push(PowerProfile::Performance),
                _ => {}
            }
        }
    }
    if available.is_empty() {
        available = vec![
            PowerProfile::PowerSaver,
            PowerProfile::Balanced,
            PowerProfile::Performance,
        ];
    }
    (profile, available)
}

fn read_battery() -> BatteryInfo {
    let bat = find_battery_path();
    let Some(path) = bat else {
        return BatteryInfo::default();
    };
    let percent = std::fs::read_to_string(path.join("capacity"))
        .ok()
        .and_then(|s| s.trim().parse().ok());
    let status = std::fs::read_to_string(path.join("status"))
        .unwrap_or_default()
        .trim()
        .to_string();
    let charging = status.eq_ignore_ascii_case("charging")
        || status.eq_ignore_ascii_case("fully charged");
    BatteryInfo {
        present: true,
        percent,
        charging,
        status: if status.is_empty() {
            "Unknown".into()
        } else {
            status
        },
        time_label: None,
    }
}

fn find_battery_path() -> Option<std::path::PathBuf> {
    for name in ["BAT0", "BAT1"] {
        let path = std::path::PathBuf::from(format!("/sys/class/power_supply/{name}"));
        if path.join("capacity").exists() {
            return Some(path);
        }
    }
    None
}
