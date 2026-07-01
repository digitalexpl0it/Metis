//! Touchpad settings — persisted to `input.json`; shown whenever a touchpad is
//! connected in the Metis session (settings are stored regardless).

use gtk::prelude::*;

use metis_config::TouchpadConfig;

use super::input_common::{self, persist};

pub fn build() -> gtk::Widget {
    let (scroller, content) = crate::ui::page_for("touchpad");
    let cfg = metis_config::load_input_config();

    let (card, body) = input_common::section_card("Touchpad", "input-touchpad-symbolic");

    let tap = gtk::Switch::new();
    tap.set_active(cfg.touchpad.tap_to_click);
    body.append(&crate::ui::row_with_icon(
        "gesture-single-tap-symbolic",
        "Tap to click",
        &tap,
    ));

    let drag = gtk::Switch::new();
    drag.set_active(cfg.touchpad.tap_and_drag);
    body.append(&crate::ui::row_with_icon(
        "gesture-drag-symbolic",
        "Tap and drag",
        &drag,
    ));

    let natural = input_common::natural_scroll_switch(cfg.touchpad.natural_scroll);
    body.append(&crate::ui::row_with_icon(
        "view-restore-symbolic",
        "Natural scrolling",
        &natural,
    ));

    let dwt = gtk::Switch::new();
    dwt.set_active(cfg.touchpad.disable_while_typing);
    body.append(&crate::ui::row_with_icon(
        "input-keyboard-symbolic",
        "Disable while typing",
        &dwt,
    ));

    let speed = input_common::speed_scale(cfg.touchpad.speed);
    body.append(&crate::ui::row_with_icon(
        "preferences-system-mouse-symbolic",
        "Pointer speed",
        &speed,
    ));

    let profile = input_common::accel_profile_dropdown(cfg.touchpad.accel_profile);
    body.append(&crate::ui::row_with_icon(
        "speedometer-symbolic",
        "Acceleration",
        &profile,
    ));

    let scroll = input_common::scroll_speed_scale(cfg.touchpad.scroll_speed);
    body.append(&crate::ui::row_with_icon(
        "go-down-symbolic",
        "Scroll speed",
        &scroll,
    ));

    body.append(&input_common::hint(
        "These options apply when a touchpad is present in the Metis DRM session. Under \
         the nested dev session they are stored but not applied.",
    ));
    content.append(&card);

    speed.connect_value_changed({
        let profile = profile.clone();
        let tap = tap.clone();
        let drag = drag.clone();
        let natural = natural.clone();
        let dwt = dwt.clone();
        let scroll = scroll.clone();
        move |s| save_touchpad(s.value(), &profile, &tap, &drag, &natural, &dwt, &scroll)
    });
    profile.connect_notify_local(Some("selected"), {
        let speed = speed.clone();
        let tap = tap.clone();
        let drag = drag.clone();
        let natural = natural.clone();
        let dwt = dwt.clone();
        let scroll = scroll.clone();
        let profile = profile.clone();
        move |_, _| save_touchpad(speed.value(), &profile, &tap, &drag, &natural, &dwt, &scroll)
    });
    tap.connect_active_notify({
        let speed = speed.clone();
        let profile = profile.clone();
        let drag = drag.clone();
        let natural = natural.clone();
        let dwt = dwt.clone();
        let scroll = scroll.clone();
        let tap = tap.clone();
        move |_| save_touchpad(speed.value(), &profile, &tap, &drag, &natural, &dwt, &scroll)
    });
    drag.connect_active_notify({
        let speed = speed.clone();
        let profile = profile.clone();
        let tap = tap.clone();
        let natural = natural.clone();
        let dwt = dwt.clone();
        let scroll = scroll.clone();
        let drag = drag.clone();
        move |_| save_touchpad(speed.value(), &profile, &tap, &drag, &natural, &dwt, &scroll)
    });
    natural.connect_active_notify({
        let speed = speed.clone();
        let profile = profile.clone();
        let tap = tap.clone();
        let drag = drag.clone();
        let dwt = dwt.clone();
        let scroll = scroll.clone();
        let natural = natural.clone();
        move |_| save_touchpad(speed.value(), &profile, &tap, &drag, &natural, &dwt, &scroll)
    });
    dwt.connect_active_notify({
        let speed = speed.clone();
        let profile = profile.clone();
        let tap = tap.clone();
        let drag = drag.clone();
        let natural = natural.clone();
        let scroll = scroll.clone();
        let dwt = dwt.clone();
        move |_| save_touchpad(speed.value(), &profile, &tap, &drag, &natural, &dwt, &scroll)
    });
    scroll.connect_value_changed({
        let speed = speed.clone();
        let profile = profile.clone();
        let tap = tap.clone();
        let drag = drag.clone();
        let natural = natural.clone();
        let dwt = dwt.clone();
        let scroll = scroll.clone();
        move |_| save_touchpad(speed.value(), &profile, &tap, &drag, &natural, &dwt, &scroll)
    });

    scroller.upcast()
}

fn save_touchpad(
    speed: f64,
    profile: &gtk::DropDown,
    tap: &gtk::Switch,
    drag: &gtk::Switch,
    natural: &gtk::Switch,
    dwt: &gtk::Switch,
    scroll: &gtk::Scale,
) {
    let mut cfg = metis_config::load_input_config();
    cfg.touchpad = TouchpadConfig {
        tap_to_click: tap.is_active(),
        tap_and_drag: drag.is_active(),
        natural_scroll: natural.is_active(),
        disable_while_typing: dwt.is_active(),
        speed,
        accel_profile: input_common::accel_profile_from_dropdown(profile),
        scroll_speed: scroll.value(),
    };
    persist(&cfg);
}
