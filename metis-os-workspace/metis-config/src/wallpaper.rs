//! Desktop background persisted to `~/.config/metis/wallpaper.json`.
//!
//! Supports three kinds of background: a picture (image file), a solid colour, or
//! a two-stop linear gradient. The settings app writes this file; the compositor
//! reads it on startup and applies changes live via the `ApplyBackground` IPC
//! command.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{config_dir, ensure_config_dirs};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundKind {
    #[default]
    Image,
    Solid,
    Gradient,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GradientDirection {
    /// Top → bottom.
    #[default]
    Vertical,
    /// Bottom → top.
    VerticalReverse,
    /// Left → right.
    Horizontal,
    /// Right → left.
    HorizontalReverse,
    /// Top-left → bottom-right.
    Diagonal,
    /// Top-right → bottom-left.
    DiagonalReverse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WallpaperConfig {
    /// Which background style is active.
    #[serde(default)]
    pub kind: BackgroundKind,
    /// Absolute path to the selected picture (used when `kind == Image`).
    #[serde(default)]
    pub path: Option<String>,
    /// Solid background colour (`#rrggbb`), used when `kind == Solid`.
    #[serde(default = "default_solid")]
    pub color: String,
    /// Gradient start colour (`#rrggbb`), used when `kind == Gradient`.
    #[serde(default = "default_grad_start")]
    pub gradient_start: String,
    /// Gradient end colour (`#rrggbb`), used when `kind == Gradient`.
    #[serde(default = "default_grad_end")]
    pub gradient_end: String,
    /// Direction the gradient sweeps.
    #[serde(default)]
    pub gradient_direction: GradientDirection,
}

fn default_solid() -> String {
    "#1e1e2e".to_string()
}

fn default_grad_start() -> String {
    "#0ea5e9".to_string()
}

fn default_grad_end() -> String {
    "#7c3aed".to_string()
}

impl Default for WallpaperConfig {
    fn default() -> Self {
        Self {
            kind: BackgroundKind::default(),
            path: None,
            color: default_solid(),
            gradient_start: default_grad_start(),
            gradient_end: default_grad_end(),
            gradient_direction: GradientDirection::default(),
        }
    }
}

pub fn wallpaper_config_path() -> PathBuf {
    config_dir().join("wallpaper.json")
}

pub fn load_wallpaper_config() -> WallpaperConfig {
    let path = wallpaper_config_path();
    if let Ok(text) = std::fs::read_to_string(&path) {
        if let Ok(cfg) = serde_json::from_str(&text) {
            return cfg;
        }
    }
    WallpaperConfig::default()
}

pub fn save_wallpaper_config(cfg: &WallpaperConfig) -> std::io::Result<()> {
    ensure_config_dirs()?;
    let json = serde_json::to_string_pretty(cfg).map_err(std::io::Error::other)?;
    std::fs::write(wallpaper_config_path(), json)
}

/// Directory where user-imported wallpapers are copied to.
pub fn wallpaper_store_dir() -> PathBuf {
    config_dir().join("wallpapers")
}

/// Parse a `#rrggbb` hex colour into an RGB triplet, falling back to black.
pub fn parse_hex_rgb(hex: &str) -> [u8; 3] {
    let h = hex.trim().trim_start_matches('#');
    if h.len() == 6 {
        if let Ok(v) = u32::from_str_radix(h, 16) {
            return [(v >> 16) as u8, (v >> 8) as u8, v as u8];
        }
    }
    [0, 0, 0]
}
