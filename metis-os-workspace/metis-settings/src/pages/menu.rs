//! Metis Menu settings: which terminal / file manager the launcher rail opens
//! (auto-detected installs or a custom path, persisted to `menu.json`), plus the
//! menu panel opacity (stored in `bar.json`). The shell reads the launcher choices
//! at click time; the opacity change nudges a `reload-bar` so it applies live.

use gtk::prelude::*;
use metis_config::MenuConfig;

use crate::{runtime, ui};

pub fn build() -> gtk::Widget {
    let (scroller, content) = ui::page_for("menu");
    let cfg = metis_config::load_menu_config();

    // ---- Quick launchers --------------------------------------------------
    let (launch_card, launch_body) =
        ui::section_with_icon("Quick launchers", "applications-utilities-symbolic");

    launch_body.append(&ui::launcher_picker(
        "utilities-terminal-symbolic",
        "Terminal",
        metis_config::KNOWN_TERMINALS,
        cfg.terminal.clone(),
        |val| {
            let mut c = metis_config::load_menu_config();
            c.terminal = val;
            persist(&c);
        },
    ));

    launch_body.append(&ui::launcher_picker(
        "system-file-manager-symbolic",
        "File manager",
        metis_config::KNOWN_FILE_MANAGERS,
        cfg.file_manager.clone(),
        |val| {
            let mut c = metis_config::load_menu_config();
            c.file_manager = val;
            persist(&c);
        },
    ));

    let launch_hint = gtk::Label::new(Some(
        "Auto-detect uses the first installed option. Choose Custom to point at any \
         executable on your system.",
    ));
    launch_hint.set_xalign(0.0);
    launch_hint.set_wrap(true);
    launch_hint.add_css_class("metis-settings-hint");
    launch_body.append(&launch_hint);
    content.append(&launch_card);

    // ---- Appearance -------------------------------------------------------
    let (look_card, look_body) = ui::section_with_icon("Appearance", "view-app-grid-symbolic");

    let menu_opacity = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.3, 1.0, 0.01);
    menu_opacity.set_value(metis_config::load_bar_config().menu_opacity as f64);
    menu_opacity.set_size_request(200, -1);
    menu_opacity.set_draw_value(true);
    look_body.append(&ui::row_with_icon(
        "display-brightness-symbolic",
        "Panel opacity",
        &menu_opacity,
    ));
    // Connect after seeding the value so opening the page doesn't write bar.json.
    menu_opacity.connect_value_changed(|s| set_menu_opacity(s.value() as f32));

    let look_hint = gtk::Label::new(Some(
        "Opacity of the Metis menu panel and its translucent surfaces. Applies within ~1s.",
    ));
    look_hint.set_xalign(0.0);
    look_hint.set_wrap(true);
    look_hint.add_css_class("metis-settings-hint");
    look_body.append(&look_hint);
    content.append(&look_card);

    scroller.upcast()
}

fn persist(cfg: &MenuConfig) {
    if let Err(err) = metis_config::save_menu_config(cfg) {
        tracing::warn!(%err, "failed to save menu.json launcher choice");
    }
}

fn set_menu_opacity(value: f32) {
    let mut cfg = metis_config::load_bar_config();
    let clamped = value.clamp(0.3, 1.0);
    if (cfg.menu_opacity - clamped).abs() < f32::EPSILON {
        return;
    }
    cfg.menu_opacity = clamped;
    if let Err(err) = metis_config::save_bar_config(&cfg) {
        tracing::warn!(%err, "failed to save bar.json menu opacity");
        return;
    }
    runtime::send("reload-bar");
}
