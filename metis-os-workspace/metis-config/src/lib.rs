//! Shared Metis configuration: pure serde + filesystem types consumed by both the
//! shell (`metis-shell`) and the settings app (`metis-settings`). No GTK here so
//! the settings binary can link it without pulling in the shell.

pub mod bar;
pub mod calendars;
pub mod clocks;
pub mod css;
pub mod dashboard;
pub mod decorations;
pub mod game_rules;
pub mod gaming;
pub mod gpu_offload;
pub mod graphics;
pub mod input;
pub mod keybinds;
pub mod kitty;
pub mod lock;
pub mod menu;
pub mod outputs;
pub mod power;
pub mod remote;
pub mod screenshot;
pub mod theme;
pub mod wallpaper;
pub mod weather;

use serde::{Deserialize, Serialize};

// `ThemeMode` is re-exported below via `pub use theme::{...}`, which also brings it
// into this module's scope for the path helpers.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default)]
    pub onboarding_complete: bool,
    #[serde(default)]
    pub gaming_setup_complete: bool,
    #[serde(default = "default_show_briefing")]
    pub show_briefing_on_login: bool,
    /// Session UI graphics profile (Auto / Compatibility / Normal). Independent of
    /// Gaming's PRIME `graphics_mode`.
    #[serde(default)]
    pub graphics_profile: graphics::GraphicsProfile,
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
            gaming_setup_complete: false,
            show_briefing_on_login: default_show_briefing(),
            graphics_profile: graphics::GraphicsProfile::default(),
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

pub fn load_graphics_profile() -> graphics::GraphicsProfile {
    load_app_config().graphics_profile
}

pub fn save_graphics_profile(profile: graphics::GraphicsProfile) -> std::io::Result<()> {
    let mut cfg = load_app_config();
    cfg.graphics_profile = profile;
    save_app_config(&cfg)
}

/// `color-scheme` / `gtk-theme` values for `org.gnome.desktop.interface`.
///
/// Libadwaita apps (Nautilus, GNOME apps) ignore the obsolete `Adwaita-dark`
/// theme name and follow `color-scheme` instead — keep `gtk-theme` as `Adwaita`
/// and drive dark via `prefer-dark`.
pub fn appearance_gsettings_values(mode: ThemeMode) -> (&'static str, &'static str) {
    match mode {
        ThemeMode::Dark => ("prefer-dark", "Adwaita"),
        ThemeMode::Light => ("prefer-light", "Adwaita"),
        ThemeMode::System => ("default", "Adwaita"),
    }
}

/// `GTK_THEME` for spawned clients (`Adwaita:dark` / `Adwaita`). Helps GTK4 apps
/// that do not yet read the Settings portal.
pub fn appearance_gtk_theme_env(mode: ThemeMode) -> Option<&'static str> {
    match mode {
        ThemeMode::Dark => Some("Adwaita:dark"),
        ThemeMode::Light => Some("Adwaita"),
        ThemeMode::System => None,
    }
}

/// GNOME WM preference for CSD traffic lights (`org.gnome.desktop.wm.preferences`).
///
/// Fresh Ubuntu images (and some greeter leftovers) ship `appmenu:close`, which
/// hides minimize/maximize on Firefox, Chromium, and GTK headerbars. Metis owns
/// the session, so we normalize to the full Ubuntu/GNOME layout.
pub const SESSION_WM_BUTTON_LAYOUT: &str = "appmenu:minimize,maximize,close";

/// GTK decoration layout string (`org.gnome.desktop.interface gtk-decoration-layout`
/// when the schema key exists) and the Settings portal value for the same idea.
pub const SESSION_GTK_DECORATION_LAYOUT: &str = "icon:minimize,maximize,close";

/// Best-effort sync so non-Metis GTK / browser CSD follows Metis light/dark and
/// shows minimize + maximize + close (not close-only).
pub fn apply_session_appearance_gsettings(mode: ThemeMode) {
    let (scheme, gtk_theme) = appearance_gsettings_values(mode);
    let _ = std::process::Command::new("gsettings")
        .args([
            "set",
            "org.gnome.desktop.interface",
            "color-scheme",
            scheme,
        ])
        .status();
    let _ = std::process::Command::new("gsettings")
        .args([
            "set",
            "org.gnome.desktop.interface",
            "gtk-theme",
            gtk_theme,
        ])
        .status();
    let _ = std::process::Command::new("gsettings")
        .args([
            "set",
            "org.gnome.desktop.wm.preferences",
            "button-layout",
            SESSION_WM_BUTTON_LAYOUT,
        ])
        .status();
    // Present on full GNOME schema installs; missing on some minimal images.
    let _ = std::process::Command::new("gsettings")
        .args([
            "set",
            "org.gnome.desktop.interface",
            "gtk-decoration-layout",
            SESSION_GTK_DECORATION_LAYOUT,
        ])
        .status();
}

/// Apply gsettings from the persisted theme preference (session start / portal).
pub fn sync_session_appearance_from_config() {
    let mode = load_theme_preference().unwrap_or(ThemeMode::Dark);
    apply_session_appearance_gsettings(mode);
}

pub fn mark_onboarding_complete() -> std::io::Result<()> {
    let mut cfg = load_app_config();
    cfg.onboarding_complete = true;
    save_app_config(&cfg)
}

pub fn mark_gaming_setup_complete() -> std::io::Result<()> {
    let mut cfg = load_app_config();
    cfg.gaming_setup_complete = true;
    save_app_config(&cfg)
}

