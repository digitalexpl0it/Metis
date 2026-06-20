//! Bar weather service.
//!
//! Fetches current conditions + a short hourly forecast for one or more
//! locations from Open-Meteo (keyless), on a dedicated thread with its own
//! Tokio runtime. Results are delivered to the GTK main thread over a channel.
//!
//! Location resolution order:
//!   1. `weather.json` pinned `locations` (first is the bar's primary reading)
//!   2. otherwise, auto-detect a single city from the system timezone
//!
//! Temperature unit follows `weather.json` (`auto` resolves US-style regions to
//! Fahrenheit, everything else to Celsius).

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use crate::config::{load_weather_config, TempUnit};

const REFRESH_SECS: u64 = 15 * 60;
/// Retry sooner than the normal refresh when a fetch fails (e.g. offline at
/// startup) so a transient hiccup doesn't strand the widget for 15 minutes.
const RETRY_SECS: u64 = 30;
const HOURLY_POINTS: usize = 5;

/// Auto-detected location, cached for the process lifetime so a later geocoding
/// failure can't wipe out a location we already resolved.
static CACHED_GEO: OnceLock<Mutex<Option<Geo>>> = OnceLock::new();

#[derive(Debug, thiserror::Error)]
enum WeatherError {
    #[error("http request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("invalid response")]
    Invalid,
}

/// A single point in the short hourly forecast strip.
#[derive(Debug, Clone, PartialEq)]
pub struct HourlyPoint {
    pub label: String,
    pub temp: f64,
    pub code: i64,
    pub is_day: bool,
}

/// Current conditions + forecast for one location.
#[derive(Debug, Clone, PartialEq)]
pub struct LocationWeather {
    pub name: String,
    pub temp: f64,
    pub code: i64,
    pub is_day: bool,
    pub label: String,
    pub high: f64,
    pub low: f64,
    pub hourly: Vec<HourlyPoint>,
}

/// Latest weather reading delivered to the bar. `locations[0]` drives the icon
/// and temperature shown in the bar itself.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WeatherSnapshot {
    pub fahrenheit: bool,
    pub locations: Vec<LocationWeather>,
    pub error: Option<String>,
}

enum WeatherCommand {
    Refresh,
}

static WEATHER_CMD_TX: OnceLock<Sender<WeatherCommand>> = OnceLock::new();

/// Spawn the weather background thread and return the snapshot receiver.
pub fn spawn_weather_service() -> Receiver<WeatherSnapshot> {
    let (tx, rx) = mpsc::channel();
    let (cmd_tx, cmd_rx) = mpsc::channel();
    let _ = WEATHER_CMD_TX.set(cmd_tx);
    tracing::debug!("weather: spawning service thread");
    thread::Builder::new()
        .name("metis-weather".into())
        .spawn(move || {
            tracing::debug!("weather: thread started, building runtime");
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(err) => {
                    tracing::error!(error = %err, "weather: failed to build runtime");
                    return;
                }
            };
            runtime.block_on(weather_loop(tx, cmd_rx));
            tracing::warn!("weather: loop exited");
        })
        .expect("weather thread");
    rx
}

/// Request an immediate refresh (e.g. when the popover opens).
pub fn weather_refresh() {
    if let Some(tx) = WEATHER_CMD_TX.get() {
        let _ = tx.send(WeatherCommand::Refresh);
    }
}

async fn weather_loop(tx: Sender<WeatherSnapshot>, cmd_rx: Receiver<WeatherCommand>) {
    loop {
        let snapshot = build_snapshot().await;
        tracing::debug!(
            locations = snapshot.locations.len(),
            error = ?snapshot.error,
            "weather: snapshot ready"
        );
        let failed = snapshot.error.is_some();
        if tx.send(snapshot).is_err() {
            return;
        }
        // Retry quickly after a failure; otherwise wait the full refresh window.
        // Wake early on a manual refresh request. (std mpsc, polled so we stay on
        // the async thread.)
        let wait_secs = if failed { RETRY_SECS } else { REFRESH_SECS };
        let mut waited = 0u64;
        loop {
            tokio::time::sleep(Duration::from_millis(500)).await;
            waited += 500;
            if cmd_rx.try_recv().is_ok() {
                // Drain any extra queued refreshes.
                while cmd_rx.try_recv().is_ok() {}
                break;
            }
            if waited >= wait_secs * 1000 {
                break;
            }
        }
    }
}

