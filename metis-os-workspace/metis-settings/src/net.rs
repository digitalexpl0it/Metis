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
    /// When true (NM `ipv4.ignore-auto-dns`), DHCP-provided DNS is ignored and
    /// only the manual `dns` list is used — i.e. a DNS override on an otherwise
    /// automatic connection.
    pub ignore_auto_dns: bool,
}

/// An active connection profile (e.g. the currently-joined Wi-Fi), with its
/// editable IPv4 config so the UI can offer a DNS override.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ActiveConn {
    pub name: String,
    pub uuid: String,
    pub ipv4: Ipv4,
}

/// System proxy configuration, mirrored to/from GNOME's `org.gnome.system.proxy`
/// gsettings (honoured by GLib/GTK apps via the default `GProxyResolver`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProxyConfig {
    /// "none", "manual", or "auto".
    pub mode: String,
    pub auto_url: String,
    pub http_host: String,
    pub http_port: u32,
    pub https_host: String,
    pub https_port: u32,
    pub socks_host: String,
    pub socks_port: u32,
    /// Comma-separated no-proxy host list.
    pub ignore_hosts: String,
    /// True when the gsettings proxy schema is available on this system.
    pub available: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct NetSnapshot {
    pub wifi_enabled: bool,
    pub wifi: Vec<WifiNet>,
    pub saved: Vec<SavedConn>,
    pub eth: Vec<EthDev>,
    /// The active Wi-Fi connection profile, if any (for the DNS override UI).
    pub active_wifi: Option<ActiveConn>,
    pub proxy: ProxyConfig,
}

/// Gather a full network snapshot (blocking — call on a worker thread).
pub fn load_snapshot() -> NetSnapshot {
    NetSnapshot {
        wifi_enabled: wifi_radio_enabled(),
        wifi: wifi_networks(),
        saved: saved_connections(),
        eth: ethernet_devices(),
        active_wifi: active_wifi_connection(),
        proxy: read_proxy(),
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
            "ipv4.method,ipv4.addresses,ipv4.gateway,ipv4.dns,ipv4.ignore-auto-dns",
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
            "ipv4.ignore-auto-dns" => out.ignore_auto_dns = val == "yes" || val == "true",
            _ => {}
        }
    }
    out
}

/// The currently-active Wi-Fi connection profile (name + uuid + IPv4), used to
/// offer a DNS override on the joined network. `None` when no Wi-Fi is up.
pub fn active_wifi_connection() -> Option<ActiveConn> {
    let text = capture(
        &[
            "-t",
            "-f",
            "NAME,UUID,TYPE",
            "connection",
            "show",
            "--active",
        ],
        Duration::from_secs(4),
    )?;
    for line in text.lines() {
        let f = split(line);
        if f.len() < 3 {
            continue;
        }
        if f[2].contains("wireless") {
            let name = f[0].clone();
            let ipv4 = read_ipv4(&name);
            return Some(ActiveConn {
                name,
                uuid: f[1].clone(),
                ipv4,
            });
        }
    }
    None
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

/// Switch a connection to automatic (DHCP). `dns_override` (comma-separated) is
/// applied as a manual DNS list with `ignore-auto-dns yes` so it overrides the
/// DHCP-provided servers; pass an empty string to use the DHCP DNS.
pub fn set_ipv4_dhcp(conn: &str, dns_override: &str) {
    let conn = conn.to_string();
    let dns = dns_override.trim().to_string();
    std::thread::spawn(move || {
        let ignore = if dns.is_empty() { "no" } else { "yes" };
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
            &dns,
            "ipv4.ignore-auto-dns",
            ignore,
        ]);
        let _ = run_blocking(&["connection", "up", &conn]);
    });
}

