//! Windows-style "keep these display settings?" confirmation with auto-revert timer.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;

const CONFIRM_SECONDS: u32 = 15;

/// Apply the new arrangement, then ask the user to keep or revert it. Reverts
/// automatically when the countdown reaches zero.
pub fn show(
    parent: &gtk::Window,
    on_keep: Rc<dyn Fn()>,
    on_revert: Rc<dyn Fn()>,
) {
    let dialog = gtk::Window::builder()
        .title("Keep these display settings?")
        .modal(true)
        .transient_for(parent)
        .resizable(false)
        .default_width(440)
        .build();
    dialog.add_css_class("metis-settings-window");

    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(16)
        .margin_top(20)
        .margin_bottom(20)
        .margin_start(24)
        .margin_end(24)
        .build();

    let heading = gtk::Label::new(Some("Keep these display settings?"));
    heading.set_xalign(0.0);
    heading.add_css_class("metis-settings-section-title");
    root.append(&heading);

    let body = gtk::Label::new(None);
    body.set_wrap(true);
    body.set_xalign(0.0);
    body.add_css_class("metis-settings-hint");
    root.append(&body);

    let btn_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .halign(gtk::Align::End)
        .build();

    let revert_btn = gtk::Button::with_label("Revert");
    revert_btn.add_css_class("metis-settings-secondary");
    let keep_btn = gtk::Button::with_label("Keep changes");
    keep_btn.add_css_class("suggested-action");
    btn_row.append(&revert_btn);
    btn_row.append(&keep_btn);
    root.append(&btn_row);

    dialog.set_child(Some(&root));

    let resolved = Rc::new(RefCell::new(false));
    let remaining = Rc::new(RefCell::new(CONFIRM_SECONDS));
    let timer_id: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));

    let update_body: Rc<dyn Fn()> = {
        let body = body.clone();
        let remaining = remaining.clone();
        Rc::new(move || {
            let secs = *remaining.borrow();
            body.set_label(&format!(
                "Your display settings have been applied. If everything still looks correct, \
                 click Keep changes.\n\nOtherwise the previous settings will be restored in \
                 {secs} seconds."
            ));
        })
    };
    update_body();

    let finish = {
        let resolved = resolved.clone();
        let timer_id = timer_id.clone();
        let dialog = dialog.clone();
        let on_keep = on_keep.clone();
        let on_revert = on_revert.clone();
        Rc::new(move |keep: bool| {
            if *resolved.borrow() {
                return;
            }
            *resolved.borrow_mut() = true;
            if let Some(id) = timer_id.borrow_mut().take() {
                id.remove();
            }
            if keep {
                on_keep();
            } else {
                on_revert();
            }
            dialog.close();
        })
    };

    *timer_id.borrow_mut() = Some(glib::timeout_add_seconds_local(1, {
        let remaining = remaining.clone();
        let update_body = update_body.clone();
        let finish = finish.clone();
        move || {
            let next = remaining.borrow().saturating_sub(1);
            *remaining.borrow_mut() = next;
            update_body();
            if next == 0 {
                finish(false);
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        }
    }));

    keep_btn.connect_clicked({
        let finish = finish.clone();
        move |_| finish(true)
    });
    revert_btn.connect_clicked({
        let finish = finish.clone();
        move |_| finish(false)
    });
    dialog.connect_close_request({
        let finish = finish.clone();
        move |_| {
            finish(false);
            glib::Propagation::Stop
        }
    });

    dialog.present();
}
