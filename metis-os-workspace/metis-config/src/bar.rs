use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BarPosition {
    Top,
    Bottom,
    Left,
    Right,
}

impl Default for BarPosition {
    fn default() -> Self {
        Self::Top
    }
}

/// Which outputs (monitors) the edge bar appears on in a multi-monitor session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BarDisplays {
    /// One bar per connected output (default).
    All,
    /// A single bar on the primary output only.
    Primary,
}

impl Default for BarDisplays {
    fn default() -> Self {
        Self::All
    }
}

/// How virtual workspaces behave across multiple outputs (monitors).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkspaceMode {
    /// Each output owns an independent set of workspaces; switching one output's
    /// workspace leaves the others alone (default).
    Separate,
    /// All outputs switch together: changing to workspace N moves every monitor to
    /// its own workspace N at once.
    Linked,
}

impl Default for WorkspaceMode {
    fn default() -> Self {
        Self::Separate
    }
}

/// The layout mode new workspaces start in. Mirrors `metis_grid::LayoutKind`
/// without pulling the grid crate into config; the compositor maps between them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DefaultLayout {
    /// Regular floating desktop (default).
    #[default]
    Free,
    /// Auto-tiling grid below desk widgets.
    Grid,
    /// A horizontally scrolling strip of columns (niri / PaperWM style).
    Scroll,
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
    Tasks,
    Spacer,
    Clock,
    Battery,
    Network,
    Bluetooth,
    Volume,
    Notifications,
    /// Clipboard history (text + image previews).
    Clipboard,
    Weather,
    /// System tray (StatusNotifierItem) host.
    Tray,
}

/// How system tray app icons appear on the edge bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TrayIconMode {
    /// One tray button opens a popover listing all tray app icons (default).
    #[default]
    Collapsed,
    /// Tray app icons are pinned inline on the bar, to the left of the tray button.
    Pinned,
}

/// Transparent padding baked into the bar's layer surface (beyond the visible
/// pill) so the pill's rounded drop shadow renders without being clipped square.
/// `SHADOW_PAD` is on the inner edge (below a top bar); `PILL_SIDE_INSET` is on
/// the two long edges. The compositor uses these to confine backdrop effects
/// (e.g. blur) to the visible pill and exclude the shadow margin.
pub const SHADOW_PAD: i32 = 16;
pub const PILL_SIDE_INSET: i32 = SHADOW_PAD - 4;

/// How the compositor strokes an accent border (title pill or window frame).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BorderMode {
    /// Follow the active theme's accent gradient (auto-tracks light/dark).
    Accent,
    /// A single flat color (`color`).
    Solid,
    /// A custom gradient across `gradient`'s stops.
    Gradient,
}

impl Default for BorderMode {
    fn default() -> Self {
        Self::Accent
    }
}

/// Appearance of the thin accent border around the compositor-drawn title pill.
/// Consumed by the compositor (via `bar.json`) and edited by the settings app's
/// Appearance page. The border only paints on the *focused* window; unfocused
/// windows always use a muted slate stroke. The pill gradient flows left→right.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TitlebarPillBorder {
    #[serde(default)]
    pub mode: BorderMode,
    /// Flat stroke color (`#rrggbb`) used when `mode = solid`.
    #[serde(default = "default_pill_color")]
    pub color: String,
    /// Gradient stops (`#rrggbb`), used when `mode = gradient`. Two or more
    /// stops recommended.
    #[serde(default = "default_pill_gradient")]
    pub gradient: Vec<String>,
    /// Stroke thickness in pixels.
    #[serde(default = "default_pill_border_width")]
    pub width_px: f32,
}

/// Appearance of the compositor-drawn window frame border (the left/right/bottom
/// edges + the titlebar ring). Independent of the title pill. The frame gradient
/// flows top→bottom; `width_px` sets the frame thickness and insets the client body
/// to match. Focused windows draw this stroke; unfocused use a muted slate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowBorder {
    #[serde(default)]
    pub mode: BorderMode,
    /// Flat stroke color (`#rrggbb`) used when `mode = solid`.
    #[serde(default = "default_window_border_color")]
    pub color: String,
    /// Gradient stops (`#rrggbb`), used when `mode = gradient`.
    #[serde(default = "default_window_border_gradient")]
    pub gradient: Vec<String>,
    /// Frame thickness in pixels (0–16). Insets the client body to match.
    #[serde(default = "default_window_border_width")]
    pub width_px: f32,
}

/// Appearance of the border drawn around the edge bar's pill, rendered by the
/// shell via GTK CSS. Independent of the window/title-pill borders. `accent`
/// follows the theme accent gradient; `gradient` uses custom stops; the gradient
/// flows along the bar's long axis (left→right when horizontal, top→bottom when
/// vertical). `width_px = 0` disables the border entirely.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BarBorder {
    #[serde(default)]
    pub mode: BorderMode,
    /// Flat stroke color (`#rrggbb`) used when `mode = solid`.
    #[serde(default = "default_bar_border_color")]
    pub color: String,
    /// Gradient stops (`#rrggbb`), used when `mode = gradient`.
    #[serde(default = "default_bar_border_gradient")]
    pub gradient: Vec<String>,
    /// Border thickness in pixels (0 disables the border).
    #[serde(default = "default_bar_border_width")]
    pub width_px: f32,
}

