use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

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

/// A single connected Bluetooth device, with battery level when the device
/// reports one (mice, keyboards, headsets via the BlueZ `Battery1` interface).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BluetoothDevice {
    /// Device MAC address (`AA:BB:CC:DD:EE:FF`) — stable key for battery alerts.
    pub address: String,
    pub name: String,
    /// Battery charge 0–100 when the device exposes it, else `None`.
    pub battery_percent: Option<u8>,
    /// Charging state when the source reports it (kernel HID `status` or UPower
    /// `state`). `None` means unknown — most devices over plain Bluetooth cannot
    /// signal charging, since the BT Battery Service has no such characteristic.
    pub battery_charging: Option<bool>,
}

/// Bluetooth adapter status for the conditional bar indicator.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BluetoothStatus {
    /// Whether a Bluetooth adapter exists at all (widget hidden when false).
    pub adapter_present: bool,
    pub powered: bool,
    pub connected: bool,
    /// First connected device's name, kept for the compact bar tooltip.
    pub device_name: Option<String>,
    /// All currently-connected devices (with battery where reported).
    pub devices: Vec<BluetoothDevice>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct BarSnapshot {
    pub battery_percent: Option<u8>,
    pub battery_charging: bool,
    pub bluetooth: BluetoothStatus,
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
    let mut wifi_scan_grace_until: Option<std::time::Instant> = None;

    loop {
        // A user audio action (slider/mute/scroll) forces an immediate volume
        // read-back below so *every* bar reflects the change within one poll cycle
        // instead of waiting for the next 800ms volume tick — without this, bars
        // other than the one whose popup is open lagged by several seconds.
        let audio_changed = drain_audio_commands(&audio_rx);
        drain_network_commands(&network_rx, &mut wifi_scan_grace_until);

        if tick % 15 == 0 {
            cached.battery_percent = read_battery_percent();
            cached.battery_charging = read_battery_charging();
            cached.bluetooth = read_bluetooth_status();
        }
        if tick % 3 == 0 {
            cached.wifi_enabled = read_wifi_radio_enabled();
            if let Some(eth) = read_ethernet_status() {
                cached.ethernet = eth;
            }
            if cached.wifi_enabled {
                if let Some(networks) = read_wifi_networks() {
                    let in_scan_grace = wifi_scan_grace_until
                        .is_some_and(|until| std::time::Instant::now() < until);
                    cached.wifi = stabilize_wifi_list(&cached.wifi, networks, in_scan_grace);
                }
                // On nmcli timeout keep the last good Wi-Fi list so the bar icon
                // does not flash offline between poll ticks.
            } else {
                cached.wifi.clear();
            }
        }
        if tick % 2 == 0 || audio_changed {
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

/// Returns true if at least one audio command was handled, so the caller can
/// force an immediate volume read-back (fast cross-bar feedback).
fn drain_audio_commands(rx: &Receiver<AudioCommand>) -> bool {
    let mut handled = false;
    while let Ok(cmd) = rx.try_recv() {
        handled = true;
        match cmd {
            AudioCommand::SetVolumeAbsolute(pct) => run_set_volume_absolute(pct),
            AudioCommand::SetVolumeRelative(delta) => run_set_volume_relative(delta),
            AudioCommand::SetMute(muted) => run_set_mute(muted),
            AudioCommand::SetMicVolumeAbsolute(pct) => run_set_mic_volume_absolute(pct),
            AudioCommand::SetMicMute(muted) => run_set_mic_mute(muted),
        }
    }
    handled
}

fn queue_audio(cmd: AudioCommand) {
    if let Some(tx) = AUDIO_CMD_TX.get() {
        let _ = tx.send(cmd);
    }
}

fn drain_network_commands(
    rx: &Receiver<NetworkCommand>,
    wifi_scan_grace_until: &mut Option<std::time::Instant>,
) {
    while let Ok(cmd) = rx.try_recv() {
        match cmd {
            NetworkCommand::Scan => {
                *wifi_scan_grace_until =
                    Some(std::time::Instant::now() + Duration::from_secs(4));
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

/// Enable or disable the Bluetooth adapter radio.
pub fn bluetooth_set_powered(on: bool) {
    let state = if on { "on" } else { "off" };
    spawn_bluetoothctl(vec!["power".into(), state.into()], Duration::from_secs(8));
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

/// Spawn a `bluetoothctl` invocation on a detached thread.
fn spawn_bluetoothctl(args: Vec<String>, timeout: Duration) {
    thread::spawn(move || {
        let mut cmd = std::process::Command::new("bluetoothctl");
        cmd.args(&args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let Ok(mut child) = cmd.spawn() else {
            return;
        };
        let deadline = Instant::now() + timeout;
        loop {
            match child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => {}
                Err(_) => return,
            }
            if Instant::now() >= deadline {
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

fn read_wifi_networks() -> Option<Vec<WifiNetwork>> {
    let mut cmd = std::process::Command::new("nmcli");
    cmd.args(["-t", "-f", "ACTIVE,SSID,SIGNAL,SECURITY", "dev", "wifi"]);
    let output = run_command_with_timeout(&mut cmd, Duration::from_millis(2_000))?;
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
    Some(nets)
}

/// During an nmcli rescan the active SSID can briefly disappear or report 0 signal.
/// Hold the last known connection so the bar icon does not flicker offline.
fn stabilize_wifi_list(
    previous: &[WifiNetwork],
    mut networks: Vec<WifiNetwork>,
    in_scan_grace: bool,
) -> Vec<WifiNetwork> {
    let prev_active = previous.iter().find(|n| n.active);

    if networks.is_empty() {
        if prev_active.is_some() {
            return previous.to_vec();
        }
        return networks;
    }

    if !in_scan_grace {
        return networks;
    }

    let Some(prev_active) = prev_active else {
        return networks;
    };
    if networks.iter().any(|n| n.active) {
        return networks;
    }
    if let Some(current) = networks.iter_mut().find(|n| n.ssid == prev_active.ssid) {
        current.active = true;
        if current.signal == 0 {
            current.signal = prev_active.signal;
        }
        networks.sort_by(|a, b| b.active.cmp(&a.active).then(b.signal.cmp(&a.signal)));
        return networks;
    }
    networks.push(WifiNetwork {
        ssid: prev_active.ssid.clone(),
        signal: prev_active.signal,
        secured: prev_active.secured,
        active: true,
    });
    networks.sort_by(|a, b| b.active.cmp(&a.active).then(b.signal.cmp(&a.signal)));
    networks
}

fn read_ethernet_status() -> Option<EthernetStatus> {
    let mut cmd = std::process::Command::new("nmcli");
    cmd.args(["-t", "-f", "DEVICE,TYPE,STATE,CONNECTION", "dev", "status"]);
    let output = run_command(&mut cmd)?;
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
        return Some(EthernetStatus {
            present: true,
            connected,
            label,
        });
    }
    Some(EthernetStatus::default())
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

fn read_bluetooth_status() -> BluetoothStatus {
    if !bluetooth_adapter_present() {
        return BluetoothStatus::default();
    }

    let mut cmd = std::process::Command::new("bluetoothctl");
    cmd.args(["show"]);
    let Some(output) = run_command(&mut cmd) else {
        return BluetoothStatus {
            adapter_present: true,
            ..BluetoothStatus::default()
        };
    };
    let text = String::from_utf8_lossy(&output.stdout);
    if !text.contains("Controller") || text.contains("No default controller") {
        return BluetoothStatus::default();
    }
    let powered = text.contains("Powered: yes");
    let mut devices = Vec::new();
    if powered {
        let mut devices_cmd = std::process::Command::new("bluetoothctl");
        devices_cmd.args(["devices", "Connected"]);
        if let Some(o) = run_command(&mut devices_cmd) {
            // Enumerate UPower once per poll so each device is a cheap map lookup
            // rather than a fresh `upower -i` spawn.
            let upower = read_upower_bt_batteries();
            let dev_text = String::from_utf8_lossy(&o.stdout);
            for line in dev_text.lines() {
                if let Some(rest) = line.strip_prefix("Device ") {
                    let mut parts = rest.splitn(2, ' ');
                    let Some(address) = parts.next() else {
                        continue;
                    };
                    let name = parts.next().unwrap_or(address).to_string();
                    let battery = read_bluetooth_device_battery(address, &upower);
                    devices.push(BluetoothDevice {
                        address: address.to_string(),
                        name,
                        battery_percent: battery.percent,
                        battery_charging: battery.charging,
                    });
                }
            }
        }
        apply_solaar_overrides(&mut devices);
    }
    let device_name = devices.first().map(|d| d.name.clone());
    BluetoothStatus {
        adapter_present: true,
        powered,
        connected: !devices.is_empty(),
        device_name,
        devices,
    }
}

/// Battery reading for a single peripheral: charge level plus charging state
/// when the underlying source can report it.
#[derive(Debug, Clone, Copy, Default)]
struct DeviceBattery {
    /// Charge 0–100, or `None` when no source reports a level.
    percent: Option<u8>,
    /// `Some(true/false)` when charging is known, `None` when unknown.
    charging: Option<bool>,
}

/// Read a connected device's battery (and charging state, where available).
///
/// Source priority, most→least accurate:
/// 1. Kernel HID battery (`/sys/class/power_supply/hid-<mac>-battery`) — exposes
///    `capacity` and a `status` we can map to charging; no extra BlueZ config.
/// 2. UPower — aggregates peripheral batteries and reports a `state` (charging /
///    discharging / fully-charged) for devices/drivers that support it.
/// 3. BlueZ `Battery1` via `bluetoothctl info` ("Battery Percentage: 0xNN (NN)")
///    — percentage only; the BT Battery Service has no charging characteristic.
fn read_bluetooth_device_battery(
    address: &str,
    upower: &HashMap<String, DeviceBattery>,
) -> DeviceBattery {
    if let Some(batt) = read_hid_battery_for_address(address) {
        return batt;
    }
    if let Some(batt) = upower.get(&address.to_ascii_uppercase()) {
        if batt.percent.is_some() {
            return *batt;
        }
    }
    let mut cmd = std::process::Command::new("bluetoothctl");
    cmd.args(["info", address]);
    let percent = run_command(&mut cmd).and_then(|output| {
        let text = String::from_utf8_lossy(&output.stdout);
        text.lines().find_map(|line| {
            line.trim()
                .strip_prefix("Battery Percentage:")
                .and_then(parse_battery_percentage)
        })
    });
    DeviceBattery {
        percent,
        charging: None,
    }
}

/// Enumerate UPower peripheral batteries once, keyed by uppercased MAC.
///
/// UPower device paths embed the address as `…_dev_AA_BB_CC_DD_EE_FF`; we use
/// that to filter to Bluetooth/peripheral devices (skipping the laptop battery,
/// AC line, and DisplayDevice) and to map each back to its BlueZ address.
fn read_upower_bt_batteries() -> HashMap<String, DeviceBattery> {
    let mut map = HashMap::new();
    let mut enum_cmd = std::process::Command::new("upower");
    enum_cmd.arg("-e");
    let Some(output) = run_command(&mut enum_cmd) else {
        return map;
    };
    let text = String::from_utf8_lossy(&output.stdout);
    for path in text.lines() {
        let path = path.trim();
        let Some(idx) = path.find("_dev_") else {
            continue;
        };
        let mac = path[idx + "_dev_".len()..]
            .replace('_', ":")
            .to_ascii_uppercase();
        if !mac.contains(':') {
            continue;
        }
        if let Some(batt) = read_upower_device(path) {
            map.insert(mac, batt);
        }
    }
    map
}

/// Parse `percentage:`/`state:` out of `upower -i <path>` for one device.
fn read_upower_device(path: &str) -> Option<DeviceBattery> {
    let mut cmd = std::process::Command::new("upower");
    cmd.args(["-i", path]);
    let output = run_command(&mut cmd)?;
    let text = String::from_utf8_lossy(&output.stdout);
    let mut batt = DeviceBattery::default();
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("percentage:") {
            if let Ok(v) = rest.trim().trim_end_matches('%').parse::<f32>() {
                batt.percent = Some(v.round().clamp(0.0, 100.0) as u8);
            }
        } else if let Some(rest) = line.strip_prefix("state:") {
            batt.charging = match rest.trim() {
                "charging" | "fully-charged" => Some(true),
                "discharging" | "pending-discharge" => Some(false),
                // "unknown" / "pending-charge" carry no reliable signal.
                _ => None,
            };
        }
    }
    (batt.percent.is_some() || batt.charging.is_some()).then_some(batt)
}

/// Cached Solaar (`solaar show`) battery readings, keyed by lowercased device
/// name. Solaar talks Logitech HID++ directly and is the only source that knows
/// charging state for Logitech peripherals over Bluetooth — but `solaar show`
/// takes ~2s, so we cache it and refresh off the poll thread.
struct SolaarCache {
    fetched_at: Option<Instant>,
    refreshing: bool,
    by_name: HashMap<String, DeviceBattery>,
}

static SOLAAR_CACHE: OnceLock<Mutex<SolaarCache>> = OnceLock::new();

/// How long a cached Solaar reading stays fresh before a background refresh.
const SOLAAR_TTL: Duration = Duration::from_secs(20);

/// Overlay Solaar's charging state (and level when BlueZ/UPower lacked one) onto
/// connected devices, matched by name. Best-effort: a no-op if Solaar is absent.
fn apply_solaar_overrides(devices: &mut [BluetoothDevice]) {
    if devices.is_empty() {
        return;
    }
    let solaar = solaar_overrides();
    if solaar.is_empty() {
        return;
    }
    for dev in devices.iter_mut() {
        let key = dev.name.to_ascii_lowercase();
        let matched = solaar.iter().find(|(name, _)| {
            // Solaar's name ("Wireless Mobile Mouse MX Anywhere 2S") is usually a
            // superset of the BlueZ name ("MX Anywhere 2S"); accept either way.
            name.contains(&key) || key.contains(name.as_str())
        });
        if let Some((_, batt)) = matched {
            if batt.charging.is_some() {
                dev.battery_charging = batt.charging;
            }
            if dev.battery_percent.is_none() {
                dev.battery_percent = batt.percent;
            }
        }
    }
}

/// Return the cached Solaar map, kicking off a background refresh when stale.
fn solaar_overrides() -> HashMap<String, DeviceBattery> {
    let cache = SOLAAR_CACHE.get_or_init(|| {
        Mutex::new(SolaarCache {
            fetched_at: None,
            refreshing: false,
            by_name: HashMap::new(),
        })
    });
    let Ok(mut guard) = cache.lock() else {
        return HashMap::new();
    };
    let stale = guard.fetched_at.is_none_or(|at| at.elapsed() >= SOLAAR_TTL);
    if stale && !guard.refreshing {
        guard.refreshing = true;
        thread::spawn(move || {
            let map = read_solaar_batteries();
            if let Ok(mut guard) = cache.lock() {
                guard.by_name = map;
                guard.fetched_at = Some(Instant::now());
                guard.refreshing = false;
            }
        });
    }
    guard.by_name.clone()
}

/// Run `solaar show` and parse each device's battery line, keyed by lowercased
/// name. Returns empty if Solaar isn't installed or times out.
fn read_solaar_batteries() -> HashMap<String, DeviceBattery> {
    let mut map = HashMap::new();
    let mut cmd = std::process::Command::new("solaar");
    cmd.arg("show");
    let Some(output) = run_command_with_timeout(&mut cmd, Duration::from_secs(4)) else {
        return map;
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let mut current: Option<String> = None;
    for line in text.lines() {
        // Device blocks start with a column-0, non-empty header line.
        let is_header = !line.is_empty()
            && !line.starts_with(char::is_whitespace)
            && !line.starts_with("solaar version");
        if is_header {
            current = Some(line.trim().to_ascii_lowercase());
            continue;
        }
        if let Some(rest) = line.trim().strip_prefix("Battery:") {
            if let Some(name) = &current {
                map.insert(name.clone(), parse_solaar_battery(rest));
            }
        }
    }
    map
}

/// Parse a Solaar battery line body like ` N/A, full, next level 0%.` or
/// ` 90%, discharging, next level 50%.` into level + charging state.
fn parse_solaar_battery(rest: &str) -> DeviceBattery {
    let mut batt = DeviceBattery::default();
    let mut segs = rest.split(',');
    if let Some(level) = segs.next() {
        let level = level.trim().trim_end_matches('.');
        if let Some(pct) = level.strip_suffix('%') {
            if let Ok(v) = pct.trim().parse::<f32>() {
                batt.percent = Some(v.round().clamp(0.0, 100.0) as u8);
            }
        }
    }
    if let Some(status) = segs.next() {
        let status = status.trim().to_ascii_lowercase();
        // Order matters: "discharging" contains "charging".
        batt.charging = if status.contains("discharg") {
            Some(false)
        } else if status.contains("recharg") || status.contains("charging") || status == "full" {
            // HID++ reports "full" only while topped up on a charger.
            Some(true)
        } else {
            None
        };
    }
    batt
}

/// Parse a BlueZ battery field like `0x40 (64)` — prefer the decimal in parens,
/// falling back to decoding the `0xNN` hex value.
fn parse_battery_percentage(field: &str) -> Option<u8> {
    let field = field.trim();
    if let (Some(start), Some(end)) = (field.find('('), field.find(')')) {
        if start < end {
            if let Ok(v) = field[start + 1..end].trim().parse::<u16>() {
                return Some(v.min(100) as u8);
            }
        }
    }
    let token = field.split_whitespace().next()?;
    let hex = token.strip_prefix("0x").or_else(|| token.strip_prefix("0X"))?;
    u16::from_str_radix(hex, 16).ok().map(|v| v.min(100) as u8)
}

/// Match a BlueZ MAC against kernel HID power-supply entries, whose names embed
/// the address as `hid-AA:BB:CC:DD:EE:FF-battery` (case-insensitive). Reads the
/// `capacity` and maps `status` (Charging/Full/Discharging) to a charging flag.
fn read_hid_battery_for_address(address: &str) -> Option<DeviceBattery> {
    let target = address.to_ascii_lowercase();
    let entries = std::fs::read_dir("/sys/class/power_supply").ok()?;
    for entry in entries.filter_map(Result::ok) {
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy().to_ascii_lowercase();
        if name.starts_with("hid-") && name.contains(&target) {
            let cap = std::fs::read_to_string(entry.path().join("capacity")).ok()?;
            let percent = cap.trim().parse::<u16>().ok().map(|v| v.min(100) as u8)?;
            let charging = match std::fs::read_to_string(entry.path().join("status")) {
                Ok(s) => match s.trim().to_ascii_lowercase().as_str() {
                    "charging" | "full" => Some(true),
                    "discharging" | "not charging" => Some(false),
                    _ => None,
                },
                Err(_) => None,
            };
            return Some(DeviceBattery {
                percent: Some(percent),
                charging,
            });
        }
    }
    None
}

fn bluetooth_adapter_present() -> bool {
    std::fs::read_dir("/sys/class/bluetooth")
        .map(|entries| {
            entries.filter_map(Result::ok).any(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("hci")
            })
        })
        .unwrap_or(false)
}

fn run_command(cmd: &mut std::process::Command) -> Option<std::process::Output> {
    run_command_with_timeout(cmd, Duration::from_millis(600))
}

fn run_command_with_timeout(
    cmd: &mut std::process::Command,
    timeout: Duration,
) -> Option<std::process::Output> {
    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    let mut child = cmd.spawn().ok()?;
    let deadline = std::time::Instant::now() + timeout;
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
