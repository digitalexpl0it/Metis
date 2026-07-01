//! Metis Menu settings: which terminal / file manager the launcher rail opens
//! (auto-detected installs or a custom path, persisted to `menu.json`), plus the
//! menu panel opacity (stored in `bar.json`). The shell reads the launcher choices
//! at click time; the opacity change nudges a `reload-bar` so it applies live.

use std::rc::Rc;

use gtk::gio;
use gtk::prelude::*;

use metis_config::MenuConfig;

use crate::{runtime, ui};

pub fn build() -> gtk::Widget {
    let (scroller, content) = ui::page_for("menu");
    let cfg = metis_config::load_menu_config();

    // ---- Quick launchers --------------------------------------------------
    let (launch_card, launch_body) =
        ui::section_with_icon("Quick launchers", "applications-utilities-symbolic");

    launch_body.append(&picker_row(
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

    launch_body.append(&picker_row(
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

/// A labelled dropdown of installed candidates (plus Auto-detect / Custom), with a
/// revealed path entry + file chooser when Custom is selected. `on_change` receives
/// the chosen value (`None` = auto-detect) whenever the selection or path changes.
fn picker_row(
    icon: &str,
    label: &str,
    candidates: &[(&str, &str)],
    current: Option<String>,
    on_change: impl Fn(Option<String>) + 'static,
) -> gtk::Box {
    let installed: Vec<(String, String)> = candidates
        .iter()
        .filter(|(bin, _)| metis_config::binary_in_path(bin))
        .map(|(bin, lbl)| (bin.to_string(), lbl.to_string()))
        .collect();

    // Dropdown labels: [Auto-detect, <installed…>, Custom…]. Index 0 = auto, the
    // last index = custom; everything between maps to `installed[idx - 1]`.
    let mut labels: Vec<String> = Vec::with_capacity(installed.len() + 2);
    labels.push("Auto-detect".to_string());
    for (_, lbl) in &installed {
        labels.push(lbl.clone());
    }
    labels.push("Custom…".to_string());
    let custom_index = (labels.len() - 1) as u32;
    let label_refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let dd = gtk::DropDown::from_strings(&label_refs);

    let entry = gtk::Entry::builder()
        .placeholder_text("Path to executable, e.g. /usr/bin/foot")
        .hexpand(true)
        .build();
    let browse = gtk::Button::from_icon_name("document-open-symbolic");
    browse.set_tooltip_text(Some("Browse…"));
    let custom_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    custom_box.append(&entry);
    custom_box.append(&browse);
    custom_box.set_visible(false);

    // Apply the saved choice before wiring signals so opening the page doesn't
    // trigger a spurious save.
    if let Some(cur) = current.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        if let Some(pos) = installed.iter().position(|(bin, _)| bin == cur) {
            dd.set_selected(1 + pos as u32);
        } else {
            dd.set_selected(custom_index);
            entry.set_text(cur);
            custom_box.set_visible(true);
        }
    }

    let on_change = Rc::new(on_change);
    let installed_bins: Vec<String> = installed.iter().map(|(bin, _)| bin.clone()).collect();

    {
        let entry = entry.clone();
        let custom_box = custom_box.clone();
        let on_change = on_change.clone();
        let installed_bins = installed_bins.clone();
        dd.connect_selected_notify(move |dd| {
            let sel = dd.selected();
            if sel == 0 {
                custom_box.set_visible(false);
                on_change(None);
            } else if sel == custom_index {
                custom_box.set_visible(true);
                on_change(non_empty(&entry.text()));
            } else {
                custom_box.set_visible(false);
                on_change(installed_bins.get((sel - 1) as usize).cloned());
            }
        });
    }

    {
        let dd = dd.clone();
        let on_change = on_change.clone();
        entry.connect_changed(move |e| {
            if dd.selected() == custom_index {
                on_change(non_empty(&e.text()));
            }
        });
    }

    {
        let entry = entry.clone();
        browse.connect_clicked(move |btn| {
            let dialog = gtk::FileDialog::new();
            dialog.set_title("Choose an executable");
            let parent = btn.root().and_downcast::<gtk::Window>();
            let entry = entry.clone();
            dialog.open(parent.as_ref(), gio::Cancellable::NONE, move |res| {
                if let Ok(file) = res {
                    if let Some(path) = file.path() {
                        entry.set_text(&path.to_string_lossy());
                    }
                }
            });
        });
    }

    let container = gtk::Box::new(gtk::Orientation::Vertical, 8);
    container.append(&ui::row_with_icon(icon, label, &dd));
    container.append(&custom_box);
    container
}

fn non_empty(text: &str) -> Option<String> {
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn persist(cfg: &MenuConfig) {
    if let Err(err) = metis_config::save_menu_config(cfg) {
        tracing::warn!(%err, "failed to save menu.json launcher choice");
    }
}

/// Re-read `bar.json`, set only `menu_opacity`, and save — mirrors the Appearance
/// page's merge so the two pages never clobber each other's bar fields.
fn set_menu_opacity(value: f32) {
    let mut bar = metis_config::load_bar_config();
    bar.menu_opacity = value;
    if let Err(err) = metis_config::save_bar_config(&bar) {
        tracing::warn!(%err, "failed to save bar.json menu opacity");
        return;
    }
    runtime::send("reload-bar");
}
