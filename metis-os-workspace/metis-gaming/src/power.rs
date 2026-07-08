//! Power profile hooks for active game sessions.

use std::process::Command;

use metis_config::PowerProfile;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionPowerAction {
    EnterPerformance,
    RestoreBalanced,
}

pub fn apply_session_power(action: SessionPowerAction, previous: Option<PowerProfile>) {
    let profile = match action {
        SessionPowerAction::EnterPerformance => "performance",
        SessionPowerAction::RestoreBalanced => match previous.unwrap_or(PowerProfile::Balanced) {
            PowerProfile::Performance => "balanced",
            PowerProfile::PowerSaver => "power-saver",
            PowerProfile::Balanced => "balanced",
        },
    };
    let _ = Command::new("powerprofilesctl")
        .args(["set", profile])
        .status();
}

pub fn read_current_power_profile() -> Option<PowerProfile> {
    let output = Command::new("powerprofilesctl").args(["get"]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).to_lowercase();
    if text.contains("performance") {
        Some(PowerProfile::Performance)
    } else if text.contains("power-saver") || text.contains("powersave") {
        Some(PowerProfile::PowerSaver)
    } else {
        Some(PowerProfile::Balanced)
    }
}

pub fn register_gamemode(pid: u32) {
    if !crate::detect::gamemode_installed() {
        return;
    }
    let _ = Command::new("busctl")
        .args([
            "call",
            "--user",
            "com.feralinteractive.GameMode",
            "/com/feralinteractive/GameMode",
            "com.feralinteractive.GameMode",
            "RegisterGame",
            "u",
            &pid.to_string(),
        ])
        .status();
}

pub fn unregister_gamemode(pid: u32) {
    let _ = Command::new("busctl")
        .args([
            "call",
            "--user",
            "com.feralinteractive.GameMode",
            "/com/feralinteractive/GameMode",
            "com.feralinteractive.GameMode",
            "UnregisterGame",
            "u",
            &pid.to_string(),
        ])
        .status();
}
