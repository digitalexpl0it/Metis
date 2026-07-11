//! Keyboard layout, repeat, modifier remaps (`input.json`), and desktop
//! shortcuts (`keybinds.json`) with capture UI.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::gdk;
use gtk::prelude::*;

use metis_config::{
    load_keybinds_config, reserved_system_rows, save_keybinds_config, CapsLockBehavior, Chord,
    ComposeKey, KeybindAction, KeybindGroup, KeybindsConfig, KeyboardConfig, ModKey,
};

use super::input_common::{self, persist};
use crate::runtime;

pub fn build() -> gtk::Widget {
    let (scroller, content) = crate::ui::page_for("keyboard");
    let cfg = metis_config::load_input_config();

    let (layout_card, layout_body) =
        input_common::section_card("Layout", "input-keyboard-symbolic");

    let layout = gtk::Entry::builder()
        .placeholder_text("Leave empty for system default, e.g. us")
        .text(&cfg.keyboard.layout)
        .hexpand(true)
        .build();
    layout_body.append(&crate::ui::row("Layout", &layout));

    let variant = gtk::Entry::builder()
        .placeholder_text("Optional variant")
        .text(&cfg.keyboard.variant)
        .hexpand(true)
        .build();
    layout_body.append(&crate::ui::row("Variant", &variant));

    let options = gtk::Entry::builder()
        .placeholder_text("Extra xkb options, comma-separated")
        .text(&cfg.keyboard.options)
        .hexpand(true)
        .build();
    layout_body.append(&crate::ui::row("Options", &options));

    layout_body.append(&input_common::hint(
        "Layout uses xkb rules. Common layouts: us, gb, de, fr, es. See \
         /usr/share/X11/xkb/rules/base.lst on your system.",
    ));
    content.append(&layout_card);

    let (typing_card, typing_body) =
        input_common::section_card("Typing", "input-keyboard-symbolic");

    let delay = gtk::Scale::with_range(gtk::Orientation::Horizontal, 200.0, 1000.0, 50.0);
    delay.set_value(cfg.keyboard.repeat_delay_ms as f64);
    delay.set_size_request(200, -1);
    delay.set_draw_value(true);
    typing_body.append(&crate::ui::row("Repeat delay (ms)", &delay));

    let rate = gtk::Scale::with_range(gtk::Orientation::Horizontal, 10.0, 50.0, 1.0);
    rate.set_value(cfg.keyboard.repeat_rate_hz as f64);
    rate.set_size_request(200, -1);
    rate.set_draw_value(true);
    typing_body.append(&crate::ui::row("Repeat rate (Hz)", &rate));

    let caps = gtk::DropDown::from_strings(&["Default", "Escape", "Control"]);
    caps.set_selected(match cfg.keyboard.caps_lock {
        CapsLockBehavior::Default => 0,
        CapsLockBehavior::Escape => 1,
        CapsLockBehavior::Control => 2,
    });
    typing_body.append(&crate::ui::row("Caps Lock", &caps));

    let compose = gtk::DropDown::from_strings(&[
        "Disabled",
        "Right Alt",
        "Menu",
        "Left Alt",
        "Scroll Lock",
    ]);
    compose.set_selected(match cfg.keyboard.compose_key {
        ComposeKey::Disabled => 0,
        ComposeKey::RightAlt => 1,
        ComposeKey::Menu => 2,
        ComposeKey::LeftAlt => 3,
        ComposeKey::ScrollLock => 4,
    });
    typing_body.append(&crate::ui::row("Compose key", &compose));

    typing_body.append(&input_common::hint(
        "Keyboard settings apply live in the Metis session within ~1s.",
    ));
    content.append(&typing_card);

    content.append(&build_shortcuts_section());

    layout.connect_changed({
        let delay = delay.clone();
        let rate = rate.clone();
        let caps = caps.clone();
        let compose = compose.clone();
        let variant = variant.clone();
        let options = options.clone();
        move |entry| save_keyboard(entry, &variant, &options, &delay, &rate, &caps, &compose)
    });
    variant.connect_changed({
        let layout = layout.clone();
        let delay = delay.clone();
        let rate = rate.clone();
        let caps = caps.clone();
        let compose = compose.clone();
        let options = options.clone();
        move |entry| save_keyboard(&layout, entry, &options, &delay, &rate, &caps, &compose)
    });
    options.connect_changed({
        let layout = layout.clone();
        let delay = delay.clone();
        let rate = rate.clone();
        let caps = caps.clone();
        let compose = compose.clone();
        let variant = variant.clone();
        move |entry| save_keyboard(&layout, &variant, entry, &delay, &rate, &caps, &compose)
    });
    delay.connect_value_changed({
        let layout = layout.clone();
        let rate = rate.clone();
        let caps = caps.clone();
        let compose = compose.clone();
        let variant = variant.clone();
        let options = options.clone();
        let delay = delay.clone();
        move |_| save_keyboard(&layout, &variant, &options, &delay, &rate, &caps, &compose)
    });
    rate.connect_value_changed({
        let layout = layout.clone();
        let delay = delay.clone();
        let caps = caps.clone();
        let compose = compose.clone();
        let variant = variant.clone();
        let options = options.clone();
        let rate = rate.clone();
        move |_| save_keyboard(&layout, &variant, &options, &delay, &rate, &caps, &compose)
    });
    caps.connect_notify_local(Some("selected"), {
        let layout = layout.clone();
        let delay = delay.clone();
        let rate = rate.clone();
        let compose = compose.clone();
        let variant = variant.clone();
        let options = options.clone();
        let caps = caps.clone();
        move |_, _| save_keyboard(&layout, &variant, &options, &delay, &rate, &caps, &compose)
    });
    compose.connect_notify_local(Some("selected"), {
        let layout = layout.clone();
        let delay = delay.clone();
        let rate = rate.clone();
        let caps = caps.clone();
        let variant = variant.clone();
        let options = options.clone();
        let compose = compose.clone();
        move |_, _| save_keyboard(&layout, &variant, &options, &delay, &rate, &caps, &compose)
    });

    scroller.upcast()
}

