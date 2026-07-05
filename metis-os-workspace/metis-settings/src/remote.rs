//! Desktop sharing status via the `metis-remote` CLI.

use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
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
    pub error: Option<String>,
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
        Ok(json) => serde_json::from_str(&json).unwrap_or_else(|_| RemoteSnapshot {
            error: Some("Failed to parse metis-remote status".into()),
            ..RemoteSnapshot::default()
        }),
        Err(err) => RemoteSnapshot {
            error: Some(err),
            port: default_port(),
            hostname: default_hostname(),
            backend: default_backend(),
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

pub fn set_credentials(username: &str, password: &str) -> Result<(), String> {
    run_remote(&["set-credentials", username, password]).map(|_| ())
}

pub fn connection_hint(snap: &RemoteSnapshot) -> String {
    let host = snap
        .addresses
        .first()
        .cloned()
        .unwrap_or_else(|| snap.hostname.clone());
    format!("{}:{}", host, snap.port)
}
