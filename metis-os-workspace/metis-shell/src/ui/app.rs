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
                SystemEvent::CompositorConnected => {
                    crate::services::windows::reconcile_now();
                }
                SystemEvent::Compositor(evt) => {
                    if let metis_protocol::CompositorEvent::WorkspaceChanged { output, active, .. } =
                        &evt
                    {
                        crate::services::set_active_workspace(output, *active);
                        crate::ui::bar::refresh_workspaces();
                        // Pull a fresh window list so each output's dock reflects
                        // the now-visible workspace (and any cross-workspace move).
                        crate::services::windows::reconcile_now();
                    }
                    crate::services::windows::apply_event(&evt);
                    // A freshly opened window arrives without its output/workspace
                    // (the event doesn't carry them); reconcile so dock filtering
                    // routes it to the right bar promptly.
                    if matches!(&evt, metis_protocol::CompositorEvent::WindowOpened { .. }) {
                        crate::services::windows::reconcile_now();
                    }
                }
                SystemEvent::BriefingReady(items) => {
                    tracing::info!(count = items.len(), "briefing ready");
                }
            }
        }
        glib::ControlFlow::Continue
    });

    // Safety-net reconcile: the event stream is authoritative, but a periodic
    // ListWindows resync guards against any dropped/missed window events.
    glib::timeout_add_seconds_local(5, || {
        crate::services::windows::reconcile_now();
        glib::ControlFlow::Continue
    });
}
