use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

use crate::services::notifications::BarNotification;
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

#[derive(Debug)]
enum NetworkCommand {
    Scan,
    Connect {
        ssid: String,
        password: Option<String>,
    },
    SetRadio(bool),
}

static NETWORK_CMD_TX: OnceLock<Sender<NetworkCommand>> = OnceLock::new();

/// A single visible Wi-Fi network (deduped by SSID).
#[derive(Debug, Clone, PartialEq)]
pub struct WifiNetwork {
    pub ssid: String,
    pub signal: u8,
    pub secured: bool,
    pub active: bool,
}

/// Read-only wired status for the network popover.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EthernetStatus {
    /// Whether an ethernet device exists at all (row is hidden when false).
    pub present: bool,
    pub connected: bool,
    pub label: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct BarSnapshot {
    pub battery_percent: Option<u8>,
    pub battery_charging: bool,
    pub wifi: Vec<WifiNetwork>,
    pub ethernet: EthernetStatus,
    pub wifi_enabled: bool,
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
    let (network_tx, network_rx) = mpsc::channel();
    let _ = NETWORK_CMD_TX.set(network_tx);
    thread::Builder::new()
        .name("metis-bar-poll".into())
        .spawn(move || poll_loop(tx, audio_rx, network_rx))
        .expect("spawn bar poller");
    rx
}

fn poll_loop(
    tx: Sender<BarSnapshot>,
    audio_rx: Receiver<AudioCommand>,
    network_rx: Receiver<NetworkCommand>,
) {
    let mut tick: u64 = 0;
    let mut cached = BarSnapshot::default();
    cached.workspaces = workspaces::workspace_snapshot();
    let mut last_sent = cached.clone();

    loop {
        drain_audio_commands(&audio_rx);
        drain_network_commands(&network_rx);

        if tick % 4 == 0 {
            cached.battery_percent = read_battery_percent();
            cached.battery_charging = read_battery_charging();
        }
        if tick % 3 == 0 {
            cached.wifi_enabled = read_wifi_radio_enabled();
            cached.ethernet = read_ethernet_status();
            cached.wifi = if cached.wifi_enabled {
                read_wifi_networks()
            } else {
                Vec::new()
            };
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

fn drain_network_commands(rx: &Receiver<NetworkCommand>) {
    while let Ok(cmd) = rx.try_recv() {
        match cmd {
            NetworkCommand::Scan => {
                spawn_nmcli(
                    vec!["dev".into(), "wifi".into(), "rescan".into()],
                    Duration::from_secs(10),
                );
            }
            NetworkCommand::Connect { ssid, password } => run_wifi_connect(ssid, password),
            NetworkCommand::SetRadio(on) => {
                let state = if on { "on" } else { "off" };
                spawn_nmcli(
                    vec!["radio".into(), "wifi".into(), state.into()],
                    Duration::from_secs(5),
                );
            }
        }
    }
}

fn queue_network(cmd: NetworkCommand) {
    if let Some(tx) = NETWORK_CMD_TX.get() {
        let _ = tx.send(cmd);
    }
}

/// Trigger a Wi-Fi rescan; refreshed results arrive on a later poll tick.
pub fn wifi_scan() {
    queue_network(NetworkCommand::Scan);
}

/// Connect to `ssid`, optionally with a password (for secured networks).
pub fn wifi_connect(ssid: String, password: Option<String>) {
    queue_network(NetworkCommand::Connect { ssid, password });
}

/// Enable or disable the Wi-Fi radio.
pub fn wifi_set_radio(on: bool) {
    queue_network(NetworkCommand::SetRadio(on));
}

fn run_wifi_connect(ssid: String, password: Option<String>) {
    let mut args = vec![
        "dev".to_string(),
        "wifi".to_string(),
        "connect".to_string(),
        ssid,
    ];
    if let Some(pw) = password {
        if !pw.is_empty() {
            args.push("password".to_string());
            args.push(pw);
        }
    }
    // Association + DHCP can take many seconds, well past the 600ms read budget,
    // so this runs detached. Success surfaces via the next snapshot's active SSID.
    run_wifi_connect_owned(args);
}

fn run_wifi_connect_owned(args: Vec<String>) {
    spawn_nmcli(args, Duration::from_secs(25));
}

/// Spawn an `nmcli` invocation on a detached thread, killing it after `timeout`.
fn spawn_nmcli(args: Vec<String>, timeout: Duration) {
    thread::spawn(move || {
        let mut cmd = std::process::Command::new("nmcli");
        cmd.args(&args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let Ok(mut child) = cmd.spawn() else {
            return;
        };
        let deadline = std::time::Instant::now() + timeout;
        loop {
            match child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => {}
                Err(_) => return,
            }
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return;
            }
            thread::sleep(Duration::from_millis(100));
        }
    });
}

/// Split a terse (`-t`) nmcli line into fields, honoring `\:` / `\\` escapes.
fn nmcli_split(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut cur = String::new();
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(next) = chars.next() {
                    cur.push(next);
                }
            }
            ':' => fields.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    fields.push(cur);
    fields
}

fn read_wifi_radio_enabled() -> bool {
    let mut cmd = std::process::Command::new("nmcli");
    cmd.args(["-t", "-f", "WIFI", "radio"]);
    run_command(&mut cmd)
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "enabled")
        .unwrap_or(true)
}

fn read_wifi_networks() -> Vec<WifiNetwork> {
    let mut cmd = std::process::Command::new("nmcli");
    cmd.args(["-t", "-f", "ACTIVE,SSID,SIGNAL,SECURITY", "dev", "wifi"]);
    let Some(output) = run_command(&mut cmd) else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let mut nets: Vec<WifiNetwork> = Vec::new();
    for line in text.lines() {
        let fields = nmcli_split(line);
        if fields.len() < 4 {
            continue;
        }
        let active = fields[0] == "yes";
        let ssid = fields[1].clone();
        if ssid.is_empty() {
            continue;
        }
        let signal = fields[2].parse().unwrap_or(0);
        let security = fields[3].trim();
        let secured = !security.is_empty() && security != "--";
        if let Some(existing) = nets.iter_mut().find(|n| n.ssid == ssid) {
            // Keep the strongest reading and remember if any BSS is active/secured.
            existing.active = existing.active || active;
            if signal > existing.signal {
                existing.signal = signal;
                existing.secured = secured;
            }
            continue;
        }
        nets.push(WifiNetwork {
            ssid,
            signal,
            secured,
            active,
        });
    }
    nets.sort_by(|a, b| b.active.cmp(&a.active).then(b.signal.cmp(&a.signal)));
    nets
}

fn read_ethernet_status() -> EthernetStatus {
    let mut cmd = std::process::Command::new("nmcli");
    cmd.args(["-t", "-f", "DEVICE,TYPE,STATE,CONNECTION", "dev", "status"]);
    if let Some(output) = run_command(&mut cmd) {
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            let fields = nmcli_split(line);
            if fields.len() < 4 || fields[1] != "ethernet" {
                continue;
            }
            let connected = fields[2].starts_with("connected");
            let label = if connected {
                let conn = fields[3].clone();
                if conn.is_empty() || conn == "--" {
                    "Connected".to_string()
                } else {
                    conn
                }
            } else {
                "Not connected".to_string()
            };
            return EthernetStatus {
                present: true,
                connected,
                label,
            };
        }
    }
    EthernetStatus::default()
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