fn build_shortcuts_section() -> gtk::Box {
    let (card, body) =
        input_common::section_card("Shortcuts", "preferences-desktop-keyboard-shortcuts-symbolic");

    let cfg = Rc::new(RefCell::new(load_keybinds_config()));
    let status = gtk::Label::new(None);
    status.set_xalign(0.0);
    status.set_wrap(true);
    status.add_css_class("metis-settings-hint");
    status.set_visible(false);

    let mod_dd = gtk::DropDown::from_strings(&["Super", "Alt", "Ctrl"]);
    mod_dd.set_selected(match cfg.borrow().mod_key {
        ModKey::Super => 0,
        ModKey::Alt => 1,
        ModKey::Ctrl => 2,
    });
    body.append(&crate::ui::row("Metis modifier (defaults)", &mod_dd));

    {
        let cfg = cfg.clone();
        let status = status.clone();
        let body_ref = body.clone();
        mod_dd.connect_notify_local(Some("selected"), move |dd, _| {
            let mod_key = match dd.selected() {
                1 => ModKey::Alt,
                2 => ModKey::Ctrl,
                _ => ModKey::Super,
            };
            {
                let mut c = cfg.borrow_mut();
                c.mod_key = mod_key;
            }
            persist_keybinds(&cfg.borrow(), &status);
            // Rebuild rows so default chords refresh for unchanged actions.
            rebuild_shortcut_rows(&body_ref, &cfg, &status);
        });
    }

    body.append(&input_common::hint(
        "Click Change, press a shortcut, then Save. Ctrl+Alt+F1–F12 and \
         Ctrl+Alt+Backspace are system-only and cannot be changed.",
    ));
    body.append(&status);

    // Placeholder marker so rebuild can clear dynamic rows.
    let list_host = gtk::Box::new(gtk::Orientation::Vertical, 12);
    list_host.add_css_class("metis-keybind-list");
    body.append(&list_host);
    fill_shortcut_rows(&list_host, &cfg, &status);

    card
}

