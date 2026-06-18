use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

use crate::services::notifications::{self, BarNotification};
use crate::services::workspaces;

#[derive(Debug)]
enum AudioCommand {
    SetVolumeAbsolute(u8),
    SetVolumeRelative(i8),
    SetMute(bool),
    SetMicVolumeAbsolute(u8),
    SetMicMute(bool),
}

static AUDIO_CMD_TX: OnceLock<Sender<AudioCommand>> = OnceLock::new();

#[derive(Debug, Clone, Default, PartialEq)]
pub struct BarSnapshot {
    pub battery_percent: Option<u8>,
    pub battery_charging: bool,
    pub network_label: String,
    pub network_connected: bool,
    pub volume_percent: u8,
    pub volume_muted: bool,
    pub mic_percent: u8,
    pub mic_muted: bool,
    pub notifications: Vec<BarNotification>,
    pub workspaces: workspaces::WorkspaceSnapshot,
}

pub fn spawn_bar_pollers() -> Receiver<BarSnapshot> {
    let (tx, rx) = mpsc::channel();
    let (audio_tx, audio_rx) = mpsc::channel();
    let _ = AUDIO_CMD_TX.set(audio_tx);
    thread::Builder::new()
        .name("metis-bar-poll".into())
        .spawn(move || poll_loop(tx, audio_rx))
        .expect("spawn bar poller");
    rx
}

fn poll_loop(tx: Sender<BarSnapshot>, audio_rx: Receiver<AudioCommand>) {
    let mut tick: u64 = 0;
    let mut cached = BarSnapshot::default();
    cached.workspaces = workspaces::workspace_snapshot();
    let mut last_sent = cached.clone();

    loop {
        drain_audio_commands(&audio_rx);

        if tick % 4 == 0 {
            cached.battery_percent = read_battery_percent();
            cached.battery_charging = read_battery_charging();
        }
        if tick % 3 == 0 {
            cached.network_label = read_network_label();
            cached.network_connected = read_network_connected();
        }
        if tick % 2 == 0 {
            // Keep the last good reading when pactl times out / reports nothing,
            // so a transient failure doesn't snap sliders to 0 or flip mute state.
            if let Some(v) = read_volume_percent() {
                cached.volume_percent = v;
            }
            if let Some(m) = read_volume_muted() {
                cached.volume_muted = m;
            }
            if let Some(v) = read_mic_percent() {
                cached.mic_percent = v;
            }
            if let Some(m) = read_mic_muted() {
                cached.mic_muted = m;
            }
        }
        if tick % 5 == 0 && std::env::var("METIS_DEMO_NOTIFICATIONS").is_ok() {
            cached.notifications = notifications::demo_notifications();
        }
        cached.workspaces = workspaces::workspace_snapshot();

        if cached != last_sent {
            last_sent = cached.clone();
            if tx.send(cached.clone()).is_err() {
                break;
            }
        }
        tick = tick.wrapping_add(1);
        thread::sleep(Duration::from_millis(400));
    }
}

fn drain_audio_commands(rx: &Receiver<AudioCommand>) {
    while let Ok(cmd) = rx.try_recv() {
        match cmd {
            AudioCommand::SetVolumeAbsolute(pct) => run_set_volume_absolute(pct),
            AudioCommand::SetVolumeRelative(delta) => run_set_volume_relative(delta),
            AudioCommand::SetMute(muted) => run_set_mute(muted),
            AudioCommand::SetMicVolumeAbsolute(pct) => run_set_mic_volume_absolute(pct),
            AudioCommand::SetMicMute(muted) => run_set_mic_mute(muted),
        }
    }
}

fn queue_audio(cmd: AudioCommand) {
    if let Some(tx) = AUDIO_CMD_TX.get() {
        let _ = tx.send(cmd);
    }
}

fn read_battery_percent() -> Option<u8> {
    let capacity = std::fs::read_to_string("/sys/class/power_supply/BAT0/capacity")
        .or_else(|_| std::fs::read_to_string("/sys/class/power_supply/BAT1/capacity"))
        .ok()?;
    capacity.trim().parse().ok()
}

fn read_battery_charging() -> bool {
    let status = std::fs::read_to_string("/sys/class/power_supply/BAT0/status")
        .or_else(|_| std::fs::read_to_string("/sys/class/power_supply/BAT1/status"))
        .unwrap_or_default();
    status.trim().eq_ignore_ascii_case("charging")
        || status.trim().eq_ignore_ascii_case("fully charged")
}

fn run_command(cmd: &mut std::process::Command) -> Option<std::process::Output> {
    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    let mut child = cmd.spawn().ok()?;
    let deadline = std::time::Instant::now() + Duration::from_millis(600);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().ok(),
            Ok(None) => {}
            Err(_) => return None,
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn read_network_connected() -> bool {
    let mut cmd = std::process::Command::new("nmcli");
    cmd.args(["-t", "-f", "STATE", "general"]);
    run_command(&mut cmd)
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "connected")
        .unwrap_or_else(|| {
            std::fs::read_to_string("/sys/class/net/wlan0/operstate")
                .or_else(|_| std::fs::read_to_string("/sys/class/net/eth0/operstate"))
                .map(|s| s.trim() == "up")
                .unwrap_or(false)
        })
}

