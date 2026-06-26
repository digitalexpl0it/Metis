//! Send a one-shot runtime command to the running Metis shell so it re-applies
//! config we just wrote. Mirrors `scripts/metis-cmd.sh` — the shell polls the
//! command file every 100ms and removes it after handling.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use metis_protocol::{CompositorCommand, CompositorEvent};

pub fn send(cmd: &str) {
    let path = metis_protocol::runtime_command_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(err) = std::fs::write(&path, format!("{cmd}\n")) {
        tracing::warn!(%err, cmd, "failed to write runtime command");
    }
}

/// Ask the compositor to re-read `wallpaper.json` and apply the background live
/// (picture, solid colour, or gradient). Best-effort.
pub fn apply_background() {
    if let Err(err) = send_command(CompositorCommand::ApplyBackground) {
        tracing::warn!(%err, "failed to apply background via compositor IPC");
    }
}

/// Apply a layout mode (grid vs. scrolling) to every workspace on every output
/// immediately, so changing the "New workspace layout" default acts as a live
/// global on/off rather than only affecting future workspaces. Best-effort.
pub fn apply_default_layout(kind: metis_protocol::LayoutKind) {
    if let Err(err) = send_command(CompositorCommand::SetDefaultLayout { kind }) {
        tracing::warn!(%err, "failed to apply default layout via compositor IPC");
    }
}

/// Query the compositor for the connected outputs (name + geometry), primary
/// first. Returns an empty list if the compositor is unreachable, so callers can
/// degrade to a single global background.
pub fn list_outputs() -> Vec<metis_protocol::OutputInfo> {
    match send_command(CompositorCommand::ListOutputs) {
        Ok(CompositorEvent::OutputList { outputs }) => outputs,
        Ok(_) => Vec::new(),
        Err(err) => {
            tracing::warn!(%err, "failed to list outputs via compositor IPC");
            Vec::new()
        }
    }
}

fn send_command(cmd: CompositorCommand) -> std::io::Result<CompositorEvent> {
    let path = metis_protocol::ipc_socket_path();
    let mut stream = UnixStream::connect(&path)?;
    stream.set_read_timeout(Some(Duration::from_millis(600)))?;
    let payload = serde_json::to_string(&cmd).map_err(std::io::Error::other)?;
    writeln!(stream, "{payload}")?;
    stream.flush()?;
    let mut reader = BufReader::new(stream);
    let mut response = String::new();
    reader.read_line(&mut response)?;
    serde_json::from_str(response.trim()).map_err(|e| std::io::Error::other(e.to_string()))
}