fn rebuild_shortcut_rows(
    body: &gtk::Box,
    cfg: &Rc<RefCell<KeybindsConfig>>,
    status: &gtk::Label,
) {
    let mut child = body.first_child();
    while let Some(w) = child {
        let next = w.next_sibling();
        if w.has_css_class("metis-keybind-list") {
            if let Ok(host) = w.downcast::<gtk::Box>() {
                while let Some(row) = host.first_child() {
                    host.remove(&row);
                }
                fill_shortcut_rows(&host, cfg, status);
            }
            break;
        }
        child = next;
    }
}

fn fill_shortcut_rows(
    host: &gtk::Box,
    cfg: &Rc<RefCell<KeybindsConfig>>,
    status: &gtk::Label,
) {
    for group in KeybindGroup::all() {
        if *group == KeybindGroup::System {
            let title = gtk::Label::new(Some(group.label()));
            title.set_xalign(0.0);
            title.add_css_class("metis-settings-section-title");
            host.append(&title);
            for (label, chord) in reserved_system_rows() {
                host.append(&reserved_row(&label, &chord));
            }
            continue;
        }

        let actions: Vec<KeybindAction> = KeybindAction::all()
            .iter()
            .copied()
            .filter(|a| a.group() == *group)
            .collect();
        if actions.is_empty() {
            continue;
        }
        let title = gtk::Label::new(Some(group.label()));
        title.set_xalign(0.0);
        title.add_css_class("metis-settings-section-title");
        host.append(&title);
        for action in actions {
            host.append(&editable_row(action, cfg, status));
        }
    }
}

fn reserved_row(label: &str, chord: &Chord) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.add_css_class("metis-settings-row");
    row.add_css_class("metis-keybind-reserved");
    let lbl = gtk::Label::new(Some(label));
    lbl.set_xalign(0.0);
    lbl.set_hexpand(true);
    row.append(&lbl);
    let chord_lbl = gtk::Label::new(Some(&chord.display()));
    chord_lbl.add_css_class("metis-keybind-chord");
    row.append(&chord_lbl);
    let lock = gtk::Label::new(Some("System"));
    lock.add_css_class("metis-settings-hint");
    row.append(&lock);
    row
}

