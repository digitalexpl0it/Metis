use std::io::{Read, Write};
use std::os::unix::net::UnixStream;

use smithay::wayland::selection::data_device::{
    request_data_device_client_selection, set_data_device_selection,
};

use crate::events::EventBus;
use crate::state::MetisState;

const TEXT_PREVIEW_CHARS: usize = 200;
const MAX_CLIPBOARD_BYTES: usize = 10 * 1024 * 1024;

const TEXT_MIMES: &[&str] = &[
    "text/plain;charset=utf-8",
    "text/plain",
    "UTF8_STRING",
    "TEXT",
    "STRING",
];
const IMAGE_MIMES: &[&str] = &["image/png", "image/bmp"];

/// Clipboard read in flight after a deferred capture request.
pub(crate) struct PendingClipboardRead {
    pub read: UnixStream,
    pub mime: String,
    pub data: Vec<u8>,
}

/// User data attached to compositor-owned clipboard offers (shell recall).
#[derive(Clone, Default)]
pub struct MetisSelectionUserData {
    pub offer: Option<CompositorClipboardOffer>,
}

#[derive(Clone)]
pub struct CompositorClipboardOffer {
    pub mime: String,
    pub data: Vec<u8>,
}

pub fn preferred_clipboard_mime(mimes: &[String]) -> Option<String> {
    for pref in TEXT_MIMES {
        if let Some(m) = mimes.iter().find(|m| m.as_str() == *pref) {
            return Some(m.clone());
        }
    }
    if mimes.iter().any(|m| m.starts_with("text/plain")) {
        return mimes
            .iter()
            .find(|m| m.starts_with("text/plain"))
            .cloned();
    }
    for pref in IMAGE_MIMES {
        if let Some(m) = mimes.iter().find(|m| m.as_str() == *pref) {
            return Some(m.clone());
        }
    }
    None
}

pub fn write_selection_to_fd(
    fd: std::os::unix::io::OwnedFd,
    offer: &CompositorClipboardOffer,
) {
    let mut file = std::fs::File::from(fd);
    if let Err(err) = file.write_all(&offer.data) {
        tracing::debug!(?err, "failed to write compositor clipboard offer");
    }
}

impl MetisState {
    /// Queue capture for the next Wayland dispatch tick. `new_selection` fires
    /// before smithay commits the offer to the seat, so an immediate read fails.
    pub fn queue_clipboard_capture(&mut self, mimes: Vec<String>) {
        if self.clipboard_capture_suppressed > 0 {
            return;
        }
        if preferred_clipboard_mime(&mimes).is_some() {
            self.pending_clipboard_mimes = Some(mimes);
        }
    }

    /// Start a read once the seat selection is committed, then poll reads without
    /// blocking the compositor thread.
    pub fn flush_pending_clipboard_capture(&mut self) {
        if let Some(mimes) = self.pending_clipboard_mimes.take() {
            self.start_clipboard_capture(mimes);
        }
        self.drain_clipboard_reads();
    }

    fn start_clipboard_capture(&mut self, mimes: Vec<String>) {
        if self.clipboard_capture_suppressed > 0 {
            return;
        }
        let Some(mime) = preferred_clipboard_mime(&mimes) else {
            tracing::debug!(?mimes, "clipboard capture: no supported mime");
            return;
        };

        let (read, write) = match UnixStream::pair() {
            Ok(pair) => pair,
            Err(err) => {
                tracing::warn!(?err, "clipboard pipe failed");
                return;
            }
        };

        if let Err(err) =
            request_data_device_client_selection(&self.seat, mime.clone(), write.into())
        {
            tracing::debug!(?err, ?mime, "clipboard read request failed");
            return;
        }

        let _ = read.set_nonblocking(true);
        self.pending_clipboard_reads.push(PendingClipboardRead {
            read,
            mime,
            data: Vec::new(),
        });
    }

    pub fn drain_clipboard_reads(&mut self) {
        use std::io::ErrorKind;

        self.pending_clipboard_reads.retain_mut(|pending| {
            let mut chunk = [0u8; 65_536];
            loop {
                match pending.read.read(&mut chunk) {
                    Ok(0) => {
                        if !pending.data.is_empty() {
                            emit_clipboard_changed(
                                &self.event_bus,
                                &pending.mime,
                                std::mem::take(&mut pending.data),
                            );
                        }
                        return false;
                    }
                    Ok(n) => {
                        if pending.data.len() + n > MAX_CLIPBOARD_BYTES {
                            tracing::debug!("clipboard payload exceeds size cap");
                            return false;
                        }
                        pending.data.extend_from_slice(&chunk[..n]);
                    }
                    Err(err) if err.kind() == ErrorKind::WouldBlock => return true,
                    Err(err) => {
                        tracing::debug!(?err, "clipboard payload read failed");
                        return false;
                    }
                }
            }
        });
    }

    pub fn set_clipboard_from_command(
        &mut self,
        mime: String,
        text: Option<String>,
        image_path: Option<String>,
    ) -> Result<(), String> {
        let data = if let Some(t) = text {
            t.into_bytes()
        } else if let Some(path) = image_path {
            std::fs::read(&path).map_err(|e| format!("read {path}: {e}"))?
        } else {
            return Err("SetClipboard requires text or image_path".into());
        };
        if data.is_empty() {
            return Err("clipboard data is empty".into());
        }
        if data.len() > MAX_CLIPBOARD_BYTES {
            return Err("clipboard payload exceeds size cap".into());
        }

        self.clipboard_capture_suppressed += 1;
        let offer = CompositorClipboardOffer {
            mime: mime.clone(),
            data,
        };
        set_data_device_selection(
            &self.display_handle,
            &self.seat,
            vec![mime],
            MetisSelectionUserData {
                offer: Some(offer),
            },
        );
        self.clipboard_capture_suppressed -= 1;
        Ok(())
    }
}

fn emit_clipboard_changed(bus: &EventBus, mime: &str, data: Vec<u8>) {
    if data.len() > MAX_CLIPBOARD_BYTES {
        return;
    }

    let (preview_text, image_path) = if mime.starts_with("text/")
        || matches!(mime, "UTF8_STRING" | "TEXT" | "STRING")
    {
        let text = String::from_utf8_lossy(&data);
        let preview = if text.chars().count() > TEXT_PREVIEW_CHARS {
            text.chars().take(TEXT_PREVIEW_CHARS).collect::<String>() + "…"
        } else {
            text.into_owned()
        };
        (Some(preview), None)
    } else if mime.starts_with("image/") {
        let ext = if mime.contains("png") {
            "png"
        } else {
            "bmp"
        };
        let dir = clipboard_image_dir();
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let id = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let path = dir.join(format!("{id}.{ext}"));
        if std::fs::write(&path, &data).is_err() {
            return;
        }
        (None, Some(path.to_string_lossy().into_owned()))
    } else {
        return;
    };

    bus.emit(&metis_protocol::CompositorEvent::ClipboardChanged {
        mime: mime.to_string(),
        preview_text: preview_text.clone(),
        image_path: image_path.clone(),
    });
    tracing::info!(
        mime,
        bytes = data.len(),
        has_text = preview_text.is_some(),
        has_image = image_path.is_some(),
        "clipboard history captured"
    );
}

fn clipboard_image_dir() -> std::path::PathBuf {
    metis_protocol::runtime_dir().join("clipboard")
}
