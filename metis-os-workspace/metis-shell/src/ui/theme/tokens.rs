use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    Dark,
    Light,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeTokens {
    pub name: String,
    pub mode: String,
    pub bg: String,
    pub surface: String,
    pub surface_raised: String,
    pub border: String,
    pub text: String,
    pub text_muted: String,
    pub accent: Vec<String>,
    pub radius_sm: u32,
    pub radius_md: u32,
    pub radius_lg: u32,
    pub gutter_px: u32,
    pub card_opacity: f32,
    pub scrim_opacity: f32,
    pub glow_intensity: f32,
    pub shadow_ambient: String,
    pub glow_cool: String,
    pub glow_warm: String,
    pub glow_violet: String,
}

impl ThemeTokens {
    pub fn dark_default() -> Self {
        serde_json::from_str(include_str!("../../../resources/themes/dark.json"))
            .expect("embedded dark theme must parse")
    }

    pub fn light_default() -> Self {
        serde_json::from_str(include_str!("../../../resources/themes/light.json"))
            .expect("embedded light theme must parse")
    }

    pub fn accent_primary(&self) -> &str {
        self.accent.first().map(String::as_str).unwrap_or("#00F2FE")
    }

    pub fn surface_rgba(&self) -> String {
        rgba_from_hex(&self.surface, self.card_opacity)
    }

    pub fn bg_rgba(&self) -> String {
        rgba_from_hex(&self.bg, self.card_opacity)
    }
}

fn rgba_from_hex(hex: &str, opacity: f32) -> String {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return format!("rgba(18, 18, 20, {opacity})");
    }
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(18);
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(18);
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(20);
    format!("rgba({r}, {g}, {b}, {opacity})")
}
