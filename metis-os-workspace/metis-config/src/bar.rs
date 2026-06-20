use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BarPosition {
    Top,
    Left,
    Right,
}

impl Default for BarPosition {
    fn default() -> Self {
        Self::Top
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClockConfig {
    #[serde(default = "default_time_format")]
    pub time_format: String,
    #[serde(default = "default_date_format")]
    pub date_format: String,
    #[serde(default)]
    pub timezones: Vec<String>,
}

fn default_time_format() -> String {
    "%I:%M %p".into()
}

fn default_date_format() -> String {
    "%a %b %d".into()
}

impl Default for ClockConfig {
    fn default() -> Self {
        Self {
            time_format: default_time_format(),
            date_format: default_date_format(),
            timezones: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BarWidgetId {
    Workspaces,
    Spacer,
    Clock,
    Battery,
    Network,
    Volume,
    Notifications,
    Weather,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BarConfig {
    #[serde(default)]
    pub position: BarPosition,
    #[serde(default = "default_height")]
    pub height: u32,
    #[serde(default = "default_width")]
    pub width: u32,
    #[serde(default = "default_margin_top")]
    pub margin_top: u32,
    #[serde(default = "default_margin_h")]
    pub margin_h: u32,
    #[serde(default = "default_full_width")]
    pub full_width: bool,
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    #[serde(default = "default_true")]
    pub blur: bool,
    /// Gaussian backdrop-blur radius (in pixels) applied by the compositor behind
    /// the bar when `blur` is enabled. Consumed by the compositor via bar.json.
    #[serde(default = "default_blur_radius")]
    pub blur_radius: f32,
    #[serde(default = "default_widgets")]
    pub widgets: Vec<BarWidgetId>,
    #[serde(default)]
    pub clock: ClockConfig,
    /// Number of workspace indicator dots (1–12).
    #[serde(default = "default_workspace_count")]
    pub workspace_count: u32,
}

fn default_workspace_count() -> u32 {
    4
}

fn default_height() -> u32 {
    36
}

fn default_width() -> u32 {
    48
}

fn default_margin_top() -> u32 {
    8
}

fn default_margin_h() -> u32 {
    10
}

fn default_full_width() -> bool {
    true
}

fn default_opacity() -> f32 {
    0.92
}

fn default_blur_radius() -> f32 {
    18.0
}

fn default_true() -> bool {
    true
}

fn default_widgets() -> Vec<BarWidgetId> {
    vec![
        BarWidgetId::Workspaces,
        BarWidgetId::Spacer,
        BarWidgetId::Weather,
        BarWidgetId::Battery,
        BarWidgetId::Network,
        BarWidgetId::Volume,
        BarWidgetId::Notifications,
        BarWidgetId::Clock,
    ]
}

impl Default for BarConfig {
    fn default() -> Self {
        Self {
            position: BarPosition::Top,
            height: default_height(),
            width: default_width(),
            margin_top: default_margin_top(),
            margin_h: default_margin_h(),
            full_width: default_full_width(),
            opacity: default_opacity(),
            blur: default_true(),
            blur_radius: default_blur_radius(),
            widgets: default_widgets(),
            clock: ClockConfig::default(),
            workspace_count: default_workspace_count(),
        }
    }
}

pub fn bar_config_path() -> std::path::PathBuf {
    super::config_dir().join("bar.json")
}

pub fn load_bar_config() -> BarConfig {
    let path = bar_config_path();
    let mut cfg = if path.exists() {
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(parsed) = serde_json::from_str(&text) {
                parsed
            } else {
                tracing::warn!("bar.json parse failed — using defaults");
                BarConfig::default()
            }
        } else {
            BarConfig::default()
        }
    } else {
        BarConfig::default()
    };
    migrate_bar_config(&mut cfg);
    cfg
}

/// Upgrade layouts saved before the eww-style pill redesign.
fn migrate_bar_config(cfg: &mut BarConfig) {
    let legacy = [
        BarWidgetId::Workspaces,
        BarWidgetId::Spacer,
        BarWidgetId::Clock,
        BarWidgetId::Spacer,
        BarWidgetId::Battery,
        BarWidgetId::Network,
        BarWidgetId::Volume,
        BarWidgetId::Notifications,
    ];
    let center_notif = [
        BarWidgetId::Workspaces,
        BarWidgetId::Spacer,
        BarWidgetId::Notifications,
        BarWidgetId::Spacer,
        BarWidgetId::Battery,
        BarWidgetId::Network,
        BarWidgetId::Volume,
        BarWidgetId::Clock,
    ];
    let needs_layout_refresh = cfg.widgets == legacy
        || cfg.widgets == center_notif
        || cfg.margin_h >= 48
        || cfg.margin_top == 0
        || cfg.height < 36;
    if needs_layout_refresh {
        cfg.widgets = default_widgets();
        cfg.height = default_height();
        cfg.margin_top = default_margin_top();
        cfg.margin_h = default_margin_h();
        cfg.full_width = default_full_width();
        if let Ok(json) = serde_json::to_string_pretty(&*cfg) {
            let _ = std::fs::write(bar_config_path(), json);
        }
    }
    if cfg.clock.time_format == "%H:%M" {
        cfg.clock.time_format = default_time_format();
    }

    // Insert the weather widget into pre-existing layouts that predate it, ahead
    // of the system/clock cluster so it leads the right-hand group.
    if !cfg.widgets.contains(&BarWidgetId::Weather) {
        let pos = cfg
            .widgets
            .iter()
            .position(|w| {
                matches!(
                    w,
                    BarWidgetId::Battery
                        | BarWidgetId::Network
                        | BarWidgetId::Volume
                        | BarWidgetId::Notifications
                        | BarWidgetId::Clock
                )
            })
            .unwrap_or(cfg.widgets.len());
        cfg.widgets.insert(pos, BarWidgetId::Weather);
        if let Ok(json) = serde_json::to_string_pretty(&*cfg) {
            let _ = std::fs::write(bar_config_path(), json);
        }
    }
}

pub fn save_default_bar_config() -> std::io::Result<()> {
    super::ensure_config_dirs()?;
    let path = bar_config_path();
    if path.exists() {
        return Ok(());
    }
    let json = serde_json::to_string_pretty(&BarConfig::default()).map_err(std::io::Error::other)?;
    std::fs::write(path, json)
}

/// Persist a full bar configuration (used by the settings app's Appearance page
/// for opacity/blur edits). The shell's `watch_bar_config` re-applies it live.
pub fn save_bar_config(config: &BarConfig) -> std::io::Result<()> {
    super::ensure_config_dirs()?;
    let json = serde_json::to_string_pretty(config).map_err(std::io::Error::other)?;
    std::fs::write(bar_config_path(), json)
}