#[derive(Debug, Clone)]
struct Geo {
    name: String,
    lat: f64,
    lon: f64,
    country_code: Option<String>,
}

async fn build_snapshot() -> WeatherSnapshot {
    let cfg = load_weather_config();

    let mut targets: Vec<Geo> = Vec::new();
    if !cfg.locations.is_empty() {
        for loc in &cfg.locations {
            targets.push(Geo {
                name: loc.name.clone(),
                lat: loc.latitude,
                lon: loc.longitude,
                country_code: None,
            });
        }
    } else if cfg.auto_detect {
        match detect_location(cfg.ip_geolocation).await {
            Some(geo) => targets.push(geo),
            None => {
                // Auto-detect is on but we couldn't resolve a location — almost
                // always a transient network/geocoding failure. Report it as
                // unavailable (it retries soon) rather than asking the user to
                // configure something.
                return WeatherSnapshot {
                    error: Some("Weather unavailable".into()),
                    ..Default::default()
                };
            }
        }
    }

    if targets.is_empty() {
        // Auto-detect disabled and no pinned locations.
        return WeatherSnapshot {
            error: Some("Set a location in Settings".into()),
            ..Default::default()
        };
    }

    let fahrenheit = match cfg.unit {
        TempUnit::Fahrenheit => true,
        TempUnit::Celsius => false,
        TempUnit::Auto => infer_fahrenheit(&targets),
    };

    let mut locations = Vec::new();
    let mut last_err = None;
    for target in &targets {
        match fetch_location(target, fahrenheit).await {
            Ok(weather) => locations.push(weather),
            Err(err) => {
                tracing::warn!(location = %target.name, error = %err, "weather: forecast fetch failed");
                last_err = Some(format!("{err}"));
            }
        }
    }

    if locations.is_empty() {
        return WeatherSnapshot {
            fahrenheit,
            error: Some(last_err.unwrap_or_else(|| "Weather unavailable".into())),
            ..Default::default()
        };
    }

    WeatherSnapshot {
        fahrenheit,
        locations,
        error: None,
    }
}

/// Resolve (and cache) the auto-detect location.
///
/// Accuracy order: IP geolocation (city-level, needs network) → system
/// `zoneinfo` tables (offline, coarse zone anchor city) → network geocode of the
/// timezone city. The first success is cached for the process lifetime.
async fn detect_location(ip_enabled: bool) -> Option<Geo> {
    if let Some(cached) = cached_geo() {
        return Some(cached);
    }

    if ip_enabled {
        if let Some(geo) = ip_geolocate().await {
            tracing::info!(city = %geo.name, lat = geo.lat, lon = geo.lon, "weather: location from IP geolocation");
            set_cached_geo(geo.clone());
            return Some(geo);
        }
        tracing::warn!("weather: IP geolocation unavailable — falling back to timezone");
    }

    let Some(tz) = system_timezone() else {
        tracing::warn!("weather: could not read the system timezone");
        return None;
    };

    if let Some(geo) = tz_geo(&tz) {
        tracing::info!(tz = %tz, city = %geo.name, lat = geo.lat, lon = geo.lon, "weather: location from zoneinfo");
        set_cached_geo(geo.clone());
        return Some(geo);
    }

    let Some(city) = tz_city(&tz) else {
        tracing::warn!(%tz, "weather: could not derive a city from the timezone");
        return None;
    };
    match geocode(&city).await {
        Some(geo) => {
            tracing::info!(city = %geo.name, lat = geo.lat, lon = geo.lon, "weather: location from geocoder");
            set_cached_geo(geo.clone());
            Some(geo)
        }
        None => {
            tracing::warn!(%city, "weather: geocoding failed for detected timezone city");
            None
        }
    }
}

