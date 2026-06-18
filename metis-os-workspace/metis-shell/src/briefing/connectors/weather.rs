use thiserror::Error;

#[derive(Debug, Error)]
pub enum WeatherError {
    #[error("http request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("invalid response")]
    Invalid,
}

pub async fn fetch_summary(lat: f64, lon: f64) -> Result<String, WeatherError> {
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}&current=temperature_2m,weather_code,wind_speed_10m"
    );
    let resp: serde_json::Value = reqwest::get(&url).await?.json().await?;
    let current = resp.get("current").ok_or(WeatherError::Invalid)?;
    let temp = current
        .get("temperature_2m")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let wind = current
        .get("wind_speed_10m")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let code = current
        .get("weather_code")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    Ok(format!(
        "{} · {:.0}°C · wind {:.0} km/h · code {}",
        weather_label(code),
        temp,
        wind,
        code
    ))
}

fn weather_label(code: i64) -> &'static str {
    match code {
        0 => "Clear",
        1..=3 => "Partly cloudy",
        45 | 48 => "Fog",
        51..=67 => "Rain",
        71..=77 => "Snow",
        80..=82 => "Showers",
        95..=99 => "Thunderstorm",
        _ => "Weather",
    }
}