fn read_network_label() -> String {
    let mut cmd = std::process::Command::new("nmcli");
    cmd.args(["-t", "-f", "ACTIVE,SSID", "dev", "wifi"]);
    if let Some(output) = run_command(&mut cmd) {
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            let mut parts = line.split(':');
            if parts.next() == Some("yes") {
                if let Some(ssid) = parts.next() {
                    if !ssid.is_empty() {
                        return ssid.to_string();
                    }
                }
            }
        }
    }
    if read_network_connected() {
        "Connected".into()
    } else {
        "Offline".into()
    }
}

fn read_volume_percent() -> Option<u8> {
    let mut cmd = std::process::Command::new("pactl");
    cmd.args(["get-sink-volume", "@DEFAULT_SINK@"]);
    run_command(&mut cmd).and_then(|o| {
        let text = String::from_utf8_lossy(&o.stdout);
        text.split_whitespace()
            .nth(4)
            .and_then(|s| s.trim_end_matches('%').parse().ok())
    })
}

fn read_volume_muted() -> Option<bool> {
    let mut cmd = std::process::Command::new("pactl");
    cmd.args(["get-sink-mute", "@DEFAULT_SINK@"]);
    run_command(&mut cmd).and_then(|o| parse_mute(&String::from_utf8_lossy(&o.stdout)))
}

fn parse_mute(text: &str) -> Option<bool> {
    if text.contains("yes") {
        Some(true)
    } else if text.contains("no") {
        Some(false)
    } else {
        None
    }
}

fn read_mic_percent() -> Option<u8> {
    let mut cmd = std::process::Command::new("pactl");
    cmd.args(["get-source-volume", "@DEFAULT_SOURCE@"]);
    run_command(&mut cmd).and_then(|o| {
        let text = String::from_utf8_lossy(&o.stdout);
        text.split_whitespace()
            .nth(4)
            .and_then(|s| s.trim_end_matches('%').parse().ok())
    })
}

fn read_mic_muted() -> Option<bool> {
    let mut cmd = std::process::Command::new("pactl");
    cmd.args(["get-source-mute", "@DEFAULT_SOURCE@"]);
    run_command(&mut cmd).and_then(|o| parse_mute(&String::from_utf8_lossy(&o.stdout)))
}

pub fn set_volume_relative(delta: i8) {
    queue_audio(AudioCommand::SetVolumeRelative(delta));
}

pub fn set_mic_volume_absolute(percent: u8) {
    queue_audio(AudioCommand::SetMicVolumeAbsolute(percent));
}

pub fn set_mic_mute(muted: bool) {
    queue_audio(AudioCommand::SetMicMute(muted));
}

pub fn set_volume_absolute(percent: u8) {
    queue_audio(AudioCommand::SetVolumeAbsolute(percent));
}

pub fn set_mute(muted: bool) {
    queue_audio(AudioCommand::SetMute(muted));
}

pub fn toggle_mute() {
    let _ = std::process::Command::new("pactl")
        .args(["set-sink-mute", "@DEFAULT_SINK@", "toggle"])
        .status();
}

fn run_set_volume_relative(delta: i8) {
    let sign = if delta >= 0 { "+" } else { "" };
    let mut cmd = std::process::Command::new("pactl");
    cmd.args([
        "set-sink-volume",
        "@DEFAULT_SINK@",
        &format!("{sign}{delta}%"),
    ]);
    let _ = run_command(&mut cmd);
}

fn run_set_volume_absolute(percent: u8) {
    let mut cmd = std::process::Command::new("pactl");
    cmd.args([
        "set-sink-volume",
        "@DEFAULT_SINK@",
        &format!("{}%", percent.min(100)),
    ]);
    let _ = run_command(&mut cmd);
}

fn run_set_mute(muted: bool) {
    let flag = if muted { "yes" } else { "no" };
    let mut cmd = std::process::Command::new("pactl");
    cmd.args(["set-sink-mute", "@DEFAULT_SINK@", flag]);
    let _ = run_command(&mut cmd);
}

fn run_set_mic_volume_absolute(percent: u8) {
    let mut cmd = std::process::Command::new("pactl");
    cmd.args([
        "set-source-volume",
        "@DEFAULT_SOURCE@",
        &format!("{}%", percent.min(100)),
    ]);
    let _ = run_command(&mut cmd);
}

fn run_set_mic_mute(muted: bool) {
    let flag = if muted { "yes" } else { "no" };
    let mut cmd = std::process::Command::new("pactl");
    cmd.args(["set-source-mute", "@DEFAULT_SOURCE@", flag]);
    let _ = run_command(&mut cmd);
}