/// City-level location from the public IP, via the keyless ipwho.is service.
async fn ip_geolocate() -> Option<Geo> {
    let resp = match http_client().get("https://ipwho.is/").send().await {
        Ok(resp) => resp,
        Err(err) => {
            tracing::warn!(error = %err, "weather: IP geolocation request failed");
            return None;
        }
    };
    let json: serde_json::Value = match resp.json().await {
        Ok(json) => json,
        Err(err) => {
            tracing::warn!(error = %err, "weather: IP geolocation parse failed");
            return None;
        }
    };
    if !json.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
        return None;
    }
    let lat = json.get("latitude")?.as_f64()?;
    let lon = json.get("longitude")?.as_f64()?;
    let city = json
        .get("city")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let region = json.get("region").and_then(|v| v.as_str());
    let name = city.unwrap_or_else(|| region.unwrap_or("Current location").to_string());
    Some(Geo {
        name,
        lat,
        lon,
        country_code: json
            .get("country_code")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    })
}

/// Look up coordinates for an IANA timezone from the system `zoneinfo` tables
/// (`zone1970.tab` / `zone.tab`). These ship with tzdata, so no network needed.
fn tz_geo(tz: &str) -> Option<Geo> {
    const TAB_PATHS: [&str; 2] = [
        "/usr/share/zoneinfo/zone1970.tab",
        "/usr/share/zoneinfo/zone.tab",
    ];
    for path in TAB_PATHS {
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        for line in content.lines() {
            if line.starts_with('#') {
                continue;
            }
            let mut cols = line.split('\t');
            let (Some(codes), Some(coords), Some(name)) =
                (cols.next(), cols.next(), cols.next())
            else {
                continue;
            };
            if name != tz {
                continue;
            }
            let Some((lat, lon)) = parse_iso6709(coords) else {
                continue;
            };
            return Some(Geo {
                name: tz_city(tz).unwrap_or_else(|| tz.to_string()),
                lat,
                lon,
                country_code: codes.split(',').next().map(|c| c.to_string()),
            });
        }
    }
    None
}

/// Parse ISO 6709 coordinates as used by the tzdata tables, e.g.
/// `+340308-1181434` -> (34.0522, -118.2428).
fn parse_iso6709(s: &str) -> Option<(f64, f64)> {
    let bytes = s.as_bytes();
    if bytes.len() < 2 {
        return None;
    }
    // The longitude begins at the next sign after the leading latitude sign.
    let split = (1..bytes.len()).find(|&i| bytes[i] == b'+' || bytes[i] == b'-')?;
    let lat = parse_angle(&s[..split], 2)?;
    let lon = parse_angle(&s[split..], 3)?;
    Some((lat, lon))
}

/// Parse a signed ±DDMM[SS] / ±DDDMM[SS] angle into decimal degrees.
fn parse_angle(s: &str, deg_len: usize) -> Option<f64> {
    let sign = match s.as_bytes().first()? {
        b'+' => 1.0,
        b'-' => -1.0,
        _ => return None,
    };
    let digits = &s[1..];
    let deg: f64 = digits.get(..deg_len)?.parse().ok()?;
    let min: f64 = digits.get(deg_len..deg_len + 2)?.parse().ok()?;
    let sec: f64 = match digits.get(deg_len + 2..deg_len + 4) {
        Some(ss) => ss.parse().ok()?,
        None => 0.0,
    };
    Some(sign * (deg + min / 60.0 + sec / 3600.0))
}

/// Shared HTTP client with a hard timeout so a stalled host can never hang the
/// weather thread (it just fails fast, logs, and retries on the next cycle).
fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .build()
        .unwrap_or_default()
}

fn cached_geo() -> Option<Geo> {
    CACHED_GEO
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()
        .and_then(|g| g.clone())
}

fn set_cached_geo(geo: Geo) {
    if let Ok(mut slot) = CACHED_GEO.get_or_init(|| Mutex::new(None)).lock() {
        *slot = Some(geo);
    }
}

