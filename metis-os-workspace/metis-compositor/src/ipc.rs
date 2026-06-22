use std::io::{Read, Write};

use metis_protocol::{CompositorCommand, CompositorEvent};

use crate::events::{accept_event_subscribers, init_events_listener};
use crate::state::MetisState;

pub fn init_ipc(state: &mut MetisState) -> Result<(), Box<dyn std::error::Error>> {
    let socket_path = metis_protocol::ipc_socket_path();
    if socket_path.exists() {
        let _ = std::fs::remove_file(&socket_path);
    }
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let listener = std::os::unix::net::UnixListener::bind(&socket_path)?;
    listener.set_nonblocking(true)?;
    state.ipc_listener = Some(listener);

    state.events_listener = Some(init_events_listener(&state.event_bus)?);

    Ok(())
}

pub fn drain_ipc(state: &mut MetisState) {
    if let Some(ref listener) = state.events_listener {
        accept_event_subscribers(listener, &state.event_bus);
    }

    let mut pending = Vec::new();
    if let Some(listener) = state.ipc_listener.as_ref() {
        loop {
            match listener.accept() {
                Ok((stream, _)) => pending.push(stream),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => {
                    tracing::warn!("IPC accept error: {e}");
                    break;
                }
            }
        }
    }

    for mut stream in pending {
        // A freshly-accepted connection may not have its command bytes available
        // in this same drain pass (the client connects, then writes). Reading
        // non-blocking here dropped the connection without a reply whenever we
        // lost that race, which surfaced as `EAGAIN` on the shell. Use a short
        // blocking read/write timeout instead: clients write immediately after
        // connecting, so this returns in well under a millisecond in practice and
        // only ever waits when a command is genuinely mid-flight.
        let timeout = std::time::Duration::from_millis(50);
        let _ = stream.set_read_timeout(Some(timeout));
        let _ = stream.set_write_timeout(Some(timeout));
        let request = match read_request_line(&mut stream) {
            Some(line) => line,
            None => continue,
        };

        let reply = match serde_json::from_str::<CompositorCommand>(&request) {
            Ok(cmd) => {
                let evt = state.handle_ipc(cmd);
                serde_json::to_string(&evt).unwrap_or_default()
            }
            Err(err) => serde_json::to_string(&CompositorEvent::Error {
                message: err.to_string(),
            })
            .unwrap_or_default(),
        };

        if writeln!(stream, "{reply}").is_err() {
            tracing::warn!("IPC reply write failed");
            continue;
        }
        if stream.flush().is_err() {
            tracing::warn!("IPC reply flush failed");
        }
    }
}

/// Read one newline-delimited command without blocking the compositor event loop.
fn read_request_line(stream: &mut std::os::unix::net::UnixStream) -> Option<String> {
    let mut acc = String::new();
    let mut buf = [0u8; 8192];

    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                acc.push_str(&String::from_utf8_lossy(&buf[..n]));
                if acc.contains('\n') {
                    break;
                }
                if acc.len() > 256 * 1024 {
                    tracing::warn!("IPC request exceeded size limit");
                    break;
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                break;
            }
            Err(e) => {
                tracing::warn!(%e, "IPC read error");
                break;
            }
        }
    }

    let line = acc.lines().next()?.trim().to_string();
    if line.is_empty() {
        None
    } else {
        Some(line)
    }
}
