//! Display: per-output scale and night-light preferences (`outputs.json`).

use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;
use metis_config::{load_outputs_config, output_prefs, save_outputs_config, OutputsConfig};

use crate::runtime;
use crate::ui;

pub fn build() -> gtk::Widget {
    let (scroller, content) = ui::page("Display");

    let cfg = Rc::new(RefCell::new(load_outputs_config()));
    let outputs = runtime::list_outputs();

    let (global_card, global_body) = ui::section("Night light");
    let night = gtk::Switch::new();
    night.set_active(cfg.borrow().night_light_enabled);
    global_body.append(&ui::row("Enable night light", &night));
    let temp = gtk::SpinButton::with_range(2700.0, 6500.0, 100.0);
    temp.set_digits(0);
    temp.set_value(cfg.borrow().night_light_temperature as f64);
    global_body.append(&ui::row("Colour temperature (K)", &temp));
    content.append(&global_card);

    let monitors = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.append(&monitors);

    if outputs.is_empty() {
        let hint = gtk::Label::new(Some(
            "Compositor not reachable — start a Metis session to configure per-display options.",
        ));
        hint.set_wrap(true);
        hint.set_xalign(0.0);
        hint.add_css_class("metis-settings-hint");
        monitors.append(&hint);
    } else {
        for out in &outputs {
            let (card, body) = ui::section(&format!(
                "{}{} — {}×{}",
                out.name,
                if out.primary { " (primary)" } else { "" },
                out.rect.width,
                out.rect.height
            ));
            let scale = gtk::DropDown::from_strings(&["100%", "125%", "150%", "175%", "200%"]);
            let prefs = output_prefs(&cfg.borrow(), &out.name);
            scale.set_selected(scale_index(prefs.scale));
            body.append(&ui::row("Scale", &scale));
            let enabled = gtk::Switch::new();
            enabled.set_active(prefs.enabled);
            body.append(&ui::row("Enabled", &enabled));
            monitors.append(&card);

            let name = out.name.clone();
            let cfg = cfg.clone();
            scale.connect_selected_notify({
                let cfg = cfg.clone();
                let name = name.clone();
                move |dd| {
                    let mut c = cfg.borrow_mut();
                    let entry = c.outputs.entry(name.clone()).or_default();
                    entry.scale = scale_from_index(dd.selected());
                    let _ = save_outputs_config(&c);
                }
            });
            enabled.connect_active_notify({
                let cfg = cfg.clone();
                let name = name.clone();
                move |sw| {
                    let mut c = cfg.borrow_mut();
                    let entry = c.outputs.entry(name.clone()).or_default();
                    entry.enabled = sw.is_active();
                    let _ = save_outputs_config(&c);
                }
            });
        }
    }

    {
        let cfg = cfg.clone();
        night.connect_active_notify(move |sw| {
            let mut c = cfg.borrow_mut();
            c.night_light_enabled = sw.is_active();
            let _ = save_outputs_config(&c);
        });
    }
    {
        let cfg = cfg.clone();
        temp.connect_value_changed(move |spin| {
            let mut c = cfg.borrow_mut();
            c.night_light_temperature = spin.value() as u32;
            let _ = save_outputs_config(&c);
        });
    }

    let note = gtk::Label::new(Some(
        "Resolution and refresh rate require the DRM display pipeline (Phase 5). \
         Scale and night-light preferences are saved to outputs.json.",
    ));
    note.set_wrap(true);
    note.set_xalign(0.0);
    note.add_css_class("metis-settings-hint");
    content.append(&note);

    scroller.upcast()
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
