//! Freedesktop notification daemon (`org.freedesktop.Notifications`).
//!
//! Runs a `zbus` server on a dedicated background thread (its own current-thread
//! tokio runtime) and forwards every incoming `Notify` into the in-bar
//! notification store. Because that store is `thread_local` to the GTK main
//! thread, the daemon hands notifications across an `mpsc` channel that the bar
//! drains on the UI thread.
//!
//! The bus name is requested with the replace flags so Metis takes over from any
//! previously running daemon (dunst/mako). If another daemon later reclaims the
//! name, Metis simply stops receiving — acceptable for a desktop shell.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;

use zbus::fdo::RequestNameFlags;
use zbus::interface;
use zbus::zvariant::{OwnedValue, Value};

use super::notifications::{BarNotification, NotificationKind};

struct NotifyServer {
    tx: Mutex<Sender<BarNotification>>,
    next_id: AtomicU32,
}

#[interface(name = "org.freedesktop.Notifications")]
impl NotifyServer {
    /// Implements `org.freedesktop.Notifications.Notify`. Returns the assigned
    /// notification id (echoes `replaces_id` when non-zero, per spec).
    #[allow(clippy::too_many_arguments)]
    async fn notify(
        &self,
        app_name: String,
        replaces_id: u32,
        _app_icon: String,
        summary: String,
        body: String,
        _actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        _expire_timeout: i32,
    ) -> u32 {
        let kind = urgency_kind(&hints);
        let title = if summary.trim().is_empty() {
            app_name
        } else {
            summary
        };
        let note = BarNotification {
            kind,
            title,
            message: body,
        };
        if let Ok(tx) = self.tx.lock() {
            let _ = tx.send(note);
        }
        if replaces_id != 0 {
            replaces_id
        } else {
            self.next_id.fetch_add(1, Ordering::Relaxed).max(1)
        }
    }

    /// `CloseNotification` — Metis notifications are dismissed via the bar UI, so
    /// this is a no-op acknowledgement.
    async fn close_notification(&self, _id: u32) {}

    /// `GetCapabilities` — advertise only what the in-bar popup actually renders.
    fn get_capabilities(&self) -> Vec<String> {
        vec!["body".to_string()]
    }

    /// `GetServerInformation` — (name, vendor, version, spec_version).
    fn get_server_information(&self) -> (String, String, String, String) {
        (
            "Metis".to_string(),
            "metis".to_string(),
            env!("CARGO_PKG_VERSION").to_string(),
            "1.2".to_string(),
        )
    }
}

/// Map the freedesktop `urgency` hint (byte: 0 low, 1 normal, 2 critical) to a
/// Metis notification kind. Anything missing/odd is treated as a normal alert.
fn urgency_kind(hints: &HashMap<String, OwnedValue>) -> NotificationKind {
    let urgency = hints.get("urgency").and_then(|v| match &**v {
        Value::U8(b) => Some(*b),
        _ => None,
    });
    match urgency {
        Some(0) => NotificationKind::Information,
        Some(2) => NotificationKind::Error,
        _ => NotificationKind::Notification,
    }
}

/// Start the notification daemon on a background thread and return the receiver
/// the bar polls on the GTK main thread.
pub fn spawn_notification_service() -> Receiver<BarNotification> {
    let (tx, rx) = channel::<BarNotification>();
    if let Err(err) = std::thread::Builder::new()
        .name("metis-notify-dbus".to_string())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(err) => {
                    tracing::error!(%err, "notify: failed to build runtime");
                    return;
                }
            };
            if let Err(err) = rt.block_on(run(tx)) {
                tracing::warn!(%err, "notify: dbus service stopped");
            }
        })
    {
        tracing::error!(%err, "notify: failed to spawn dbus thread");
    }
    rx
}

async fn run(tx: Sender<BarNotification>) -> zbus::Result<()> {
    let server = NotifyServer {
        tx: Mutex::new(tx),
        next_id: AtomicU32::new(1),
    };
    let conn = zbus::connection::Builder::session()?
        .serve_at("/org/freedesktop/Notifications", server)?
        .build()
        .await?;

    let flags = RequestNameFlags::ReplaceExisting | RequestNameFlags::AllowReplacement;
    conn.request_name_with_flags("org.freedesktop.Notifications", flags)
        .await?;
    tracing::info!("notify: acquired org.freedesktop.Notifications");

    // Keep the connection (and thus the service) alive for the process lifetime.
    std::future::pending::<()>().await;
    Ok(())
}
