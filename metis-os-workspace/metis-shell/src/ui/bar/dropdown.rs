use std::cell::RefCell;

use gtk::prelude::*;

thread_local! {
    static POPOVERS: RefCell<Vec<gtk::Popover>> = const { RefCell::new(Vec::new()) };
}

/// Styled panel container for popover content.
pub fn build_panel() -> gtk::Box {
    let panel = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    panel.add_css_class("metis-bar-dropdown-panel");
    panel
}

/// Single-click toggle using a GTK popover (no layer-shell resize).
pub fn wire_toggle(button: &gtk::Button, panel: &gtk::Box, _key: &str) {
    wire_toggle_prepare(button, panel, || {});
}

/// Run `prepare` before opening.
pub fn wire_toggle_prepare(
    button: &gtk::Button,
    panel: &gtk::Box,
    prepare: impl Fn() + 'static,
) {
    let popover = gtk::Popover::builder()
        .autohide(true)
        .has_arrow(true)
        .child(panel)
        .build();
    popover.add_css_class("metis-bar-popover");
    popover.set_parent(button);

    POPOVERS.with(|list| list.borrow_mut().push(popover.clone()));

    let popover_weak = popover.downgrade();
    button.connect_clicked(move |_| {
        let Some(popover) = popover_weak.upgrade() else {
            return;
        };
        if popover.is_visible() {
            glib::idle_add_local_once(move || {
                popover.popdown();
            });
            return;
        }
        prepare();
        // Defer popup so we are not inside the compositor's pointer-dispatch stack.
        glib::idle_add_local_once(move || {
            popover.popup();
        });
    });
}

pub fn close_all() {
    POPOVERS.with(|list| {
        for popover in list.borrow().iter() {
            popover.popdown();
        }
    });
}

pub fn is_open() -> bool {
    POPOVERS.with(|list| list.borrow().iter().any(|p| p.is_visible()))
}

pub fn is_open_for(_key: &str) -> bool {
    is_open()
}

pub fn request_close_all() {
    if !is_open() {
        return;
    }
    glib::idle_add_local_once(close_all);
}
