//! Power & battery: profile selection, idle timeouts, lid-close action.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use gtk::prelude::*;
use metis_config::{LidCloseAction, PowerConfig, PowerProfile};

use crate::bluetooth::{self, BluetoothSnapshot, DeviceState};
use crate::power::{self, PowerSnapshot};
use crate::ui;

struct Sections {
    battery_label: gtk::Label,
    profile: gtk::DropDown,
    blank: gtk::SpinButton,
    suspend: gtk::SpinButton,
    lid: gtk::DropDown,
    dim: gtk::Switch,
    devices_body: gtk::Box,
    devices_empty: gtk::Label,
}

pub fn build() -> gtk::Widget {
    let (scroller, content) = ui::page_for("power");

    let (bat_card, bat_body) = ui::section("Battery");
    let battery_label = gtk::Label::new(Some("No battery detected"));
    battery_label.set_xalign(0.0);
    let battery_row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    battery_row.add_css_class("metis-settings-row");
    battery_row.append(&battery_label);
    bat_body.append(&battery_row);
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

    let (dev_card, devices_body) = ui::section("Connected devices");
    let devices_empty = gtk::Label::new(Some("No wireless devices connected"));
    devices_empty.set_xalign(0.0);
    devices_empty.add_css_class("metis-settings-hint");
    content.append(&dev_card);

    let sections = Rc::new(Sections {
        battery_label,
        profile,
        blank,
        suspend,
        lid,
        dim,
        devices_body,
        devices_empty,
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

    // Bluetooth device battery list — refreshed on its own background thread
    // because bluetoothctl can take seconds; never block the settings UI thread.
    let (bt_tx, bt_rx) = mpsc::channel::<BluetoothSnapshot>();
    let refresh_devices = {
        let bt_tx = bt_tx.clone();
        Rc::new(move || {
            let bt_tx = bt_tx.clone();
            std::thread::spawn(move || {
                let _ = bt_tx.send(bluetooth::load_snapshot());
            });
        })
    };

    {
        let sections_poll = sections.clone();
        // Populate the editable idle controls (blank/suspend/lid/dim/profile)
        // only from the first snapshot. Later periodic refreshes update the
        // battery + device state but must not stomp a value the user is typing.
        let config_applied = Rc::new(std::cell::Cell::new(false));
        glib::timeout_add_local(Duration::from_millis(200), move || {
            if let Ok(snap) = rx.try_recv() {
                let apply_config = !config_applied.replace(true);
                render(&sections_poll, &snap, apply_config);
            }
            if let Ok(bt) = bt_rx.try_recv() {
                render_devices(&sections_poll, &bt);
            }
            glib::ControlFlow::Continue
        });

        // Periodically re-read battery + device state so the page stays live
        // while open (device battery changes slowly; 10s is plenty).
        let refresh_periodic = refresh.clone();
        let refresh_devices_periodic = refresh_devices.clone();
        glib::timeout_add_seconds_local(10, move || {
            refresh_periodic();
            refresh_devices_periodic();
            glib::ControlFlow::Continue
        });

        // Debounce persistence: `power::save_config` shells out to
        // `powerprofilesctl` + several `busctl` calls (and pings the compositor),
        // so running it synchronously on every spin tick / keystroke froze the
        // UI — which also made the entry feel un-editable. Coalesce rapid edits
        // into one save that runs on a background thread.
        let save_pending: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
        let persist = {
            let sections = sections.clone();
            let save_pending = save_pending.clone();
            move || {
                let cfg = read_config(&sections);
                if let Some(id) = save_pending.borrow_mut().take() {
                    id.remove();
                }
                let save_pending_inner = save_pending.clone();
                let id = glib::timeout_add_local_once(Duration::from_millis(400), move || {
                    save_pending_inner.borrow_mut().take();
                    std::thread::spawn(move || power::save_config(&cfg));
                });
                *save_pending.borrow_mut() = Some(id);
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
    refresh_devices();
    scroller.upcast()
}

fn render(sections: &Sections, snap: &PowerSnapshot, apply_config: bool) {
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
    if !apply_config {
        return;
    }
    sections.profile.set_selected(profile_index(snap.profile));
    sections.blank.set_value(snap.config.blank_after_minutes as f64);
    sections.suspend.set_value(snap.config.suspend_after_minutes as f64);
    sections.lid.set_selected(lid_index(snap.config.lid_close));
    sections.dim.set_active(snap.config.dim_on_battery);
}

fn render_devices(sections: &Sections, bt: &BluetoothSnapshot) {
    // Clear previous rows (keep the empty-state label, which we toggle).
    while let Some(child) = sections.devices_body.first_child() {
        sections.devices_body.remove(&child);
    }

    let connected: Vec<_> = bt
        .devices
        .iter()
        .filter(|d| d.state == DeviceState::Connected)
        .collect();

    if connected.is_empty() {
        sections.devices_empty.set_text(if bt.adapter_present {
            "No wireless devices connected"
        } else {
            "No Bluetooth adapter"
        });
        let empty_row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        empty_row.add_css_class("metis-settings-row");
        empty_row.append(&sections.devices_empty);
        sections.devices_body.append(&empty_row);
        return;
    }

    for dev in connected {
        let battery = match dev.battery_percent {
            Some(pct) => {
                let charging = dev.battery_charging == Some(true);
                let icon = battery_icon_name(pct, charging);
                let text = if charging {
                    format!("{pct}% (charging)")
                } else {
                    format!("{pct}%")
                };
                let value = gtk::Label::new(Some(&text));
                value.add_css_class("metis-settings-value");
                // Charging devices aren't "low" regardless of level.
                if pct <= 20 && !charging {
                    value.add_css_class("metis-bt-battery-low");
                }
                let row = ui::row_with_icon(icon, &dev.name, &value);
                row
            }
            None => {
                let value = gtk::Label::new(Some("No battery info"));
                value.add_css_class("metis-settings-hint");
                ui::row_with_icon("bluetooth-active-symbolic", &dev.name, &value)
            }
        };
        sections.devices_body.append(&battery);
    }
}

/// Pick a symbolic battery icon bucket for a charge level, preferring the
/// charging variant when the device reports it's charging.
fn battery_icon_name(pct: u8, charging: bool) -> &'static str {
    if charging {
        return match pct {
            0..=10 => "battery-level-10-charging-symbolic",
            11..=30 => "battery-level-30-charging-symbolic",
            31..=50 => "battery-level-50-charging-symbolic",
            51..=70 => "battery-level-70-charging-symbolic",
            71..=90 => "battery-level-90-charging-symbolic",
            _ => "battery-level-100-charged-symbolic",
        };
    }
    match pct {
        0..=10 => "battery-level-10-symbolic",
        11..=30 => "battery-level-30-symbolic",
        31..=50 => "battery-level-50-symbolic",
        51..=70 => "battery-level-70-symbolic",
        71..=90 => "battery-level-90-symbolic",
        _ => "battery-level-100-symbolic",
    }
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