async fn geocode(query: &str) -> Option<Geo> {
    let url = format!(
        "https://geocoding-api.open-meteo.com/v1/search?name={}&count=1&language=en&format=json",
        encode(query)
    );
    let resp = match http_client().get(&url).send().await {
        Ok(resp) => resp,
        Err(err) => {
            tracing::warn!(error = %err, "weather: geocode request failed");
            return None;
        }
    };
    let json: serde_json::Value = match resp.json().await {
        Ok(json) => json,
        Err(err) => {
            tracing::warn!(error = %err, "weather: geocode response parse failed");
            return None;
        }
    };
    let first = json.get("results")?.as_array()?.first()?;
    Some(Geo {
        name: first.get("name")?.as_str()?.to_string(),
        lat: first.get("latitude")?.as_f64()?,
        lon: first.get("longitude")?.as_f64()?,
        country_code: first
            .get("country_code")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    })
}

async fn fetch_location(geo: &Geo, fahrenheit: bool) -> Result<LocationWeather, WeatherError> {
    let unit = if fahrenheit { "fahrenheit" } else { "celsius" };
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}\
         &current=temperature_2m,weather_code,is_day\
         &hourly=temperature_2m,weather_code,is_day\
         &daily=temperature_2m_max,temperature_2m_min\
         &temperature_unit={unit}&timezone=auto&forecast_days=2",
        lat = geo.lat,
        lon = geo.lon,
    );
    let resp: serde_json::Value = http_client().get(&url).send().await?.json().await?;

    let current = resp.get("current").ok_or(WeatherError::Invalid)?;
    let temp = current
        .get("temperature_2m")
        .and_then(|v| v.as_f64())
        .ok_or(WeatherError::Invalid)?;
    let code = current.get("weather_code").and_then(|v| v.as_i64()).unwrap_or(0);
    let is_day = current.get("is_day").and_then(|v| v.as_i64()).unwrap_or(1) != 0;
    let current_time = current.get("time").and_then(|v| v.as_str()).unwrap_or("");

    let daily = resp.get("daily");
    let high = daily
        .and_then(|d| d.get("temperature_2m_max"))
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|v| v.as_f64())
        .unwrap_or(temp);
    let low = daily
        .and_then(|d| d.get("temperature_2m_min"))
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|v| v.as_f64())
        .unwrap_or(temp);

    let hourly = parse_hourly(resp.get("hourly"), current_time);

    Ok(LocationWeather {
        name: geo.name.clone(),
        temp,
        code,
        is_day,
        label: weather_label(code).to_string(),
        high,
        low,
        hourly,
    })
}

fn parse_hourly(hourly: Option<&serde_json::Value>, current_time: &str) -> Vec<HourlyPoint> {
    let Some(hourly) = hourly else {
        return Vec::new();
    };
    let times = hourly.get("time").and_then(|v| v.as_array());
    let temps = hourly.get("temperature_2m").and_then(|v| v.as_array());
    let codes = hourly.get("weather_code").and_then(|v| v.as_array());
    let days = hourly.get("is_day").and_then(|v| v.as_array());
    let (Some(times), Some(temps)) = (times, temps) else {
        return Vec::new();
    };

    // Compare on the "YYYY-MM-DDTHH" prefix so a minute-level current time still
    // aligns to the hourly buckets.
    let cur_hour = current_time.get(..13).unwrap_or(current_time);
    let start = times
        .iter()
        .position(|t| {
            t.as_str()
                .map(|s| s.get(..13).unwrap_or(s) >= cur_hour)
                .unwrap_or(false)
        })
        .unwrap_or(0);

    let mut points = Vec::new();
    for i in start..(start + HOURLY_POINTS).min(times.len()) {
        let label = times[i]
            .as_str()
            .and_then(|s| s.get(11..13))
            .and_then(|h| h.parse::<u32>().ok())
            .map(format_hour)
            .unwrap_or_default();
        let temp = temps.get(i).and_then(|v| v.as_f64()).unwrap_or(0.0);
        let code = codes
            .and_then(|c| c.get(i))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let is_day = days
            .and_then(|d| d.get(i))
            .and_then(|v| v.as_i64())
            .unwrap_or(1)
            != 0;
        points.push(HourlyPoint {
            label,
            temp,
            code,
            is_day,
        });
    }
    points
}

