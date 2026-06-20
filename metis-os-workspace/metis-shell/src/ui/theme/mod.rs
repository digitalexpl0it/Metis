mod loader;

pub use loader::{
    apply_bar_opacity, export_embedded_themes_to_config, init_theme, reload_stylesheet,
    set_theme_mode,
};
// Theme tokens, semantic colors, mode, and the stylesheet builder now live in the
// shared `metis-config` crate; re-export so `crate::ui::theme::...` keeps working
// for any external reference.
#[allow(unused_imports)]
pub use metis_config::{build_stylesheet, SemanticColors, ThemeMode, ThemeTokens};

pub fn install_theme() {
    let _ = init_theme();
    crate::ui::icons::install();
}
