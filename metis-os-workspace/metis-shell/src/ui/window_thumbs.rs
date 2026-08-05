//! Per-window thumbnails for Alt+Tab.
//!
//! Prefers compositor-rendered PNGs (`CaptureWindowThumbs`) so each card shows
//! that window's own buffers — not a misleading crop of whatever is on screen.
//! Falls back to app icons when a thumb file is missing.

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use gtk::gdk;
use gtk::prelude::*;
use metis_protocol::{CompositorCommand, CompositorEvent, WindowInfo};

#[derive(Clone)]
pub struct ThumbSet {
    pub textures: HashMap<u32, gdk::Texture>,
}

/// Ask the compositor to refresh thumbs, wait briefly for PNGs, load them.
pub fn load_window_thumbs(windows: &[WindowInfo]) -> Option<ThumbSet> {
    let ids: Vec<u32> = windows.iter().map(|w| w.id).collect();
    if ids.is_empty() {
        return None;
    }

    // Queue a GL capture for every id. Reply lists paths that already exist
    // (from prior focus); newly queued ones appear after the next compositor frame.
    let _ = metis_protocol::send_compositor_command(&CompositorCommand::CaptureWindowThumbs {
        ids: ids.clone(),
    });

    let deadline = Instant::now() + Duration::from_millis(400);
    while Instant::now() < deadline {
        let ready = ids.iter().filter(|id| thumb_path(**id).is_file()).count();
        if ready == ids.len() {
            break;
        }
        // Separate compositor process can render while we wait; keep this short
        // so Alt+Tab still feels snappy when some thumbs are already cached.
        std::thread::sleep(Duration::from_millis(20));
    }

    // One more nudge in case the first frame missed the queue (idle compositor).
    if ids.iter().any(|id| !thumb_path(*id).is_file()) {
        if let Ok(CompositorEvent::WindowThumbs { .. }) =
            metis_protocol::send_compositor_command(&CompositorCommand::CaptureWindowThumbs {
                ids: ids.clone(),
            })
        {
            let extra = Instant::now() + Duration::from_millis(200);
            while Instant::now() < extra {
                if ids.iter().all(|id| thumb_path(*id).is_file()) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }

    let mut textures = HashMap::new();
    for id in ids {
        let path = thumb_path(id);
        if let Some(tex) = load_png_texture(&path) {
            textures.insert(id, tex);
        }
    }
    if textures.is_empty() {
        None
    } else {
        Some(ThumbSet { textures })
    }
}

pub fn thumb_path(id: u32) -> std::path::PathBuf {
    metis_protocol::runtime_dir()
        .join("thumbs")
        .join(format!("{id}.png"))
}

fn load_png_texture(path: &Path) -> Option<gdk::Texture> {
    if !path.is_file() {
        return None;
    }
    let file = gio::File::for_path(path);
    gdk::Texture::from_file(&file).ok()
}

/// Prefer a live thumb Picture; otherwise an app-icon Image.
pub fn thumb_or_icon_widget(
    thumbs: Option<&ThumbSet>,
    w: &WindowInfo,
    icon_px: i32,
) -> gtk::Widget {
    if let Some(tex) = thumbs.and_then(|t| t.textures.get(&w.id)) {
        let pic = gtk::Picture::for_paintable(tex);
        pic.set_content_fit(gtk::ContentFit::Contain);
        pic.set_can_shrink(true);
        pic.add_css_class("metis-window-thumb");
        return pic.upcast();
    }
    let img = gtk::Image::from_gicon(&crate::services::applications::resolve_icon_for_app_id(
        w.app_id.as_deref(),
    ));
    img.set_pixel_size(icon_px);
    img.upcast()
}
