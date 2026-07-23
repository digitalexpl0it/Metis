//! Shared helpers for the Input settings pages.

use gtk::prelude::*;

use metis_config::{AccelProfile, InputConfig};

use crate::ui;

pub fn persist(cfg: &InputConfig) {
    if let Err(err) = metis_config::save_input_config(cfg) {
        tracing::warn!(%err, "failed to save input.json");
        return;
    }
    crate::runtime::reload_input();
}

pub fn hint(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.set_hexpand(true);
    // Cap reported width so long hints don't lock Settings' minimum size.
    label.set_width_chars(28);
    label.set_max_width_chars(72);
    label.add_css_class("metis-settings-hint");
    label
}

pub fn accel_profile_dropdown(current: AccelProfile) -> gtk::DropDown {
    let dd = gtk::DropDown::from_strings(&["Adaptive", "Flat"]);
    dd.set_selected(match current {
        AccelProfile::Adaptive => 0,
        AccelProfile::Flat => 1,
    });
    dd
}

pub fn accel_profile_from_dropdown(dd: &gtk::DropDown) -> AccelProfile {
    match dd.selected() {
        1 => AccelProfile::Flat,
        _ => AccelProfile::Adaptive,
    }
}

pub fn speed_scale(value: f64) -> gtk::Scale {
    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, -1.0, 1.0, 0.05);
    scale.set_value(value);
    scale.set_size_request(200, -1);
    scale.set_draw_value(true);
    scale
}

pub fn natural_scroll_switch(enabled: bool) -> gtk::Switch {
    let sw = gtk::Switch::new();
    sw.set_active(enabled);
    sw
}

pub fn left_handed_switch(enabled: bool) -> gtk::Switch {
    let sw = gtk::Switch::new();
    sw.set_active(enabled);
    sw
}

pub fn scroll_speed_scale(value: f64) -> gtk::Scale {
    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.25, 4.0, 0.05);
    scale.set_value(value);
    scale.set_size_request(200, -1);
    scale.set_draw_value(true);
    scale
}

pub fn section_card(title: &str, icon: &str) -> (gtk::Box, gtk::Box) {
    ui::section_with_icon(title, icon)
}
