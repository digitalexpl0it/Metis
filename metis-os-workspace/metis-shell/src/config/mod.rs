mod bar;
use serde::{Deserialize, Serialize};

use crate::ui::theme::ThemeMode;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default)]
    pub onboarding_complete: bool,
    #[serde(default = "default_show_briefing")]
    pub show_briefing_on_login: bool,
}

fn default_theme() -> String {
    "dark".into()
}

fn default_show_briefing() -> bool {
    true
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            theme: default_theme(),
            onboarding_complete: false,
            show_briefing_on_login: default_show_briefing(),
        }
    }
}

pub fn config_dir() -> std::path::PathBuf {
    // On Linux, ProjectDirs uses only the `application` component for the path,
    // so `application = "metis"` yields ~/.config/metis (the documented location).
    directories::ProjectDirs::from("com", "metis", "metis")
        .map(|dirs| dirs.config_dir().to_path_buf())
        .unwrap_or_else(|| {
            std::env::var("HOME")
                .map(|h| std::path::PathBuf::from(h).join(".config/metis"))
                .unwrap_or_else(|_| std::path::PathBuf::from(".config/metis"))
        })
}

pub fn ensure_config_dirs() -> std::io::Result<()> {
    std::fs::create_dir_all(config_dir())?;
    std::fs::create_dir_all(config_dir().join("themes"))?;
    Ok(())
}

pub fn app_config_path() -> std::path::PathBuf {
    config_dir().join("config.json")
}

pub fn desk_config_path() -> std::path::PathBuf {
    config_dir().join("desk.json")
}

pub fn briefing_config_path() -> std::path::PathBuf {
    config_dir().join("briefing.json")
}

pub fn theme_file_path(mode: &ThemeMode) -> std::path::PathBuf {
    theme_file_path_for_name(match mode {
        ThemeMode::Light => "light",
        ThemeMode::Dark => "dark",
        ThemeMode::System => "system",
    })
}

pub fn theme_file_path_for_name(name: &str) -> std::path::PathBuf {
    config_dir().join("themes").join(format!("{name}.json"))
}

pub fn load_app_config() -> AppConfig {
    let path = app_config_path();
    if path.exists() {
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(cfg) = serde_json::from_str(&text) {
                return cfg;
            }
        }
    }
    AppConfig::default()
}

pub fn save_app_config(config: &AppConfig) -> std::io::Result<()> {
    ensure_config_dirs()?;
    let json = serde_json::to_string_pretty(config).map_err(std::io::Error::other)?;
    std::fs::write(app_config_path(), json)
}

pub fn load_theme_preference() -> Option<ThemeMode> {
    let cfg = load_app_config();
    match cfg.theme.as_str() {
        "light" => Some(ThemeMode::Light),
        "system" => Some(ThemeMode::System),
        _ => Some(ThemeMode::Dark),
    }
}

pub fn save_theme_preference(mode: ThemeMode) -> std::io::Result<()> {
    let mut cfg = load_app_config();
    cfg.theme = match mode {
        ThemeMode::Light => "light",
        ThemeMode::Dark => "dark",
        ThemeMode::System => "system",
    }
    .into();
    save_app_config(&cfg)
}

pub fn mark_onboarding_complete() -> std::io::Result<()> {
    let mut cfg = load_app_config();
    cfg.onboarding_complete = true;
    save_app_config(&cfg)
}

pub use bar::{
    bar_config_path, load_bar_config, save_default_bar_config, BarConfig, BarPosition,
    BarWidgetId, ClockConfig,
};
