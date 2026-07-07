//! Theme-aware mini charts for the system dashboard.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;

use metis_config::ThemeTokens;

const LABEL_W: f64 = 34.0;
const PLOT_PAD_TOP: f64 = 4.0;
const PLOT_PAD_BOTTOM: f64 = 4.0;
const PLOT_PAD_RIGHT: f64 = 4.0;
const SMOOTH_TENSION: f64 = 0.22;

#[derive(Clone, Copy)]
enum YAxis {
    Percent,
    Rate(f64),
}

struct PlotRect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

impl PlotRect {
    fn from_size(width: i32, height: i32) -> Self {
        let w = width as f64;
        let h = height as f64;
        Self {
            x: LABEL_W,
            y: PLOT_PAD_TOP,
            w: (w - LABEL_W - PLOT_PAD_RIGHT).max(1.0),
            h: (h - PLOT_PAD_TOP - PLOT_PAD_BOTTOM).max(1.0),
        }
    }

    fn value_y(&self, val: f64, vmax: f64) -> f64 {
        let t = (val / vmax).clamp(0.0, 1.0);
        self.y + self.h - t * self.h
    }
}

/// History sparkline (0–100 scale) with optional filled area under the curve.
pub fn wire_percent_chart(area: &gtk::DrawingArea, history: Rc<RefCell<Vec<f32>>>, fill: bool) {
    let area = area.clone();
    area.set_draw_func(move |_, cr, width, height| {
        let history = history.borrow();
        if history.is_empty() {
            return;
        }
        let tokens = crate::ui::theme::active_tokens();
        let plot = PlotRect::from_size(width, height);
        draw_chart_axes(cr, &plot, &tokens, YAxis::Percent);
        let (accent, accent2) = accent_pair(&tokens);
        draw_series(
            cr,
            &plot,
            &history,
            100.0,
            accent,
            accent2,
            fill,
            2.2,
            fill,
        );
    });
}

/// Per-core CPU lines with aggregate gradient fill/stroke on top.
pub fn wire_multi_core_chart(
    area: &gtk::DrawingArea,
    cores: Rc<RefCell<Vec<Vec<f32>>>>,
    aggregate: Rc<RefCell<Vec<f32>>>,
) {
    let area = area.clone();
    area.set_draw_func(move |_, cr, width, height| {
        let cores = cores.borrow();
        let aggregate = aggregate.borrow();
        if cores.is_empty() && aggregate.is_empty() {
            return;
        }
        let tokens = crate::ui::theme::active_tokens();
        let plot = PlotRect::from_size(width, height);
        draw_chart_axes(cr, &plot, &tokens, YAxis::Percent);

        let (accent, accent2) = accent_pair(&tokens);
        if !aggregate.is_empty() {
            draw_series_fill(
                cr,
                &plot,
                &aggregate,
                100.0,
                accent,
                accent2,
                true,
            );
        }

        let n = cores.len().max(1);
        for (i, hist) in cores.iter().enumerate() {
            if hist.is_empty() {
                continue;
            }
            let color = core_color(i, n, &tokens);
            let fade = core_color_fade(i, n, &tokens);
            draw_series_stroke(cr, &plot, hist, 100.0, color, fade, true, 1.5);
        }

        if !aggregate.is_empty() {
            draw_series_stroke(cr, &plot, &aggregate, 100.0, accent, accent2, true, 2.2);
        }
    });
}

/// RAM % history with optional swap % overlay line.
pub fn wire_memory_chart(
    area: &gtk::DrawingArea,
    mem: Rc<RefCell<Vec<f32>>>,
    swap: Rc<RefCell<Vec<f32>>>,
    has_swap: Rc<Cell<bool>>,
) {
    let area = area.clone();
    area.set_draw_func(move |_, cr, width, height| {
        let mem = mem.borrow();
        let swap = swap.borrow();
        if mem.is_empty() {
            return;
        }
        let tokens = crate::ui::theme::active_tokens();
        let plot = PlotRect::from_size(width, height);
        draw_chart_axes(cr, &plot, &tokens, YAxis::Percent);
        let (accent, accent2) = accent_pair(&tokens);
        draw_series(cr, &plot, &mem, 100.0, accent, accent2, true, 2.2, true);
        if has_swap.get() && !swap.is_empty() {
            let warn = parse_hex(&tokens.semantic.warning);
            draw_series(cr, &plot, &swap, 100.0, warn, warn, true, 1.6, true);
        }
    });
}

