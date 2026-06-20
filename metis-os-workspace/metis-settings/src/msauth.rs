//! Microsoft 365 device-code login for the Calendars page. Self-contained
//! (reqwest + metis-secrets) so settings doesn't depend on the shell's calendar
//! service. Mirrors the shell's prior implementation.

use std::time::{Duration, Instant};

use serde::Deserialize;

const SCOPE: &str = "https://graph.microsoft.com/Calendars.ReadWrite offline_access openid profile";

#[derive(Clone, Debug, Deserialize)]
#[allow(dead_code)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub message: String,
    pub interval: u64,
    pub expires_in: u64,
}

#[derive(Deserialize)]
struct TokenResponse {
    refresh_token: Option<String>,
    error: Option<String>,
}

fn token_endpoint(tenant: &str) -> String {
    format!("https://login.microsoftonline.com/{tenant}/oauth2/v2.0/token")
}

fn devicecode_endpoint(tenant: &str) -> String {
    format!("https://login.microsoftonline.com/{tenant}/oauth2/v2.0/devicecode")
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("metis-settings/0.1")
        .build()
        .unwrap_or_default()
}

pub async fn start_device_login(tenant: &str, client_id: &str) -> Result<DeviceCode, String> {
    client()
        .post(devicecode_endpoint(tenant))
        .form(&[("client_id", client_id), ("scope", SCOPE)])
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json::<DeviceCode>()
        .await
        .map_err(|e| e.to_string())
}

/// Poll the token endpoint until the user authorizes, then persist the refresh
/// token in the Secret Service.
pub async fn complete_device_login(
    account_id: &str,
    tenant: &str,
    client_id: &str,
    code: &DeviceCode,
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(code.expires_in);
    let interval = Duration::from_secs(code.interval.max(1));
    loop {
        if Instant::now() >= deadline {
            return Err("Device-code login timed out".into());
        }
        tokio::time::sleep(interval).await;
        let resp = client()
            .post(token_endpoint(tenant))
            .form(&[
                ("client_id", client_id),
                (
                    "grant_type",
                    "urn:ietf:params:oauth:grant-type:device_code",
                ),
                ("device_code", &code.device_code),
            ])
            .send()
            .await
            .map_err(|e| e.to_string())?
            .json::<TokenResponse>()
            .await
            .map_err(|e| e.to_string())?;

        if let Some(refresh) = resp.refresh_token {
            return metis_secrets::store(account_id, metis_secrets::MS_REFRESH_TOKEN, &refresh)
                .await
                .map_err(|e| e.to_string());
        }
        match resp.error.as_deref() {
            Some("authorization_pending") | Some("slow_down") => continue,
            Some(other) => return Err(format!("Device-code login failed: {other}")),
            None => continue,
        }
    }
}
