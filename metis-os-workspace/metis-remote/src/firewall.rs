//! LAN-only firewall helpers for RDP port 3389.
//!
//! When `remote.json` has `lan_only: true`, Metis applies an idempotent nftables
//! (preferred) or ufw rule set that accepts TCP 3389 only from private /
//! loopback / link-local ranges and drops other inbound traffic to that port.

use std::process::{Command, Output};

const NFT_TABLE: &str = "metis_rdp";
const UFW_COMMENT: &str = "metis-rdp-lan-only";
const PORT: u16 = 3389;

const LAN_V4: &[&str] = &[
    "10.0.0.0/8",
    "172.16.0.0/12",
    "192.168.0.0/16",
    "127.0.0.0/8",
];
const LAN_V6: &[&str] = &["fe80::/10", "::1/128"];

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Default)]
pub struct FirewallStatus {
    /// Whether Metis believes LAN-only rules are present.
    pub applied: bool,
    /// Backend that owns the rules: `nft`, `ufw`, or empty when none.
    pub backend: String,
    /// Human-readable detail / last error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

fn run(cmd: &str, args: &[&str]) -> Result<Output, String> {
    Command::new(cmd)
        .args(args)
        .output()
        .map_err(|e| format!("failed to run {cmd}: {e}"))
}

fn is_root() -> bool {
    Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim() == "0")
        .unwrap_or(false)
}