fn editable_row(
    action: KeybindAction,
    cfg: &Rc<RefCell<KeybindsConfig>>,
    status: &gtk::Label,
) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Vertical, 6);
    row.add_css_class("metis-keybind-editable");

    let top = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    top.add_css_class("metis-settings-row");
    let lbl = gtk::Label::new(Some(action.label()));
    lbl.set_xalign(0.0);
    lbl.set_hexpand(true);
    top.append(&lbl);

    let chord_lbl = gtk::Label::new(Some(&cfg.borrow().chord_for(action).display()));
    chord_lbl.add_css_class("metis-keybind-chord");
    top.append(&chord_lbl);

    let change = gtk::Button::with_label("Change");
    let reset = gtk::Button::with_label("Reset");
    top.append(&change);
    top.append(&reset);
    row.append(&top);

    let capture = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    capture.set_visible(false);
    let listen = gtk::Label::new(Some("Press a shortcut…"));
    listen.set_hexpand(true);
    listen.set_xalign(0.0);
    let save = gtk::Button::with_label("Save");
    save.set_sensitive(false);
    let cancel = gtk::Button::with_label("Cancel");
    capture.append(&listen);
    capture.append(&save);
    capture.append(&cancel);
    row.append(&capture);

    let candidate: Rc<RefCell<Option<Chord>>> = Rc::new(RefCell::new(None));

    {
        let capture = capture.clone();
        let change_btn = change.clone();
        let reset_btn = reset.clone();
        let listen = listen.clone();
        let save = save.clone();
        let candidate = candidate.clone();
        let status = status.clone();
        change.connect_clicked(move |_| {
            status.set_visible(false);
            capture.set_visible(true);
            change_btn.set_sensitive(false);
            reset_btn.set_sensitive(false);
            listen.set_text("Press a shortcut…");
            save.set_sensitive(false);
            *candidate.borrow_mut() = None;
            runtime::set_keybind_capture_async(true);
        });
    }

    {
        let capture = capture.clone();
        let change_btn = change.clone();
        let reset_btn = reset.clone();
        let candidate = candidate.clone();
        cancel.connect_clicked(move |_| {
            capture.set_visible(false);
            change_btn.set_sensitive(true);
            reset_btn.set_sensitive(true);
            *candidate.borrow_mut() = None;
            runtime::set_keybind_capture_async(false);
        });
    }

    {
        let cfg = cfg.clone();
        let chord_lbl = chord_lbl.clone();
        let status = status.clone();
        reset.connect_clicked(move |_| {
            {
                let mut c = cfg.borrow_mut();
                c.reset_binding(action);
            }
            chord_lbl.set_text(&cfg.borrow().chord_for(action).display());
            persist_keybinds(&cfg.borrow(), &status);
        });
    }

    {
        let cfg = cfg.clone();
        let chord_lbl = chord_lbl.clone();
        let capture = capture.clone();
        let change_btn = change.clone();
        let reset_btn = reset.clone();
        let candidate = candidate.clone();
        let status = status.clone();
        save.connect_clicked(move |_| {
            let Some(chord) = candidate.borrow().clone() else {
                return;
            };
            if chord.is_reserved() {
                status.set_text("That shortcut is reserved for the system and cannot be used.");
                status.set_visible(true);
                return;
            }
            if let Some(owner) = cfg.borrow().action_for_chord(&chord) {
                if owner != action {
                    status.set_text(&format!(
                        "Already used by “{}”. Choose a different shortcut.",
                        owner.label()
                    ));
                    status.set_visible(true);
                    return;
                }
            }
            {
                let mut c = cfg.borrow_mut();
                c.set_binding(action, chord);
            }
            chord_lbl.set_text(&cfg.borrow().chord_for(action).display());
            capture.set_visible(false);
            change_btn.set_sensitive(true);
            reset_btn.set_sensitive(true);
            *candidate.borrow_mut() = None;
            runtime::set_keybind_capture_async(false);
            persist_keybinds(&cfg.borrow(), &status);
        });
    }

    let key = gtk::EventControllerKey::new();
    {
        let capture = capture.clone();
        let listen = listen.clone();
        let save = save.clone();
        let candidate = candidate.clone();
        let status = status.clone();
        key.connect_key_pressed(move |_, keyval, _keycode, mods| {
            if !capture.is_visible() {
                return glib::Propagation::Proceed;
            }
            if keyval == gdk::Key::Escape {
                return glib::Propagation::Proceed;
            }
            // Ignore pure modifier presses.
            if matches!(
                keyval,
                gdk::Key::Control_L
                    | gdk::Key::Control_R
                    | gdk::Key::Shift_L
                    | gdk::Key::Shift_R
                    | gdk::Key::Alt_L
                    | gdk::Key::Alt_R
                    | gdk::Key::Meta_L
                    | gdk::Key::Meta_R
                    | gdk::Key::Super_L
                    | gdk::Key::Super_R
            ) {
                return glib::Propagation::Stop;
            }
            match chord_from_gdk(keyval, mods) {
                Ok(chord) => {
                    listen.set_text(&chord.display());
                    *candidate.borrow_mut() = Some(chord);
                    save.set_sensitive(true);
                    status.set_visible(false);
                }
                Err(err) => {
                    listen.set_text(&err);
                    save.set_sensitive(false);
                    *candidate.borrow_mut() = None;
                }
            }
            glib::Propagation::Stop
        });
    }
    row.add_controller(key);

    row
}

