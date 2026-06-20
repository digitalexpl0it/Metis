use std::cell::RefCell;

use gtk::{CssProvider, STYLE_PROVIDER_PRIORITY_APPLICATION};

use crate::config;
use metis_config::{build_stylesheet, ThemeMode, ThemeTokens};

thread_local! {
    static THEME_STATE: RefCell<ThemeState> = RefCell::new(ThemeState {
        tokens: ThemeTokens::dark_default(),
        provider: CssProvider::new(),
    });
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

pub fn set_theme_mode(mode: ThemeMode) {
    let tokens = match mode {
        ThemeMode::Dark => ThemeTokens::dark_default(),
        ThemeMode::Light => ThemeTokens::light_default(),
        ThemeMode::System => detect_system_theme(),
    };
    apply_tokens(&tokens);
    if let Err(err) = config::save_theme_preference(mode) {
        tracing::warn!("failed to save theme preference: {err}");
    }
}

fn load_active_theme() -> ThemeTokens {
    let mode = config::load_theme_preference().unwrap_or(ThemeMode::Dark);
    let user_path = config::theme_file_path(&mode);
    if user_path.exists() {
        if let Ok(text) = std::fs::read_to_string(&user_path) {
            if let Ok(tokens) = serde_json::from_str(&text) {
                return tokens;
            }
        }
    }
    match mode {
        ThemeMode::Light => ThemeTokens::light_default(),
        ThemeMode::System => detect_system_theme(),
        ThemeMode::Dark => ThemeTokens::dark_default(),
    }
}

fn detect_system_theme() -> ThemeTokens {
    if let Some(settings) = gtk::Settings::default() {
        if settings.is_gtk_application_prefer_dark_theme() {
            return ThemeTokens::dark_default();
        }
    }
    ThemeTokens::light_default()
}

fn apply_tokens(tokens: &ThemeTokens) {
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
}

pub fn reload_stylesheet() {
    THEME_STATE.with(|state| {
        let tokens = state.borrow().tokens.clone();
        apply_tokens(&tokens);
    });
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
