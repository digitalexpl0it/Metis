//! Per-app Metis titlebar overrides — Auto / Metis titlebar / App titlebar.
//! Persists to `decorations.json` and reloads the compositor live.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use metis_config::{DecorationsConfig, DecorationsOverride};

use crate::apps::{self, AppEntry};
use crate::{runtime, ui};

struct RowEntry {
    widget: gtk::Widget,
    /// Lowercase haystack for filtering (name + desktop id).
    search_text: String,
}

pub fn build() -> gtk::Widget {
    apps::watch_app_index();

    let (scroller, content) = ui::page_for("titlebars");

    let hint = gtk::Label::new(Some(
        "Auto uses Metis defaults. Override only when an app shows a double \
         titlebar or is missing Metis window controls. Changes apply immediately.",
    ));
    hint.set_xalign(0.0);
    hint.set_wrap(true);
    hint.add_css_class("metis-settings-hint");
    content.append(&hint);

    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Search applications…"));
    search.set_hexpand(true);
    content.append(&search);

    let (list_card, list_body) =
        ui::section_with_icon("Applications", "application-x-executable-symbolic");
    list_body.set_spacing(4);
    content.append(&list_card);

    let bottom_pad = gtk::Box::new(gtk::Orientation::Vertical, 0);
    bottom_pad.set_size_request(-1, 48);
    content.append(&bottom_pad);

    let list_body = Rc::new(list_body);
    let rows: Rc<RefCell<Vec<RowEntry>>> = Rc::new(RefCell::new(Vec::new()));
    let empty_label = gtk::Label::new(Some("No matching applications"));
    empty_label.add_css_class("metis-settings-hint");
    empty_label.set_xalign(0.0);
    empty_label.set_visible(false);
    let empty_label = Rc::new(empty_label);

    let rebuild_block = Rc::new(RefCell::new(false));
    let search_q = Rc::new(RefCell::new(String::new()));
    // Match other Settings pages: clear the slot inside the timeout body so we
    // never `SourceId::remove()` a timer that has already fired (that aborts).
    let debounce: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));

    // Drop Auto-covered / icon-noise keys left from earlier expand passes.
    prune_redundant_overrides();

    let rebuild_all: Rc<dyn Fn()> = {
        let list_body = list_body.clone();
        let rows = rows.clone();
        let empty_label = empty_label.clone();
        let rebuild_block = rebuild_block.clone();
        let search_q = search_q.clone();
        let debounce = debounce.clone();
        Rc::new(move || {
            // Cancel pending filter — it would `borrow` rows while we rebuild.
            if let Some(id) = debounce.borrow_mut().take() {
                id.remove();
            }
            rebuild_list(
                &list_body,
                &rows,
                &empty_label,
                &rebuild_block,
                &search_q.borrow(),
            );
        })
    };

    rebuild_all();
    apps::register_refresh(rebuild_all.clone());

    search.connect_search_changed({
        let search_q = search_q.clone();
        let rows = rows.clone();
        let empty_label = empty_label.clone();
        let debounce = debounce.clone();
        move |entry| {
            let new_q = entry.text().to_string();
            if *search_q.borrow() == new_q {
                return;
            }
            *search_q.borrow_mut() = new_q.clone();

            let mut slot = debounce.borrow_mut();
            if let Some(id) = slot.take() {
                id.remove();
            }
            let rows = rows.clone();
            let empty_label = empty_label.clone();
            let debounce = debounce.clone();
            let gen = new_q;
            let id = glib::timeout_add_local(std::time::Duration::from_millis(80), move || {
                *debounce.borrow_mut() = None;
                apply_filter(&rows, &empty_label, &gen);
                glib::ControlFlow::Break
            });
            *slot = Some(id);
        }
    });

    scroller.upcast()
}

/// Remove noise keys and Auto-covered `"client"` overrides so decorations.json
/// stays a small escape hatch (e.g. keep `mpv: server` only).
fn prune_redundant_overrides() {
    let mut cfg = metis_config::load_decorations_config();
    let before = cfg.overrides.len();
    cfg.overrides.retain(|key, _mode| {
        if is_noise_override_key(key) {
            return false;
        }
        // Text Editor + Metis Settings are intentionally Metis SSD — drop
        // accidental "App titlebar" forces from the bulk pass. Do NOT strip
        // ordinary "client" overrides for apps Auto supposedly covers: Auto can
        // still miss (app_id mismatch), and pruning made Settings toggles look
        // like they never stick (Transmission).
        if matches!(
            key.as_str(),
            "org.gnome.texteditor"
                | "gnome-text-editor"
                | "texteditor"
                | "metis-settings"
                | "com.metis.settings"
                | "io.github.shiftey.desktop"
                | "io.github.shiftkey.githubdesktop"
                | "github desktop"
        ) {
            return false;
        }
        true
    });
    if cfg.overrides.len() != before {
        if let Err(err) = metis_config::save_decorations_config(&cfg) {
            tracing::warn!(%err, "failed to prune decorations.json");
            return;
        }
        runtime::reload_decorations_async();
        tracing::info!(
            before,
            after = cfg.overrides.len(),
            "pruned decorations.json overrides"
        );
    }
}

