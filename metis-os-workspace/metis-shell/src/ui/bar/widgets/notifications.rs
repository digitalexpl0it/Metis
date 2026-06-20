use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use gtk::prelude::*;

use crate::services::{
    clear_notifications, notification_count, register_refresh, runtime_notifications,
    BarNotification, NotificationEntry, NotificationKind,
};

thread_local! {
    static DND: Cell<bool> = const { Cell::new(false) };
}

pub fn do_not_disturb() -> bool {
    DND.with(|d| d.get())
}

const SLIDE_MS: u32 = 240;

pub struct NotificationsWidget {
    root: gtk::Button,
    refresh: Rc<dyn Fn()>,
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
        panel.set_width_request(400);

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

        header.append(&title);
        header.append(&dnd_label);
        header.append(&dnd_switch);
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
            .spacing(10)
            .build();
        list.set_margin_top(2);
        list.set_margin_bottom(2);
        list.set_margin_start(2);
        list.set_margin_end(2);
        scrolled.set_child(Some(&list));
        panel.append(&scrolled);

        // Footer with the clear-all action, bottom-right.
        let footer = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .build();
        let clear_btn = gtk::Button::with_label("Clear all");
        clear_btn.add_css_class("metis-notif-clear");
        clear_btn.set_halign(gtk::Align::End);
        clear_btn.set_hexpand(true);
        footer.append(&clear_btn);
        panel.append(&footer);

        // Recompute the feed from the runtime store and repaint badge + list.
        // Runs on the GTK main thread; invoked on open and on runtime changes.
        let refresh: Rc<dyn Fn()> = {
            let badge = badge.clone();
            let list = list.clone();
            let clear_btn = clear_btn.clone();
            Rc::new(move || {
                let entries = runtime_notifications();
                let total = notification_count();
                if do_not_disturb() || total == 0 {
                    badge.set_visible(false);
                } else {
                    badge.set_label(&total.to_string());
                    badge.set_visible(true);
                }
                clear_btn.set_sensitive(!entries.is_empty());
                fill_list(&list, &entries);
            })
        };

        dnd_switch.connect_state_set({
            let refresh = refresh.clone();
            move |_, state| {
                DND.with(|d| d.set(state));
                refresh();
                glib::Propagation::Proceed
            }
        });

        clear_btn.connect_clicked({
            let list = list.clone();
            move |_| animate_clear(&list)
        });

        register_refresh(refresh.clone());

        // Optional demo feed for exercising the popup (grouping, scroll, icons).
        if std::env::var("METIS_DEMO_NOTIFICATIONS").is_ok() {
            seed_demo_notifications();
        }

        refresh();

        {
            let refresh = refresh.clone();
            super::super::dropdown::wire_toggle_prepare(&root, &panel, move || refresh());
        }

        Self {
            root: root.clone(),
            refresh,
        }
    }

    pub fn root(&self) -> &gtk::Button {
        &self.root
    }

    /// Kept for the bar snapshot pipeline; runtime notifications are the source
    /// of truth now, so poll-provided notifications are ignored here.
    pub fn update(&self, _notifications: &[BarNotification]) {
        (self.refresh)();
    }
}

/// Slide every card out to the left, then clear the store once the animation
/// has had time to play.
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
        glib::timeout_add_local_once(Duration::from_millis(SLIDE_MS as u64 + 20), || {
            clear_notifications();
        });
    } else {
        clear_notifications();
    }
}

fn fill_list(list: &gtk::Box, entries: &[NotificationEntry]) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    if entries.is_empty() {
        let empty = gtk::Label::builder()
            .label("No notifications")
            .wrap(true)
            .xalign(0.0)
            .build();
        empty.add_css_class("metis-notif-empty");
        list.append(&empty);
        return;
    }
    for entry in entries {
        let card = build_notification_card(entry);
        let revealer = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::SlideRight)
            .transition_duration(SLIDE_MS)
            .reveal_child(true)
            .child(&card)
            .build();
        list.append(&revealer);
    }
}

fn build_notification_card(entry: &NotificationEntry) -> gtk::Box {
    let notif = &entry.notification;
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .hexpand(true)
        .build();
    card.add_css_class("metis-notif-card");
    card.add_css_class(&format!("metis-notif-card-{}", notif.kind.css_suffix()));

    let icon = gtk::Image::from_icon_name(notif.kind.icon_name());
    icon.add_css_class("metis-notif-icon");
    icon.set_valign(gtk::Align::Start);
    icon.set_halign(gtk::Align::Start);
    icon.set_hexpand(false);
    icon.set_vexpand(false);
    card.append(&icon);

    let text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .hexpand(true)
        .build();

    // Wrapping labels need a bounded width or GTK's height-for-width pass
    // overflows inside the horizontal card (it allocated INT_MIN/huge widths).
    let title = gtk::Label::builder()
        .label(&notif.title)
        .halign(gtk::Align::Fill)
        .xalign(0.0)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .max_width_chars(34)
        .build();
    title.add_css_class("metis-notif-title");

    let message = gtk::Label::builder()
        .label(&notif.message)
        .halign(gtk::Align::Fill)
        .xalign(0.0)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .max_width_chars(34)
        .build();
    message.add_css_class("metis-notif-message");

    text.append(&title);
    text.append(&message);
    card.append(&text);

    if entry.count > 1 {
        let count = gtk::Label::new(Some(&format!("{}", entry.count)));
        count.add_css_class("metis-notif-count");
        count.set_valign(gtk::Align::Start);
        card.append(&count);
    }

    card
}

fn seed_demo_notifications() {
    use crate::services::push_notification;
    let demos = [
        (NotificationKind::Success, "Workspace saved", "Layout stored to disk."),
        (NotificationKind::Information, "Update available", "Restart Metis when convenient."),
        (NotificationKind::Payment, "Payment received", "Invoice #1042 was paid."),
        (NotificationKind::Error, "Sync failed", "Could not reach the calendar server."),
        (NotificationKind::Notification, "New message", "Ping from Metis Core."),
        (NotificationKind::Notification, "New message", "Ping from Metis Core."),
        (NotificationKind::Notification, "New message", "Ping from Metis Core."),
    ];
    for (kind, title, message) in demos {
        push_notification(BarNotification {
            kind,
            title: title.into(),
            message: message.into(),
        });
    }
}
