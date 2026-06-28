//! Mouse pointer settings — persisted to `input.json`; the compositor live-reloads.

use gtk::prelude::*;

use metis_config::MouseConfig;

use super::input_common::{self, persist};

pub fn build() -> gtk::Widget {
    let (scroller, content) = crate::ui::page("Mouse");
    let cfg = metis_config::load_input_config();

    let (card, body) = input_common::section_card("Pointer", "input-mouse-symbolic");

    let speed = input_common::speed_scale(cfg.mouse.speed);
    body.append(&crate::ui::row_with_icon(
        "preferences-system-mouse-symbolic",
        "Pointer speed",
        &speed,
    ));

    let profile = input_common::accel_profile_dropdown(cfg.mouse.accel_profile);
    body.append(&crate::ui::row_with_icon(
        "speedometer-symbolic",
        "Acceleration",
        &profile,
    ));

    let natural = input_common::natural_scroll_switch(cfg.mouse.natural_scroll);
    body.append(&crate::ui::row_with_icon(
        "view-restore-symbolic",
        "Natural scrolling",
        &natural,
    ));

    let left = input_common::left_handed_switch(cfg.mouse.left_handed);
    body.append(&crate::ui::row_with_icon(
        "object-flip-horizontal-symbolic",
        "Primary button on right",
        &left,
    ));

    let scroll = input_common::scroll_speed_scale(cfg.mouse.scroll_speed);
    body.append(&crate::ui::row_with_icon(
        "go-down-symbolic",
        "Scroll speed",
        &scroll,
    ));

    body.append(&input_common::hint(
        "Applies to mice and other non-touchpad pointers in the Metis session (wheel, \
         including lists in the Metis menu). Changes apply immediately.",
    ));
    content.append(&card);

    speed.connect_value_changed({
        let profile = profile.clone();
        let natural = natural.clone();
        let left = left.clone();
        let scroll = scroll.clone();
        move |s| save_mouse(s.value(), &profile, &natural, &left, &scroll)
    });
    profile.connect_notify_local(Some("selected"), {
        let speed = speed.clone();
        let natural = natural.clone();
        let left = left.clone();
        let scroll = scroll.clone();
        move |dd, _| save_mouse(speed.value(), dd, &natural, &left, &scroll)
    });
    natural.connect_active_notify({
        let speed = speed.clone();
        let profile = profile.clone();
        let left = left.clone();
        let scroll = scroll.clone();
        move |sw| save_mouse(speed.value(), &profile, sw, &left, &scroll)
    });
    left.connect_active_notify({
        let speed = speed.clone();
        let profile = profile.clone();
        let natural = natural.clone();
        let scroll = scroll.clone();
        move |sw| save_mouse(speed.value(), &profile, &natural, sw, &scroll)
    });
    scroll.connect_value_changed({
        let speed = speed.clone();
        let profile = profile.clone();
        let natural = natural.clone();
        let left = left.clone();
        move |s| save_mouse(speed.value(), &profile, &natural, &left, s)
    });

    scroller.upcast()
}

fn save_mouse(
    speed: f64,
    profile: &gtk::DropDown,
    natural: &gtk::Switch,
    left: &gtk::Switch,
    scroll: &gtk::Scale,
) {
    let mut cfg = metis_config::load_input_config();
    cfg.mouse = MouseConfig {
        speed,
        accel_profile: input_common::accel_profile_from_dropdown(profile),
        natural_scroll: natural.is_active(),
        left_handed: left.is_active(),
        scroll_speed: scroll.value(),
    };
    persist(&cfg);
}
