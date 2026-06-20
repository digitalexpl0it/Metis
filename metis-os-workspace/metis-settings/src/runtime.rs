//! Send a one-shot runtime command to the running Metis shell so it re-applies
//! config we just wrote. Mirrors `scripts/metis-cmd.sh` — the shell polls the
//! command file every 100ms and removes it after handling.

pub fn send(cmd: &str) {
    let path = metis_protocol::runtime_command_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(err) = std::fs::write(&path, format!("{cmd}\n")) {
        tracing::warn!(%err, cmd, "failed to write runtime command");
    }
}
