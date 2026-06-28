//! Power & battery: profile selection, idle timeouts, lid-close action.

use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use gtk::prelude::*;
use metis_config::{LidCloseAction, PowerConfig, PowerProfile};

use crate::power::{self, PowerSnapshot};
use crate::ui;

struct Sections {
    battery_label: gtk::Label,
    profile: gtk::DropDown,
    blank: gtk::SpinButton,
    suspend: gtk::SpinButton,
    lid: gtk::DropDown,
    dim: gtk::Switch,
}

pub fn build() -> gtk::Widget {
    let (scroller, content) = ui::page("Power");

    let (bat_card, bat_body) = ui::section("Battery");
    let battery_label = gtk::Label::new(Some("No battery detected"));
    battery_label.set_xalign(0.0);
    bat_body.append(&battery_label);
    content.append(&bat_card);

    let (prof_card, prof_body) = ui::section("Power mode");
    let profile = gtk::DropDown::from_strings(&["Power saver", "Balanced", "Performance"]);
    prof_body.append(&ui::row("Profile", &profile));
    content.append(&prof_card);

    let (idle_card, idle_body) = ui::section("Power saving");
    let blank = gtk::SpinButton::with_range(0.0, 120.0, 1.0);
    blank.set_digits(0);
    idle_body.append(&ui::row("Blank screen after (min, 0=never)", &blank));
    let suspend = gtk::SpinButton::with_range(0.0, 240.0, 1.0);
    suspend.set_digits(0);
    idle_body.append(&ui::row("Suspend after idle (min, 0=never)", &suspend));
    let lid = gtk::DropDown::from_strings(&["Suspend", "Ignore", "Hibernate", "Power off"]);
    idle_body.append(&ui::row("When lid is closed", &lid));
    let dim = gtk::Switch::new();
    idle_body.append(&ui::row("Dim on battery", &dim));
    content.append(&idle_card);

    let sections = Rc::new(Sections {
        battery_label,
        profile,
        blank,
        suspend,
        lid,
        dim,
    });

    let (tx, rx) = mpsc::channel::<PowerSnapshot>();
    let refresh = {
        let tx = tx.clone();
        Rc::new(move || {
            let tx = tx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(power::load_snapshot());
            });
        })
    };

    {
        let sections_poll = sections.clone();
        let refresh_poll = refresh.clone();
        glib::timeout_add_local(Duration::from_millis(200), move || {
            if let Ok(snap) = rx.try_recv() {
                render(&sections_poll, &snap);
            }
            glib::ControlFlow::Continue
        });

        let persist = {
            let sections = sections.clone();
            move || {
                let cfg = read_config(&sections);
                power::save_config(&cfg);
            }
        };

        sections.profile.connect_selected_notify({
            let persist = persist.clone();
            move |_| persist()
        });
        sections.blank.connect_value_changed({
            let persist = persist.clone();
            move |_| persist()
        });
        sections.suspend.connect_value_changed({
            let persist = persist.clone();
            move |_| persist()
        });
        sections.lid.connect_selected_notify({
            let persist = persist.clone();
            move |_| persist()
        });
        sections.dim.connect_active_notify({
            let persist = persist.clone();
            move |_| persist()
        });
    }

    refresh();
    scroller.upcast()
}

fn render(sections: &Sections, snap: &PowerSnapshot) {
    if snap.battery.present {
        let pct = snap.battery.percent.unwrap_or(0);
        let charge = if snap.battery.charging { "charging" } else { "discharging" };
        sections.battery_label.set_text(&format!(
            "{pct}% · {charge} · {}",
            snap.battery.status
        ));
    } else {
        sections.battery_label.set_text("No battery detected (desktop / AC-only)");
    }
    sections.profile.set_selected(profile_index(snap.profile));
    sections.blank.set_value(snap.config.blank_after_minutes as f64);
    sections.suspend.set_value(snap.config.suspend_after_minutes as f64);
    sections.lid.set_selected(lid_index(snap.config.lid_close));
    sections.dim.set_active(snap.config.dim_on_battery);
}

fn read_config(sections: &Sections) -> PowerConfig {
    PowerConfig {
        profile: profile_from_index(sections.profile.selected()),
        blank_after_minutes: sections.blank.value() as u32,
        suspend_after_minutes: sections.suspend.value() as u32,
        lid_close: lid_from_index(sections.lid.selected()),
        dim_on_battery: sections.dim.is_active(),
    }
}

fn profile_index(p: PowerProfile) -> u32 {
    match p {
        PowerProfile::PowerSaver => 0,
        PowerProfile::Balanced => 1,
        PowerProfile::Performance => 2,
    }
}

fn profile_from_index(i: u32) -> PowerProfile {
    match i {
        0 => PowerProfile::PowerSaver,
        2 => PowerProfile::Performance,
        _ => PowerProfile::Balanced,
    }
}

fn lid_index(a: LidCloseAction) -> u32 {
    match a {
        LidCloseAction::Suspend => 0,
        LidCloseAction::Ignore => 1,
        LidCloseAction::Hibernate => 2,
        LidCloseAction::PowerOff => 3,
    }
}

fn lid_from_index(i: u32) -> LidCloseAction {
    match i {
        1 => LidCloseAction::Ignore,
        2 => LidCloseAction::Hibernate,
        3 => LidCloseAction::PowerOff,
        _ => LidCloseAction::Suspend,
    }
}
