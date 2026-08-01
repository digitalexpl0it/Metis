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
pub fn capture_rgba(
    options: CaptureOptions,
    crop: Option<PixelRect>,
) -> Result<(u32, u32, Vec<u8>), String> {
    let frame = crate::wayland::capture_output_frame(options)?;
    let rgba = frame_to_rgba(&frame);
    if let Some(rect) = crop {
        let cropped = crop_rgba(&rgba, frame.width, frame.height, rect)?;
        let w = rect.width.max(0) as u32;
        let h = rect.height.max(0) as u32;
        Ok((w, h, cropped))
    } else {
        Ok((frame.width, frame.height, rgba))
    }
}

/// Capture one output and optionally crop to `crop` in output-local coordinates.
pub fn capture_png(
    options: CaptureOptions,
    crop: Option<PixelRect>,
    path: &Path,
) -> Result<(u32, u32), String> {
    let (width, height, out_rgba) = capture_rgba(options, crop)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| format!("create dir: {err}"))?;
    }
    write_png(path, width, height, &out_rgba)?;
    Ok((width, height))
}

/// Vertically stitch scroll-capture frames. Finds the best overlap between the
/// bottom of `acc` and the top of `next`, then appends the novel rows.
pub fn stitch_vertical_append(
    acc_w: u32,
    acc_h: u32,
    acc: &[u8],
    next_w: u32,
    next_h: u32,
    next: &[u8],
) -> Result<(u32, u32, Vec<u8>), String> {
    if acc_w == 0 || next_w == 0 || acc_h == 0 || next_h == 0 {
        return Err("empty stitch frame".into());
    }
    if acc_w != next_w {
        return Err("scroll frames must share width".into());
    }
    let max_overlap = (acc_h.min(next_h).saturating_sub(1)).min(next_h * 4 / 5).max(1);
    let mut best_overlap = 0u32;
    let mut best_score = i64::MAX;
    for overlap in 8..=max_overlap {
        let score = row_diff_score(
            acc,
            acc_w,
            acc_h.saturating_sub(overlap),
            next,
            next_w,
            0,
            overlap,
        );
        if score < best_score {
            best_score = score;
            best_overlap = overlap;
        }
    }
    let novel = next_h.saturating_sub(best_overlap);
    if novel == 0 {
        return Ok((acc_w, acc_h, acc.to_vec()));
    }
    let new_h = acc_h + novel;
    let mut out = vec![0u8; (acc_w * new_h * 4) as usize];
    out[..acc.len()].copy_from_slice(acc);
    let src_off = (best_overlap * next_w * 4) as usize;
    let dst_off = (acc_h * acc_w * 4) as usize;
    let bytes = (novel * next_w * 4) as usize;
    out[dst_off..dst_off + bytes].copy_from_slice(&next[src_off..src_off + bytes]);
    Ok((acc_w, new_h, out))
}

fn row_diff_score(
    a: &[u8],
    aw: u32,
    a_row: u32,
    b: &[u8],
    bw: u32,
    b_row: u32,
    rows: u32,
) -> i64 {
    let mut score = 0i64;
    let step = 4u32; // sample every 4th pixel for speed
    for r in 0..rows {
        let ar = ((a_row + r) * aw * 4) as usize;
        let br = ((b_row + r) * bw * 4) as usize;
        let mut x = 0u32;
        while x < aw {
            let ai = ar + (x * 4) as usize;
            let bi = br + (x * 4) as usize;
            if ai + 2 < a.len() && bi + 2 < b.len() {
                score += (a[ai] as i64 - b[bi] as i64).abs();
                score += (a[ai + 1] as i64 - b[bi + 1] as i64).abs();
                score += (a[ai + 2] as i64 - b[bi + 2] as i64).abs();
            }
            x += step;
        }
    }
    score
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