fn chord_from_gdk(keyval: gdk::Key, mods: gdk::ModifierType) -> Result<Chord, String> {
    let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
    let alt = mods.contains(gdk::ModifierType::ALT_MASK);
    let shift = mods.contains(gdk::ModifierType::SHIFT_MASK);
    let super_key = mods.contains(gdk::ModifierType::SUPER_MASK)
        || mods.contains(gdk::ModifierType::META_MASK);

    let key = keyval_to_token(keyval).ok_or_else(|| "Unsupported key".to_string())?;
    Ok(Chord {
        ctrl,
        alt,
        shift,
        super_key,
        key,
    })
}

fn keyval_to_token(keyval: gdk::Key) -> Option<String> {
    if let Some(n) = keyval_digit(keyval) {
        return Some(n.to_string());
    }
    let name = keyval.name()?.as_str().to_string();
    Some(match name.as_str() {
        "Escape" => "Escape".into(),
        "Return" | "KP_Enter" => "Return".into(),
        "space" => "Space".into(),
        "Print" | "Sys_Req" => "Print".into(),
        "Left" | "Right" | "Up" | "Down" => name,
        "slash" => "slash".into(),
        "backslash" => "backslash".into(),
        "comma" => "comma".into(),
        "period" => "period".into(),
        "minus" => "minus".into(),
        "equal" | "plus" => "equal".into(),
        "BackSpace" => "BackSpace".into(),
        s if s.len() == 1 => s.to_ascii_uppercase(),
        s if s.starts_with('F') && s[1..].chars().all(|c| c.is_ascii_digit()) => s.to_string(),
        _ => return None,
    })
}

fn keyval_digit(keyval: gdk::Key) -> Option<u8> {
    match keyval {
        gdk::Key::_1 | gdk::Key::KP_1 => Some(1),
        gdk::Key::_2 | gdk::Key::KP_2 => Some(2),
        gdk::Key::_3 | gdk::Key::KP_3 => Some(3),
        gdk::Key::_4 | gdk::Key::KP_4 => Some(4),
        gdk::Key::_5 | gdk::Key::KP_5 => Some(5),
        gdk::Key::_6 | gdk::Key::KP_6 => Some(6),
        gdk::Key::_7 | gdk::Key::KP_7 => Some(7),
        gdk::Key::_8 | gdk::Key::KP_8 => Some(8),
        gdk::Key::_9 | gdk::Key::KP_9 => Some(9),
        _ => None,
    }
}

fn persist_keybinds(cfg: &KeybindsConfig, status: &gtk::Label) {
    match save_keybinds_config(cfg) {
        Ok(()) => {
            status.set_visible(false);
            runtime::reload_keybinds_async();
        }
        Err(err) => {
            status.set_text(&format!("Failed to save keybinds: {err}"));
            status.set_visible(true);
        }
    }
}

fn save_keyboard(
    layout: &gtk::Entry,
    variant: &gtk::Entry,
    options: &gtk::Entry,
    delay: &gtk::Scale,
    rate: &gtk::Scale,
    caps: &gtk::DropDown,
    compose: &gtk::DropDown,
) {
    let mut cfg = metis_config::load_input_config();
    cfg.keyboard = KeyboardConfig {
        layout: layout.text().to_string(),
        variant: variant.text().to_string(),
        options: options.text().to_string(),
        repeat_delay_ms: delay.value().round() as i32,
        repeat_rate_hz: rate.value().round() as i32,
        caps_lock: caps_lock_from_dropdown(caps),
        compose_key: compose_key_from_dropdown(compose),
    };
    persist(&cfg);
}

fn caps_lock_from_dropdown(dd: &gtk::DropDown) -> CapsLockBehavior {
    match dd.selected() {
        1 => CapsLockBehavior::Escape,
        2 => CapsLockBehavior::Control,
        _ => CapsLockBehavior::Default,
    }
}

fn compose_key_from_dropdown(dd: &gtk::DropDown) -> ComposeKey {
    match dd.selected() {
        1 => ComposeKey::RightAlt,
        2 => ComposeKey::Menu,
        3 => ComposeKey::LeftAlt,
        4 => ComposeKey::ScrollLock,
        _ => ComposeKey::Disabled,
    }
}
