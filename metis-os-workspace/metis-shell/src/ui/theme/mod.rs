mod css;
mod loader;
mod tokens;

pub use loader::{export_embedded_themes_to_config, init_theme, reload_stylesheet, set_theme_mode};
pub use tokens::ThemeMode;

pub fn install_theme() {
    let _ = init_theme();
    crate::ui::icons::install();
}