/// RX + TX throughput on one chart (shared scale, gradient fills under both curves).
pub fn wire_dual_rate_chart(
    area: &gtk::DrawingArea,
    rx: Rc<RefCell<Vec<f64>>>,
    tx: Rc<RefCell<Vec<f64>>>,
) {
    let area = area.clone();
    area.set_draw_func(move |_, cr, width, height| {
        let rx = rx.borrow();
        let tx = tx.borrow();
        let tokens = crate::ui::theme::active_tokens();
        let plot = PlotRect::from_size(width, height);
        if rx.is_empty() && tx.is_empty() {
            draw_chart_axes(cr, &plot, &tokens, YAxis::Rate(1.0));
            return;
        }
        let max = rx
            .iter()
            .chain(tx.iter())
            .copied()
            .fold(1.0_f64, f64::max);
        draw_chart_axes(cr, &plot, &tokens, YAxis::Rate(max));
        let down = parse_hex(&tokens.semantic.info);
        let down2 = tokens
            .accent
            .get(1)
            .map(|s| parse_hex(s))
            .unwrap_or_else(|| parse_hex(&tokens.semantic.info));
        let up = parse_hex(&tokens.semantic.success);
        let up2 = tokens
            .accent
            .get(2)
            .map(|s| parse_hex(s))
            .unwrap_or_else(|| parse_hex(&tokens.semantic.success));
        if !rx.is_empty() {
            let vals: Vec<f32> = rx.iter().map(|v| (*v / max * 100.0) as f32).collect();
            draw_series(cr, &plot, &vals, 100.0, down, down2, true, 1.8, true);
        }
        if !tx.is_empty() {
            let vals: Vec<f32> = tx.iter().map(|v| (*v / max * 100.0) as f32).collect();
            draw_series(cr, &plot, &vals, 100.0, up, up2, true, 1.8, true);
        }
    });
}

/// Network throughput chart (auto-scaled max from history).
pub fn wire_rate_chart(area: &gtk::DrawingArea, history: Rc<RefCell<Vec<f64>>>, secondary: bool) {
    let area = area.clone();
    area.set_draw_func(move |_, cr, width, height| {
        let history = history.borrow();
        let tokens = crate::ui::theme::active_tokens();
        let plot = PlotRect::from_size(width, height);
        if history.is_empty() {
            draw_chart_axes(cr, &plot, &tokens, YAxis::Rate(1.0));
            return;
        }
        let max = history.iter().copied().fold(1.0_f64, f64::max);
        draw_chart_axes(cr, &plot, &tokens, YAxis::Rate(max));
        let floats: Vec<f32> = history.iter().map(|v| (*v / max * 100.0) as f32).collect();
        let (c1, c2) = if secondary {
            (
                parse_hex(&tokens.semantic.info),
                parse_hex(&tokens.semantic.success),
            )
        } else {
            accent_pair(&tokens)
        };
        draw_series(cr, &plot, &floats, 100.0, c1, c2, true, 2.0, true);
    });
}

pub fn rate_series_color(primary: bool) -> (f64, f64, f64) {
    let tokens = crate::ui::theme::active_tokens();
    if primary {
        parse_hex(&tokens.semantic.info)
    } else {
        parse_hex(&tokens.semantic.success)
    }
}

pub fn accent_series_color(tokens: &ThemeTokens) -> (f64, f64, f64) {
    accent_pair(tokens).0
}

pub fn core_color(index: usize, _total: usize, tokens: &ThemeTokens) -> (f64, f64, f64) {
    let mut palette = vec![
        (0.97, 0.97, 0.98), // C0 — white
        parse_hex(tokens.accent_primary()),
        parse_hex(&tokens.semantic.info),
        parse_hex(&tokens.semantic.success),
        parse_hex(&tokens.semantic.warning),
        parse_hex(&tokens.semantic.error),
        (0.72, 0.45, 0.98), // purple
        (0.25, 0.82, 0.78), // teal
        (0.98, 0.55, 0.25), // orange
        (0.95, 0.35, 0.65), // pink
        (0.45, 0.72, 0.38), // green
        (0.55, 0.62, 0.95), // periwinkle
        (0.90, 0.82, 0.30), // gold
        (0.35, 0.78, 0.95), // sky
        (0.82, 0.42, 0.42), // coral
        (0.62, 0.90, 0.52), // lime
    ];
    for accent in &tokens.accent {
        palette.push(parse_hex(accent));
    }
    palette[index % palette.len()]
}

