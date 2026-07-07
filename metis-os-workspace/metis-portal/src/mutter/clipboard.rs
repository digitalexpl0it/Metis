//! Remote-desktop clipboard bridge for gnome-remote-desktop.
//!
//! Mutter's `org.gnome.Mutter.RemoteDesktop.Session` uses option dicts and fd
//! passing — not the wrong `(as, v)` shape that caused `UnknownMethod` errors.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::fd::{FromRawFd, OwnedFd};
use std::sync::{Arc, Mutex};

use zbus::zvariant::Value;

use crate::compositor_ipc;

#[derive(Clone)]
pub struct ClipboardSession {
    inner: Arc<Mutex<Inner>>,
    conn: zbus::Connection,
    session_path: String,
}

struct Inner {
    enabled: bool,
    /// True while the remote RDP client owns the clipboard (after SetSelection).
    remote_owner: bool,
    remote_mimes: Vec<String>,
    transfer_serial: u32,
    pending_write_serial: Option<u32>,
    last_local: Option<LocalClip>,
}

#[derive(Clone)]
struct LocalClip {
    mime: String,
    text: Option<String>,
    image_path: Option<String>,
}

impl ClipboardSession {
    pub fn new(conn: zbus::Connection, session_path: String) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                enabled: false,
                remote_owner: false,
                remote_mimes: Vec::new(),
                transfer_serial: 0,
                pending_write_serial: None,
                last_local: None,
            })),
            conn,
            session_path,
        }
    }

    pub fn enable(&self, options: &HashMap<&str, Value<'_>>) -> Result<(), String> {
        let mut inner = self.inner.lock().map_err(|_| "clipboard lock".to_string())?;
        if inner.enabled {
            return Err("Already enabled".into());
        }
        inner.enabled = true;
        inner.remote_owner = false;
        inner.remote_mimes = mime_types_from_options(options);
        drop(inner);
        if let Some(local) = self.inner.lock().ok().and_then(|i| i.last_local.clone()) {
            self.emit_owner_changed(false, &local_mimes(&local));
        }
        Ok(())
    }

    pub fn disable(&self) -> Result<(), String> {
        let mut inner = self.inner.lock().map_err(|_| "clipboard lock".to_string())?;
        if !inner.enabled {
            return Err("Was not enabled".into());
        }
        inner.enabled = false;
        inner.remote_owner = false;
        inner.remote_mimes.clear();
        inner.pending_write_serial = None;
        Ok(())
    }

    pub fn set_selection(&self, options: &HashMap<&str, Value<'_>>) -> Result<(), String> {
        let mut inner = self.inner.lock().map_err(|_| "clipboard lock".to_string())?;
        if !inner.enabled {
            return Err("Clipboard not enabled".into());
        }
        let mimes = mime_types_from_options(options);
        if mimes.is_empty() {
            inner.remote_owner = false;
            inner.remote_mimes.clear();
            drop(inner);
            self.emit_owner_changed(false, &[]);
        } else {
            inner.remote_owner = true;
            inner.remote_mimes = mimes.clone();
            drop(inner);
            self.emit_owner_changed(true, &mimes);
        }
        Ok(())
    }

    /// GRD writes remote clipboard bytes to the returned fd, then calls SelectionWriteDone.
    pub fn selection_write(&self, serial: u32) -> Result<OwnedFd, String> {
        let mut inner = self.inner.lock().map_err(|_| "clipboard lock".to_string())?;
        if !inner.enabled {
            return Err("Clipboard not enabled".into());
        }
        if !inner.remote_owner {
            return Err("No current selection owned".into());
        }
        if inner.pending_write_serial.is_some() && inner.pending_write_serial != Some(serial) {
            tracing::warn!(
                serial,
                expected = ?inner.pending_write_serial,
                "SelectionWrite serial mismatch — accepting anyway"
            );
        }
        inner.pending_write_serial = None;
        drop(inner);

        let (read_fd, write_fd) = pipe::pipe().map_err(|err| format!("pipe: {err}"))?;
        let read_for_thread = read_fd.try_clone().map_err(|err| format!("pipe clone: {err}"))?;
        std::thread::Builder::new()
            .name("metis-rd-clip".into())
            .spawn(move || {
                let mut file = std::fs::File::from(read_for_thread);
                let mut buf = Vec::new();
                if file.read_to_end(&mut buf).is_ok() && !buf.is_empty() {
                    if let Ok(text) = std::str::from_utf8(&buf) {
                        compositor_ipc::set_clipboard("text/plain;charset=utf-8", Some(text), None);
                    }
                }
            })
            .ok();
        Ok(write_fd)
    }

    pub fn selection_write_done(&self, _serial: u32, _success: bool) -> Result<(), String> {
        let inner = self.inner.lock().map_err(|_| "clipboard lock".to_string())?;
        if !inner.enabled {
            return Err("Clipboard not enabled".into());
        }
        Ok(())
    }

    /// GRD reads local clipboard bytes from the returned fd.
    pub fn selection_read(&self, mime_type: &str) -> Result<OwnedFd, String> {
        let inner = self.inner.lock().map_err(|_| "clipboard lock".to_string())?;
        if !inner.enabled {
            return Err("Clipboard not enabled".into());
        }
        if inner.remote_owner {
            return Err("Tried to read own selection".into());
        }
        let Some(local) = inner.last_local.clone() else {
            return Err("No selection owner available".into());
        };
        drop(inner);

        let data = local_clip_bytes(&local, mime_type)?;
        let (mut read_fd, write_fd) = pipe::pipe().map_err(|err| format!("pipe: {err}"))?;
        let write_for_thread = write_fd.try_clone().map_err(|err| format!("pipe clone: {err}"))?;
        std::thread::spawn(move || {
            let mut file = std::fs::File::from(write_for_thread);
            let _ = file.write_all(&data);
        });
        Ok(read_fd)
    }

    pub fn request_transfer(&self, mime_type: &str) {
        let serial = {
            let mut inner = match self.inner.lock() {
                Ok(i) => i,
                Err(_) => return,
            };
            if !inner.enabled || !inner.remote_owner {
                return;
            }
            inner.transfer_serial = inner.transfer_serial.saturating_add(1);
            inner.pending_write_serial = Some(inner.transfer_serial);
            inner.transfer_serial
        };
        let _ = self.conn.emit_signal(
            None::<&str>,
            self.session_path.as_str(),
            "org.gnome.Mutter.RemoteDesktop.Session",
            "SelectionTransfer",
            &(mime_type, serial),
        );
    }

    pub fn on_local_clipboard_changed(
        &self,
        mime: &str,
        preview_text: Option<&str>,
        image_path: Option<&str>,
    ) {
        let enabled = self
            .inner
            .lock()
            .map(|i| i.enabled)
            .unwrap_or(false);
        if !enabled {
            return;
        }
        let local = LocalClip {
            mime: mime.to_string(),
            text: preview_text.map(str::to_string),
            image_path: image_path.map(str::to_string),
        };
        let mimes = local_mimes(&local);
        if let Ok(mut inner) = self.inner.lock() {
            inner.last_local = Some(local);
            inner.remote_owner = false;
        }
        self.emit_owner_changed(false, &mimes);
    }

    fn emit_owner_changed(&self, session_is_owner: bool, mime_types: &[String]) {
        let mut options: HashMap<String, Value<'_>> = HashMap::new();
        if !mime_types.is_empty() {
            options.insert("mime-types".into(), Value::from(mime_types.to_vec()));
            options.insert(
                "session-is-owner".into(),
                Value::from(session_is_owner),
            );
        }
        let _ = self.conn.emit_signal(
            None::<&str>,
            self.session_path.as_str(),
            "org.gnome.Mutter.RemoteDesktop.Session",
            "SelectionOwnerChanged",
            &(options,),
        );
    }
}

