//! Alt+Tab MRU window switcher (GTK layer-shell overlay).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::gdk;
use gtk::prelude::*;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use metis_protocol::WindowInfo;

use crate::services::applications;
use crate::services::windows;
use crate::ui::window_thumbs::{self, ThumbSet};

thread_local! {
    static OVERLAY: RefCell<Option<Rc<Switcher>>> = const { RefCell::new(None) };
    static ACTIVATING: Cell<bool> = const { Cell::new(false) };
}

struct Switcher {
    window: gtk::Window,
    strip: gtk::Box,
    cards: RefCell<Vec<gtk::Button>>,
    windows: RefCell<Vec<WindowInfo>>,
    selected: Cell<usize>,
    output: RefCell<Option<String>>,
    thumbs: RefCell<Option<ThumbSet>>,
}

pub fn is_open() -> bool {
    OVERLAY.with(|o| o.borrow().is_some())
}

pub fn cycle_next() {
    cycle(true);
}

pub fn cycle_prev() {
    cycle(false);
}

pub fn activate_selected() {
    if ACTIVATING.with(|c| c.get()) {
        return;
    }
    let id = OVERLAY.with(|o| {
        let borrow = o.borrow();
        let sw = borrow.as_ref()?;
        let windows = sw.windows.borrow();
        let idx = sw.selected.get().min(windows.len().saturating_sub(1));
        windows.get(idx).map(|w| w.id)
    });
    let Some(id) = id else {
        return;
    };
    ACTIVATING.with(|c| c.set(true));
    dismiss();
    // Defer until the Exclusive layer is unmapped so keyboard focus can land
    // on the client instead of the dying overlay surface.
    glib::idle_add_local_once(move || {
        ACTIVATING.with(|c| c.set(false));
        if let Err(err) = crate::compositor::activate_window(id) {
            tracing::warn!(id, %err, "window switcher activate failed");
        } else {
            tracing::info!(id, "window switcher activated");
        }
    });
}

pub fn dismiss() {
    OVERLAY.with(|o| {
        if let Some(sw) = o.borrow_mut().take() {
            sw.window.set_visible(false);
            sw.window.destroy();
        }
    });
    restore_sibling_layers();
}

fn demote_sibling_layers() {
    crate::ui::notification_center::set_below_screenshot(true);
    crate::ui::dashboard::set_below_screenshot(true);
    crate::ui::bar::widgets::set_menu_below_screenshot(true);
}

fn restore_sibling_layers() {
    crate::ui::notification_center::set_below_screenshot(false);
    crate::ui::dashboard::set_below_screenshot(false);
    crate::ui::bar::widgets::set_menu_below_screenshot(false);
}

fn cycle(forward: bool) {
    if crate::ui::workspace_overview::is_open() {
        crate::ui::workspace_overview::dismiss();
    }

    if !is_open() {
        open(forward);
        return;
    }

    OVERLAY.with(|o| {
        let borrow = o.borrow();
        let Some(sw) = borrow.as_ref() else {
            return;
        };
        let n = sw.windows.borrow().len();
        if n == 0 {
            return;
        }
        let cur = sw.selected.get();
        let next = if forward {
            (cur + 1) % n
        } else {
            cur.checked_sub(1).unwrap_or(n - 1)
        };
        sw.selected.set(next);
        refresh_selection(sw);
    });
}

