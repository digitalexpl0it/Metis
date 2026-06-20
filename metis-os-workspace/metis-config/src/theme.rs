use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    Dark,
    Light,
    System,
}

/// Semantic status colors used by notifications and state highlights. Declared
/// with serde defaults so older `themes/*.json` (written before this palette
/// existed) keep parsing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticColors {
    #[serde(default = "default_error")]
    pub error: String,
    #[serde(default = "default_warning")]
    pub warning: String,
    #[serde(default = "default_success")]
    pub success: String,
    #[serde(default = "default_info")]
    pub info: String,
    #[serde(default = "default_payment")]
    pub payment: String,
}

impl Default for SemanticColors {
    fn default() -> Self {
        Self {
            error: default_error(),
            warning: default_warning(),
            success: default_success(),
            info: default_info(),
            payment: default_payment(),
        }
    }
}

fn default_error() -> String {
    "#ef4444".into()
}
fn default_warning() -> String {
    "#f59e0b".into()
}
fn default_success() -> String {
    "#10b981".into()
}
fn default_info() -> String {
    "#3b82f6".into()
}
fn default_payment() -> String {
    "#84cc16".into()
}
fn default_text_on_accent() -> String {
    "#0a0e14".into()
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
    #[serde(default)]
    pub semantic: SemanticColors,
    #[serde(default = "default_text_on_accent")]
    pub text_on_accent: String,
}

impl ThemeTokens {
    pub fn dark_default() -> Self {
        serde_json::from_str(include_str!("../resources/themes/dark.json"))
            .expect("embedded dark theme must parse")
    }

    pub fn light_default() -> Self {
        serde_json::from_str(include_str!("../resources/themes/light.json"))
            .expect("embedded light theme must parse")
    }

    pub fn accent_primary(&self) -> &str {
        self.accent.first().map(String::as_str).unwrap_or("#00F2FE")
    }

    /// The secondary accent (`accent[1]`), used for gradients and toggles. Falls
    /// back to the primary accent when a theme only declares one accent.
    pub fn accent_secondary(&self) -> &str {
        self.accent
            .get(1)
            .map(String::as_str)
            .unwrap_or_else(|| self.accent_primary())
    }

    /// The primary accent as a bare `r, g, b` triplet, so the stylesheet can
    /// inline it into `rgba(<triplet>, <alpha>)` with per-rule opacities.
    pub fn accent_rgb(&self) -> String {
        rgb_triplet_from_hex(self.accent_primary())
    }

    /// The secondary accent as a bare `r, g, b` triplet.
    pub fn accent_secondary_rgb(&self) -> String {
        rgb_triplet_from_hex(self.accent_secondary())
    }

    pub fn surface_rgba(&self) -> String {
        rgba_from_hex(&self.surface, self.card_opacity)
    }

    pub fn bg_rgba(&self) -> String {
        rgba_from_hex(&self.bg, self.card_opacity)
    }
}

/// Parse a `#rrggbb` string into a bare `r, g, b` triplet (for inlining into a
/// CSS `rgba(<triplet>, <alpha>)`). Falls back to the dark accent on bad input.
pub(crate) fn rgb_triplet_from_hex(hex: &str) -> String {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return "0, 242, 254".to_string();
    }
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(242);
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(254);
    format!("{r}, {g}, {b}")
}

/// Convert a `#rrggbb` string plus an alpha into a CSS `rgba(...)` string.
pub(crate) fn rgba_from_hex(hex: &str, opacity: f32) -> String {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return format!("rgba(18, 18, 20, {opacity})");
    }
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(18);
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(18);
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(20);
    format!("rgba({r}, {g}, {b}, {opacity})")
}