fn mime_types_from_options(_options: &HashMap<&str, Value<'_>>) -> Vec<String> {
    // GRD advertises text clipboard mimes; exact parsing of the options dict is
    // best-effort — empty still enables the sync path.
    vec![
        "text/plain;charset=utf-8".into(),
        "text/plain".into(),
        "UTF8_STRING".into(),
    ]
}

fn local_mimes(local: &LocalClip) -> Vec<String> {
    if local.text.is_some() {
        vec![
            "text/plain;charset=utf-8".into(),
            "text/plain".into(),
            "UTF8_STRING".into(),
        ]
    } else if local.image_path.is_some() {
        vec![local.mime.clone()]
    } else {
        vec![local.mime.clone()]
    }
}

fn local_clip_bytes(local: &LocalClip, mime_type: &str) -> Result<Vec<u8>, String> {
    if mime_type.contains("text") || mime_type == "UTF8_STRING" {
        if let Some(text) = &local.text {
            return Ok(text.as_bytes().to_vec());
        }
    }
    if let Some(path) = &local.image_path {
        return std::fs::read(path).map_err(|err| format!("read clipboard image: {err}"));
    }
    if let Some(text) = &local.text {
        return Ok(text.as_bytes().to_vec());
    }
    Err("No clipboard data for requested mime".into())
}

mod pipe {
    use std::os::fd::{FromRawFd, OwnedFd};

    pub fn pipe() -> std::io::Result<(OwnedFd, OwnedFd)> {
        let mut fds = [0i32; 2];
        let ret = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) };
        if ret != 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: pipe2 returned valid fds.
        unsafe {
            Ok((
                OwnedFd::from_raw_fd(fds[0]),
                OwnedFd::from_raw_fd(fds[1]),
            ))
        }
    }
}