fn core_color_fade(index: usize, total: usize, tokens: &ThemeTokens) -> (f64, f64, f64) {
    let (r, g, b) = core_color(index, total, tokens);
    if index % 16 == 0 {
        return (r * 0.42, g * 0.42, b * 0.48);
    }
    (r * 0.5 + 0.1, g * 0.5 + 0.1, b * 0.5 + 0.12)
}

fn draw_chart_axes(cr: &gtk::cairo::Context, plot: &PlotRect, tokens: &ThemeTokens, axis: YAxis) {
    if plot.w <= 1.0 || plot.h <= 1.0 {
        return;
    }
    let (r, g, b) = parse_hex(&tokens.text_muted);
    let ticks: Vec<(f64, String)> = match axis {
        YAxis::Percent => vec![
            (100.0, "100%".into()),
            (75.0, "75%".into()),
            (50.0, "50%".into()),
            (25.0, "25%".into()),
            (0.0, "0".into()),
        ],
        YAxis::Rate(max) => vec![
            (100.0, format_rate_tick(max)),
            (50.0, format_rate_tick(max * 0.5)),
            (0.0, "0".into()),
        ],
    };

    cr.set_source_rgba(r, g, b, 0.22);
    cr.set_line_width(1.0);
    for (pct, label) in &ticks {
        let y = plot.value_y(*pct, 100.0);
        cr.move_to(plot.x, y);
        cr.line_to(plot.x + plot.w, y);
        let _ = cr.stroke();

        draw_axis_label(cr, plot.x - 4.0, y, label, tokens);
    }
}

fn draw_axis_label(cr: &gtk::cairo::Context, x: f64, y: f64, text: &str, tokens: &ThemeTokens) {
    let (r, g, b) = parse_hex(&tokens.text_muted);
    cr.save().ok();
    let _ = cr.select_font_face(
        "Sans",
        gtk::cairo::FontSlant::Normal,
        gtk::cairo::FontWeight::Normal,
    );
    cr.set_font_size(9.0);
    cr.set_source_rgba(r, g, b, 0.88);
    let extents = cr.text_extents(text).ok();
    let ascent = extents.map(|e| e.height()).unwrap_or(9.0);
    cr.move_to(x - extents.map(|e| e.width()).unwrap_or(0.0), y + ascent * 0.35);
    let _ = cr.show_text(text);
    cr.restore().ok();
}

fn format_rate_tick(bps: f64) -> String {
    if bps >= 1_000_000_000.0 {
        format!("{:.0}G", bps / 1_000_000_000.0)
    } else if bps >= 1_000_000.0 {
        format!("{:.0}M", bps / 1_000_000.0)
    } else if bps >= 1_000.0 {
        format!("{:.0}K", bps / 1_000.0)
    } else if bps >= 10.0 {
        format!("{:.0}", bps)
    } else if bps > 0.0 {
        format!("{:.1}", bps)
    } else {
        "0".into()
    }
}

fn accent_pair(tokens: &ThemeTokens) -> ((f64, f64, f64), (f64, f64, f64)) {
    let a = tokens
        .accent
        .first()
        .map(|s| parse_hex(s))
        .unwrap_or((0.35, 0.75, 1.0));
    let b = tokens.accent.get(1).map(|s| parse_hex(s)).unwrap_or(a);
    (a, b)
}

fn parse_hex(hex: &str) -> (f64, f64, f64) {
    let h = hex.trim_start_matches('#');
    if h.len() != 6 {
        return (0.5, 0.5, 0.5);
    }
    let r = u8::from_str_radix(&h[0..2], 16).unwrap_or(128) as f64 / 255.0;
    let g = u8::from_str_radix(&h[2..4], 16).unwrap_or(128) as f64 / 255.0;
    let b = u8::from_str_radix(&h[4..6], 16).unwrap_or(128) as f64 / 255.0;
    (r, g, b)
}

fn series_points(plot: &PlotRect, values: &[f32], vmax: f32) -> Vec<(f64, f64)> {
    let n = values.len();
    if n == 0 {
        return Vec::new();
    }
    values
        .iter()
        .enumerate()
        .map(|(i, val)| {
            let x = if n <= 1 {
                plot.x
            } else {
                plot.x + (i as f64 / (n - 1) as f64) * plot.w
            };
            let y = plot.value_y(*val as f64, vmax as f64);
            (x, y)
        })
        .collect()
}