/// Apply just a DNS override to a connection, preserving its current IPv4 method
/// (auto or manual). Empty `dns` clears the override and re-enables DHCP DNS.
pub fn set_dns_override(conn: &str, dns: &str) {
    let conn = conn.to_string();
    let dns = dns.trim().to_string();
    std::thread::spawn(move || {
        let ignore = if dns.is_empty() { "no" } else { "yes" };
        let _ = run_blocking(&[
            "connection",
            "modify",
            &conn,
            "ipv4.dns",
            &dns,
            "ipv4.ignore-auto-dns",
            ignore,
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

// ---- proxy (GNOME gsettings) ---------------------------------------------

const PROXY_SCHEMA: &str = "org.gnome.system.proxy";

/// Read the system proxy config from gsettings. `available` is false when the
/// schema isn't installed (then the Proxy tab shows a hint instead of controls).
pub fn read_proxy() -> ProxyConfig {
    let Some(mode) = gsettings_get(PROXY_SCHEMA, "mode") else {
        return ProxyConfig::default();
    };
    ProxyConfig {
        mode: unquote(&mode),
        auto_url: gsettings_get(PROXY_SCHEMA, "autoconfig-url")
            .map(|s| unquote(&s))
            .unwrap_or_default(),
        ignore_hosts: gsettings_get(PROXY_SCHEMA, "ignore-hosts")
            .map(|s| parse_str_array(&s))
            .unwrap_or_default(),
        http_host: proxy_host("http"),
        http_port: proxy_port("http"),
        https_host: proxy_host("https"),
        https_port: proxy_port("https"),
        socks_host: proxy_host("socks"),
        socks_port: proxy_port("socks"),
        available: true,
    }
}

fn proxy_host(kind: &str) -> String {
    gsettings_get(&format!("{PROXY_SCHEMA}.{kind}"), "host")
        .map(|s| unquote(&s))
        .unwrap_or_default()
}

fn proxy_port(kind: &str) -> u32 {
    gsettings_get(&format!("{PROXY_SCHEMA}.{kind}"), "port")
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

/// Write the proxy config to gsettings (fire-and-forget on a worker thread).
pub fn set_proxy(cfg: ProxyConfig) {
    std::thread::spawn(move || {
        let _ = gsettings_set(PROXY_SCHEMA, "mode", &cfg.mode);
        let _ = gsettings_set(PROXY_SCHEMA, "autoconfig-url", &cfg.auto_url);
        let _ = gsettings_set(
            PROXY_SCHEMA,
            "ignore-hosts",
            &to_str_array(&cfg.ignore_hosts),
        );
        for (kind, host, port) in [
            ("http", &cfg.http_host, cfg.http_port),
            ("https", &cfg.https_host, cfg.https_port),
            ("socks", &cfg.socks_host, cfg.socks_port),
        ] {
            let schema = format!("{PROXY_SCHEMA}.{kind}");
            let _ = gsettings_set(&schema, "host", host);
            let _ = gsettings_set(&schema, "port", &port.to_string());
        }
    });
}

fn gsettings_get(schema: &str, key: &str) -> Option<String> {
    let out = Command::new("gsettings")
        .args(["get", schema, key])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn gsettings_set(schema: &str, key: &str, value: &str) -> Option<()> {
    let status = Command::new("gsettings")
        .args(["set", schema, key, value])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()?;
    status.success().then_some(())
}

/// Strip the surrounding single quotes gsettings wraps string values in.
fn unquote(s: &str) -> String {
    s.trim().trim_matches('\'').to_string()
}

/// Parse a gsettings string array (`['a', 'b']`) into a comma-separated list.
fn parse_str_array(s: &str) -> String {
    let inner = s.trim().trim_start_matches('[').trim_end_matches(']');
    inner
        .split(',')
        .map(|p| p.trim().trim_matches('\'').trim())
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Render a comma-separated list as a gsettings string array literal.
fn to_str_array(csv: &str) -> String {
    let items: Vec<String> = csv
        .split(',')
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .map(|p| format!("'{p}'"))
        .collect();
    format!("[{}]", items.join(", "))
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
