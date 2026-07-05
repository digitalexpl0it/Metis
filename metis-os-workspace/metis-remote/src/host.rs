//! Hostname and LAN address discovery for connection hints.

use std::process::Command;

pub fn hostname() -> String {
    Command::new("hostname")
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        })
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("HOSTNAME").ok())
        .unwrap_or_else(|| "localhost".into())
}

/// Non-loopback addresses from `hostname -I` (best-effort).
pub fn lan_addresses() -> Vec<String> {
    let output = Command::new("hostname").arg("-I").output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .map(str::trim)
        .filter(|ip| !ip.is_empty() && *ip != "127.0.0.1")
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostname_non_empty() {
        assert!(!hostname().is_empty());
    }
}