fn append_smooth_path(cr: &gtk::cairo::Context, points: &[(f64, f64)]) {
    if points.is_empty() {
        return;
    }
    cr.move_to(points[0].0, points[0].1);
    trace_smooth_through(cr, points);
}

/// Trace a smooth curve through `points`, starting from the current pen position at `points[0]`.
fn trace_smooth_through(cr: &gtk::cairo::Context, points: &[(f64, f64)]) {
    match points.len() {
        0 | 1 => {}
        2 => {
            cr.line_to(points[1].0, points[1].1);
        }
        _ => {
            for i in 0..points.len() - 1 {
                let p0 = if i == 0 { points[0] } else { points[i - 1] };
                let p1 = points[i];
                let p2 = points[i + 1];
                let p3 = if i + 2 < points.len() {
                    points[i + 2]
                } else {
                    points[i + 1]
                };
                let cp1x = p1.0 + (p2.0 - p0.0) * SMOOTH_TENSION;
                let cp1y = p1.1 + (p2.1 - p0.1) * SMOOTH_TENSION;
                let cp2x = p2.0 - (p3.0 - p1.0) * SMOOTH_TENSION;
                let cp2y = p2.1 - (p3.1 - p1.1) * SMOOTH_TENSION;
                cr.curve_to(cp1x, cp1y, cp2x, cp2y, p2.0, p2.1);
            }
        }
    }
}

fn draw_series_fill(
    cr: &gtk::cairo::Context,
    plot: &PlotRect,
    values: &[f32],
    vmax: f32,
    color: (f64, f64, f64),
    fill_color: (f64, f64, f64),
    gradient_fill: bool,
) {
    let points = series_points(plot, values, vmax);
    if points.len() < 2 {
        return;
    }
    let floor = plot.y + plot.h;
    cr.move_to(points[0].0, floor);
    cr.line_to(points[0].0, points[0].1);
    trace_smooth_through(cr, &points);
    cr.line_to(points.last().map(|p| p.0).unwrap_or(plot.x + plot.w), floor);
    cr.close_path();
    if gradient_fill {
        let pattern = gtk::cairo::LinearGradient::new(0.0, plot.y, 0.0, floor);
        pattern.add_color_stop_rgba(0.0, color.0, color.1, color.2, 0.68);
        pattern.add_color_stop_rgba(0.40, fill_color.0, fill_color.1, fill_color.2, 0.30);
        pattern.add_color_stop_rgba(1.0, fill_color.0, fill_color.1, fill_color.2, 0.0);
        cr.set_source(&pattern).ok();
    } else {
        cr.set_source_rgba(fill_color.0, fill_color.1, fill_color.2, 0.16);
    }
    let _ = cr.fill();
}

fn draw_series_stroke(
    cr: &gtk::cairo::Context,
    plot: &PlotRect,
    values: &[f32],
    vmax: f32,
    color: (f64, f64, f64),
    fill_color: (f64, f64, f64),
    gradient_stroke: bool,
    line_width: f64,
) {
    let points = series_points(plot, values, vmax);
    if points.is_empty() {
        return;
    }
    append_smooth_path(cr, &points);
    if gradient_stroke {
        let floor = plot.y + plot.h;
        let pattern = gtk::cairo::LinearGradient::new(0.0, plot.y, 0.0, floor);
        pattern.add_color_stop_rgba(0.0, color.0, color.1, color.2, 1.0);
        pattern.add_color_stop_rgba(1.0, fill_color.0, fill_color.1, fill_color.2, 0.50);
        cr.set_source(&pattern).ok();
    } else {
        cr.set_source_rgb(color.0, color.1, color.2);
    }
    cr.set_line_width(line_width);
    cr.set_line_join(gtk::cairo::LineJoin::Round);
    cr.set_line_cap(gtk::cairo::LineCap::Round);
    let _ = cr.stroke();
}

