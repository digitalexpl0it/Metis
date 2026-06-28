//! Printers: CUPS queue listing and launcher for system printer settings.

use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use gtk::prelude::*;

use crate::printers::{self, PrintersSnapshot};
use crate::ui;

struct Sections {
    status: gtk::Label,
    list: gtk::Box,
}

pub fn build() -> gtk::Widget {
    let (scroller, content) = ui::page("Printers");

    let open = gtk::Button::with_label("Open printer settings…");
    open.add_css_class("suggested-action");
    content.append(&open);

    let (card, body) = ui::section("Installed printers");
    let status = gtk::Label::new(None);
    status.set_xalign(0.0);
    body.append(&status);
    let list = gtk::Box::new(gtk::Orientation::Vertical, 6);
    list.add_css_class("metis-settings-list");
    body.append(&list);
    content.append(&card);

    let sections = Rc::new(Sections { status, list });

    let (tx, rx) = mpsc::channel::<PrintersSnapshot>();
    let refresh = {
        let tx = tx.clone();
        Rc::new(move || {
            let tx = tx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(printers::load_snapshot());
            });
        })
    };

    {
        let sections = sections.clone();
        glib::timeout_add_local(Duration::from_millis(200), move || {
            if let Ok(snap) = rx.try_recv() {
                render(&sections, &snap);
            }
            glib::ControlFlow::Continue
        });
    }

    open.connect_clicked(|_| printers::open_printer_settings());
    refresh();
    scroller.upcast()
}

fn render(sections: &Sections, snap: &PrintersSnapshot) {
    while let Some(child) = sections.list.first_child() {
        sections.list.remove(&child);
    }
    if !snap.cups_available {
        sections
            .status
            .set_text("CUPS not available. Install cups and system-config-printer.");
        return;
    }
    if let Some(default) = &snap.default_printer {
        sections
            .status
            .set_text(&format!("Default printer: {default}"));
    } else {
        sections.status.set_text("No default printer set");
    }
    if snap.printers.is_empty() {
        let empty = gtk::Label::new(Some("No printers configured."));
        empty.set_xalign(0.0);
        empty.add_css_class("metis-settings-hint");
        sections.list.append(&empty);
        return;
    }
    for p in &snap.printers {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        row.add_css_class("metis-settings-row");
        let name = gtk::Label::new(Some(&p.name));
        name.set_xalign(0.0);
        name.set_hexpand(true);
        row.append(&name);
        let state = gtk::Label::new(Some(&p.state));
        state.add_css_class("metis-settings-hint");
        row.append(&state);
        if p.is_default {
            let badge = gtk::Label::new(Some("Default"));
            badge.add_css_class("metis-settings-badge");
            row.append(&badge);
        }
        sections.list.append(&row);
    }
}
