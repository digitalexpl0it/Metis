//! Frame conversion, crop, and PNG encoding.

use std::path::Path;

use metis_grid::PixelRect;
use wayland_client::protocol::wl_shm::Format;

use crate::wayland::{CaptureOptions, Frame};

pub fn frame_to_rgba(frame: &Frame) -> Vec<u8> {
    match frame.shm_format {
        Format::Abgr8888 | Format::Xbgr8888 => {
            abgr_to_rgba(&frame.data, frame.width, frame.height, frame.stride)
        }
        _ => bgra_to_rgba(&frame.data, frame.width, frame.height, frame.stride),
    }
}

pub fn crop_rgba(
    rgba: &[u8],
    frame_width: u32,
    frame_height: u32,
    crop: PixelRect,
) -> Result<Vec<u8>, String> {
    let x = crop.x.max(0) as u32;
    let y = crop.y.max(0) as u32;
    if crop.width <= 0 || crop.height <= 0 {
        return Err("empty crop rect".into());
    }
    let w = (crop.width as u32).min(frame_width.saturating_sub(x));
    let h = (crop.height as u32).min(frame_height.saturating_sub(y));
    if w == 0 || h == 0 {
        return Err("crop rect outside frame".into());
    }

    let mut out = vec![0u8; (w * h * 4) as usize];
    for row in 0..h {
        let src_row = ((y + row) * frame_width * 4) as usize;
        let dst_row = (row * w * 4) as usize;
        for col in 0..w {
            let si = src_row + ((x + col) * 4) as usize;
            let di = dst_row + (col * 4) as usize;
            if si + 3 >= rgba.len() || di + 3 >= out.len() {
                continue;
            }
            out[di..di + 4].copy_from_slice(&rgba[si..si + 4]);
        }
    }
    Ok(out)
}

pub fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|err| format!("create png: {err}"))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|err| format!("png header: {err}"))?;
    writer
        .write_image_data(rgba)
        .map_err(|err| format!("png write: {err}"))?;
    writer
        .finish()
        .map_err(|err| format!("png finish: {err}"))?;
    Ok(())
}

/// Capture one output and optionally crop to `crop` in output-local coordinates.
pub fn capture_png(
    options: CaptureOptions,
    crop: Option<PixelRect>,
    path: &Path,
) -> Result<(u32, u32), String> {
    let frame = crate::wayland::capture_output_frame(options)?;
    let rgba = frame_to_rgba(&frame);
    let (out_rgba, width, height) = if let Some(rect) = crop {
        let cropped = crop_rgba(&rgba, frame.width, frame.height, rect)?;
        let w = rect.width.max(0) as u32;
        let h = rect.height.max(0) as u32;
        (cropped, w, h)
    } else {
        (rgba, frame.width, frame.height)
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| format!("create dir: {err}"))?;
    }
    write_png(path, width, height, &out_rgba)?;
    Ok((width, height))
}

fn bgra_to_rgba(data: &[u8], width: u32, height: u32, stride: u32) -> Vec<u8> {
    let mut out = vec![0u8; (width * height * 4) as usize];
    for y in 0..height {
        let src_row = (y * stride) as usize;
        let dst_row = (y * width * 4) as usize;
        for x in 0..width {
            let si = src_row + (x * 4) as usize;
            let di = dst_row + (x * 4) as usize;
            if si + 3 >= data.len() || di + 3 >= out.len() {
                continue;
            }
            out[di] = data[si + 2];
            out[di + 1] = data[si + 1];
            out[di + 2] = data[si];
            out[di + 3] = 255;
        }
    }
    out
}

fn abgr_to_rgba(data: &[u8], width: u32, height: u32, stride: u32) -> Vec<u8> {
    let mut out = vec![0u8; (width * height * 4) as usize];
    for y in 0..height {
        let src_row = (y * stride) as usize;
        let dst_row = (y * width * 4) as usize;
        for x in 0..width {
            let si = src_row + (x * 4) as usize;
            let di = dst_row + (x * 4) as usize;
            if si + 3 >= data.len() || di + 3 >= out.len() {
                continue;
            }
            out[di] = data[si];
            out[di + 1] = data[si + 1];
            out[di + 2] = data[si + 2];
            out[di + 3] = 255;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crop_extracts_subregion() {
        let mut rgba = vec![0u8; 16 * 4];
        rgba[20] = 255;
        rgba[21] = 128;
        rgba[22] = 64;
        rgba[23] = 255;
        let cropped = crop_rgba(
            &rgba,
            4,
            4,
            PixelRect {
                x: 1,
                y: 1,
                width: 2,
                height: 2,
            },
        )
        .expect("crop");
        assert_eq!(cropped.len(), 2 * 2 * 4);
        assert_eq!(cropped[0], 255);
    }
}
