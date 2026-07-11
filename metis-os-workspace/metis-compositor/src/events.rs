use std::io::{ErrorKind, Write};
use std::sync::{Arc, Mutex};

use metis_protocol::CompositorEvent;

/// Broadcast compositor events to subscribed shell clients (newline-delimited JSON).
#[derive(Clone, Default)]
pub struct EventBus {
    subscribers: Arc<Mutex<Vec<std::os::unix::net::UnixStream>>>,
}

impl EventBus {
    pub fn subscribe(&self, stream: std::os::unix::net::UnixStream) {
        // Non-blocking: a stalled shell/portal reader must never freeze the
        // compositor (ClipboardChanged after screenshots previously could).
        let _ = stream.set_nonblocking(true);
        if let Ok(mut subs) = self.subscribers.lock() {
            subs.push(stream);
        }
    }

    pub fn emit(&self, event: &CompositorEvent) {
        let Ok(line) = serde_json::to_string(event) else {
            return;
        };
        let mut payload = line;
        payload.push('\n');
        let bytes = payload.as_bytes();

        if let Ok(mut subs) = self.subscribers.lock() {
            subs.retain_mut(|stream| write_event_nonblocking(stream, bytes));
        }
    }

    pub fn prune_dead(&self) {
        if let Ok(mut subs) = self.subscribers.lock() {
            subs.retain(|stream| stream.peer_addr().is_ok());
        }
    }
}

/// Best-effort non-blocking write. Drop the subscriber on hard errors; keep it
/// on WouldBlock so a briefly busy reader is not permanently removed.
fn write_event_nonblocking(stream: &mut std::os::unix::net::UnixStream, bytes: &[u8]) -> bool {
    let mut offset = 0;
    while offset < bytes.len() {
        match stream.write(&bytes[offset..]) {
            Ok(0) => return false,
            Ok(n) => offset += n,
            Err(err) if err.kind() == ErrorKind::Interrupted => continue,
            Err(err) if err.kind() == ErrorKind::WouldBlock => {
                tracing::debug!("event subscriber temporarily busy; dropping event for that client");
                return true;
            }
            Err(_) => return false,
        }
    }
    true
}

pub fn init_events_listener(
    bus: &EventBus,
) -> Result<std::os::unix::net::UnixListener, std::io::Error> {
    let path = metis_protocol::events_socket_path();
    if path.exists() {
        let _ = std::fs::remove_file(&path);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let listener = std::os::unix::net::UnixListener::bind(&path)?;
    listener.set_nonblocking(true)?;
    tracing::info!(path = ?path, "compositor event socket ready");
    let _ = bus;
    Ok(listener)
}

pub fn accept_event_subscribers(
    listener: &std::os::unix::net::UnixListener,
    bus: &EventBus,
) {
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                tracing::info!("shell subscribed to compositor events");
                bus.subscribe(stream);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(e) => {
                tracing::warn!("event subscriber accept error: {e}");
                break;
            }
        }
    }
}
