use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;

use crate::services::BarNotification;

thread_local! {
    static DND: Cell<bool> = const { Cell::new(false) };
}

pub fn do_not_disturb() -> bool {
    DND.with(|d| d.get())
}

pub struct NotificationsWidget {
    root: gtk::Button,
    badge: gtk::Label,
    pending: Rc<RefCell<Vec<BarNotification>>>,
    list_built: Rc<Cell<bool>>,
}

impl NotificationsWidget {
    pub fn new() -> Self {
        let root = gtk::Button::builder().build();
        root.add_css_class("metis-bar-widget");
        root.add_css_class("metis-bar-notifications");
        root.add_css_class("metis-bar-sys-icon");

        let icon = gtk::Label::builder().label("🔔").build();
        icon.add_css_class("metis-bar-notif-icon");

        let overlay = gtk::Overlay::new();
        overlay.add_css_class("metis-bar-notif-overlay");
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
        panel.set_margin_top(12);
        panel.set_margin_bottom(12);
        panel.set_margin_start(14);
        panel.set_margin_end(14);
        panel.set_width_request(360);

        let header = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(10)
            .build();

        let title = gtk::Label::builder()
            .label("Notifications")
            .hexpand(true)
            .halign(gtk::Align::Start)
            .build();
        title.add_css_class("metis-bar-section-title");

        let dnd_label = gtk::Label::builder()
            .label("Do Not Disturb")
            .halign(gtk::Align::End)
            .build();
        dnd_label.add_css_class("metis-notif-dnd-label");

        let dnd_switch = gtk::Switch::new();
        dnd_switch.set_active(do_not_disturb());
        dnd_switch.connect_state_set(|_, state| {
            DND.with(|d| d.set(state));
            glib::Propagation::Proceed
        });

        header.append(&title);
        header.append(&dnd_label);
        header.append(&dnd_switch);
        panel.append(&header);

        let scrolled = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .height_request(320)
            .width_request(332)
            .build();
        scrolled.add_css_class("metis-notif-scrolled");

        let list = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(10)
            .build();
        scrolled.set_child(Some(&list));
        panel.append(&scrolled);

        let widget = Self {
            root: root.clone(),
            badge,
            pending: Rc::new(RefCell::new(Vec::new())),
            list_built: Rc::new(Cell::new(false)),
        };

        let prepare_list = {
            let list = list.clone();
            let pending = widget.pending.clone();
            let list_built = widget.list_built.clone();
            move || {
                if list_built.get() {
                    return;
                }
                fill_list(&list, &pending.borrow());
                list_built.set(true);
            }
        };

        super::super::dropdown::wire_toggle_prepare(
            &root,
            &panel,
            prepare_list,
        );

        widget
    }

    pub fn root(&self) -> &gtk::Button {
        &self.root
    }

    pub fn update(&self, notifications: &[BarNotification]) {
        *self.pending.borrow_mut() = notifications.to_vec();
        self.list_built.set(false);

        let count = notifications.len() as u32;
        if do_not_disturb() || count == 0 {
            self.badge.set_visible(false);
        } else {
            self.badge.set_label(&count.to_string());
            self.badge.set_visible(true);
        }
    }
}

fn fill_list(list: &gtk::Box, notifications: &[BarNotification]) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    if notifications.is_empty() {
        let empty = gtk::Label::builder()
            .label("No notifications")
            .wrap(true)
            .xalign(0.0)
            .build();
        empty.add_css_class("metis-notif-empty");
        list.append(&empty);
        return;
    }
    for notif in notifications {
        list.append(&build_notification_card(notif));
    }
}

fn build_notification_card(notif: &BarNotification) -> gtk::Box {
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .margin_top(10)
        .margin_bottom(10)
        .margin_start(12)
        .margin_end(12)
        .hexpand(true)
        .build();
    card.add_css_class("metis-notif-card");
    card.add_css_class(&format!("metis-notif-card-{}", notif.kind.css_suffix()));

    let title = gtk::Label::builder()
        .label(&notif.title)
        .halign(gtk::Align::Start)
        .xalign(0.0)
        .build();
    title.add_css_class("metis-notif-title");

    let message = gtk::Label::builder()
        .label(&notif.message)
        .halign(gtk::Align::Start)
        .xalign(0.0)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::Word)
        .build();
    message.add_css_class("metis-notif-message");

    card.append(&title);
    card.append(&message);
    card
}
