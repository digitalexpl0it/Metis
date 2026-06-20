//! Apply the same Metis theme tokens the shell uses, so the settings window looks
//! native to the desktop. Builds CSS from the shared `metis_config` stylesheet.

use gtk::CssProvider;
use gtk::STYLE_PROVIDER_PRIORITY_APPLICATION;

use metis_config::{ThemeMode, ThemeTokens};

/// Resolve the currently active theme tokens (honouring the saved mode, with a
/// GTK fallback for `system`).
pub fn active_tokens() -> ThemeTokens {
    let mode = metis_config::load_theme_preference().unwrap_or(ThemeMode::Dark);
    let name = match mode {
        ThemeMode::Light => "light",
        ThemeMode::Dark => "dark",
        ThemeMode::System => {
            if prefers_dark() {
                "dark"
            } else {
                "light"
            }
        }
    };
    metis_config::load_theme_tokens(name)
}

fn prefers_dark() -> bool {
    gtk::Settings::default()
        .map(|s| s.is_gtk_application_prefer_dark_theme())
        .unwrap_or(true)
}

pub fn install() {
    let tokens = active_tokens();
    let Some(display) = gtk::gdk::Display::default() else {
        return;
    };

    let base = CssProvider::new();
    base.load_from_data(&metis_config::build_stylesheet(&tokens));
    gtk::style_context_add_provider_for_display(&display, &base, STYLE_PROVIDER_PRIORITY_APPLICATION);

    let extra = CssProvider::new();
    extra.load_from_data(&settings_css(&tokens));
    gtk::style_context_add_provider_for_display(
        &display,
        &extra,
        STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
    );
}

/// Settings-window chrome that isn't part of the shared bar stylesheet.
fn settings_css(t: &ThemeTokens) -> String {
    let bg = &t.bg;
    let surface = &t.surface;
    let raised = &t.surface_raised;
    let border = &t.border;
    let text = &t.text;
    let muted = &t.text_muted;
    let accent = t.accent_primary();
    let rl = t.radius_lg;
    format!(
        r#"
        .metis-settings-window {{ background-color: {bg}; color: {text}; }}
        .metis-settings-root {{ background-color: {bg}; }}
        .metis-settings-sidebar {{ background-color: {surface}; }}
        .metis-settings-sidebar list {{ background-color: transparent; padding: 8px; }}
        .metis-settings-sidebar row {{ border-radius: {rl}px; padding: 8px 12px; }}
        .metis-settings-sidebar row:selected {{ background-color: {accent}; }}
        .metis-settings-page {{ background-color: {bg}; }}
        .metis-settings-title {{ font-size: 24px; font-weight: 800; color: {text}; }}
        .metis-settings-section {{
            background-color: {surface};
            border: 1px solid {border};
            border-radius: {rl}px;
            padding: 16px;
        }}
        .metis-settings-section-title {{ font-size: 13px; font-weight: 700; color: {muted}; }}
        .metis-settings-row {{ padding: 4px 0; }}
        .metis-settings-row > label {{ color: {text}; }}
        .metis-settings-hint {{ color: {muted}; font-size: 12px; }}
        .metis-settings-list {{
            background-color: {raised};
            border: 1px solid {border};
            border-radius: {rl}px;
        }}
        .metis-settings-list row {{ padding: 8px 10px; }}
        "#
    )
}
