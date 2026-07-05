//! Gaming — read-only gamepad/touchscreen list and session hints.

use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use gtk::prelude::*;

use crate::gaming::{GamingSnapshot, InputDevice, SteamInstall};
use crate::ui;

struct Sections {
    steam: gtk::Label,
    gpu: gtk::Label,
    gamepad_list: gtk::Box,
    touch_list: gtk::Box,
}

pub fn build() -> gtk::Widget {
    let (scroller, content) = ui::page_for("gaming");

    let (session_card, session_body) =
        ui::section_with_icon("Session", "applications-games-symbolic");
    let steam = value_label("Checking…");
    session_body.append(&readout_row("Steam", &steam));
    let gpu = value_label("");
    gpu.set_wrap(true);
    gpu.add_css_class("metis-settings-hint");
    session_body.append(&readout_row("GPU offload", &gpu));
    content.append(&session_card);

    let (pad_card, pad_body) =
        ui::section_with_icon("Gamepads", "input-gamepad-symbolic");
    let gamepad_list = gtk::Box::new(gtk::Orientation::Vertical, 6);
    gamepad_list.add_css_class("metis-settings-list");
    pad_body.append(&gamepad_list);
    content.append(&pad_card);

    let (touch_card, touch_body) =
        ui::section_with_icon("Touchscreens", "input-touchpad-symbolic");
    let touch_list = gtk::Box::new(gtk::Orientation::Vertical, 6);
    touch_list.add_css_class("metis-settings-list");
    touch_body.append(&touch_list);
    content.append(&touch_card);

    let (help_card, help_body) = ui::section("Tips");
    let hint = gtk::Label::new(Some(
        "Metis does not grab gamepads — native and Proton games read /dev/input/event* \
         directly. Flatpak games often need: flatpak override --user --device=all <app-id>. \
         See docs/USER_GUIDE.md for Steam, GameMode, and MangoHud launch options.",
    ));
    hint.set_xalign(0.0);
    hint.set_wrap(true);
    hint.add_css_class("metis-settings-hint");
    help_body.append(&hint);
    content.append(&help_card);

    let (tx, rx) = mpsc::channel::<GamingSnapshot>();
    let refresh = {
        let tx = tx.clone();
        Rc::new(move || {
            let tx = tx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(crate::gaming::load_snapshot());
            });
        })
    };

    let sections = Rc::new(Sections {
        steam,
        gpu,
        gamepad_list,
        touch_list,
    });

    {
        let refresh = refresh.clone();
        glib::timeout_add_local(Duration::from_secs(2), move || {
            refresh();
            glib::ControlFlow::Continue
        });
    }

    {
        let sections = sections.clone();
        glib::timeout_add_local(Duration::from_millis(250), move || {
            while let Ok(snapshot) = rx.try_recv() {
                apply_snapshot(&sections, &snapshot);
            }
            glib::ControlFlow::Continue
        });
    }

    refresh();
    scroller.upcast()
}

fn value_label(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.add_css_class("metis-settings-value");
    label
}

fn readout_row(title: &str, value: &gtk::Label) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.add_css_class("metis-settings-row");
    let title = gtk::Label::new(Some(title));
    title.set_xalign(0.0);
    title.set_hexpand(true);
    row.append(&title);
    row.append(value);
    row
}

fn apply_snapshot(sections: &Sections, snapshot: &GamingSnapshot) {
    sections.steam.set_text(match snapshot.steam {
        SteamInstall::Native => "Installed (native .deb / PATH)",
        SteamInstall::Flatpak => "Installed (Flatpak com.valvesoftware.Steam)",
        SteamInstall::None => "Not detected",
    });
    sections.gpu.set_text(&snapshot.gpu_hint);
    rebuild_device_list(&sections.gamepad_list, &snapshot.gamepads, "No gamepads detected");
    rebuild_device_list(
        &sections.touch_list,
        &snapshot.touchscreens,
        "No touchscreens detected",
    );
}

fn rebuild_device_list(list: &gtk::Box, devices: &[InputDevice], empty_label: &str) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    if devices.is_empty() {
        let label = gtk::Label::new(Some(empty_label));
        label.set_xalign(0.0);
        label.add_css_class("metis-settings-hint");
        list.append(&label);
        return;
    }
    for dev in devices {
        list.append(&device_row(dev));
    }
}

fn device_row(dev: &InputDevice) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Vertical, 2);
    row.add_css_class("metis-settings-row");

    let title = gtk::Label::new(Some(&dev.name));
    title.set_xalign(0.0);
    row.append(&title);

    let mut details = Vec::new();
    if let Some(v) = &dev.vendor {
        details.push(format!("Vendor {v}"));
    }
    if let Some(p) = &dev.product {
        details.push(format!("Product {p}"));
    }
    if !dev.handlers.is_empty() {
        details.push(format!("Handlers: {}", dev.handlers.join(", ")));
    }
    if let Some(path) = &dev.sysfs_path {
        details.push(path.clone());
    }
    if !details.is_empty() {
        let sub = gtk::Label::new(Some(&details.join(" · ")));
        sub.set_xalign(0.0);
        sub.set_wrap(true);
        sub.add_css_class("metis-settings-hint");
        row.append(&sub);
    }

    row
}
