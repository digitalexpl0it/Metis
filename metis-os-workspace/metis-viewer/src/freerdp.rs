//! Resolve and spawn FreeRDP without a shell.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Preferred FreeRDP clients under `/usr/bin` only (no `PATH`, no `sh -c`).
const CANDIDATES: &[&str] = &["wlfreerdp3", "wlfreerdp", "xfreerdp3", "xfreerdp"];

pub fn freerdp_install_hint() -> String {
    "FreeRDP was not found under /usr/bin.\n\
     Install a client (Ubuntu):\n\
     sudo apt install freerdp3-wayland\n\
     # or: sudo apt install freerdp2-x11"
        .into()
}

/// Absolute path to the first available FreeRDP binary, if any.
pub fn resolve_freerdp() -> Option<PathBuf> {
    for name in CANDIDATES {
        let path = Path::new("/usr/bin").join(name);
        if is_executable(&path) {
            return Some(path);
        }
    }
    None
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    match path.metadata() {
        Ok(meta) if meta.is_file() => meta.permissions().mode() & 0o111 != 0,
        _ => false,
    }
}

#[derive(Debug, Clone)]
pub struct ConnectRequest {
    pub host: String,
    pub port: u16,
    pub username: String,
    /// When set and non-empty, passed as `/p:` on the child argv only (never logged).
    /// When empty/`None`, `/p:` is omitted so FreeRDP can prompt interactively.
    pub password: Option<String>,
}

/// Spawn FreeRDP with argv-only arguments. Does not wait for the client to exit.
pub fn spawn_freerdp(req: &ConnectRequest) -> Result<PathBuf, String> {
    let bin = resolve_freerdp().ok_or_else(freerdp_install_hint)?;
    let host = req.host.trim();
    if host.is_empty() {
        return Err("Host is required.".into());
    }
    if req.port == 0 {
        return Err("Port must be between 1 and 65535.".into());
    }

    let mut args: Vec<String> = vec![
        format!("/v:{host}:{}", req.port),
        format!("/u:{}", req.username.trim()),
        "/dynamic-resolution".into(),
        "/network:auto".into(),
    ];
    if let Some(pw) = req.password.as_ref() {
        if !pw.is_empty() {
            args.push(format!("/p:{pw}"));
        }
    }

    tracing::info!(
        binary = %bin.display(),
        host = %host,
        port = req.port,
        user = %req.username.trim(),
        "spawning FreeRDP"
    );

    Command::new(&bin)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("failed to start {}: {e}", bin.display()))?;

    Ok(bin)
}
