use std::cell::{Cell, RefCell};

use gtk::{CssProvider, STYLE_PROVIDER_PRIORITY_APPLICATION, STYLE_PROVIDER_PRIORITY_USER};

use crate::config;
use metis_config::{
    build_stylesheet, BarBorder, BarPosition, BorderMode, ThemeMode, ThemeTokens,
};

thread_local! {
    static THEME_STATE: RefCell<ThemeState> = RefCell::new(ThemeState {
        tokens: ThemeTokens::dark_default(),
        provider: CssProvider::new(),
    });
    static BAR_BG_PROVIDER: CssProvider = CssProvider::new();
    static BAR_OPACITY: Cell<f32> = const { Cell::new(1.0) };
    static BAR_BORDER: RefCell<BarBorder> = RefCell::new(BarBorder::default());
    static BAR_POSITION: Cell<BarPosition> = const { Cell::new(BarPosition::Top) };
    static MENU_BG_PROVIDER: CssProvider = CssProvider::new();
    static MENU_OPACITY: Cell<f32> = const { Cell::new(1.0) };
}

struct ThemeState {
    tokens: ThemeTokens,
    provider: CssProvider,
}

pub fn init_theme() -> ThemeTokens {
    let tokens = load_active_theme();
    apply_tokens(&tokens);
    tokens
}

pub fn active_tokens() -> ThemeTokens {
    THEME_STATE.with(|state| state.borrow().tokens.clone())
}

fn load_active_theme() -> ThemeTokens {
    config::load_theme_tokens(effective_theme_name())
}

/// Resolve the on-disk theme token file name for the saved preference.
fn effective_theme_name() -> &'static str {
    match config::load_theme_preference().unwrap_or(ThemeMode::Dark) {
        ThemeMode::Light => "light",
        ThemeMode::Dark => "dark",
        ThemeMode::System => {
            if detect_system_prefers_dark() {
                "dark"
            } else {
                "light"
            }
        }
    }
}

fn detect_system_prefers_dark() -> bool {
    gtk::Settings::default()
        .map(|s| s.is_gtk_application_prefer_dark_theme())
        .unwrap_or(true)
}

/// Match GTK's built-in Adwaita variant to the saved theme preference so native
/// widget chrome (scrollbars, undershoot, popovers) stays consistent after a
/// live `reload-theme` — mirrors `metis-settings::theme::reapply`.
fn sync_gtk_theme_variant(_tokens: &ThemeTokens) {
    if let Some(settings) = gtk::Settings::default() {
        settings.set_gtk_application_prefer_dark_theme(active_mode_is_dark());
    }
}

fn active_mode_is_dark() -> bool {
    match config::load_theme_preference().unwrap_or(ThemeMode::Dark) {
        ThemeMode::Dark => true,
        ThemeMode::Light => false,
        ThemeMode::System => detect_system_prefers_dark(),
    }
}

fn apply_tokens(tokens: &ThemeTokens) {
    sync_gtk_theme_variant(tokens);
    let css = build_stylesheet(tokens);
    THEME_STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.tokens = tokens.clone();
        state.provider.load_from_data(&css);
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &state.provider,
                STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
    });
    // Re-apply the bar + menu background opacity (and bar border) on top of the
    // (possibly new) surface/accent colours so theme edits don't reset the
    // configured transparency or border.
    apply_bar_appearance(
        BAR_OPACITY.with(Cell::get),
        &BAR_BORDER.with(|b| b.borrow().clone()),
        BAR_POSITION.with(Cell::get),
    );
    apply_menu_opacity(MENU_OPACITY.with(Cell::get));
    crate::ui::dashboard::on_theme_changed();
    crate::ui::screenshot::on_theme_changed();
    crate::ui::notification_center::on_theme_changed();
}