fn default_bar_border_color() -> String {
    "#00F2FE".into()
}

fn default_bar_border_gradient() -> Vec<String> {
    vec!["#00F2FE".into(), "#4FACFE".into(), "#A24BFF".into()]
}

fn default_bar_border_width() -> f32 {
    1.0
}

impl Default for BarBorder {
    fn default() -> Self {
        Self {
            mode: BorderMode::default(),
            color: default_bar_border_color(),
            gradient: default_bar_border_gradient(),
            width_px: default_bar_border_width(),
        }
    }
}

fn default_pill_color() -> String {
    "#00F2FE".into()
}

fn default_pill_gradient() -> Vec<String> {
    vec!["#00F2FE".into(), "#4FACFE".into(), "#A24BFF".into()]
}

fn default_pill_border_width() -> f32 {
    1.0
}

fn default_window_border_color() -> String {
    "#00F2FE".into()
}

fn default_window_border_gradient() -> Vec<String> {
    vec!["#00F2FE".into(), "#4FACFE".into(), "#A24BFF".into()]
}

fn default_window_border_width() -> f32 {
    1.0
}

impl Default for TitlebarPillBorder {
    fn default() -> Self {
        Self {
            mode: BorderMode::default(),
            color: default_pill_color(),
            gradient: default_pill_gradient(),
            width_px: default_pill_border_width(),
        }
    }
}

