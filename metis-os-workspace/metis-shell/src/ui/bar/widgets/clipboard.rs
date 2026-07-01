use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;

use crate::services::{
    active_entry_id, clear_history, delete_entry, filtered_entries, load_history, page_size,
    private_mode, recall_entry, register_clipboard_refresh, set_page_size, set_private_mode,
    toggle_favorite, ClipboardEntry, ClipboardPage,
};
use crate::ui::icons;

struct UiState {
    search: String,
    page: usize,
}

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
        root.set_child(Some(&icons::image(icons::names::CLIPBOARD)));

        let panel = super::super::dropdown::build_panel();
        panel.add_css_class("metis-clipboard-panel");
        panel.set_spacing(8);
        panel.set_width_request(420);

        let search = gtk::SearchEntry::builder()
            .placeholder_text("Type here to search…")
            .hexpand(true)
            .build();
        search.add_css_class("metis-clipboard-search");
        panel.append(&search);

        let scrolled = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .height_request(340)
            .vexpand(true)
            .build();
        scrolled.add_css_class("metis-notif-scrolled");

        let list = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(0)
            .build();
        list.add_css_class("metis-clipboard-list");
        scrolled.set_child(Some(&list));
        panel.append(&scrolled);

        let footer = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .build();
        footer.add_css_class("metis-clipboard-footer");

        let prev_btn = icon_button("go-previous-symbolic", "Previous page");
        let next_btn = icon_button("go-next-symbolic", "Next page");
        let page_label = gtk::Label::builder()
            .label("")
            .halign(gtk::Align::Start)
            .build();
        page_label.add_css_class("metis-muted-label");

        let nav = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(4)
            .build();
        nav.append(&prev_btn);
        nav.append(&next_btn);
        nav.append(&page_label);

        let private_label = gtk::Label::builder().label("Private mode").build();
        private_label.add_css_class("metis-notif-dnd-label");
        let private_switch = gtk::Switch::new();
        private_switch.set_active(private_mode());
        private_switch.set_valign(gtk::Align::Center);

        let private_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(6)
            .hexpand(true)
            .halign(gtk::Align::End)
            .build();
        private_box.append(&private_label);
        private_box.append(&private_switch);

        let clear_btn = icon_button("user-trash-symbolic", "Clear history");
        clear_btn.add_css_class("metis-clipboard-footer-btn");

        let settings_btn = icon_button("emblem-system-symbolic", "Clipboard settings");
        settings_btn.add_css_class("metis-clipboard-footer-btn");

        footer.append(&nav);
        footer.append(&private_box);
        footer.append(&clear_btn);
        footer.append(&settings_btn);
        panel.append(&footer);

        let ui_state = Rc::new(RefCell::new(UiState {
            search: String::new(),
            page: 0,
        }));

        let refresh: Rc<dyn Fn()> = {
            let list = list.clone();
            let page_label = page_label.clone();
            let private_switch = private_switch.clone();
            let ui_state = ui_state.clone();
            Rc::new(move || {
                private_switch.set_active(private_mode());
                let page_data = {
                    let mut state = ui_state.borrow_mut();
                    let page_data = filtered_entries(&state.search, state.page);
                    state.page = page_data.page;
                    page_data
                };
                page_label.set_label(&page_indicator(&page_data));
                let list = list.clone();
                let active = active_entry_id();
                glib::idle_add_local_once(move || {
                    fill_list(&list, &page_data.entries, active);
                });
            })
        };

        let settings_popover = gtk::Popover::builder().has_arrow(true).build();
        settings_popover.add_css_class("metis-bar-popover");
        settings_popover.set_parent(&settings_btn);
        let settings_panel = super::super::dropdown::build_panel();
        settings_panel.add_css_class("metis-clipboard-settings-menu");
        settings_panel.set_spacing(2);
        settings_panel.set_width_request(220);

        let settings_items: Rc<RefCell<Vec<(gtk::Button, gtk::Image, usize)>>> =
            Rc::new(RefCell::new(Vec::new()));
        for (label, size) in [
            ("25 entries per page", 25_usize),
            ("50 entries per page", 50),
            ("100 entries per page", 100),
        ] {
            let btn = gtk::Button::builder().has_frame(false).hexpand(true).build();
            btn.add_css_class("metis-clipboard-settings-item");

            let row = gtk::Box::builder()
                .orientation(gtk::Orientation::Horizontal)
                .spacing(8)
                .build();
            let check = icons::image("object-select-symbolic");
            check.add_css_class("metis-clipboard-settings-check");
            let text = gtk::Label::builder()
                .label(label)
                .halign(gtk::Align::Start)
                .hexpand(true)
                .build();
            text.add_css_class("metis-clipboard-settings-label");
            row.append(&check);
            row.append(&text);
            btn.set_child(Some(&row));

            let popover = settings_popover.clone();
            let items = settings_items.clone();
            let repaint = refresh.clone();
            btn.connect_clicked(move |b| {
                set_page_size(size);
                sync_settings_selection(&items, size);
                popover.popdown();
                repaint();
            });
            settings_items
                .borrow_mut()
                .push((btn.clone(), check, size));
            settings_panel.append(&btn);
        }
        settings_popover.set_child(Some(&settings_panel));

        settings_btn.connect_clicked({
            let settings_popover = settings_popover.clone();
            let items = settings_items.clone();
            move |_| {
                sync_settings_selection(&items, page_size());
                settings_popover.popup();
            }
        });

        search.connect_search_changed({
            let ui_state = ui_state.clone();
            let repaint = refresh.clone();
            move |entry| {
                ui_state.borrow_mut().search = entry.text().to_string();
                ui_state.borrow_mut().page = 0;
                repaint();
            }
        });

        prev_btn.connect_clicked({
            let ui_state = ui_state.clone();
            let repaint = refresh.clone();
            move |_| {
                let mut state = ui_state.borrow_mut();
                state.page = state.page.saturating_sub(1);
                repaint();
            }
        });

        next_btn.connect_clicked({
            let ui_state = ui_state.clone();
            let repaint = refresh.clone();
            move |_| {
                let mut state = ui_state.borrow_mut();
                state.page = state.page.saturating_add(1);
                repaint();
            }
        });

        private_switch.connect_active_notify({
            let repaint = refresh.clone();
            move |sw| {
                set_private_mode(sw.is_active());
                repaint();
            }
        });

        clear_btn.connect_clicked({
            let repaint = refresh.clone();
            move |_| {
                clear_history();
                repaint();
            }
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

fn sync_settings_selection(
    items: &Rc<RefCell<Vec<(gtk::Button, gtk::Image, usize)>>>,
    active_size: usize,
) {
    for (btn, check, size) in items.borrow().iter() {
        let selected = *size == active_size;
        check.set_visible(selected);
        if selected {
            btn.add_css_class("metis-clipboard-settings-active");
        } else {
            btn.remove_css_class("metis-clipboard-settings-active");
        }
    }
}

fn icon_button(icon_name: &str, tooltip: &str) -> gtk::Button {
    let btn = gtk::Button::builder().has_frame(false).build();
    btn.add_css_class("metis-clipboard-icon-btn");
    btn.set_tooltip_text(Some(tooltip));
    btn.set_child(Some(&icons::image(icon_name)));
    btn
}

fn page_indicator(page: &ClipboardPage) -> String {
    if page.total_matching == 0 {
        "0 entries".to_string()
    } else {
        format!(
            "Page {} / {} · {} entries",
            page.page + 1,
            page.total_pages,
            page.total_matching
        )
    }
}

fn fill_list(list: &gtk::Box, entries: &[ClipboardEntry], active_id: Option<u64>) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    if entries.is_empty() {
        let empty = gtk::Label::builder()
            .label("No clipboard history")
            .halign(gtk::Align::Center)
            .margin_top(24)
            .margin_bottom(24)
            .build();
        empty.add_css_class("metis-muted-label");
        list.append(&empty);
        return;
    }
    for entry in entries {
        list.append(&build_row(entry, active_id == Some(entry.id)));
    }
}

fn build_row(entry: &ClipboardEntry, is_active: bool) -> gtk::Widget {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .hexpand(true)
        .build();
    row.add_css_class("metis-clipboard-row");

    let marker = gtk::Label::builder().label("●").build();
    marker.add_css_class(if is_active {
        "metis-clipboard-active-marker"
    } else {
        "metis-clipboard-inactive-marker"
    });
    marker.set_width_request(14);
    row.append(&marker);

    let body = gtk::Button::builder().has_frame(false).hexpand(true).build();
    body.add_css_class("metis-clipboard-body");

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .build();

    if let Some(path) = entry.image_path.as_deref() {
        let picture = gtk::Picture::for_filename(path);
        picture.set_content_fit(gtk::ContentFit::Contain);
        picture.set_size_request(40, 40);
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
        .max_width_chars(48)
        .lines(3)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    label.add_css_class("metis-clipboard-preview");
    content.append(&label);
    body.set_child(Some(&content));

    let entry_click = entry.clone();
    body.connect_clicked(move |_| {
        if let Err(err) = recall_entry(&entry_click) {
            tracing::warn!(%err, "clipboard recall failed");
        }
    });
    row.append(&body);

    let pin_btn = icon_button(
        "view-pin-symbolic",
        if entry.favorited {
            "Unpin (allow auto-removal)"
        } else {
            "Pin (keep forever)"
        },
    );
    pin_btn.add_css_class("metis-clipboard-row-action");
    if entry.favorited {
        pin_btn.add_css_class("metis-clipboard-pinned");
    }
    let entry_id = entry.id;
    pin_btn.connect_clicked(move |_| toggle_favorite(entry_id));
    row.append(&pin_btn);

    let delete_btn = icon_button("user-trash-symbolic", "Delete entry");
    delete_btn.add_css_class("metis-clipboard-row-action");
    delete_btn.connect_clicked(move |_| delete_entry(entry_id));
    row.append(&delete_btn);

    row.upcast()
}
