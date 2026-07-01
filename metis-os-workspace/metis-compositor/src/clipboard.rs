use std::io::{Read, Write};
use std::os::unix::net::UnixStream;

use smithay::wayland::selection::data_device::{
    current_data_device_selection_userdata, request_data_device_client_selection,
    set_data_device_selection,
};
use smithay::wayland::selection::primary_selection::set_primary_selection;
use smithay::wayland::selection::SelectionTarget;

use crate::events::EventBus;
use crate::state::MetisState;

const TEXT_PREVIEW_CHARS: usize = 200;
const MAX_CLIPBOARD_BYTES: usize = 10 * 1024 * 1024;
const DRAIN_CLIPBOARD_BYTES_PER_TICK: usize = 512 * 1024;
const ASYNC_WRITE_THRESHOLD: usize = 256 * 1024;

const TEXT_MIMES: &[&str] = &[
    "text/plain;charset=utf-8",
    "text/plain",
    "UTF8_STRING",
    "TEXT",
    "STRING",
];
const IMAGE_MIMES: &[&str] = &[
    "image/png",
    "image/jpeg",
    "image/bmp",
    "image/webp",
    "image/x-png",
];

/// Clipboard read in flight after a deferred capture request.
pub(crate) struct PendingClipboardRead {
    pub read: UnixStream,
    pub mime: String,
    pub data: Vec<u8>,
}

/// User data attached to compositor-owned clipboard offers (shell recall).
#[derive(Clone, Default)]
pub struct MetisSelectionUserData {
    /// In-memory payload (text recall and small data).
    pub offer: Option<CompositorClipboardOffer>,
    /// Image recall: read from disk only when a client requests paste.
    pub lazy_file: Option<(String, String)>,
}

#[derive(Clone)]
pub struct CompositorClipboardOffer {
    pub mime: String,
    pub data: Vec<u8>,
}

impl MetisSelectionUserData {
    /// Resolve bytes to serve for a paste request.
    pub fn resolve_payload(&self) -> Option<(String, Vec<u8>)> {
        if let Some(offer) = self.offer.as_ref() {
            return Some((offer.mime.clone(), offer.data.clone()));
        }
        if let Some((path, mime)) = self.lazy_file.as_ref() {
            let data = std::fs::read(path).ok()?;
            if data.is_empty() || data.len() > MAX_CLIPBOARD_BYTES {
                return None;
            }
            return Some((mime.clone(), data));
        }
        None
    }
}

fn normalize_mime(mime: &str) -> String {
    mime.split(';')
        .next()
        .unwrap_or(mime)
        .trim()
        .to_ascii_lowercase()
}

fn push_unique_mime(out: &mut Vec<String>, mime: &str) {
    let normalized = normalize_mime(mime);
    if normalized.is_empty() {
        return;
    }
    if out.iter().any(|m| normalize_mime(m) == normalized) {
        return;
    }
    out.push(normalized);
}

/// Whether a paste request can be satisfied by an offer mime.
pub fn selection_mime_satisfies(offer_mime: &str, request_mime: &str) -> bool {
    let offer = normalize_mime(offer_mime);
    let request = normalize_mime(request_mime);
    if offer == request {
        return true;
    }
    if offer.starts_with("image/") && request.starts_with("image/") {
        // Clients often negotiate a different image/* alias than we captured.
        return true;
    }
    matches!(
        (offer.as_str(), request.as_str()),
        ("image/png", "image/x-png") | ("image/x-png", "image/png")
            | ("image/jpeg", "image/jpg")
            | ("image/jpg", "image/jpeg")
    )
}

fn recall_mime_types(stored_mime: &str, path: Option<&str>) -> Vec<String> {
    let mut mimes = Vec::new();
    push_unique_mime(&mut mimes, stored_mime);
    if let Some(path) = path {
        if let Some(ext) = std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
        {
            match ext.to_ascii_lowercase().as_str() {
                "png" => {
                    push_unique_mime(&mut mimes, "image/png");
                    push_unique_mime(&mut mimes, "image/x-png");
                }
                "jpg" | "jpeg" => {
                    push_unique_mime(&mut mimes, "image/jpeg");
                    push_unique_mime(&mut mimes, "image/jpg");
                }
                "webp" => push_unique_mime(&mut mimes, "image/webp"),
                "bmp" => push_unique_mime(&mut mimes, "image/bmp"),
                _ => {}
            }
        }
    }
    mimes
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
        if let Some(m) = mimes.iter().find(|m| normalize_mime(m) == *pref) {
            return Some(m.clone());
        }
    }
    mimes
        .iter()
        .find(|m| normalize_mime(m).starts_with("image/"))
        .cloned()
}