fn open(forward: bool) {
    // Fresh geometry + output/workspace before filtering / capture.
    windows::reconcile_now();

    let output = windows::focused_output_name();
    let workspace = crate::services::active_workspace_for(output.as_deref());
    let list = windows::windows_for_output_workspace(output.as_deref(), workspace);
    if list.is_empty() {
        return;
    }

    // Capture live crops before the dim overlay maps onto the output.
    let thumbs = window_thumbs::capture_window_thumbs(output.as_deref(), &list);

    demote_sibling_layers();

    let window = gtk::Window::builder()
        .title(metis_i18n::tr("Window Switcher"))
        .decorated(false)
        .build();
    window.add_css_class("metis-window-switcher");
    window.init_layer_shell();
    window.set_layer(Layer::Overlay);
    window.set_keyboard_mode(KeyboardMode::Exclusive);
    window.set_namespace("metis-window-switcher");
    if let Some(monitor) = gdk_monitor_for_output(output.as_deref()) {
        window.set_monitor(&monitor);
    }
    for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
        window.set_anchor(edge, true);
        window.set_exclusive_zone(-1);
        window.set_margin(edge, 0);
    }

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.set_halign(gtk::Align::Fill);
    root.set_valign(gtk::Align::Fill);
    root.add_css_class("metis-window-switcher-backdrop");
    window.set_child(Some(&root));

    let center = gtk::Box::new(gtk::Orientation::Vertical, 12);
    center.set_halign(gtk::Align::Center);
    center.set_valign(gtk::Align::Center);
    center.set_hexpand(true);
    center.set_vexpand(true);
    root.append(&center);

    let strip = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    strip.add_css_class("metis-window-switcher-strip");
    strip.set_halign(gtk::Align::Center);
    center.append(&strip);

    let hint = gtk::Label::new(Some(&metis_i18n::tr(
        "Release Alt to switch · Esc to cancel",
    )));
    hint.add_css_class("metis-window-switcher-hint");
    center.append(&hint);

    let selected = if list.len() == 1 {
        0
    } else if forward {
        1 % list.len()
    } else {
        list.len() - 1
    };

    let sw = Rc::new(Switcher {
        window: window.clone(),
        strip: strip.clone(),
        cards: RefCell::new(Vec::new()),
        windows: RefCell::new(list),
        selected: Cell::new(selected),
        output: RefCell::new(output),
        thumbs: RefCell::new(thumbs),
    });

    rebuild_cards(&sw);
    wire_keyboard(&sw);

    OVERLAY.with(|o| *o.borrow_mut() = Some(sw.clone()));
    window.present();
    window.grab_focus();
}

fn rebuild_cards(sw: &Rc<Switcher>) {
    while let Some(child) = sw.strip.first_child() {
        sw.strip.remove(&child);
    }
    sw.cards.borrow_mut().clear();

    let windows = sw.windows.borrow().clone();
    let thumbs = sw.thumbs.borrow();
    for (i, w) in windows.iter().enumerate() {
        let card = build_card(w, i, thumbs.as_ref());
        let idx = i;
        card.connect_clicked(move |_| {
            OVERLAY.with(|o| {
                if let Some(sw) = o.borrow().as_ref() {
                    sw.selected.set(idx);
                }
            });
            activate_selected();
        });
        sw.strip.append(&card);
        sw.cards.borrow_mut().push(card);
    }
    refresh_selection(sw);
}

fn build_card(w: &WindowInfo, index: usize, thumbs: Option<&ThumbSet>) -> gtk::Button {
    let card = gtk::Button::new();
    card.add_css_class("metis-window-switcher-card");
    card.set_focus_on_click(false);

    let col = gtk::Box::new(gtk::Orientation::Vertical, 8);
    col.set_halign(gtk::Align::Center);
    col.set_valign(gtk::Align::Center);

    let badge_row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    badge_row.set_halign(gtk::Align::Fill);
    let badge = gtk::Label::new(Some(&(index + 1).to_string()));
    badge.add_css_class("metis-window-switcher-badge");
    badge.set_halign(gtk::Align::End);
    badge.set_hexpand(true);
    badge_row.append(&badge);
    col.append(&badge_row);

    let preview = gtk::Box::new(gtk::Orientation::Vertical, 0);
    preview.add_css_class("metis-window-switcher-preview");
    preview.set_size_request(160, 96);
    preview.set_halign(gtk::Align::Center);
    preview.set_overflow(gtk::Overflow::Hidden);

    let thumb = window_thumbs::thumb_or_icon_widget(thumbs, w, 48);
    thumb.set_halign(gtk::Align::Center);
    thumb.set_valign(gtk::Align::Center);
    thumb.set_hexpand(true);
    thumb.set_vexpand(true);
    thumb.set_size_request(160, 96);
    preview.append(&thumb);
    col.append(&preview);

    let app_label = w
        .app_id
        .as_deref()
        .and_then(applications::resolve_entry_for_app_id)
        .map(|e| e.name.clone())
        .unwrap_or_else(|| {
            w.app_id
                .clone()
                .unwrap_or_else(|| metis_i18n::tr("Application"))
        });
    let name = gtk::Label::new(Some(&app_label));
    name.add_css_class("metis-window-switcher-app");
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    name.set_max_width_chars(16);
    col.append(&name);

    let title = gtk::Label::new(Some(w.title.as_str()));
    title.add_css_class("metis-window-switcher-title");
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.set_max_width_chars(16);
    col.append(&title);

    card.set_child(Some(&col));
    card
}

