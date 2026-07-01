//! Bluetooth: adapter power, scan, pair/connect, and device list.

use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use gtk::prelude::*;

use crate::bluetooth::{self, BluetoothSnapshot, DeviceState};
use crate::ui;

struct Sections {
    powered: gtk::Switch,
    scan_btn: gtk::Button,
    status: gtk::Label,
    list: gtk::Box,
    /// Mirrors the adapter's `Discovering` flag so the scan button can toggle
    /// between starting and stopping a scan without re-reading a snapshot.
    scanning: std::cell::Cell<bool>,
}

pub fn build() -> gtk::Widget {
    let (scroller, content) = ui::page_for("bluetooth");

    let (adapter_card, adapter_body) = ui::section("Adapter");
    let powered = gtk::Switch::new();
    adapter_body.append(&ui::row("Bluetooth", &powered));
    let scan_btn = gtk::Button::with_label("Scan for devices");
    adapter_body.append(&scan_btn);
    let status = gtk::Label::new(Some("Checking adapter…"));
    status.set_xalign(0.0);
    status.add_css_class("metis-settings-hint");
    adapter_body.append(&status);
    content.append(&adapter_card);

    let (dev_card, dev_body) = ui::section("Devices");
    let list = gtk::Box::new(gtk::Orientation::Vertical, 6);
    list.add_css_class("metis-settings-list");
    dev_body.append(&list);
    content.append(&dev_card);

    let sections = Rc::new(Sections {
        powered: powered.clone(),
        scan_btn: scan_btn.clone(),
        status,
        list,
        scanning: std::cell::Cell::new(false),
    });

    let (tx, rx) = mpsc::channel::<BluetoothSnapshot>();
    let refresh = {
        let tx = tx.clone();
        Rc::new(move || {
            let tx = tx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(bluetooth::load_snapshot());
            });
        })
    };

    {
        let sections_poll = sections.clone();
        let refresh_poll = refresh.clone();
        glib::timeout_add_local(Duration::from_millis(250), move || {
            if let Ok(snap) = rx.try_recv() {
                render(&sections_poll, &snap, &refresh_poll);
            }
            glib::ControlFlow::Continue
        });
        powered.connect_active_notify({
            let refresh = refresh.clone();
            move |s| {
                bluetooth::set_powered(s.is_active());
                schedule_refresh(&refresh, 800);
            }
        });
        scan_btn.connect_clicked({
            let refresh = refresh.clone();
            let sections = sections.clone();
            move |_| {
                if sections.scanning.get() {
                    bluetooth::stop_scan();
                    sections.scanning.set(false);
                    sections.scan_btn.set_label("Scan for devices");
                    schedule_refresh(&refresh, 600);
                } else {
                    bluetooth::start_scan();
                    sections.scanning.set(true);
                    sections.scan_btn.set_label("Stop scanning");
                    schedule_refresh(&refresh, 1500);
                    // Discovery keeps the radio busy and drains battery — stop it
                    // automatically if the user wanders off without stopping.
                    schedule_auto_stop_scan(&sections, &refresh, 30_000);
                }
            }
        });
    }

    refresh();
    scroller.upcast()
}

fn render(sections: &Sections, snap: &BluetoothSnapshot, refresh: &Rc<impl Fn() + 'static>) {
    if !snap.adapter_present {
        sections.powered.set_sensitive(false);
        sections.scan_btn.set_sensitive(false);
        sections.status.set_text("No Bluetooth adapter found.");
        clear_list(&sections.list);
        return;
    }
    sections.powered.set_sensitive(true);
    sections.scan_btn.set_sensitive(snap.powered);
    sections.powered.set_active(snap.powered);
    // Keep the toggle in lockstep with the adapter's real discovery state, so an
    // externally started/stopped scan (or a completed one) is reflected here.
    sections.scanning.set(snap.discovering && snap.powered);
    sections.scan_btn.set_label(if sections.scanning.get() {
        "Stop scanning"
    } else {
        "Scan for devices"
    });
    let status_text = if snap.discovering {
        "Scanning for nearby devices…".to_string()
    } else if snap.powered {
        format!("Adapter: {}", snap.adapter_name)
    } else {
        "Bluetooth is off".to_string()
    };
    sections.status.set_text(&status_text);

    clear_list(&sections.list);
    if snap.devices.is_empty() {
        let empty = gtk::Label::new(Some("No devices found. Tap Scan to search."));
        empty.set_xalign(0.0);
        empty.add_css_class("metis-settings-hint");
        sections.list.append(&empty);
        return;
    }
    for dev in &snap.devices {
        let row = device_row(dev, refresh);
        sections.list.append(&row);
    }
}

fn device_row(dev: &crate::bluetooth::BtDevice, refresh: &Rc<impl Fn() + 'static>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("metis-settings-row");

    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    let title = gtk::Label::new(Some(&dev.name));
    title.set_xalign(0.0);
    title.add_css_class("metis-settings-row-title");
    let sub = gtk::Label::new(Some(&format!(
        "{} · {}",
        dev.address,
        state_label(&dev.state)
    )));
    sub.set_xalign(0.0);
    sub.add_css_class("metis-settings-hint");
    text.append(&title);
    text.append(&sub);
    row.append(&text);

    if dev.state != DeviceState::Connected {
        let connect = gtk::Button::with_label("Connect");
        let addr = dev.address.clone();
        let refresh = refresh.clone();
        connect.connect_clicked(move |_| {
            bluetooth::pair_and_connect(&addr);
            schedule_refresh(&refresh, 2000);
        });
        row.append(&connect);
    } else {
        let disconnect = gtk::Button::with_label("Disconnect");
        let addr = dev.address.clone();
        let refresh = refresh.clone();
        disconnect.connect_clicked(move |_| {
            bluetooth::disconnect(&addr);
            schedule_refresh(&refresh, 1000);
        });
        row.append(&disconnect);
    }

    let remove = gtk::Button::with_label("Remove");
    remove.add_css_class("destructive-action");
    let addr = dev.address.clone();
    let refresh = refresh.clone();
    remove.connect_clicked(move |_| {
        bluetooth::remove_device(&addr);
        schedule_refresh(&refresh, 1000);
    });
    row.append(&remove);
    row
}

fn state_label(s: &DeviceState) -> &'static str {
    match s {
        DeviceState::Connected => "Connected",
        DeviceState::Paired => "Paired",
        DeviceState::Available => "Available",
    }
}

fn clear_list(list: &gtk::Box) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
}

fn schedule_refresh(refresh: &Rc<impl Fn() + 'static>, delay_ms: u64) {
    let refresh = refresh.clone();
    glib::timeout_add_local_once(Duration::from_millis(delay_ms), move || refresh());
}

/// Stop an in-progress scan after `delay_ms` (no-op if the user already stopped
/// it or it ended), then refresh so the button + status return to idle.
fn schedule_auto_stop_scan(
    sections: &Rc<Sections>,
    refresh: &Rc<impl Fn() + 'static>,
    delay_ms: u64,
) {
    let sections = sections.clone();
    let refresh = refresh.clone();
    glib::timeout_add_local_once(Duration::from_millis(delay_ms), move || {
        if sections.scanning.get() {
            bluetooth::stop_scan();
            sections.scanning.set(false);
            sections.scan_btn.set_label("Scan for devices");
            refresh();
        }
    });
}