/// Persist a theme token set to `themes/<name>.json` (used by the settings app's
/// Appearance page). The shell's file watcher re-applies it live.
pub fn save_theme_tokens(name: &str, tokens: &theme::ThemeTokens) -> std::io::Result<()> {
    ensure_config_dirs()?;
    let json = serde_json::to_string_pretty(tokens).map_err(std::io::Error::other)?;
    std::fs::write(theme_file_path_for_name(name), json)
}

/// Load a theme token set from `themes/<name>.json`, falling back to the embedded
/// default for that name (dark/light) when missing or unparsable.
pub fn load_theme_tokens(name: &str) -> theme::ThemeTokens {
    let path = theme_file_path_for_name(name);
    if path.exists() {
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(tokens) = serde_json::from_str(&text) {
                return tokens;
            }
        }
    }
    match name {
        "light" => theme::ThemeTokens::light_default(),
        _ => theme::ThemeTokens::dark_default(),
    }
}

pub use outputs::{
    load_outputs_config, load_outputs_config_with_fallback, night_light_effective, output_prefs, outputs_config_path,
    parse_hhmm, save_outputs_config, DisplayLayoutMode, NightLightSchedule, OutputPrefs,
    OutputsConfig, format_schedule_hhmm, format_schedule_minutes, minutes_to_hhmm,
    parse_schedule_input, schedule_half_hour_presets,
};
pub use power::{
    load_power_config, power_config_path, save_power_config, LidCloseAction, PowerConfig,
    PowerProfile,
};
pub use remote::{
    load_remote_config, remote_config_path, save_remote_config, RemoteBackend, RemoteConfig,
};
pub use screenshot::{
    expand_save_dir, load_screenshot_config, save_default_screenshot_config,
    save_screenshot_config, screenshot_config_path, AfterCaptureAction, ScreenshotConfig,
    ScreenshotMode,
};
pub use lock::{
    load_lock_config, lock_config_path, save_lock_config, LockBackgroundSource, LockConfig,
};
pub use dashboard::{
    dashboard_config_path, load_dashboard_config, process_monitor_needs_terminal,
    save_dashboard_config, save_default_dashboard_config, DashboardConfig, DashboardWidgetId,
    KNOWN_PROCESS_MONITORS,
};
pub use decorations::{
    decorations_config_path, load_decorations_config, save_decorations_config, DecorationsConfig,
    DecorationsOverride,
};
pub use game_rules::{
    game_rules_config_path, load_game_rules_config, save_game_rules_config, GameRulesConfig,
    WindowRule, WindowRuleOutcome,
};
pub use gaming::{
    command_prefers_dgpu, gaming_config_path, gaming_flatpak_state_path, load_gaming_config,
    load_gaming_flatpak_state, on_battery, prefer_dgpu_for_launch, save_default_gaming_config,
    save_gaming_config, save_gaming_flatpak_state, GameScopeProfile, GamingConfig,
    GamingFlatpakState, GraphicsMode,
};
pub use gpu_offload::{
    detect_hybrid_gpu, display_gpu_pci, offload_env_vars, GpuOffloadKind, HybridGpuInfo,
};
pub use graphics::{
    effective_graphics_compatibility, effective_graphics_profile_label, is_virtual_machine,
    session_graphics_compatibility, GraphicsProfile,
};
pub use input::{
    load_input_config, save_input_config, input_config_path, AccelProfile, CapsLockBehavior,
    ComposeKey, InputConfig, KeyboardConfig, MouseConfig, TouchpadConfig,
};
pub use keybinds::{
    default_chord, keybinds_config_path, load_keybinds_config, reserved_chords,
    reserved_system_rows, save_default_keybinds_config, save_keybinds_config, Chord, KeybindAction,
    KeybindGroup, KeybindsConfig, ModKey,
};
pub use kitty::{ensure_kitty_defaults, kitty_config_path, KITTY_DEFAULT_CONF};
pub use bar::{
    bar_config_path, load_bar_config, save_bar_config, save_default_bar_config, BarBorder,
    BarConfig, BarDisplays, BarPosition, BarWidgetId, BorderMode, ClockConfig, DefaultLayout,
    TitlebarPillBorder, TrayIconMode, WindowBorder, WorkspaceMode,
};
pub use calendars::{
    calendars_config_path, default_local_dir, load_calendars_config, save_calendars_config,
    AccountKind, CalendarAccount, CalendarsConfig,
};
pub use clocks::{
    alarm_sound_canberra_id, clocks_config_path, load_clocks_config, save_clocks_config, Alarm,
    AlarmSound, ClocksConfig, ALARM_SOUNDS,
};
pub use css::build_stylesheet;
pub use menu::{
    binary_in_path, load_menu_config, menu_config_path, save_menu_config, MenuConfig,
    KNOWN_FILE_MANAGERS, KNOWN_TERMINALS,
};
pub use theme::{SemanticColors, ThemeMode, ThemeTokens};
pub use wallpaper::{
    bundled_wallpaper_dir, bundled_wallpaper_dirs, collect_wallpaper_images,
    default_wallpaper_path, list_bundled_wallpapers, load_wallpaper_config, parse_hex_rgb,
    save_wallpaper_config, wallpaper_config_path, wallpaper_store_dir, WALLPAPER_IMAGE_EXTS,
    BackgroundKind, GradientDirection, WallpaperConfig,
};
pub use weather::{
    load_weather_config, save_weather_config, weather_config_path, TempUnit, WeatherConfig,
    WeatherLocation,
};
