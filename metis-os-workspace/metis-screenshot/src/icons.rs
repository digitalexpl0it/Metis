//! Vector glyphs drawn with Cairo.
//!
//! The editor ships its own icon set instead of symbolic icon names so the
//! toolbar looks identical on hosts whose icon theme lacks the drawing/annotation
//! icons (most themes do), and so every glyph shares one stroke weight.

use gtk::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    Pen,
    Highlighter,
    Arrow,
    Rect,
    Ellipse,
    Text,
    Pixelate,
    Crop,
    Ocr,
    Undo,
    Redo,
    Trash,
    Copy,
    Save,
    SaveAs,
    Pin,
    Check,
}

pub fn image(glyph: Glyph, size: i32) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_width(size);
    area.set_content_height(size);
    area.set_can_target(false);
    area.set_valign(gtk::Align::Center);
    area.set_halign(gtk::Align::Center);
    area.set_draw_func(move |widget, cr, width, height| {
        let color = widget.color();
        cr.set_source_rgba(
            color.red() as f64,
            color.green() as f64,
            color.blue() as f64,
            color.alpha() as f64,
        );
        cr.set_line_width(1.6);
        cr.set_line_cap(gtk::cairo::LineCap::Round);
        cr.set_line_join(gtk::cairo::LineJoin::Round);
        // Every glyph is authored on a 24x24 grid, then scaled to the request.
        let scale = (width.min(height) as f64) / 24.0;
        cr.save().ok();
        cr.translate(
            (width as f64 - 24.0 * scale) / 2.0,
            (height as f64 - 24.0 * scale) / 2.0,
        );
        cr.scale(scale, scale);
        // Keep one device-pixel-consistent stroke weight at any icon size.
        cr.set_line_width(1.7 / scale.max(0.01));
        draw(cr, glyph);
        cr.restore().ok();
    });
    area
}

