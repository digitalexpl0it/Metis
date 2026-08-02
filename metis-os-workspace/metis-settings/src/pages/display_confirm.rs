//! Windows-style "keep these display settings?" confirmation with auto-revert timer.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::gdk::prelude::*;
use gtk::glib;
use gtk::prelude::*;
use metis_i18n::tr;

const CONFIRM_SECONDS: u32 = 15;

/// Apply the new arrangement, then ask the user to keep or revert it. Reverts
/// automatically when the countdown reaches zero.
pub fn show(parent: &gtk::Window, on_keep: Rc<dyn Fn()>, on_revert: Rc<dyn Fn()>) {
    // Force the parent to repaint after a modeset/layout apply — GTK can keep a
    // fully transparent buffer while Metis SSD still draws the outer chrome.
    parent.queue_draw();
    if let Some(surface) = parent.surface() {
        surface.queue_render();
    }

    let title = tr("Keep these display settings?");
    let dialog = gtk::Window::builder()
        .title(&title)
        .modal(true)
        .transient_for(parent)
        .resizable(false)
        .default_width(440)
        .build();
    // Opaque sheet (not a transparent buffer with only a border) — same pattern
    // as password / widget dialogs, but with a solid window fill so modeset
    // redraw glitches cannot leave a hollow frame.
    dialog.add_css_class("metis-settings-window");
    dialog.add_css_class("metis-settings-confirm-dialog");

    let sheet = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(16)
        .margin_top(20)
        .margin_bottom(20)
        .margin_start(24)
        .margin_end(24)
        .build();
    sheet.add_css_class("metis-settings-dialog-sheet");

    let heading = gtk::Label::new(Some(&title));
    heading.set_xalign(0.0);
    heading.add_css_class("metis-settings-section-title");
    sheet.append(&heading);

    let body = gtk::Label::new(None);
    body.set_wrap(true);
    body.set_xalign(0.0);
    body.add_css_class("metis-settings-hint");
    sheet.append(&body);

    let btn_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .halign(gtk::Align::End)
        .build();

    let revert_label = tr("Revert");
    let keep_label = tr("Keep changes");
    let revert_btn = gtk::Button::with_label(&revert_label);
    revert_btn.add_css_class("metis-settings-secondary");
    let keep_btn = gtk::Button::with_label(&keep_label);
    keep_btn.add_css_class("suggested-action");
    btn_row.append(&revert_btn);
    btn_row.append(&keep_btn);
    sheet.append(&btn_row);

    dialog.set_child(Some(&sheet));

    let resolved = Rc::new(RefCell::new(false));
    let remaining = Rc::new(RefCell::new(CONFIRM_SECONDS));
    let timer_id: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));

    let update_body: Rc<dyn Fn()> = {
        let body = body.clone();
        let remaining = remaining.clone();
        Rc::new(move || {
            let secs = *remaining.borrow();
            body.set_label(
                &tr(
                    "Your display settings have been applied. If everything still looks correct, \
                     click Keep changes.\n\nOtherwise the previous settings will be restored in \
                     %1 seconds.",
                )
                .replace("%1", &secs.to_string()),
            );
        })
    };
    update_body();

    let finish = {
        let resolved = resolved.clone();
        let timer_id = timer_id.clone();
        let dialog = dialog.clone();
        let parent = parent.clone();
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
            // Drop the modal grab before destroying — otherwise a failed close
            // leaves Settings input-dead behind a zombie dialog stuck at "0 seconds".
            dialog.set_modal(false);
            let dialog = dialog.clone();
            let parent = parent.clone();
            // Defer destroy so we aren't tearing the window down from inside a
            // close-request / button handler.
            glib::idle_add_local_once(move || {
                dialog.destroy();
                parent.queue_draw();
                if let Some(surface) = parent.surface() {
                    surface.queue_render();
                }
            });
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
    // Must Proceed so GTK actually destroys the window. Stop + finish→close was
    // a deadlock: close re-entered this handler, finish no-op'd (already resolved),
    // and Stop kept the modal window alive forever.
    dialog.connect_close_request({
        let finish = finish.clone();
        move |_| {
            finish(false);
            glib::Propagation::Proceed
        }
    });

    dialog.present();
    dialog.queue_draw();
}
