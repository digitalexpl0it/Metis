use std::cell::RefCell;
use std::sync::mpsc::Receiver;

use gtk::prelude::*;
use gtk::Application;

use crate::state::{MetisInit, SystemEvent};
use crate::ui::{bar, splash, theme};

thread_local! {
    /// Must outlive the GTK main loop — dropping `ApplicationHoldGuard` releases the app.
    static APP_HOLD: RefCell<Option<gtk::gio::ApplicationHoldGuard>> = const { RefCell::new(None) };
}

/// Minimal GTK shell — edge bar only (no command overlay).
pub fn run(init: MetisInit) {
    let app = Application::builder()
        .application_id("com.metis.shell")
        .build();

    let event_rx = RefCell::new(Some(init.event_rx));
    app.connect_activate(move |app| {
        tracing::info!("GTK application activate — initializing edge bar");
        theme::install_theme();
        splash::show(app);
        bar::init_and_show(app);
        if let Some(rx) = event_rx.borrow_mut().take() {
            attach_system_events(rx);
        }
        // The bar maps and pollers attach shortly after activate; let the splash
        // ramp to completion and fade once the shell is up and running.
        glib::timeout_add_seconds_local_once(2, splash::finish);
        APP_HOLD.with(|hold| {
            *hold.borrow_mut() = Some(app.hold());
        });
    });

    app.run();
}

fn attach_system_events(event_rx: Receiver<SystemEvent>) {
    glib::timeout_add_local(std::time::Duration::from_millis(32), move || {
        while let Ok(event) = event_rx.try_recv() {
            match event {
                SystemEvent::Status(msg) => tracing::debug!(%msg, "status"),
                SystemEvent::CompositorConnected | SystemEvent::Compositor(_) => {}
                SystemEvent::BriefingReady(items) => {
                    tracing::info!(count = items.len(), "briefing ready");
                }
            }
        }
        glib::ControlFlow::Continue
    });
}