fn draw_series(
    cr: &gtk::cairo::Context,
    plot: &PlotRect,
    values: &[f32],
    vmax: f32,
    color: (f64, f64, f64),
    fill_color: (f64, f64, f64),
    fill: bool,
    line_width: f64,
    gradient_fill: bool,
) {
    if values.is_empty() {
        return;
    }
    if fill {
        draw_series_fill(cr, plot, values, vmax, color, fill_color, gradient_fill);
    }
    draw_series_stroke(
        cr,
        plot,
        values,
        vmax,
        color,
        fill_color,
        gradient_fill,
        line_width,
    );
}

const GAUGE_MIN_C: f32 = 0.0;
const GAUGE_MAX_C: f32 = 150.0;

/// Semicircular temperature gauge (°C, 0–150). `None` draws an empty / unavailable state.
pub fn wire_temp_gauge(area: &gtk::DrawingArea, temp_c: Rc<RefCell<Option<f32>>>) {
    let area = area.clone();
    area.set_draw_func(move |_, cr, width, height| {
        let temp = *temp_c.borrow();
        let tokens = crate::ui::theme::active_tokens();
        draw_temp_gauge(cr, width, height, temp, &tokens);
    });
}

fn draw_temp_gauge(
    cr: &gtk::cairo::Context,
    width: i32,
    height: i32,
    temp_c: Option<f32>,
    tokens: &ThemeTokens,
) {
    let w = width as f64;
    let h = height as f64;
    if w <= 8.0 || h <= 8.0 {
        return;
    }

    let pad = 8.0;
    let cx = w * 0.5;
    // Center sits on the baseline so the upper semicircle is symmetric left/right.
    let cy = h - pad;
    let radius = (w * 0.5 - pad).min(h - pad).max(18.0) * 0.88;
    let band = 9.0;
    let r_outer = radius + band * 0.5;
    let r_inner = radius - band * 0.5;
    // Cairo angles increase clockwise; upper semicircle runs PI (left) → 0 (right) via 3π/2 (top).
    let start = std::f64::consts::PI;
    let end_full = 0.0;

    let (mr, mg, mb) = parse_hex(&tokens.text_muted);
    draw_arc_band(cr, cx, cy, r_inner, r_outer, start, end_full);
    cr.set_source_rgba(mr, mg, mb, 0.18);
    let _ = cr.fill();

    gauge_scale_label(cr, cx - r_outer, cy + 5.0, "0", tokens);
    gauge_scale_label(cr, cx + r_outer - 18.0, cy + 5.0, "150", tokens);

    let Some(temp) = temp_c else {
        return;
    };

    let t = temp.clamp(GAUGE_MIN_C, GAUGE_MAX_C);
    let frac = ((t - GAUGE_MIN_C) / (GAUGE_MAX_C - GAUGE_MIN_C)) as f64;
    if frac <= 0.005 {
        return;
    }

    // Clockwise from PI by `frac * π` — 78 °C on 0–150 scale ≈ 52 % of the semicircle.
    let value_angle = start + frac * std::f64::consts::PI;
    let (accent, accent2) = accent_pair(tokens);
    draw_arc_band(cr, cx, cy, r_inner, r_outer, start, value_angle);
    let pattern = gtk::cairo::LinearGradient::new(cx, cy - r_outer, cx, cy);
    pattern.add_color_stop_rgba(0.0, accent.0, accent.1, accent.2, 0.82);
    pattern.add_color_stop_rgba(1.0, accent2.0, accent2.1, accent2.2, 0.95);
    cr.set_source(&pattern).ok();
    let _ = cr.fill();
}

/// Filled annular sector from `start` to `end` (clockwise, upper semicircle).
fn draw_arc_band(
    cr: &gtk::cairo::Context,
    cx: f64,
    cy: f64,
    r_inner: f64,
    r_outer: f64,
    start: f64,
    end: f64,
) {
    cr.new_path();
    cr.arc(cx, cy, r_outer, start, end);
    cr.arc_negative(cx, cy, r_inner, end, start);
    cr.close_path();
}

fn gauge_scale_label(
    cr: &gtk::cairo::Context,
    x: f64,
    y: f64,
    text: &str,
    tokens: &ThemeTokens,
) {
    cr.select_font_face("Sans", gtk::cairo::FontSlant::Normal, gtk::cairo::FontWeight::Normal);
    cr.set_font_size(9.0);
    let (mr, mg, mb) = parse_hex(&tokens.text_muted);
    cr.set_source_rgba(mr, mg, mb, 0.85);
    let _ = cr.move_to(x, y);
    let _ = cr.show_text(text);
}
