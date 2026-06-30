use std::rc::Rc;
use std::time::Duration;

use gtk::prelude::*;

use crate::services::{
    clear_history, clipboard_count, load_history, recall_entry, register_clipboard_refresh,
    runtime_clipboard_entries, ClipboardEntry,
};

const SLIDE_MS: u32 = 240;

pub struct ClipboardWidget {
    root: gtk::Button,
    refresh: Rc<dyn Fn()>,
}

impl ClipboardWidget {
    pub fn new() -> Self {
        load_history();

        let root = gtk::Button::builder().build();
        root.add_css_class("metis-bar-widget");
        root.add_css_class("metis-bar-clipboard");
        root.add_css_class("metis-bar-sys-icon");

        let icon = gtk::Label::builder().label("📋").build();
        icon.add_css_class("metis-bar-clipboard-icon");

        let overlay = gtk::Overlay::new();
        overlay.add_css_class("metis-bar-clipboard-overlay");
        overlay.set_child(Some(&icon));

        let badge = gtk::Label::builder().label("").build();
        badge.add_css_class("metis-bar-notif-badge");
        badge.set_visible(false);
        badge.set_halign(gtk::Align::End);
        badge.set_valign(gtk::Align::Start);
        overlay.add_overlay(&badge);
        root.set_child(Some(&overlay));

        let panel = super::super::dropdown::build_panel();
        panel.set_spacing(10);
        panel.set_width_request(400);

        let header = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(10)
            .build();

        let title = gtk::Label::builder()
            .label("Clipboard")
            .hexpand(true)
            .halign(gtk::Align::Start)
            .build();
        title.add_css_class("metis-bar-section-title");
        header.append(&title);
        panel.append(&header);

        let scrolled = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .height_request(320)
            .width_request(372)
            .build();
        scrolled.add_css_class("metis-notif-scrolled");

        let list = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .build();
        list.set_margin_top(2);
        list.set_margin_bottom(2);
        list.set_margin_start(2);
        list.set_margin_end(2);
        scrolled.set_child(Some(&list));
        panel.append(&scrolled);

        let footer = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .build();
        let clear_btn = gtk::Button::with_label("Clear history");
        clear_btn.add_css_class("metis-notif-clear");
        clear_btn.set_halign(gtk::Align::End);
        clear_btn.set_hexpand(true);
        footer.append(&clear_btn);
        panel.append(&footer);

        let refresh: Rc<dyn Fn()> = {
            let badge = badge.clone();
            let list = list.clone();
            let clear_btn = clear_btn.clone();
            Rc::new(move || {
                let entries = runtime_clipboard_entries();
                let total = clipboard_count();
                if total == 0 {
                    badge.set_visible(false);
                } else {
                    badge.set_label(&total.to_string());
                    badge.set_visible(true);
                }
                clear_btn.set_sensitive(!entries.is_empty());
                let list = list.clone();
                glib::idle_add_local_once(move || fill_list(&list, &entries));
            })
        };

        clear_btn.connect_clicked({
            let list = list.clone();
            move |_| animate_clear(&list)
        });

        register_clipboard_refresh(refresh.clone());
        refresh();

        {
            let refresh = refresh.clone();
            super::super::dropdown::wire_toggle_prepare(&root, &panel, move || refresh());
        }

        Self { root, refresh }
    }

    pub fn root(&self) -> &gtk::Button {
        &self.root
    }

    pub fn update(&self) {
        (self.refresh)();
    }
}

fn animate_clear(list: &gtk::Box) {
    let mut any = false;
    let mut child = list.first_child();
    while let Some(c) = child {
        let next = c.next_sibling();
        if let Ok(rev) = c.clone().downcast::<gtk::Revealer>() {
            rev.set_reveal_child(false);
            any = true;
        }
        child = next;
    }
    if any {
        glib::timeout_add_local_once(Duration::from_millis(SLIDE_MS as u64 + 20), clear_history);
    } else {
        clear_history();
    }
}

fn fill_list(list: &gtk::Box, entries: &[ClipboardEntry]) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    if entries.is_empty() {
        let empty = gtk::Label::builder()
            .label("No clipboard history")
            .halign(gtk::Align::Center)
            .build();
        empty.add_css_class("metis-muted-label");
        list.append(&empty);
        return;
    }
    for entry in entries {
        list.append(&build_row(entry));
    }
}

fn build_row(entry: &ClipboardEntry) -> gtk::Widget {
    let revealer = gtk::Revealer::new();
    revealer.set_reveal_child(true);
    revealer.set_transition_type(gtk::RevealerTransitionType::SlideRight);
    revealer.set_transition_duration(SLIDE_MS);

    let row = gtk::Button::builder().hexpand(true).build();
    row.add_css_class("metis-clipboard-row");

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .build();

    if let Some(path) = entry.image_path.as_deref() {
        let picture = gtk::Picture::for_filename(path);
        picture.set_content_fit(gtk::ContentFit::Contain);
        picture.set_size_request(48, 48);
        content.append(&picture);
    }

    let label_text = entry
        .preview_text
        .clone()
        .unwrap_or_else(|| entry.mime.clone());
    let label = gtk::Label::builder()
        .label(&label_text)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .xalign(0.0)
        .hexpand(true)
        .halign(gtk::Align::Fill)
        .max_width_chars(40)
        .build();
    label.add_css_class("metis-clipboard-preview");
    content.append(&label);
    row.set_child(Some(&content));

    let entry = entry.clone();
    row.connect_clicked(move |_| {
        if let Err(err) = recall_entry(&entry) {
            tracing::warn!(%err, "clipboard recall failed");
        }
    });

    revealer.set_child(Some(&row));
    revealer.upcast()
}
