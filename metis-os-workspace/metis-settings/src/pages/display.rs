//! Display: per-output scale and night-light preferences (`outputs.json`).

use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;
use metis_config::{load_outputs_config, output_prefs, save_outputs_config};
use metis_protocol::OutputInfo;

use crate::runtime;
use crate::ui;

pub fn build() -> gtk::Widget {
    let (scroller, content) = ui::page("Display");

    let cfg = Rc::new(RefCell::new(load_outputs_config()));
    let outputs = Rc::new(RefCell::new(runtime::list_outputs()));
    let selected = Rc::new(RefCell::new(0_usize));

    let (global_card, global_body) = ui::section("Night light");
    let night = gtk::Switch::new();
    night.set_active(cfg.borrow().night_light_enabled);
    global_body.append(&ui::row("Enable night light", &night));
    let temp = gtk::SpinButton::with_range(2700.0, 6500.0, 100.0);
    temp.set_digits(0);
    temp.set_value(cfg.borrow().night_light_temperature as f64);
    global_body.append(&ui::row("Colour temperature (K)", &temp));
    content.append(&global_card);

    let picker_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .margin_bottom(12)
        .build();
    content.append(&picker_row);

    let detail_host = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.append(&detail_host);

    let empty_hint = gtk::Label::new(Some(
        "Compositor not reachable — start a Metis session to configure displays.",
    ));
    empty_hint.set_wrap(true);
    empty_hint.set_xalign(0.0);
    empty_hint.add_css_class("metis-settings-hint");

    let rebuild_slot: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));

    let rebuild_ui: Rc<dyn Fn()> = {
        let cfg = cfg.clone();
        let outputs = outputs.clone();
        let selected = selected.clone();
        let picker_row = picker_row.clone();
        let detail_host = detail_host.clone();
        let empty_hint = empty_hint.clone();
        let rebuild_slot = rebuild_slot.clone();
        Rc::new(move || {
            while let Some(child) = picker_row.first_child() {
                picker_row.remove(&child);
            }
            while let Some(child) = detail_host.first_child() {
                detail_host.remove(&child);
            }

            let list = outputs.borrow();
            if list.is_empty() {
                detail_host.append(&empty_hint);
                return;
            }

            let mut sel = *selected.borrow();
            if sel >= list.len() {
                sel = 0;
                *selected.borrow_mut() = sel;
            }

            let refresh = rebuild_slot.borrow().clone();
            for (idx, out) in list.iter().enumerate() {
                let btn = display_chip(out, idx, idx == sel);
                let selected = selected.clone();
                if let Some(refresh) = refresh.clone() {
                    btn.connect_clicked(move |_| {
                        *selected.borrow_mut() = idx;
                        refresh();
                    });
                }
                picker_row.append(&btn);
            }

            let out = &list[sel];
            detail_host.append(&build_output_panel(out, sel, &cfg));
        })
    };
    *rebuild_slot.borrow_mut() = Some(rebuild_ui.clone());
    rebuild_ui();

    {
        let cfg = cfg.clone();
        night.connect_active_notify(move |sw| {
            let mut c = cfg.borrow_mut();
            c.night_light_enabled = sw.is_active();
            save_and_apply(&c);
        });
    }
    {
        let cfg = cfg.clone();
        temp.connect_value_changed(move |spin| {
            let mut c = cfg.borrow_mut();
            c.night_light_temperature = spin.value() as u32;
            save_and_apply(&c);
        });
    }

    let note = gtk::Label::new(Some(
        "Scale applies live via the compositor. Resolution, refresh rate, mirror, \
         and turn-off display are Phase 5 (DRM mode-setting). Night-light colour \
         shift is not wired in the compositor yet.",
    ));
    note.set_wrap(true);
    note.set_xalign(0.0);
    note.add_css_class("metis-settings-hint");
    content.append(&note);

    scroller.upcast()
}

fn display_chip(out: &OutputInfo, index: usize, active: bool) -> gtk::Button {
    let btn = gtk::Button::builder().has_frame(false).build();
    btn.add_css_class("metis-settings-display-chip");
    if active {
        btn.add_css_class("metis-settings-display-chip-active");
    }

    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    let icon = gtk::Image::from_icon_name("video-display-symbolic");
    icon.set_pixel_size(20);
    let label = display_label(out, index);
    let text = gtk::Label::new(Some(&label));
    text.set_xalign(0.0);
    row.append(&icon);
    row.append(&text);
    btn.set_child(Some(&row));
    btn
}

fn display_label(out: &OutputInfo, index: usize) -> String {
    let name = if !out.make.is_empty() || !out.model.is_empty() {
        format!(
            "{} {}",
            out.make.trim(),
            out.model.trim()
        )
        .trim()
        .to_string()
    } else {
        out.name.clone()
    };
    let primary = if out.primary { " · primary" } else { "" };
    format!(
        "Display {} — {}{} · {}×{}",
        index + 1,
        name,
        primary,
        out.rect.width,
        out.rect.height
    )
}

fn build_output_panel(
    out: &OutputInfo,
    index: usize,
    cfg: &Rc<RefCell<metis_config::OutputsConfig>>,
) -> gtk::Widget {
    let (card, body) = ui::section(&display_label(out, index));

    let scale_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    let scale_label = gtk::Label::new(Some("Scale"));
    scale_label.set_hexpand(true);
    scale_label.set_xalign(0.0);
    let scale = gtk::DropDown::from_strings(&["100%", "125%", "150%", "175%", "200%"]);
    let prefs = output_prefs(&cfg.borrow(), &out.name);
    scale.set_selected(scale_index(prefs.scale));
    scale_row.append(&scale_label);
    scale_row.append(&scale);
    body.append(&scale_row);

    let enabled = gtk::Switch::new();
    enabled.set_active(prefs.enabled);
    body.append(&ui::row("Enabled", &enabled));

    let applied = gtk::Label::new(Some(&format!(
        "Current compositor scale: {}%",
        (out.scale * 100.0).round() as i32
    )));
    applied.set_xalign(0.0);
    applied.add_css_class("metis-settings-hint");
    body.append(&applied);

    let name = out.name.clone();
    let cfg = cfg.clone();
    scale.connect_selected_notify({
        let cfg = cfg.clone();
        let name = name.clone();
        move |dd| {
            let mut c = cfg.borrow_mut();
            let entry = c.outputs.entry(name.clone()).or_default();
            entry.scale = scale_from_index(dd.selected());
            save_and_apply(&c);
        }
    });
    enabled.connect_active_notify({
        let cfg = cfg.clone();
        let name = name.clone();
        move |sw| {
            let mut c = cfg.borrow_mut();
            let entry = c.outputs.entry(name.clone()).or_default();
            entry.enabled = sw.is_active();
            save_and_apply(&c);
        }
    });

    card.upcast()
}

fn save_and_apply(cfg: &metis_config::OutputsConfig) {
    if let Err(err) = save_outputs_config(cfg) {
        tracing::warn!(%err, "failed to save outputs.json");
    }
    runtime::reload_outputs();
}

fn scale_index(scale: f64) -> u32 {
    match scale {
        s if (s - 1.25).abs() < 0.01 => 1,
        s if (s - 1.5).abs() < 0.01 => 2,
        s if (s - 1.75).abs() < 0.01 => 3,
        s if (s - 2.0).abs() < 0.01 => 4,
        _ => 0,
    }
}

fn scale_from_index(i: u32) -> f64 {
    match i {
        1 => 1.25,
        2 => 1.5,
        3 => 1.75,
        4 => 2.0,
        _ => 1.0,
    }
}