fn is_noise_override_key(key: &str) -> bool {
    let k = key.trim().to_ascii_lowercase();
    if k.is_empty() || k.chars().all(|c| c.is_ascii_digit()) {
        return true;
    }
    if k.contains('_') && k.chars().any(|c| c.is_ascii_hexdigit()) && k.contains('.') {
        // Wine installer desktop-id noise: `87f4_notepad.0`
        if k.split('_').next().is_some_and(|p| p.len() <= 6) {
            return true;
        }
    }
    matches!(
        k.as_str(),
        "0" | "application"
            | "env"
            | "flatpak"
            | "desktop"
            | "package-x-generic"
            | "multimedia-volume-control"
            | "printer"
            | "preferences-desktop-locale"
            | "preferences-desktop-theme"
            | "settings"
            | "notepad"
            | "session-properties"
            | "powerstats"
            | "volman"
            | "rhythmbox3"
            | "texteditor"
    ) || k.starts_with("wine-programs-")
        || k.ends_with(".0")
        || k.ends_with(".chm")
}

fn rebuild_list(
    list_body: &gtk::Box,
    rows: &Rc<RefCell<Vec<RowEntry>>>,
    empty_label: &gtk::Label,
    rebuild_block: &Rc<RefCell<bool>>,
    query: &str,
) {
    while let Some(child) = list_body.first_child() {
        list_body.remove(&child);
    }
    rows.borrow_mut().clear();

    let cfg = metis_config::load_decorations_config();
    let mut apps = apps::list_apps();
    apps.sort_by(|a, b| {
        let a_custom = cfg.mode_for_candidates(&a.decoration_candidates()).is_some();
        let b_custom = cfg.mode_for_candidates(&b.decoration_candidates()).is_some();
        match (a_custom, b_custom) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        }
    });

    *rebuild_block.borrow_mut() = true;
    let mut built = Vec::with_capacity(apps.len());
    for entry in apps {
        let search_text = format!("{} {}", entry.name, entry.id).to_lowercase();
        let row = app_row(&entry, &cfg, rebuild_block);
        list_body.append(&row);
        built.push(RowEntry {
            widget: row,
            search_text,
        });
    }
    list_body.append(empty_label);
    *rebuild_block.borrow_mut() = false;
    *rows.borrow_mut() = built;
    apply_filter(rows, empty_label, query);
}

fn apply_filter(rows: &Rc<RefCell<Vec<RowEntry>>>, empty_label: &gtk::Label, query: &str) {
    let Ok(rows) = rows.try_borrow() else {
        // Rebuild in progress — skip this frame.
        return;
    };
    let q = query.trim().to_lowercase();
    let mut visible = 0usize;
    for row in rows.iter() {
        let show = q.is_empty() || row.search_text.contains(&q);
        row.widget.set_visible(show);
        if show {
            visible += 1;
        }
    }
    empty_label.set_visible(visible == 0);
}

fn app_row(
    entry: &AppEntry,
    cfg: &DecorationsConfig,
    rebuild_block: &Rc<RefCell<bool>>,
) -> gtk::Widget {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.set_hexpand(true);
    row.add_css_class("metis-settings-row");

    let image = gtk::Image::new();
    image.set_pixel_size(28);
    if let Some(icon) = &entry.icon {
        image.set_from_gicon(icon);
    } else {
        image.set_icon_name(Some("application-x-executable-symbolic"));
    }
    row.append(&image);

    let labels = gtk::Box::new(gtk::Orientation::Vertical, 2);
    labels.set_hexpand(true);
    labels.set_valign(gtk::Align::Center);
    let name = gtk::Label::new(Some(&entry.name));
    name.set_xalign(0.0);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    labels.append(&name);
    if cfg
        .mode_for_candidates(&entry.decoration_candidates())
        .is_some()
    {
        let badge = gtk::Label::new(Some("Customized"));
        badge.set_xalign(0.0);
        badge.add_css_class("metis-settings-hint");
        labels.append(&badge);
    }
    row.append(&labels);

    let mode_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    mode_box.add_css_class("linked");
    mode_box.set_halign(gtk::Align::End);
    mode_box.set_valign(gtk::Align::Center);

    let current = mode_to_index(cfg.mode_for_candidates(&entry.decoration_candidates()));
    let candidates = Rc::new(entry.decoration_candidates());
    let buttons: Rc<RefCell<Vec<gtk::ToggleButton>>> = Rc::new(RefCell::new(Vec::new()));

    for (idx, label) in ["Auto", "Metis", "App"].iter().enumerate() {
        let btn = gtk::ToggleButton::with_label(label);
        btn.set_active(idx as u32 == current);
        if idx > 0 {
            if let Some(first) = buttons.borrow().first() {
                btn.set_group(Some(first));
            }
        }
        buttons.borrow_mut().push(btn.clone());

        let rebuild_block = rebuild_block.clone();
        let candidates = candidates.clone();
        let buttons_ref = buttons.clone();
        btn.connect_toggled(move |btn| {
            if *rebuild_block.borrow() || !btn.is_active() {
                return;
            }
            let selected = buttons_ref
                .borrow()
                .iter()
                .position(|b| b.is_active())
                .unwrap_or(0) as u32;
            let mode = index_to_mode(selected);
            let mut cfg = metis_config::load_decorations_config();
            cfg.set_for_candidates(&candidates, mode);
            if let Err(err) = metis_config::save_decorations_config(&cfg) {
                tracing::warn!(%err, "failed to save decorations.json");
                return;
            }
            runtime::reload_decorations_async();
        });
        mode_box.append(&btn);
    }

    row.append(&mode_box);
    row.upcast()
}

fn mode_to_index(mode: Option<DecorationsOverride>) -> u32 {
    match mode {
        None => 0,
        Some(DecorationsOverride::Server) => 1,
        Some(DecorationsOverride::Client) => 2,
    }
}

fn index_to_mode(idx: u32) -> Option<DecorationsOverride> {
    match idx {
        1 => Some(DecorationsOverride::Server),
        2 => Some(DecorationsOverride::Client),
        _ => None,
    }
}
