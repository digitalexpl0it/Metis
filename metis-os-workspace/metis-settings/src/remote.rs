//! Desktop sharing status via the `metis-remote` CLI.

use std::io::Write;
use std::process::{Command, Stdio};

use gio::prelude::*;
use gtk::prelude::*;
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RemoteSnapshot {
    #[serde(default)]
    pub available: bool,
    #[serde(default)]
    pub running: bool,
    #[serde(default)]
    pub rdp_enabled: bool,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub password_set: bool,
    pub username: Option<String>,
    #[serde(default = "default_hostname")]
    pub hostname: String,
    #[serde(default)]
    pub addresses: Vec<String>,
    #[serde(default = "default_backend")]
    pub backend: String,
    #[serde(default)]
    pub config_enabled: bool,
    #[serde(default = "default_true")]
    pub lan_only: bool,
    #[serde(default)]
    pub firewall_applied: bool,
    #[serde(default)]
    pub firewall_backend: String,
    #[serde(default)]
    pub firewall_detail: Option<String>,
    pub error: Option<String>,
}

impl Default for RemoteSnapshot {
    fn default() -> Self {
        Self {
            available: false,
            running: false,
            rdp_enabled: false,
            port: default_port(),
            password_set: false,
            username: None,
            hostname: default_hostname(),
            addresses: Vec::new(),
            backend: default_backend(),
            config_enabled: false,
            lan_only: true,
            firewall_applied: false,
            firewall_backend: String::new(),
            firewall_detail: None,
            error: None,
        }
    }
}

fn default_port() -> u16 {
    3389
}

fn default_hostname() -> String {
    "localhost".into()
}

fn default_backend() -> String {
    "gnome-rdp".into()
}

fn default_true() -> bool {
    true
}

fn metis_remote_bin() -> String {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let sibling = dir.join("metis-remote");
            if sibling.is_file() {
                return sibling.to_string_lossy().into_owned();
            }
        }
    }
    "metis-remote".into()
}

fn run_remote(args: &[&str]) -> Result<String, String> {
    let bin = metis_remote_bin();
    let output = Command::new(&bin)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("failed to run {bin}: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if output.status.success() {
        Ok(stdout)
    } else {
        let msg = if stderr.trim().is_empty() {
            stdout.trim().to_string()
        } else {
            stderr.trim().to_string()
        };
        Err(if msg.is_empty() {
            format!("{bin} {} failed", args.join(" "))
        } else {
            msg
        })
    }
}

pub fn load_snapshot() -> RemoteSnapshot {
    match run_remote(&["status"]) {
        Ok(json) => {
            // Tolerate leading junk (e.g. accidental tool stdout) by parsing from
            // the first `{` — status is always a single JSON object.
            let trimmed = json.trim();
            let payload = trimmed.find('{').map(|i| &trimmed[i..]).unwrap_or(trimmed);
            serde_json::from_str(payload).unwrap_or_else(|err| RemoteSnapshot {
                error: Some(format!("Failed to parse metis-remote status: {err}")),
                ..RemoteSnapshot::default()
            })
        }
        Err(err) => RemoteSnapshot {
            error: Some(err),
            port: default_port(),
            hostname: default_hostname(),
            backend: default_backend(),
            lan_only: true,
            ..Default::default()
        },
    }
}

pub fn enable_sharing() -> Result<(), String> {
    run_remote(&["enable"]).map(|_| ())
}

pub fn disable_sharing() -> Result<(), String> {
    run_remote(&["disable"]).map(|_| ())
}

pub fn set_lan_only(lan_only: bool) -> Result<(), String> {
    let flag = if lan_only { "true" } else { "false" };
    run_remote(&["set-lan-only", flag]).map(|_| ())
}

/// Apply LAN-only firewall rules (may show a PolicyKit password dialog).
pub fn apply_firewall() -> Result<(), String> {
    run_remote(&["firewall", "apply"]).map(|_| ())
}

/// Set RDP credentials. Password is piped on stdin — never placed on argv.
pub fn set_credentials(username: &str, password: &str) -> Result<(), String> {
    let bin = metis_remote_bin();
    let mut child = Command::new(&bin)
        .args(["set-credentials", username])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to run {bin}: {e}"))?;
    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| format!("{bin}: missing stdin pipe"))?;
        stdin
            .write_all(password.as_bytes())
            .and_then(|_| stdin.write_all(b"\n"))
            .map_err(|e| format!("write password to {bin}: {e}"))?;
    }
    let output = child
        .wait_with_output()
        .map_err(|e| format!("{bin} wait: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let msg = if !stderr.trim().is_empty() {
            stderr.trim().to_string()
        } else {
            stdout.trim().to_string()
        };
        Err(if msg.is_empty() {
            format!("{bin} set-credentials failed")
        } else {
            msg
        })
    }
}

/// Zeroize an owned password string after credentials were submitted.
pub fn scrub_password(password: &mut String) {
    password.zeroize();
}

pub fn connection_hint(snap: &RemoteSnapshot) -> String {
    let host = snap
        .addresses
        .first()
        .cloned()
        .unwrap_or_else(|| snap.hostname.clone());
    format!("{}:{}", host, snap.port)
}

/// Desktop notification for sharing state changes.
///
/// Uses `notify-send` so Metis's `org.freedesktop.Notifications` daemon (Notification
/// Center) receives the message. GTK `Application::send_notification` alone often
/// never reaches the shell's NC.
pub fn notify_sharing(title: &str, body: &str) {
    let sent = Command::new("notify-send")
        .args([
            "-a",
            "Metis",
            "-u",
            "normal",
            "--hint=string:desktop-entry:metis-settings",
            title,
            body,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if sent {
        return;
    }
    // Fallback when notify-send is missing.
    let app = gtk::Application::default();
    let note = gio::Notification::new(title);
    note.set_body(Some(body));
    note.set_priority(gio::NotificationPriority::Normal);
    app.send_notification(Some("metis-remote-sharing"), &note);
}