fn nft_available() -> bool {
    Command::new("sh")
        .args(["-c", "command -v nft >/dev/null 2>&1"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn ufw_available() -> bool {
    Command::new("sh")
        .args(["-c", "command -v ufw >/dev/null 2>&1"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn nft_table_present() -> bool {
    run("nft", &["list", "table", "inet", NFT_TABLE])
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn ufw_rules_present() -> bool {
    let Ok(output) = run("ufw", &["status", "numbered"]) else {
        return false;
    };
    let text = String::from_utf8_lossy(&output.stdout);
    text.contains(UFW_COMMENT)
}

/// Report whether LAN-only rules appear active.
pub fn status() -> FirewallStatus {
    if nft_available() && nft_table_present() {
        return FirewallStatus {
            applied: true,
            backend: "nft".into(),
            detail: None,
        };
    }
    if ufw_available() && ufw_rules_present() {
        return FirewallStatus {
            applied: true,
            backend: "ufw".into(),
            detail: None,
        };
    }
    FirewallStatus {
        applied: false,
        backend: String::new(),
        detail: None,
    }
}

/// Apply LAN-only rules. Escalates via `pkexec` when not root.
pub fn apply() -> Result<FirewallStatus, String> {
    if is_root() {
        return apply_as_root();
    }
    escalate(&["firewall", "apply-as-root"])?;
    Ok(status())
}

/// Clear LAN-only rules. Escalates via `pkexec` when not root.
pub fn clear() -> Result<FirewallStatus, String> {
    if is_root() {
        return clear_as_root();
    }
    escalate(&["firewall", "clear-as-root"])?;
    Ok(status())
}

/// Privileged entry used under `pkexec` / already-root.
pub fn apply_as_root() -> Result<FirewallStatus, String> {
    if !is_root() {
        return Err("firewall apply-as-root requires root".into());
    }
    if nft_available() {
        apply_nft()?;
        return Ok(FirewallStatus {
            applied: true,
            backend: "nft".into(),
            detail: None,
        });
    }
    if ufw_available() {
        apply_ufw()?;
        return Ok(FirewallStatus {
            applied: true,
            backend: "ufw".into(),
            detail: None,
        });
    }
    Err(
        "Neither nftables (`nft`) nor ufw is available — install one to enforce LAN-only RDP"
            .into(),
    )
}

/// Privileged clear entry.
pub fn clear_as_root() -> Result<FirewallStatus, String> {
    if !is_root() {
        return Err("firewall clear-as-root requires root".into());
    }
    let mut cleared = false;
    if nft_available() && nft_table_present() {
        clear_nft()?;
        cleared = true;
    }
    if ufw_available() && ufw_rules_present() {
        clear_ufw()?;
        cleared = true;
    }
    Ok(FirewallStatus {
        applied: false,
        backend: String::new(),
        detail: if cleared {
            None
        } else {
            Some("No Metis RDP firewall rules were present".into())
        },
    })
}

fn escalate(args: &[&str]) -> Result<(), String> {
    let bin = std::env::current_exe().map_err(|e| format!("current exe: {e}"))?;
    let output = Command::new("pkexec")
        .arg(&bin)
        .args(args)
        .output()
        .map_err(|e| {
            format!(
                "pkexec failed ({e}) — install policykit-1 or run as root to apply LAN-only firewall rules"
            )
        })?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let detail = if !stderr.trim().is_empty() {
        stderr.trim().to_string()
    } else {
        stdout.trim().to_string()
    };
    Err(if detail.is_empty() {
        "pkexec firewall helper failed (permission denied or cancelled)".into()
    } else {
        detail
    })
}

fn apply_nft() -> Result<(), String> {
    // Replace any previous table so re-apply is idempotent.
    let _ = run("nft", &["delete", "table", "inet", NFT_TABLE]);

    let mut script = String::from("table inet metis_rdp {\n");
    script.push_str("  chain input {\n");
    script.push_str("    type filter hook input priority filter; policy accept;\n");
    for cidr in LAN_V4 {
        script.push_str(&format!(
            "    tcp dport {PORT} ip saddr {cidr} accept\n"
        ));
    }
    for cidr in LAN_V6 {
        script.push_str(&format!(
            "    tcp dport {PORT} ip6 saddr {cidr} accept\n"
        ));
    }
    script.push_str(&format!("    tcp dport {PORT} drop\n"));
    script.push_str("  }\n}\n");

    let status = Command::new("nft")
        .arg("-f")
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(script.as_bytes())?;
            }
            child.wait_with_output()
        })
        .map_err(|e| format!("nft apply: {e}"))?;
    if status.status.success() {
        Ok(())
    } else {
        Err(format!(
            "nft apply failed: {}",
            String::from_utf8_lossy(&status.stderr).trim()
        ))
    }
}

fn clear_nft() -> Result<(), String> {
    let out = run("nft", &["delete", "table", "inet", NFT_TABLE])?;
    if out.status.success()
        || String::from_utf8_lossy(&out.stderr).contains("No such file")
        || String::from_utf8_lossy(&out.stderr).contains("does not exist")
    {
        Ok(())
    } else {
        Err(format!(
            "nft clear failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

fn apply_ufw() -> Result<(), String> {
    // Ensure previous Metis rules are gone so re-apply stays idempotent.
    let _ = clear_ufw();
    for cidr in LAN_V4 {
        let out = run(
            "ufw",
            &[
                "allow",
                "from",
                cidr,
                "to",
                "any",
                "port",
                &PORT.to_string(),
                "proto",
                "tcp",
                "comment",
                UFW_COMMENT,
            ],
        )?;
        if !out.status.success() {
            return Err(format!(
                "ufw allow {cidr}: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
    }
    for cidr in LAN_V6 {
        let out = run(
            "ufw",
            &[
                "allow",
                "from",
                cidr,
                "to",
                "any",
                "port",
                &PORT.to_string(),
                "proto",
                "tcp",
                "comment",
                UFW_COMMENT,
            ],
        )?;
        if !out.status.success() {
            return Err(format!(
                "ufw allow {cidr}: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
    }
    // Deny other inbound 3389 (after allows).
    let out = run(
        "ufw",
        &[
            "deny",
            "in",
            "to",
            "any",
            "port",
            &PORT.to_string(),
            "proto",
            "tcp",
            "comment",
            UFW_COMMENT,
        ],
    )?;
    if !out.status.success() {
        return Err(format!(
            "ufw deny 3389: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(())
}

fn clear_ufw() -> Result<(), String> {
    // Delete by matching comment repeatedly until gone (ufw renumbers).
    for _ in 0..32 {
        let Ok(output) = run("ufw", &["status", "numbered"]) else {
            break;
        };
        let text = String::from_utf8_lossy(&output.stdout);
        let Some(num) = text.lines().find_map(|line| {
            if !line.contains(UFW_COMMENT) {
                return None;
            }
            let trimmed = line.trim_start();
            let start = trimmed.strip_prefix('[')?;
            let (num, _) = start.split_once(']')?;
            Some(num.trim().to_string())
        }) else {
            break;
        };
        let _ = Command::new("sh")
            .arg("-c")
            .arg(format!("yes | ufw delete {num}"))
            .output();
    }
    Ok(())
}
