//! Thin `nmcli` wrappers for the Network settings page. Reads are blocking and
//! meant to run on a worker thread; mutations are fire-and-forget on a detached
//! thread (NetworkManager applies them and the next refresh reflects the result).

use std::process::Command;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq)]
pub struct WifiNet {
    pub ssid: String,
    pub signal: u8,
    pub secured: bool,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SavedConn {
    pub name: String,
    pub uuid: String,
    pub ctype: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EthDev {
    pub device: String,
    pub connection: Option<String>,
    pub connected: bool,
    pub ipv4: Ipv4,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Ipv4 {
    pub method: String,
    pub addresses: String,
    pub gateway: String,
    pub dns: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct NetSnapshot {
    pub wifi_enabled: bool,
    pub wifi: Vec<WifiNet>,
    pub saved: Vec<SavedConn>,
    pub eth: Vec<EthDev>,
}

/// Gather a full network snapshot (blocking — call on a worker thread).
pub fn load_snapshot() -> NetSnapshot {
    NetSnapshot {
        wifi_enabled: wifi_radio_enabled(),
        wifi: wifi_networks(),
        saved: saved_connections(),
        eth: ethernet_devices(),
    }
}

pub fn wifi_radio_enabled() -> bool {
    capture(&["-t", "-f", "WIFI", "radio"], Duration::from_secs(3))
        .map(|s| s.trim() == "enabled")
        .unwrap_or(true)
}

fn wifi_networks() -> Vec<WifiNet> {
    let Some(text) = capture(
        &["-t", "-f", "ACTIVE,SSID,SIGNAL,SECURITY", "dev", "wifi"],
        Duration::from_secs(6),
    ) else {
        return Vec::new();
    };
    let mut nets: Vec<WifiNet> = Vec::new();
    for line in text.lines() {
        let f = split(line);
        if f.len() < 4 {
            continue;
        }
        let ssid = f[1].clone();
        if ssid.is_empty() {
            continue;
        }
        let active = f[0] == "yes";
        let signal = f[2].parse().unwrap_or(0);
        let sec = f[3].trim();
        let secured = !sec.is_empty() && sec != "--";
        if let Some(e) = nets.iter_mut().find(|n| n.ssid == ssid) {
            e.active = e.active || active;
            if signal > e.signal {
                e.signal = signal;
                e.secured = secured;
            }
            continue;
        }
        nets.push(WifiNet {
            ssid,
            signal,
            secured,
            active,
        });
    }
    nets.sort_by(|a, b| b.active.cmp(&a.active).then(b.signal.cmp(&a.signal)));
    nets
}

pub fn saved_connections() -> Vec<SavedConn> {
    let Some(text) = capture(
        &["-t", "-f", "NAME,UUID,TYPE", "connection", "show"],
        Duration::from_secs(4),
    ) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| {
            let f = split(line);
            if f.len() < 3 || f[0].is_empty() {
                return None;
            }
            Some(SavedConn {
                name: f[0].clone(),
                uuid: f[1].clone(),
                ctype: f[2].clone(),
            })
        })
        .collect()
}

fn ethernet_devices() -> Vec<EthDev> {
    let Some(text) = capture(
        &["-t", "-f", "DEVICE,TYPE,STATE,CONNECTION", "dev", "status"],
        Duration::from_secs(4),
    ) else {
        return Vec::new();
    };
    let mut devs = Vec::new();
    for line in text.lines() {
        let f = split(line);
        if f.len() < 4 || f[1] != "ethernet" {
            continue;
        }
        let connected = f[2].starts_with("connected");
        let conn = f[3].clone();
        let connection = (!conn.is_empty() && conn != "--").then_some(conn);
        let ipv4 = connection
            .as_deref()
            .map(read_ipv4)
            .unwrap_or_default();
        devs.push(EthDev {
            device: f[0].clone(),
            connection,
            connected,
            ipv4,
        });
    }
    devs
}

/// Read the IPv4 config of a saved connection (blocking).
pub fn read_ipv4(conn: &str) -> Ipv4 {
    let Some(text) = capture(
        &[
            "-t",
            "-f",
            "ipv4.method,ipv4.addresses,ipv4.gateway,ipv4.dns",
            "connection",
            "show",
            conn,
        ],
        Duration::from_secs(4),
    ) else {
        return Ipv4::default();
    };
    let mut out = Ipv4::default();
    for line in text.lines() {
        let Some((key, val)) = line.split_once(':') else {
            continue;
        };
        let val = val.trim().to_string();
        match key.trim() {
            "ipv4.method" => out.method = val,
            "ipv4.addresses" => out.addresses = val,
            "ipv4.gateway" => out.gateway = val,
            "ipv4.dns" => out.dns = val,
            _ => {}
        }
    }
    out
}

pub fn connect_wifi(ssid: String, password: Option<String>) {
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
    detached(args, Duration::from_secs(30));
}

pub fn set_radio(on: bool) {
    detached(
        vec![
            "radio".into(),
            "wifi".into(),
            if on { "on" } else { "off" }.into(),
        ],
        Duration::from_secs(5),
    );
}

pub fn forget(target: &str) {
    detached(
        vec!["connection".into(), "delete".into(), target.into()],
        Duration::from_secs(8),
    );
}

pub fn set_ipv4_dhcp(conn: &str) {
    let conn = conn.to_string();
    std::thread::spawn(move || {
        let _ = run_blocking(&[
            "connection",
            "modify",
            &conn,
            "ipv4.method",
            "auto",
            "ipv4.addresses",
            "",
            "ipv4.gateway",
            "",
            "ipv4.dns",
            "",
        ]);
        let _ = run_blocking(&["connection", "up", &conn]);
    });
}

pub fn set_ipv4_static(conn: &str, address: &str, gateway: &str, dns: &str) {
    let (conn, address, gateway, dns) = (
        conn.to_string(),
        address.to_string(),
        gateway.to_string(),
        dns.to_string(),
    );
    std::thread::spawn(move || {
        let _ = run_blocking(&[
            "connection",
            "modify",
            &conn,
            "ipv4.method",
            "manual",
            "ipv4.addresses",
            &address,
            "ipv4.gateway",
            &gateway,
            "ipv4.dns",
            &dns,
        ]);
        let _ = run_blocking(&["connection", "up", &conn]);
    });
}

// ---- internals -----------------------------------------------------------

fn detached(args: Vec<String>, timeout: Duration) {
    std::thread::spawn(move || {
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let _ = run_with_timeout(&refs, timeout);
    });
}

fn run_blocking(args: &[&str]) -> Option<String> {
    run_with_timeout(args, Duration::from_secs(20))
}

fn capture(args: &[&str], timeout: Duration) -> Option<String> {
    run_with_timeout(args, timeout)
}

fn run_with_timeout(args: &[&str], timeout: Duration) -> Option<String> {
    let mut child = Command::new("nmcli")
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                return child
                    .wait_with_output()
                    .ok()
                    .map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
            }
            Ok(None) => {}
            Err(_) => return None,
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Split a terse (`-t`) nmcli line into fields, honoring `\:` / `\\` escapes.
fn split(line: &str) -> Vec<String> {
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
