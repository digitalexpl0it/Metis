use std::io::Write;
use std::sync::{Arc, Mutex};

use metis_protocol::CompositorEvent;

/// Broadcast compositor events to subscribed shell clients (newline-delimited JSON).
#[derive(Clone, Default)]
pub struct EventBus {
    subscribers: Arc<Mutex<Vec<std::os::unix::net::UnixStream>>>,
}

impl EventBus {
    pub fn subscribe(&self, stream: std::os::unix::net::UnixStream) {
        // Blocking writes so ClipboardChanged (and other) events are not dropped
        // when the shell reader is briefly behind.
        let _ = stream.set_nonblocking(false);
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

        if let Ok(mut subs) = self.subscribers.lock() {
            subs.retain_mut(|stream| stream.write_all(payload.as_bytes()).is_ok());
        }
    }

    pub fn prune_dead(&self) {
        if let Ok(mut subs) = self.subscribers.lock() {
            subs.retain(|stream| stream.peer_addr().is_ok());
        }
    }
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
