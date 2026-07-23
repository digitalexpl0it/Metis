//! Shaped / bidi-aware text layout for compositor-drawn UI (lock screen, SSD).
//! Uses `unicode-bidi` for visual order and `rustybuzz` for complex-script shaping
//! probes; glyph bitmaps still come from `fontdue` over the visual character stream
//! (fontdue glyph ids are not interchangeable with TrueType glyph ids).

use fontdue::Font;
use rustybuzz::{Face, UnicodeBuffer};
use unicode_bidi::BidiInfo;

/// Reorder `text` into visual order (LTR/RTL).
pub fn visual_order(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let bidi = BidiInfo::new(text, None);
    let Some(para) = bidi.paragraphs.first() else {
        return text.to_string();
    };
    let (levels, runs) = bidi.visual_runs(para, para.range.clone());
    let mut visual = String::with_capacity(text.len());
    for run in runs {
        let slice = &text[run.clone()];
        if levels[run.start].is_rtl() {
            visual.extend(slice.chars().rev());
        } else {
            visual.push_str(slice);
        }
    }
    visual
}

/// Run rustybuzz shaping so complex scripts exercise HarfBuzz; returns glyph count.
pub fn shape_probe(font_data: &[u8], text: &str) -> usize {
    let Some(face) = Face::from_slice(font_data, 0) else {
        return text.chars().count();
    };
    let visual = visual_order(text);
    let mut buffer = UnicodeBuffer::new();
    buffer.push_str(&visual);
    buffer.guess_segment_properties();
    let glyphs = rustybuzz::shape(&face, &[], buffer);
    glyphs.len()
}

/// Rasterize `text` with bidi visual order via fontdue.
pub fn rasterize_text_bidi(
    font: &Font,
    font_data: &[u8],
    text: &str,
    font_px: f32,
    color: [f32; 4],
) -> Option<(Vec<u8>, i32, i32)> {
    if text.is_empty() {
        return None;
    }
    let _ = shape_probe(font_data, text);
    let visual = visual_order(text);

    let pad = (font_px * 0.3).ceil() as i32;
    let mut pen = 0f32;
    let mut placements: Vec<(fontdue::Metrics, Vec<u8>, i32)> = Vec::new();
    for ch in visual.chars().take(256) {
        let (metrics, bitmap) = font.rasterize(ch, font_px);
        placements.push((metrics, bitmap, pen.round() as i32));
        pen += metrics.advance_width;
    }
    let text_w = pen.ceil() as i32;
    let width = (text_w + 2 * pad).clamp(1, 8192);
    let height = ((font_px * 1.5).ceil() as i32 + 2 * pad).clamp(1, 512);
    let baseline = pad + font_px.round() as i32;

    let mut pixels = vec![0u8; (width * height * 4) as usize];
    let (cr, cg, cb, ca) = (color[0], color[1], color[2], color[3]);
    for (metrics, bitmap, pen_x) in &placements {
        let gx = pad + pen_x + metrics.xmin;
        let gy = baseline - metrics.ymin - metrics.height as i32;
        for row in 0..metrics.height as i32 {
            let py = gy + row;
            if py < 0 || py >= height {
                continue;
            }
            for col in 0..metrics.width as i32 {
                let px = gx + col;
                if px < 0 || px >= width {
                    continue;
                }
                let cov = bitmap[(row * metrics.width as i32 + col) as usize] as f32 / 255.0;
                let a = cov * ca;
                let idx = ((py * width + px) * 4) as usize;
                let inv = 1.0 - a;
                let dr = pixels[idx] as f32 / 255.0;
                let dg = pixels[idx + 1] as f32 / 255.0;
                let db = pixels[idx + 2] as f32 / 255.0;
                let da = pixels[idx + 3] as f32 / 255.0;
                pixels[idx] = (((cr * a) + dr * inv).clamp(0.0, 1.0) * 255.0) as u8;
                pixels[idx + 1] = (((cg * a) + dg * inv).clamp(0.0, 1.0) * 255.0) as u8;
                pixels[idx + 2] = (((cb * a) + db * inv).clamp(0.0, 1.0) * 255.0) as u8;
                pixels[idx + 3] = ((a + da * inv).clamp(0.0, 1.0) * 255.0) as u8;
            }
        }
    }
    Some((pixels, width, height))
}