/// Apply the edge bar's background transparency *and* its configurable border
/// (accent / solid / custom gradient, with a width of 0 disabling it).
///
/// We override only the `.metis-bar-pill` surface via a dedicated, higher-priority
/// provider — never `window.set_opacity()`, which would also fade the icons and
/// text. For a gradient/accent border on the rounded pill we use the layered
/// `background-clip` trick (a `padding-box` surface layer over a `border-box`
/// gradient under a transparent border) so the stroke follows the pill's rounded
/// corners, which `border-image` cannot do. The gradient flows along the bar's
/// long axis (left→right when horizontal, top→bottom when vertical).
pub fn apply_bar_appearance(opacity: f32, border: &BarBorder, position: BarPosition) {
    let alpha = opacity.clamp(0.0, 1.0);
    BAR_OPACITY.with(|o| o.set(alpha));
    BAR_BORDER.with(|b| *b.borrow_mut() = border.clone());
    BAR_POSITION.with(|p| p.set(position));

    let surface_rgb = active_tokens().surface_rgb();
    let surface = format!("rgba({surface_rgb}, {alpha:.3})");
    let width = border.width_px.max(0.0);

    let css = if width <= 0.0 {
        format!(
            ".metis-bar-pill {{ border: none; background-image: none; \
             background-color: {surface}; }}"
        )
    } else {
        match border.mode {
            BorderMode::Solid => format!(
                ".metis-bar-pill {{ border: {width}px solid {color}; \
                 background-image: none; background-color: {surface}; }}",
                color = border.color,
            ),
            BorderMode::Accent | BorderMode::Gradient => {
                let dir = match position {
                    BarPosition::Left | BarPosition::Right => "to bottom",
                    BarPosition::Top | BarPosition::Bottom => "to right",
                };
                // GTK's CSS engine does not reliably confine the lower gradient
                // layer to the border ring (background-clip is ignored), so an
                // opaque gradient bleeds across the whole pill and the opacity
                // slider has no visible effect. When the fill is translucent,
                // fall back to a solid accent stroke + rgba surface instead.
                if alpha < 1.0 - f32::EPSILON {
                    let stroke = bar_border_primary_color(border);
                    format!(
                        ".metis-bar-pill {{ border: {width}px solid {stroke}; \
                         background-image: none; background-color: {surface}; }}"
                    )
                } else {
                    let stops = bar_border_stops(border);
                    format!(
                        ".metis-bar-pill {{ \
                         border: {width}px solid transparent; \
                         background-color: transparent; \
                         background-image: linear-gradient({surface}, {surface}), \
                         linear-gradient({dir}, {stops}); \
                         background-clip: padding-box, border-box; \
                         background-origin: padding-box, border-box; }}"
                    )
                }
            }
        }
    };

    BAR_BG_PROVIDER.with(|provider| {
        provider.load_from_data(&css);
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                provider,
                STYLE_PROVIDER_PRIORITY_USER,
            );
        }
    });
}

/// Primary stroke colour for accent/gradient border modes (first accent or
/// gradient stop).
fn bar_border_primary_color(border: &BarBorder) -> String {
    match border.mode {
        BorderMode::Accent => active_tokens().accent_primary().to_string(),
        BorderMode::Gradient => border
            .gradient
            .first()
            .cloned()
            .unwrap_or_else(|| "#00F2FE".into()),
        BorderMode::Solid => border.color.clone(),
    }
}

/// Build the comma-separated CSS gradient stops for the bar border at full
/// opacity (used only when the fill is opaque — see `apply_bar_appearance`).
fn bar_border_stops(border: &BarBorder) -> String {
    let mut hexes: Vec<String> = match border.mode {
        BorderMode::Accent => active_tokens().accent.clone(),
        BorderMode::Gradient => border.gradient.clone(),
        BorderMode::Solid => vec![border.color.clone()],
    };
    hexes.retain(|s| !s.trim().is_empty());
    if hexes.is_empty() {
        hexes.push("#00F2FE".to_string());
    }
    if hexes.len() == 1 {
        hexes.push(hexes[0].clone());
    }
    hexes.join(", ")
}

/// Apply the start-menu popover's configurable background transparency.
///
/// Like [`apply_bar_opacity`], this overrides only the `.metis-menu-panel`
/// background alpha via a dedicated, higher-priority provider — never
/// `popover.set_opacity()` — so the menu's text, icons, and tiles stay fully
/// opaque while only the panel surface dims.
pub fn apply_menu_opacity(opacity: f32) {
    let alpha = opacity.clamp(0.0, 1.0);
    MENU_OPACITY.with(|o| o.set(alpha));
    let raised_rgb = active_tokens().surface_raised_rgb();
    let css = format!(
        ".metis-menu-panel {{ background-color: rgba({raised_rgb}, {alpha:.3}); }}"
    );
    MENU_BG_PROVIDER.with(|provider| {
        provider.load_from_data(&css);
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                provider,
                STYLE_PROVIDER_PRIORITY_USER,
            );
        }
    });
}

pub fn reload_stylesheet() {
    let tokens = load_active_theme();
    apply_tokens(&tokens);
}

pub fn export_embedded_themes_to_config() -> std::io::Result<()> {
    config::ensure_config_dirs()?;
    write_theme_file(&config::theme_file_path_for_name("dark"), &ThemeTokens::dark_default())?;
    write_theme_file(
        &config::theme_file_path_for_name("light"),
        &ThemeTokens::light_default(),
    )?;
    Ok(())
}

fn write_theme_file(path: &std::path::Path, tokens: &ThemeTokens) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(tokens).map_err(std::io::Error::other)?;
    std::fs::write(path, json)
}
