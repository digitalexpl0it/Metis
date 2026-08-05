//! Live window thumbnails via one output capture + per-window crops.
//!
//! Capture runs **before** Exclusive overlays map so the dimmer is not in the
//! frame. Failures fall back to app icons.
//!
//! Full-output (near-maximized) crops are skipped: a single screenshot cannot
//! show buried windows — every full-screen crop would just be the focused app.

use std::collections::HashMap;

use gtk::gdk;
use gtk::prelude::*;
use metis_capture::{capture_rgba, crop_rgba, CaptureOptions};
use metis_protocol::{OutputInfo, PixelRect, WindowInfo};

/// Skip live thumbs that cover this fraction of the output (look identical).
const FULLSCREEN_COVERAGE: f64 = 0.72;

#[derive(Clone)]
pub struct ThumbSet {
    /// window id → RGBA texture
    pub textures: HashMap<u32, gdk::MemoryTexture>,
}

/// Capture the given output and crop each window's global rect into a texture.
/// Returns `None` when capture fails or no crops succeed.
pub fn capture_window_thumbs(
    output_name: Option<&str>,
    windows: &[WindowInfo],
) -> Option<ThumbSet> {
    let outputs = list_outputs();
    let (connector, origin, out_size) = resolve_output(&outputs, output_name)?;
    let options = CaptureOptions {
        draw_cursor: false,
        output_index: 0,
        connector: Some(connector),
    };
    let (frame_w, frame_h, rgba) = match capture_rgba(options, None) {
        Ok(v) => v,
        Err(err) => {
            tracing::debug!(%err, "window thumb capture failed");
            return None;
        }
    };

    // Logical layout coords → buffer pixels (handles fractional/integer scale).
    let sx = frame_w as f64 / (out_size.0.max(1) as f64);
    let sy = frame_h as f64 / (out_size.1.max(1) as f64);
    let out_area = (out_size.0.max(1) as f64) * (out_size.1.max(1) as f64);

    let mut textures = HashMap::new();
    for w in windows {
        if w.rect.width < 8 || w.rect.height < 8 {
            continue;
        }
        let coverage = (w.rect.width as f64 * w.rect.height as f64) / out_area;
        if coverage >= FULLSCREEN_COVERAGE {
            // Maximized / near-fullscreen: screenshot only shows the topmost
            // client in that region, so every card would look like the focused app.
            continue;
        }

        let local_x = w.rect.x - origin.0;
        let local_y = w.rect.y - origin.1;
        let crop = PixelRect {
            x: (local_x as f64 * sx).round() as i32,
            y: (local_y as f64 * sy).round() as i32,
            width: (w.rect.width as f64 * sx).round() as i32,
            height: (w.rect.height as f64 * sy).round() as i32,
        };
        let crop = PixelRect {
            x: crop.x.clamp(0, frame_w.saturating_sub(1) as i32),
            y: crop.y.clamp(0, frame_h.saturating_sub(1) as i32),
            width: crop.width.min(frame_w as i32 - crop.x.max(0)).max(1),
            height: crop.height.min(frame_h as i32 - crop.y.max(0)).max(1),
        };
        let Ok(cropped) = crop_rgba(&rgba, frame_w, frame_h, crop) else {
            continue;
        };
        let cw = crop.width as u32;
        let ch = crop.height as u32;
        let stride = (cw * 4) as usize;
        if cropped.len() < stride * ch as usize {
            continue;
        }
        // Downscale huge crops for GTK cards (keeps memory/UI snappy).
        let (cw, ch, cropped) = downscale_rgba(cropped, cw, ch, 320);
        let bytes = glib::Bytes::from_owned(cropped);
        let texture = gdk::MemoryTexture::new(
            cw as i32,
            ch as i32,
            gdk::MemoryFormat::R8g8b8a8,
            &bytes,
            (cw * 4) as usize,
        );
        textures.insert(w.id, texture);
    }
    if textures.is_empty() {
        None
    } else {
        Some(ThumbSet { textures })
    }
}

fn downscale_rgba(rgba: Vec<u8>, width: u32, height: u32, max_edge: u32) -> (u32, u32, Vec<u8>) {
    let max_dim = width.max(height);
    if max_dim <= max_edge || width == 0 || height == 0 {
        return (width, height, rgba);
    }
    let scale = max_edge as f64 / max_dim as f64;
    let nw = ((width as f64) * scale).round().max(1.0) as u32;
    let nh = ((height as f64) * scale).round().max(1.0) as u32;
    let mut out = vec![0u8; (nw * nh * 4) as usize];
    for y in 0..nh {
        let sy = ((y as f64 + 0.5) / scale) as u32;
        let sy = sy.min(height - 1);
        for x in 0..nw {
            let sx = ((x as f64 + 0.5) / scale) as u32;
            let sx = sx.min(width - 1);
            let si = ((sy * width + sx) * 4) as usize;
            let di = ((y * nw + x) * 4) as usize;
            out[di..di + 4].copy_from_slice(&rgba[si..si + 4]);
        }
    }
    (nw, nh, out)
}

fn list_outputs() -> Vec<OutputInfo> {
    match metis_protocol::send_compositor_command(&metis_protocol::CompositorCommand::ListOutputs) {
        Ok(metis_protocol::CompositorEvent::OutputList { outputs }) => outputs,
        _ => Vec::new(),
    }
}

fn resolve_output(
    outputs: &[OutputInfo],
    name: Option<&str>,
) -> Option<(String, (i32, i32), (i32, i32))> {
    let out = if let Some(n) = name {
        outputs
            .iter()
            .find(|o| o.name.eq_ignore_ascii_case(n))
            .or_else(|| outputs.iter().find(|o| o.primary))
            .or_else(|| outputs.first())
    } else {
        outputs
            .iter()
            .find(|o| o.primary)
            .or_else(|| outputs.first())
    }?;
    Some((
        out.name.clone(),
        (out.rect.x, out.rect.y),
        (out.rect.width, out.rect.height),
    ))
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
