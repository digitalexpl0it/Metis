use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ScreenshotMode {
    #[default]
    Selection,
    Screen,
    Window,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AfterCaptureAction {
    #[default]
    Copy,
    Save,
    CopyAndSave,
    Open,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScreenshotConfig {
    #[serde(default)]
    pub default_mode: ScreenshotMode,
    #[serde(default)]
    pub draw_cursor: bool,
    #[serde(default)]
    pub delay_seconds: u32,
    #[serde(default)]
    pub after_capture: AfterCaptureAction,
    #[serde(default = "default_save_dir")]
    pub save_dir: String,
}

fn default_save_dir() -> String {
    "~/Pictures/Metis".into()
}

impl Default for ScreenshotConfig {
    fn default() -> Self {
        Self {
            default_mode: ScreenshotMode::Selection,
            draw_cursor: false,
            delay_seconds: 0,
            after_capture: AfterCaptureAction::Copy,
            save_dir: default_save_dir(),
        }
    }
}

pub fn screenshot_config_path() -> std::path::PathBuf {
    super::config_dir().join("screenshot.json")
}

pub fn load_screenshot_config() -> ScreenshotConfig {
    let path = screenshot_config_path();
    if path.exists() {
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(cfg) = serde_json::from_str(&text) {
                return sanitize_screenshot_config(cfg);
            }
        }
    }
    ScreenshotConfig::default()
}

pub fn save_default_screenshot_config() -> std::io::Result<()> {
    let path = screenshot_config_path();
    if path.exists() {
        return Ok(());
    }
    save_screenshot_config(&ScreenshotConfig::default())
}

pub fn save_screenshot_config(config: &ScreenshotConfig) -> std::io::Result<()> {
    super::ensure_config_dirs()?;
    let json = serde_json::to_string_pretty(&sanitize_screenshot_config(config.clone()))
        .map_err(std::io::Error::other)?;
    std::fs::write(screenshot_config_path(), json)
}

fn sanitize_screenshot_config(mut cfg: ScreenshotConfig) -> ScreenshotConfig {
    cfg.delay_seconds = cfg.delay_seconds.min(30);
    if cfg.save_dir.trim().is_empty() {
        cfg.save_dir = default_save_dir();
    }
    cfg
}

pub fn expand_save_dir(path: &str) -> std::path::PathBuf {
    let trimmed = path.trim();
    if trimmed.starts_with("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return std::path::PathBuf::from(home).join(trimmed.trim_start_matches("~/"));
        }
    }
    if trimmed == "~" {
        if let Ok(home) = std::env::var("HOME") {
            return std::path::PathBuf::from(home);
        }
    }
    std::path::PathBuf::from(trimmed)
}
