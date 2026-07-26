//! Input sanitizers for NetworkManager / spawn argv (Phase 15 §B).

/// Reject control characters and shell metacharacters in free-form NM strings.
pub fn is_safe_nm_token(s: &str) -> bool {
    if s.is_empty() || s.len() > 256 {
        return false;
    }
    !s.chars().any(|c| {
        c.is_control()
            || matches!(
                c,
                '\0' | '\n' | '\r' | ';' | '|' | '&' | '`' | '$' | '\'' | '"' | '\\' | '<' | '>'
            )
    })
}

/// SSID: printable UTF-8 without control chars; length 1–32 (802.11).
pub fn validate_ssid(ssid: &str) -> Result<(), String> {
    let ssid = ssid.trim();
    if ssid.is_empty() || ssid.len() > 32 {
        return Err("SSID must be 1–32 characters".into());
    }
    if ssid.chars().any(|c| c.is_control() || c == '\0') {
        return Err("SSID contains invalid characters".into());
    }
    Ok(())
}

/// Connection name / UUID as passed to nmcli (no control chars / shell meta).
pub fn validate_nm_id(id: &str) -> Result<(), String> {
    if !is_safe_nm_token(id) {
        return Err("network connection id contains invalid characters".into());
    }
    Ok(())
}

/// Single OpenVPN `vpn.data` key=value token fragment.
pub fn validate_vpn_data_fragment(s: &str) -> Result<(), String> {
    if s.is_empty() || s.len() > 4096 {
        return Err("VPN data fragment length invalid".into());
    }
    if s.chars().any(|c| c.is_control() || c == '\0' || c == ',') {
        return Err("VPN data fragment contains invalid characters".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssid_ok() {
        assert!(validate_ssid("HomeWiFi").is_ok());
    }

    #[test]
    fn ssid_rejects_newline() {
        assert!(validate_ssid("bad\nssid").is_err());
    }

    #[test]
    fn nm_id_rejects_meta() {
        assert!(validate_nm_id("ok-name").is_ok());
        assert!(validate_nm_id("evil;rm").is_err());
    }
}
