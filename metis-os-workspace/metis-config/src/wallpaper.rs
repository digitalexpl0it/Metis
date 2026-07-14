//! Desktop background persisted to `~/.config/metis/wallpaper.json`.
//!
//! Supports three kinds of background: a picture (image file), a solid colour, or
//! a two-stop linear gradient. The settings app writes this file; the compositor
//! reads it on startup and applies changes live via the `ApplyBackground` IPC
//! command.

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

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
    /// Per-output image overrides, keyed by the compositor's output name
    /// (e.g. `metis-0`, `metis-1`). When an output appears here, that display
    /// shows the given picture instead of the global background. Outputs not
    /// listed fall back to the global `kind`/`path` above. Each display is
    /// always cover-cropped to its own resolution, so the same image on two
    /// differently sized monitors still fills each one correctly.
    #[serde(default)]
    pub per_output: HashMap<String, String>,
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
            per_output: HashMap::new(),
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

/// Image extensions treated as selectable wallpapers.
pub const WALLPAPER_IMAGE_EXTS: &[&str] = &["jpg", "jpeg", "png", "webp"];

/// Compile-time path to the workspace bundled wallpapers directory.
pub fn bundled_wallpaper_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../assets/wallpapers")
}

/// Candidate directories holding bundled wallpapers (compile-time bundle plus
/// installed FHS paths and exe-relative fallbacks).
pub fn bundled_wallpaper_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut push = |p: PathBuf| {
        if p.is_dir() && !dirs.iter().any(|d| d == &p) {
            dirs.push(p);
        }
    };
    // Packaged installs (`.deb` / `--install-session`).
    push(PathBuf::from("/usr/share/metis/wallpapers"));
    push(PathBuf::from("/usr/local/share/metis/wallpapers"));
    push(bundled_wallpaper_dir());
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            for rel in [
                "assets/wallpapers",
                "../assets/wallpapers",
                "../../assets/wallpapers",
                "../../../assets/wallpapers",
                "../share/metis/wallpapers",
                "../../share/metis/wallpapers",
            ] {
                push(parent.join(rel));
            }
        }
    }
    dirs
}

fn is_wallpaper_image(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| WALLPAPER_IMAGE_EXTS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// Collect image files from `dir` into `out`, skipping paths already in `seen`.
pub fn collect_wallpaper_images(dir: &Path, out: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() || !is_wallpaper_image(&path) {
            continue;
        }
        let canon = path.canonicalize().unwrap_or(path.clone());
        if seen.insert(canon) {
            found.push(path);
        }
    }
    found.sort();
    out.extend(found);
}

/// All bundled wallpaper images, sorted by filename.
pub fn list_bundled_wallpapers() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for dir in bundled_wallpaper_dirs() {
        collect_wallpaper_images(&dir, &mut out, &mut seen);
    }
    out
}

/// First bundled wallpaper (typically `default.png`), if any exist.
pub fn default_wallpaper_path() -> Option<PathBuf> {
    list_bundled_wallpapers().into_iter().next()
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