fn weather_label(code: i64) -> &'static str {
    match code {
        0 => "Clear",
        1 => "Mostly clear",
        2 => "Partly cloudy",
        3 => "Cloudy",
        45 | 48 => "Fog",
        51..=57 => "Drizzle",
        61..=67 => "Rain",
        71..=77 => "Snow",
        80..=82 => "Showers",
        85 | 86 => "Snow showers",
        95..=99 => "Thunderstorm",
        _ => "Weather",
    }
}

fn format_hour(hour24: u32) -> String {
    let (h12, suffix) = match hour24 % 24 {
        0 => (12, "AM"),
        h @ 1..=11 => (h, "AM"),
        12 => (12, "PM"),
        h => (h - 12, "PM"),
    };
    format!("{h12}{suffix}")
}

/// US-style regions report in Fahrenheit; otherwise fall back to the locale.
fn infer_fahrenheit(targets: &[Geo]) -> bool {
    const F_REGIONS: [&str; 8] = ["US", "PR", "GU", "VI", "AS", "MP", "LR", "MM"];
    if targets.iter().any(|g| {
        g.country_code
            .as_deref()
            .map(|c| F_REGIONS.contains(&c))
            .unwrap_or(false)
    }) {
        return true;
    }
    std::env::var("LC_MEASUREMENT")
        .or_else(|_| std::env::var("LANG"))
        .map(|l| l.contains("US"))
        .unwrap_or(false)
}

/// Resolve the IANA timezone name (e.g. `America/New_York`).
fn system_timezone() -> Option<String> {
    if let Ok(tz) = std::env::var("TZ") {
        let tz = tz.trim_start_matches(':').trim().to_string();
        if !tz.is_empty() {
            return Some(tz);
        }
    }
    if let Ok(contents) = std::fs::read_to_string("/etc/timezone") {
        let tz = contents.trim().to_string();
        if !tz.is_empty() {
            return Some(tz);
        }
    }
    if let Ok(target) = std::fs::read_link("/etc/localtime") {
        let path = target.to_string_lossy();
        if let Some(idx) = path.find("zoneinfo/") {
            let tz = path[idx + "zoneinfo/".len()..].to_string();
            if !tz.is_empty() {
                return Some(tz);
            }
        }
    }
    None
}

/// Extract a geocodable city from an IANA timezone (`America/New_York` -> `New York`).
fn tz_city(tz: &str) -> Option<String> {
    let city = tz.rsplit('/').next()?.replace('_', " ");
    if city.is_empty() || city.eq_ignore_ascii_case("UTC") || city.eq_ignore_ascii_case("GMT") {
        return None;
    }
    Some(city)
}

fn encode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_iso6709_la() {
        let (lat, lon) = parse_iso6709("+340308-1181434").expect("parse");
        assert!((lat - 34.0522).abs() < 0.01, "lat={lat}");
        assert!((lon + 118.2428).abs() < 0.01, "lon={lon}");
    }

    #[test]
    fn tz_city_strips_zone() {
        assert_eq!(tz_city("America/New_York").as_deref(), Some("New York"));
        assert_eq!(tz_city("Etc/UTC"), None);
    }

    #[test]
    fn detects_system_location_offline() {
        // This machine has tzdata; ensure offline detection yields coordinates.
        if let Some(tz) = system_timezone() {
            if let Some(geo) = tz_geo(&tz) {
                assert!(geo.lat.abs() <= 90.0);
                assert!(geo.lon.abs() <= 180.0);
                eprintln!("detected: {} ({}, {})", geo.name, geo.lat, geo.lon);
            }
        }
    }
}