impl Default for WindowBorder {
    fn default() -> Self {
        Self {
            mode: BorderMode::default(),
            color: default_window_border_color(),
            gradient: default_window_border_gradient(),
            width_px: default_window_border_width(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BarConfig {
    #[serde(default)]
    pub position: BarPosition,
    /// Which outputs the bar appears on (all monitors vs. primary only).
    #[serde(default)]
    pub displays: BarDisplays,
    #[serde(default = "default_height")]
    pub height: u32,
    /// Legacy field; vertical bars use `height` for cross-axis thickness so the
    /// strip matches a horizontal top/bottom bar. Kept for config compatibility.
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
    /// Background opacity for the start-menu popover (text/icons stay opaque).
    /// Applied by the shell as a CSS override, mirroring `opacity` for the bar.
    #[serde(default = "default_menu_opacity")]
    pub menu_opacity: f32,
    /// Background opacity for compositor-drawn window titlebars (title text and
    /// the traffic-light buttons stay opaque). Consumed by the compositor.
    #[serde(default = "default_titlebar_opacity")]
    pub titlebar_opacity: f32,
    /// Appearance of the thin accent border around window title pills. Consumed by
    /// the compositor.
    #[serde(default)]
    pub titlebar_pill_border: TitlebarPillBorder,
    /// Appearance + thickness of the window frame border. Consumed by the compositor;
    /// `width_px` also insets the client body.
    #[serde(default)]
    pub window_border: WindowBorder,
    /// Appearance + thickness of the border around the edge bar's pill. Consumed by
    /// the shell (rendered via GTK CSS); `width_px = 0` disables it.
    #[serde(default)]
    pub bar_border: BarBorder,
    #[serde(default = "default_true")]
    pub blur: bool,
    /// Gaussian backdrop-blur radius (in pixels) applied by the compositor behind
    /// the bar when `blur` is enabled. Consumed by the compositor via bar.json.
    #[serde(default = "default_blur_radius")]
    pub blur_radius: f32,
    /// When false, compositor window effects (minimize genie, maximize wobble,
    /// titlebar slide) run instantly.
    #[serde(default = "default_true")]
    pub window_animations: bool,
    /// How StatusNotifier tray icons are shown on the edge bar.
    #[serde(default)]
    pub tray_icon_mode: TrayIconMode,
    #[serde(default = "default_widgets")]
    pub widgets: Vec<BarWidgetId>,
    #[serde(default)]
    pub clock: ClockConfig,
    /// Number of workspace indicator dots (1–12).
    #[serde(default = "default_workspace_count")]
    pub workspace_count: u32,
    /// How workspaces behave across multiple monitors (independent vs. linked).
    #[serde(default)]
    pub workspace_mode: WorkspaceMode,
    /// Layout mode new workspaces start in (grid tiling vs. scrolling strip).
    #[serde(default)]
    pub default_layout: DefaultLayout,
    /// App ids pinned to the taskbar/dock, in display order. Independent of the
    /// launcher's `menu.json` pins. Persisted by the tasks widget.
    #[serde(default)]
    pub taskbar_pinned: Vec<String>,
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
    4
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

fn default_menu_opacity() -> f32 {
    0.92
}

fn default_titlebar_opacity() -> f32 {
    1.0
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
        BarWidgetId::Tasks,
        BarWidgetId::Spacer,
        BarWidgetId::Tray,
        BarWidgetId::Weather,
        BarWidgetId::Battery,
        BarWidgetId::Network,
        BarWidgetId::Bluetooth,
        BarWidgetId::Volume,
        BarWidgetId::Clipboard,
        BarWidgetId::Notifications,
        BarWidgetId::Clock,
    ]
}

impl Default for BarConfig {
    fn default() -> Self {
        Self {
            position: BarPosition::Top,
            displays: BarDisplays::default(),
            height: default_height(),
            width: default_width(),
            margin_top: default_margin_top(),
            margin_h: default_margin_h(),
            full_width: default_full_width(),
            opacity: default_opacity(),
            menu_opacity: default_menu_opacity(),
            titlebar_opacity: default_titlebar_opacity(),
            titlebar_pill_border: TitlebarPillBorder::default(),
            window_border: WindowBorder::default(),
            bar_border: BarBorder::default(),
            blur: default_true(),
            blur_radius: default_blur_radius(),
            window_animations: default_true(),
            tray_icon_mode: TrayIconMode::default(),
            widgets: default_widgets(),
            clock: ClockConfig::default(),
            workspace_count: default_workspace_count(),
            workspace_mode: WorkspaceMode::default(),
            default_layout: DefaultLayout::default(),
            taskbar_pinned: Vec::new(),
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
        BarWidgetId::Bluetooth,
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
        BarWidgetId::Bluetooth,
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
                        | BarWidgetId::Bluetooth
                        |                     BarWidgetId::Volume
                        | BarWidgetId::Clipboard
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

    // Insert the taskbar/dock into pre-existing layouts that predate it, just
    // after the workspaces cluster on the left (or at the front otherwise).
    if !cfg.widgets.contains(&BarWidgetId::Tasks) {
        let pos = cfg
            .widgets
            .iter()
            .position(|w| matches!(w, BarWidgetId::Workspaces))
            .map(|i| i + 1)
            .unwrap_or(0);
        cfg.widgets.insert(pos, BarWidgetId::Tasks);
        if let Ok(json) = serde_json::to_string_pretty(&*cfg) {
            let _ = std::fs::write(bar_config_path(), json);
        }
    }

    // Insert the Bluetooth indicator after Network in layouts that predate it.
    if !cfg.widgets.contains(&BarWidgetId::Bluetooth) {
        let pos = cfg
            .widgets
            .iter()
            .position(|w| matches!(w, BarWidgetId::Network))
            .map(|i| i + 1)
            .unwrap_or(cfg.widgets.len());
        cfg.widgets.insert(pos, BarWidgetId::Bluetooth);
        if let Ok(json) = serde_json::to_string_pretty(&*cfg) {
            let _ = std::fs::write(bar_config_path(), json);
        }
    }

    // Insert the clipboard history widget before notifications in older layouts.
    if !cfg.widgets.contains(&BarWidgetId::Clipboard) {
        let pos = cfg
            .widgets
            .iter()
            .position(|w| matches!(w, BarWidgetId::Notifications))
            .unwrap_or(cfg.widgets.len());
        cfg.widgets.insert(pos, BarWidgetId::Clipboard);
        if let Ok(json) = serde_json::to_string_pretty(&*cfg) {
            let _ = std::fs::write(bar_config_path(), json);
        }
    }

    // Insert the system tray widget immediately left of the weather cluster.
    if !cfg.widgets.contains(&BarWidgetId::Tray) {
        let pos = cfg
            .widgets
            .iter()
            .position(|w| matches!(w, BarWidgetId::Weather))
            .unwrap_or(cfg.widgets.len());
        cfg.widgets.insert(pos, BarWidgetId::Tray);
        if let Ok(json) = serde_json::to_string_pretty(&*cfg) {
            let _ = std::fs::write(bar_config_path(), json);
        }
    } else {
        // Reposition tray if it was placed elsewhere in an older layout.
        let weather_pos = cfg.widgets.iter().position(|w| matches!(w, BarWidgetId::Weather));
        let tray_pos = cfg.widgets.iter().position(|w| matches!(w, BarWidgetId::Tray));
        if let (Some(wpos), Some(tpos)) = (weather_pos, tray_pos) {
            if tpos != wpos.saturating_sub(1) {
                cfg.widgets.remove(tpos);
                let insert_at = cfg
                    .widgets
                    .iter()
                    .position(|w| matches!(w, BarWidgetId::Weather))
                    .unwrap_or(cfg.widgets.len());
                cfg.widgets.insert(insert_at, BarWidgetId::Tray);
                if let Ok(json) = serde_json::to_string_pretty(&*cfg) {
                    let _ = std::fs::write(bar_config_path(), json);
                }
            }
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
    let path = bar_config_path();
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &json)?;
    std::fs::rename(tmp, path)
}
