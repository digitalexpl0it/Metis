//! Keyboard layout, repeat, and modifier remaps — persisted to `input.json`.

use gtk::prelude::*;

use metis_config::{CapsLockBehavior, ComposeKey, KeyboardConfig};

use super::input_common::{self, persist};

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
        input_common::section_card("Typing", "preferences-desktop-keyboard-shortcuts-symbolic");

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