/// Write compositor-owned selection bytes to the fd offered by the client.
pub fn write_selection_to_fd(fd: std::os::unix::io::OwnedFd, data: Vec<u8>) {
    if data.len() >= ASYNC_WRITE_THRESHOLD {
        std::thread::spawn(move || {
            let mut file = std::fs::File::from(fd);
            if let Err(err) = file.write_all(&data) {
                tracing::debug!(?err, "failed to write compositor clipboard offer (async)");
            }
        });
    } else {
        let mut file = std::fs::File::from(fd);
        if let Err(err) = file.write_all(&data) {
            tracing::debug!(?err, "failed to write compositor clipboard offer");
        }
    }
}

/// Serve a compositor-owned selection if possible.
/// Returns `Ok(())` when handled; returns the fd back when not applicable.
pub fn serve_compositor_selection(
    user_data: &MetisSelectionUserData,
    request_mime: &str,
    fd: std::os::unix::io::OwnedFd,
) -> Result<(), std::os::unix::io::OwnedFd> {
    let Some((offer_mime, data)) = user_data.resolve_payload() else {
        return Err(fd);
    };
    if !selection_mime_satisfies(&offer_mime, request_mime) {
        return Err(fd);
    }
    write_selection_to_fd(fd, data);
    Ok(())
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
        if current_data_device_selection_userdata(&self.seat).is_some() {
            tracing::debug!("skip clipboard capture: compositor owns selection");
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

        let mut budget = DRAIN_CLIPBOARD_BYTES_PER_TICK;
        self.pending_clipboard_reads.retain_mut(|pending| {
            let mut chunk = [0u8; 65_536];
            loop {
                if budget == 0 {
                    return true;
                }
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
                        budget = budget.saturating_sub(n);
                        pending.data.extend_from_slice(&chunk[..n]);
                        if budget == 0 {
                            return true;
                        }
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
        let (user_data, mime_types) = if let Some(t) = text {
            let data = t.into_bytes();
            if data.is_empty() {
                return Err("clipboard data is empty".into());
            }
            if data.len() > MAX_CLIPBOARD_BYTES {
                return Err("clipboard payload exceeds size cap".into());
            }
            let offer = CompositorClipboardOffer {
                mime: mime.clone(),
                data,
            };
            (
                MetisSelectionUserData {
                    offer: Some(offer),
                    lazy_file: None,
                },
                recall_mime_types(&mime, None),
            )
        } else if let Some(path) = image_path {
            if !std::path::Path::new(&path).is_file() {
                return Err(format!("image not found: {path}"));
            }
            (
                MetisSelectionUserData {
                    offer: None,
                    lazy_file: Some((path.clone(), mime.clone())),
                },
                recall_mime_types(&mime, Some(&path)),
            )
        } else {
            return Err("SetClipboard requires text or image_path".into());
        };

        self.install_compositor_selection(mime_types, user_data);
        Ok(())
    }

    fn install_compositor_selection(
        &mut self,
        mime_types: Vec<String>,
        user_data: MetisSelectionUserData,
    ) {
        self.clipboard_capture_suppressed += 1;
        set_data_device_selection(
            &self.display_handle,
            &self.seat,
            mime_types.clone(),
            user_data.clone(),
        );
        set_primary_selection(
            &self.display_handle,
            &self.seat,
            mime_types.clone(),
            user_data,
        );
        self.clipboard_capture_suppressed -= 1;

        // Electron / XWayland apps (e.g. Cursor) read the X11 clipboard, not wl_data_device.
        if let Some(xwm) = self.xwm.as_mut() {
            let mirror = Some(mime_types.clone());
            for target in [SelectionTarget::Clipboard, SelectionTarget::Primary] {
                if let Err(err) = xwm.new_selection(target, mirror.clone()) {
                    tracing::debug!(?err, ?target, "mirror compositor clipboard to XWayland");
                }
            }
        }
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
    } else if mime.starts_with("image/") || normalize_mime(mime).starts_with("image/") {
        let ext = image_file_extension(mime);
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

fn image_file_extension(mime: &str) -> &'static str {
    let mime = normalize_mime(mime);
    if mime.contains("png") {
        "png"
    } else if mime.contains("jpeg") || mime.contains("jpg") {
        "jpg"
    } else if mime.contains("webp") {
        "webp"
    } else {
        "bmp"
    }
}

fn clipboard_image_dir() -> std::path::PathBuf {
    metis_protocol::runtime_dir().join("clipboard")
}