fn draw(cr: &gtk::cairo::Context, glyph: Glyph) {
    match glyph {
        Glyph::Pen => {
            cr.move_to(4.5, 19.5);
            cr.line_to(5.5, 15.5);
            cr.line_to(16.0, 5.0);
            cr.line_to(19.0, 8.0);
            cr.line_to(8.5, 18.5);
            cr.close_path();
            let _ = cr.stroke();
            cr.move_to(14.0, 7.0);
            cr.line_to(17.0, 10.0);
            let _ = cr.stroke();
        }
        Glyph::Highlighter => {
            let weight = cr.line_width();
            cr.set_line_width(weight * 3.0);
            cr.move_to(6.0, 15.0);
            cr.line_to(16.5, 6.0);
            let _ = cr.stroke();
            cr.set_line_width(weight);
            cr.rectangle(4.0, 18.0, 16.0, 2.5);
            let _ = cr.fill();
        }
        Glyph::Arrow => {
            cr.move_to(5.0, 19.0);
            cr.line_to(18.5, 5.5);
            let _ = cr.stroke();
            cr.move_to(11.5, 5.5);
            cr.line_to(18.5, 5.5);
            cr.line_to(18.5, 12.5);
            let _ = cr.stroke();
        }
        Glyph::Rect => {
            rounded_rect(cr, 4.0, 6.0, 16.0, 12.0, 2.0);
            let _ = cr.stroke();
        }
        Glyph::Ellipse => {
            cr.save().ok();
            cr.translate(12.0, 12.0);
            cr.scale(1.0, 0.78);
            cr.arc(0.0, 0.0, 8.0, 0.0, std::f64::consts::TAU);
            cr.restore().ok();
            let _ = cr.stroke();
        }
        Glyph::Text => {
            cr.move_to(5.0, 7.0);
            cr.line_to(5.0, 5.0);
            cr.line_to(19.0, 5.0);
            cr.line_to(19.0, 7.0);
            let _ = cr.stroke();
            cr.move_to(12.0, 5.0);
            cr.line_to(12.0, 19.0);
            let _ = cr.stroke();
            cr.move_to(9.0, 19.0);
            cr.line_to(15.0, 19.0);
            let _ = cr.stroke();
        }
        Glyph::Pixelate => {
            rounded_rect(cr, 4.0, 4.0, 16.0, 16.0, 2.5);
            let _ = cr.stroke();
            cr.rectangle(6.5, 6.5, 5.0, 5.0);
            let _ = cr.fill();
            cr.rectangle(12.5, 12.5, 5.0, 5.0);
            let _ = cr.fill();
        }
        Glyph::Crop => {
            cr.move_to(7.0, 3.0);
            cr.line_to(7.0, 17.0);
            cr.line_to(21.0, 17.0);
            let _ = cr.stroke();
            cr.move_to(3.0, 7.0);
            cr.line_to(17.0, 7.0);
            cr.line_to(17.0, 21.0);
            let _ = cr.stroke();
        }
        Glyph::Ocr => {
            for (dx, dy, sx, sy) in [
                (4.0, 4.0, 1.0, 1.0),
                (20.0, 4.0, -1.0, 1.0),
                (4.0, 20.0, 1.0, -1.0),
                (20.0, 20.0, -1.0, -1.0),
            ] {
                cr.move_to(dx, dy + 4.0 * sy);
                cr.line_to(dx, dy);
                cr.line_to(dx + 4.0 * sx, dy);
                let _ = cr.stroke();
            }
            cr.move_to(8.0, 15.0);
            cr.line_to(12.0, 8.5);
            cr.line_to(16.0, 15.0);
            let _ = cr.stroke();
            cr.move_to(9.6, 12.5);
            cr.line_to(14.4, 12.5);
            let _ = cr.stroke();
        }
        Glyph::Undo | Glyph::Redo => {
            cr.save().ok();
            if glyph == Glyph::Redo {
                cr.translate(24.0, 0.0);
                cr.scale(-1.0, 1.0);
            }
            cr.move_to(4.5, 9.0);
            cr.curve_to(15.0, 9.0, 20.0, 11.5, 19.0, 19.5);
            let _ = cr.stroke();
            cr.move_to(10.0, 4.0);
            cr.line_to(4.5, 9.0);
            cr.line_to(10.0, 14.0);
            let _ = cr.stroke();
            cr.restore().ok();
        }
        Glyph::Trash => {
            cr.move_to(4.5, 6.5);
            cr.line_to(19.5, 6.5);
            let _ = cr.stroke();
            cr.move_to(9.5, 6.5);
            cr.line_to(9.5, 4.5);
            cr.line_to(14.5, 4.5);
            cr.line_to(14.5, 6.5);
            let _ = cr.stroke();
            rounded_rect(cr, 6.5, 6.5, 11.0, 13.0, 1.5);
            let _ = cr.stroke();
            for x in [10.0, 14.0] {
                cr.move_to(x, 9.5);
                cr.line_to(x, 16.5);
                let _ = cr.stroke();
            }
        }
        Glyph::Copy => {
            rounded_rect(cr, 8.5, 8.5, 11.0, 11.0, 2.0);
            let _ = cr.stroke();
            cr.move_to(15.5, 5.5);
            cr.line_to(6.5, 5.5);
            cr.line_to(6.5, 14.5);
            let _ = cr.stroke();
        }
        Glyph::Save => {
            rounded_rect(cr, 4.5, 4.5, 15.0, 15.0, 2.0);
            let _ = cr.stroke();
            cr.rectangle(8.5, 4.5, 7.0, 5.0);
            let _ = cr.stroke();
            cr.rectangle(8.0, 13.0, 8.0, 6.5);
            let _ = cr.stroke();
        }
        Glyph::SaveAs => {
            rounded_rect(cr, 4.5, 4.5, 12.0, 12.0, 2.0);
            let _ = cr.stroke();
            cr.rectangle(7.5, 4.5, 6.0, 4.0);
            let _ = cr.stroke();
            cr.move_to(14.0, 19.5);
            cr.line_to(20.5, 13.0);
            let _ = cr.stroke();
            cr.move_to(16.5, 19.5);
            cr.line_to(20.5, 19.5);
            cr.line_to(20.5, 15.5);
            let _ = cr.stroke();
        }
        Glyph::Pin => {
            cr.move_to(12.0, 20.0);
            cr.line_to(12.0, 13.5);
            let _ = cr.stroke();
            cr.move_to(8.0, 4.5);
            cr.line_to(16.0, 4.5);
            cr.line_to(14.5, 9.0);
            cr.line_to(16.5, 13.5);
            cr.line_to(7.5, 13.5);
            cr.line_to(9.5, 9.0);
            cr.close_path();
            let _ = cr.stroke();
        }
        Glyph::Check => {
            cr.move_to(5.0, 12.5);
            cr.line_to(10.0, 17.5);
            cr.line_to(19.0, 6.5);
            let _ = cr.stroke();
        }
    }
}

fn rounded_rect(cr: &gtk::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let half = std::f64::consts::FRAC_PI_2;
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -half, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, half);
    cr.arc(x + r, y + h - r, r, half, std::f64::consts::PI);
    cr.arc(
        x + r,
        y + r,
        r,
        std::f64::consts::PI,
        1.5 * std::f64::consts::PI,
    );
    cr.close_path();
}
