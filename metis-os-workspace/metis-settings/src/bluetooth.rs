//! Bluetooth adapter and device management via `bluetoothctl` (BlueZ).

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceState {
    Connected,
    Paired,
    Available,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BtDevice {
    pub address: String,
    pub name: String,
    pub state: DeviceState,
    pub battery_percent: Option<u8>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct BluetoothSnapshot {
    pub adapter_present: bool,
    pub powered: bool,
    pub discovering: bool,
    pub adapter_name: String,
    pub devices: Vec<BtDevice>,
}

pub fn load_snapshot() -> BluetoothSnapshot {
    let show = btctl(&["show"]);
    let adapter_present = show.contains("Controller") && !show.contains("No default controller");
    if !adapter_present {
        return BluetoothSnapshot::default();
    }
    let powered = show.contains("Powered: yes");
    let discovering = show.contains("Discovering: yes");
    let adapter_name = parse_field(&show, "Alias:").unwrap_or_else(|| "Bluetooth".into());
    let devices = list_devices();
    BluetoothSnapshot {
        adapter_present: true,
        powered,
        discovering,
        adapter_name,
        devices,
    }
}

pub fn set_powered(on: bool) {
    let _ = btctl(&["power", if on { "on" } else { "off" }]);
}

pub fn start_scan() {
    let _ = btctl(&["scan", "on"]);
}

pub fn stop_scan() {
    let _ = btctl(&["scan", "off"]);
}

pub fn pair_and_connect(address: &str) {
    let _ = btctl(&["pair", address]);
    let _ = btctl(&["trust", address]);
    let _ = btctl(&["connect", address]);
}

pub fn disconnect(address: &str) {
    let _ = btctl(&["disconnect", address]);
}

pub fn remove_device(address: &str) {
    let _ = btctl(&["remove", address]);
}

fn list_devices() -> Vec<BtDevice> {
    let text = btctl(&["devices"]);
    let mut devices = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("Device ") else {
            continue;
        };
        let mut parts = rest.splitn(2, ' ');
        let address = parts.next().unwrap_or("").to_string();
        let name = parts.next().unwrap_or(&address).to_string();
        if address.is_empty() {
            continue;
        }
        let info = btctl(&["info", &address]);
        let state = if info.contains("Connected: yes") {
            DeviceState::Connected
        } else if info.contains("Paired: yes") {
            DeviceState::Paired
        } else {
            DeviceState::Available
        };
        let battery_percent = parse_field(&info, "Battery Percentage:")
            .and_then(|s| s.trim_end_matches('%').parse().ok());
        devices.push(BtDevice {
            address,
            name,
            state,
            battery_percent,
        });
    }
    devices.sort_by(|a, b| {
        fn rank(s: &DeviceState) -> u8 {
            match s {
                DeviceState::Connected => 0,
                DeviceState::Paired => 1,
                DeviceState::Available => 2,
            }
        }
        rank(&a.state)
            .cmp(&rank(&b.state))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    devices
}

fn parse_field(text: &str, key: &str) -> Option<String> {
    text.lines()
        .find(|l| l.trim_start().starts_with(key))
        .map(|l| l.split_once(key).map(|(_, v)| v.trim().to_string()))
        .flatten()
        .filter(|s| !s.is_empty())
}

fn btctl(args: &[&str]) -> String {
    let mut cmd = Command::new("bluetoothctl");
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::null());
    let Ok(mut child) = cmd.spawn() else {
        return String::new();
    };
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                return child
                    .wait_with_output()
                    .ok()
                    .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                    .unwrap_or_default();
            }
            Ok(None) => {}
            Err(_) => return String::new(),
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return String::new();
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
