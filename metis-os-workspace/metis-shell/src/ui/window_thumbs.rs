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
use metis_protocol::{CompositorCommand, WindowInfo};

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

    let _ = metis_protocol::send_compositor_command(&CompositorCommand::CaptureWindowThumbs {
        ids: ids.clone(),
    });

    // Compositor renders on the next GL frame; wait for files (cached thumbs
    // from prior focus are often already present).
    let deadline = Instant::now() + Duration::from_millis(280);
    while Instant::now() < deadline {
        let ready = ids
            .iter()
            .filter(|id| crate::ui::window_thumbs::thumb_path(**id).is_file())
            .count();
        if ready == ids.len() {
            break;
        }
        // Yield so the compositor IPC/render tick can run in nested sessions;
        // on DRM the shell is a separate process so this is just a short wait.
        std::thread::sleep(Duration::from_millis(16));
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
    metis_protocol::runtime_dir().join("thumbs").join(format!("{id}.png"))
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