fn refresh_selection(sw: &Switcher) {
    let sel = sw.selected.get();
    for (i, card) in sw.cards.borrow().iter().enumerate() {
        if i == sel {
            card.add_css_class("selected");
        } else {
            card.remove_css_class("selected");
        }
    }
}

fn wire_keyboard(sw: &Rc<Switcher>) {
    let controller = gtk::EventControllerKey::new();
    let sw_key = sw.clone();
    controller.connect_key_pressed(move |_, key, _, mods| {
        use gdk::Key;
        if key == Key::Escape {
            dismiss();
            return glib::Propagation::Stop;
        }
        if key == Key::Return || key == Key::KP_Enter {
            activate_selected();
            return glib::Propagation::Stop;
        }
        if key == Key::Tab || key == Key::ISO_Left_Tab {
            let forward = key != Key::ISO_Left_Tab && !mods.contains(gdk::ModifierType::SHIFT_MASK);
            let n = sw_key.windows.borrow().len();
            if n > 0 {
                let cur = sw_key.selected.get();
                let next = if forward {
                    (cur + 1) % n
                } else {
                    cur.checked_sub(1).unwrap_or(n - 1)
                };
                sw_key.selected.set(next);
                refresh_selection(&sw_key);
            }
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    sw.window.add_controller(controller);
}

fn gdk_monitor_for_output(connector: Option<&str>) -> Option<gdk::Monitor> {
    let display = gdk::Display::default()?;
    let monitors = display.monitors();
    let n = monitors.n_items();
    if let Some(name) = connector {
        for i in 0..n {
            let Ok(monitor) = monitors.item(i)?.downcast::<gdk::Monitor>() else {
                continue;
            };
            if monitor
                .connector()
                .map(|c| c.eq_ignore_ascii_case(name))
                .unwrap_or(false)
            {
                return Some(monitor);
            }
        }
    }
    for i in 0..n {
        let Ok(monitor) = monitors.item(i)?.downcast::<gdk::Monitor>() else {
            continue;
        };
        return Some(monitor);
    }
    None
}

/// Refresh card list while open (workspace/window events).
pub fn on_windows_changed() {
    let should_dismiss = OVERLAY.with(|o| {
        let borrow = o.borrow();
        let Some(sw) = borrow.as_ref() else {
            return false;
        };
        let output = sw.output.borrow().clone();
        let workspace = crate::services::active_workspace_for(output.as_deref());
        let list = windows::windows_for_output_workspace(output.as_deref(), workspace);
        if list.is_empty() {
            return true;
        }
        let prev_id = sw.windows.borrow().get(sw.selected.get()).map(|w| w.id);
        *sw.windows.borrow_mut() = list;
        let new_sel = prev_id
            .and_then(|id| sw.windows.borrow().iter().position(|w| w.id == id))
            .unwrap_or(0);
        sw.selected.set(new_sel);
        rebuild_cards(sw);
        false
    });
    if should_dismiss {
        dismiss();
    }
}
