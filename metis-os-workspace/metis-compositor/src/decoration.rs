//! Server-side window decorations (titlebar + border + window controls).
//!
//! Metis forces SSD on every toplevel (see the `XdgDecorationHandler` impl), so
//! the compositor is responsible for drawing the chrome around each tiled app
//! window. We draw it with cheap `SolidColorRenderElement`s for the titlebar,
//! border edges and the three macOS-style control buttons, plus a cached
//! `fontdue`-rasterized texture for the window title.
//!
//! The decoration rects live in the gap between the tile frame (full cell) and
//! the client surface (inset by titlebar + border via `app_tile_body_rect`), so
//! they never overlap the client buffer.

use std::collections::HashMap;

use fontdue::Font;
use metis_grid::{PixelRect, APP_TILE_BORDER_PX, APP_TILE_HEADER_PX};
use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::element::solid::SolidColorRenderElement;
use smithay::backend::renderer::element::texture::{TextureBuffer, TextureRenderElement};
use smithay::backend::renderer::element::{render_elements, Id, Kind};
use smithay::backend::renderer::gles::{GlesRenderer, GlesTexture};
use smithay::backend::renderer::utils::CommitCounter;
use smithay::backend::renderer::{Color32F, ImportMem};
use smithay::utils::{Logical, Physical, Point, Rectangle, Size, Transform};

// Decoration palette (dark theme). Kept self-contained so the compositor needn't
// depend on metis-config; values track the shell's dark surface tokens.
const TITLEBAR_ACTIVE: [f32; 4] = [0.13, 0.14, 0.17, 1.0];
const TITLEBAR_INACTIVE: [f32; 4] = [0.10, 0.11, 0.13, 1.0];
const BORDER_ACTIVE: [f32; 4] = [0.30, 0.33, 0.40, 1.0];
const BORDER_INACTIVE: [f32; 4] = [0.16, 0.17, 0.20, 1.0];
const TITLE_TEXT: [f32; 4] = [0.92, 0.93, 0.96, 1.0];
const BTN_CLOSE: [f32; 4] = [0.93, 0.33, 0.31, 1.0];
const BTN_MIN: [f32; 4] = [0.96, 0.74, 0.25, 1.0];
const BTN_MAX: [f32; 4] = [0.30, 0.78, 0.34, 1.0];
/// Traffic-light buttons desaturate to a flat gray when the window is unfocused.
const BTN_INACTIVE: [f32; 4] = [0.34, 0.36, 0.40, 1.0];
/// Glyph (×, +, −) drawn over a focused button — a dark translucent symbol.
const BTN_GLYPH: [f32; 4] = [0.0, 0.0, 0.0, 0.55];
/// Supersample factor for button textures so the circle/glyph edges are smooth.
const BTN_SS: i32 = 3;

const BTN_SIZE: i32 = 14;
const BTN_GAP: i32 = 8;
const BTN_RIGHT_PAD: i32 = 12;
const TITLE_LEFT_PAD: i32 = 12;
const TITLE_FONT_PX: f32 = 14.0;

render_elements! {
    pub DecorationElement<=GlesRenderer>;
    Solid=SolidColorRenderElement,
    Text=TextureRenderElement<GlesTexture>,
}

/// One window's decoration geometry + identity, gathered before drawing.
pub struct WindowDeco {
    pub id: u32,
    /// Full tile frame in monitor-logical coordinates.
    pub frame: PixelRect,
    pub title: String,
    pub focused: bool,
}

/// Hit-test regions for a window's controls, in monitor-logical coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecoControl {
    Close,
    Minimize,
    Maximize,
    Titlebar,
}

#[derive(Clone)]
struct CachedTitle {
    text: String,
    color: [f32; 4],
    width: i32,
    height: i32,
    buffer: TextureBuffer<GlesTexture>,
}

/// A cached, anti-aliased control-button texture (rounded circle + glyph). Keyed
/// per `(window, role)` so each window owns a distinct buffer id; re-rasterized
/// only when the window's focus state flips.
#[derive(Clone)]
struct CachedButton {
    focused: bool,
    buffer: TextureBuffer<GlesTexture>,
}

/// Persistent decoration resources owned by `MetisState`.
pub struct DecorationRuntime {
    font: Option<Font>,
    ids: HashMap<(u32, u8), Id>,
    titles: HashMap<u32, CachedTitle>,
    buttons: HashMap<(u32, u8), CachedButton>,
    commit: CommitCounter,
    last_sig: u64,
}

impl Default for DecorationRuntime {
    fn default() -> Self {
        Self {
            font: load_font(),
            ids: HashMap::new(),
            titles: HashMap::new(),
            buttons: HashMap::new(),
            commit: CommitCounter::default(),
            last_sig: 0,
        }
    }
}

impl DecorationRuntime {
    /// Build the render elements for every decorated window. Front-to-back order
    /// within the returned vec does not matter — decoration rects never overlap.
    pub fn elements(
        &mut self,
        renderer: &mut GlesRenderer,
        windows: &[WindowDeco],
    ) -> Vec<DecorationElement> {
        // Drop title caches / ids for windows that no longer exist.
        let live: std::collections::HashSet<u32> = windows.iter().map(|w| w.id).collect();
        self.titles.retain(|id, _| live.contains(id));
        self.ids.retain(|(id, _), _| live.contains(id));
        self.buttons.retain(|(id, _), _| live.contains(id));

        let sig = signature(windows);
        if sig != self.last_sig {
            self.last_sig = sig;
            self.commit.increment();
        }
        let commit = self.commit;

        let mut out = Vec::with_capacity(windows.len() * 7);
        for w in windows {
            let frame = w.frame;
            if frame.width <= 2 || frame.height <= APP_TILE_HEADER_PX {
                continue;
            }
            let titlebar = BORDER_or(TITLEBAR_ACTIVE, TITLEBAR_INACTIVE, w.focused);
            let border = BORDER_or(BORDER_ACTIVE, BORDER_INACTIVE, w.focused);
            let header = APP_TILE_HEADER_PX.min(frame.height);
            let b = APP_TILE_BORDER_PX;

            // Titlebar background (full width across the top of the frame).
            out.push(self.solid(
                w.id,
                0,
                PixelRect { x: frame.x, y: frame.y, width: frame.width, height: header },
                titlebar,
                commit,
            ));
            // Left / right / bottom borders around the client area.
            out.push(self.solid(
                w.id,
                1,
                PixelRect { x: frame.x, y: frame.y + header, width: b, height: frame.height - header },
                border,
                commit,
            ));
            out.push(self.solid(
                w.id,
                2,
                PixelRect {
                    x: frame.x + frame.width - b,
                    y: frame.y + header,
                    width: b,
                    height: frame.height - header,
                },
                border,
                commit,
            ));
            out.push(self.solid(
                w.id,
                3,
                PixelRect {
                    x: frame.x,
                    y: frame.y + frame.height - b,
                    width: frame.width,
                    height: b,
                },
                border,
                commit,
            ));

            // Control buttons, laid out from the right: close, maximize, minimize.
            // Each is a cached rounded texture; unfocused windows get gray buttons.
            let cy = frame.y + (header - BTN_SIZE) / 2;
            let close_x = frame.x + frame.width - BTN_RIGHT_PAD - BTN_SIZE;
            let max_x = close_x - (BTN_GAP + BTN_SIZE);
            let min_x = max_x - (BTN_GAP + BTN_SIZE);
            for (role, kind, x) in [
                (4u8, DecoControl::Close, close_x),
                (5u8, DecoControl::Maximize, max_x),
                (6u8, DecoControl::Minimize, min_x),
            ] {
                if let Some(elem) = self.button_element(renderer, w, role, kind, x, cy) {
                    out.push(elem);
                }
            }

            // Title text (cached texture), clipped to the space before the buttons.
            let max_text_w = (min_x - (frame.x + TITLE_LEFT_PAD) - BTN_GAP).max(0);
            if max_text_w > 8 {
                if let Some(elem) = self.title_element(renderer, w, max_text_w, header) {
                    out.push(elem);
                }
            }
        }
        out
    }

    fn solid(
        &mut self,
        window_id: u32,
        role: u8,
        rect: PixelRect,
        color: [f32; 4],
        commit: CommitCounter,
    ) -> DecorationElement {
        let id = self
            .ids
            .entry((window_id, role))
            .or_insert_with(Id::new)
            .clone();
        let geo = phys(rect);
        DecorationElement::Solid(SolidColorRenderElement::new(
            id,
            geo,
            commit,
            Color32F::from(color),
            Kind::Unspecified,
        ))
    }

    /// Build (or reuse) a rounded control-button texture and place it at `(x, cy)`.
    fn button_element(
        &mut self,
        renderer: &mut GlesRenderer,
        w: &WindowDeco,
        role: u8,
        kind: DecoControl,
        x: i32,
        cy: i32,
    ) -> Option<DecorationElement> {
        let needs_render = self
            .buttons
            .get(&(w.id, role))
            .map(|c| c.focused != w.focused)
            .unwrap_or(true);

        if needs_render {
            let (pixels, pw, ph) = rasterize_button(kind, w.focused)?;
            let texture = renderer
                .import_memory(&pixels, Fourcc::Abgr8888, Size::from((pw, ph)), false)
                .ok()?;
            let buffer = TextureBuffer::from_texture(renderer, texture, BTN_SS, Transform::Normal, None);
            self.buttons.insert(
                (w.id, role),
                CachedButton {
                    focused: w.focused,
                    buffer,
                },
            );
        }

        let cached = self.buttons.get(&(w.id, role))?;
        let src = Rectangle::<f64, Logical>::new(
            Point::from((0.0, 0.0)),
            Size::from((BTN_SIZE as f64, BTN_SIZE as f64)),
        );
        let loc = Point::<i32, Logical>::from((x, cy)).to_physical(1);
        Some(DecorationElement::Text(
            TextureRenderElement::from_texture_buffer(
                loc.to_f64(),
                &cached.buffer,
                None,
                Some(src),
                Some(Size::from((BTN_SIZE, BTN_SIZE))),
                Kind::Unspecified,
            ),
        ))
    }

    fn title_element(
        &mut self,
        renderer: &mut GlesRenderer,
        w: &WindowDeco,
        max_w: i32,
        header: i32,
    ) -> Option<DecorationElement> {
        let font = self.font.as_ref()?;
        let color = BORDER_or(TITLE_TEXT, [0.7, 0.71, 0.74, 1.0], w.focused);

        let needs_render = self
            .titles
            .get(&w.id)
            .map(|c| c.text != w.title || c.color != color)
            .unwrap_or(true);

        if needs_render {
            if let Some((pixels, tw, th)) = rasterize(font, &w.title, TITLE_FONT_PX, color) {
                if let Ok(texture) = renderer.import_memory(
                    &pixels,
                    Fourcc::Abgr8888,
                    Size::from((tw, th)),
                    false,
                ) {
                    let buffer =
                        TextureBuffer::from_texture(renderer, texture, 1, Transform::Normal, None);
                    self.titles.insert(
                        w.id,
                        CachedTitle {
                            text: w.title.clone(),
                            color,
                            width: tw,
                            height: th,
                            buffer,
                        },
                    );
                } else {
                    return None;
                }
            } else {
                self.titles.remove(&w.id);
                return None;
            }
        }

        let cached = self.titles.get(&w.id)?;
        let (tw, th) = (cached.width, cached.height);
        let draw_w = tw.min(max_w);
        let x = w.frame.x + TITLE_LEFT_PAD;
        let y = w.frame.y + (header - th) / 2;
        let src = Rectangle::<f64, Logical>::new(
            Point::from((0.0, 0.0)),
            Size::from((draw_w as f64, th as f64)),
        );
        let loc = Point::<i32, Logical>::from((x, y)).to_physical(1);
        Some(DecorationElement::Text(
            TextureRenderElement::from_texture_buffer(
                loc.to_f64(),
                &cached.buffer,
                None,
                Some(src),
                Some(Size::from((draw_w, th))),
                Kind::Unspecified,
            ),
        ))
    }
}

/// Compute hit-test rects (monitor-logical) for a window's controls, given its
/// full tile frame. Returns `(control, rect)` pairs in priority order (buttons
/// before the general titlebar drag region).
pub fn control_hitboxes(frame: PixelRect) -> Vec<(DecoControl, PixelRect)> {
    let header = APP_TILE_HEADER_PX.min(frame.height);
    let cy = frame.y + (header - BTN_SIZE) / 2;
    let close_x = frame.x + frame.width - BTN_RIGHT_PAD - BTN_SIZE;
    let max_x = close_x - (BTN_GAP + BTN_SIZE);
    let min_x = max_x - (BTN_GAP + BTN_SIZE);
    let hit = |x: i32| PixelRect {
        x: x - BTN_GAP / 2,
        y: frame.y,
        width: BTN_SIZE + BTN_GAP,
        height: header,
    };
    let _ = cy;
    vec![
        (DecoControl::Close, hit(close_x)),
        (DecoControl::Maximize, hit(max_x)),
        (DecoControl::Minimize, hit(min_x)),
        (
            DecoControl::Titlebar,
            PixelRect { x: frame.x, y: frame.y, width: frame.width, height: header },
        ),
    ]
}

#[allow(non_snake_case)]
fn BORDER_or(active: [f32; 4], inactive: [f32; 4], focused: bool) -> [f32; 4] {
    if focused {
        active
    } else {
        inactive
    }
}

fn phys(r: PixelRect) -> Rectangle<i32, Physical> {
    Rectangle::<i32, Logical>::new(
        Point::from((r.x, r.y)),
        Size::from((r.width.max(1), r.height.max(1))),
    )
    .to_physical(1)
}

/// Rasterize `text` to a premultiplied RGBA buffer at `font_px`. Returns
/// `(pixels, width, height)` or `None` when the string is empty/too small.
fn rasterize(font: &Font, text: &str, font_px: f32, color: [f32; 4]) -> Option<(Vec<u8>, i32, i32)> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    // Two-pass: measure, then draw. Canvas is sized to the glyph extents with a
    // little vertical padding; baseline sits at ~80% of the font size.
    let canvas_h = (font_px * 1.4).ceil() as i32;
    let baseline = (font_px).round() as i32;

    let mut pen_x = 0f32;
    let mut placements: Vec<(fontdue::Metrics, Vec<u8>, i32)> = Vec::new();
    for ch in text.chars().take(256) {
        let (metrics, bitmap) = font.rasterize(ch, font_px);
        placements.push((metrics, bitmap, pen_x.round() as i32));
        pen_x += metrics.advance_width;
    }
    let width = pen_x.ceil() as i32;
    if width <= 0 || canvas_h <= 0 {
        return None;
    }
    let (width, canvas_h) = (width.min(4096), canvas_h.min(256));
    let mut pixels = vec![0u8; (width * canvas_h * 4) as usize];

    let (cr, cg, cb, _) = (color[0], color[1], color[2], color[3]);
    for (metrics, bitmap, pen) in &placements {
        let gx = pen + metrics.xmin;
        let gy = baseline - metrics.ymin - metrics.height as i32;
        for row in 0..metrics.height as i32 {
            let py = gy + row;
            if py < 0 || py >= canvas_h {
                continue;
            }
            for col in 0..metrics.width as i32 {
                let px = gx + col;
                if px < 0 || px >= width {
                    continue;
                }
                let cov = bitmap[(row * metrics.width as i32 + col) as usize] as f32 / 255.0;
                if cov <= 0.0 {
                    continue;
                }
                let idx = ((py * width + px) * 4) as usize;
                // Premultiplied alpha (matches the GL pipeline's blend setup).
                pixels[idx] = (cr * cov * 255.0) as u8;
                pixels[idx + 1] = (cg * cov * 255.0) as u8;
                pixels[idx + 2] = (cb * cov * 255.0) as u8;
                pixels[idx + 3] = (cov * 255.0) as u8;
            }
        }
    }
    Some((pixels, width, canvas_h))
}

/// Rasterize a control button: an anti-aliased filled circle (traffic-light color
/// when focused, gray when not) with a dark glyph (× close, + maximize, − minimize)
/// drawn only on focused buttons. Returns premultiplied RGBA at `BTN_SS`× scale.
fn rasterize_button(kind: DecoControl, focused: bool) -> Option<(Vec<u8>, i32, i32)> {
    let n = BTN_SIZE * BTN_SS;
    if n <= 0 {
        return None;
    }
    let circle = if focused {
        match kind {
            DecoControl::Close => BTN_CLOSE,
            DecoControl::Maximize => BTN_MAX,
            DecoControl::Minimize => BTN_MIN,
            DecoControl::Titlebar => return None,
        }
    } else {
        BTN_INACTIVE
    };

    let mut pixels = vec![0u8; (n * n * 4) as usize];
    let center = n as f32 / 2.0;
    let radius = center - BTN_SS as f32 * 0.5;
    let half_len = radius * 0.46;
    let half_thick = BTN_SS as f32 * 0.7;

    for y in 0..n {
        for x in 0..n {
            let dx = x as f32 + 0.5 - center;
            let dy = y as f32 + 0.5 - center;
            let dist = (dx * dx + dy * dy).sqrt();
            let circle_cov = aa(radius - dist);
            if circle_cov <= 0.0 {
                continue;
            }

            let mut rgb = [circle[0], circle[1], circle[2]];
            if focused {
                let g = glyph_coverage(kind, dx, dy, half_len, half_thick);
                if g > 0.0 {
                    let ga = g * BTN_GLYPH[3];
                    rgb[0] = BTN_GLYPH[0] * ga + rgb[0] * (1.0 - ga);
                    rgb[1] = BTN_GLYPH[1] * ga + rgb[1] * (1.0 - ga);
                    rgb[2] = BTN_GLYPH[2] * ga + rgb[2] * (1.0 - ga);
                }
            }

            let a = circle_cov;
            let idx = ((y * n + x) * 4) as usize;
            pixels[idx] = (rgb[0] * a * 255.0) as u8;
            pixels[idx + 1] = (rgb[1] * a * 255.0) as u8;
            pixels[idx + 2] = (rgb[2] * a * 255.0) as u8;
            pixels[idx + 3] = (a * 255.0) as u8;
        }
    }
    Some((pixels, n, n))
}

/// Coverage (0–1) of a button glyph at offset `(dx, dy)` from the button center.
fn glyph_coverage(kind: DecoControl, dx: f32, dy: f32, len: f32, thick: f32) -> f32 {
    // A single bar running `along` an axis with thickness measured `across` it.
    let bar = |along: f32, across: f32| aa(thick - across.abs()) * aa(len - along.abs() + 0.5);
    match kind {
        DecoControl::Minimize => bar(dx, dy),
        DecoControl::Maximize => bar(dx, dy).max(bar(dy, dx)),
        DecoControl::Close => {
            // Rotate 45° so the two bars form an ×.
            let inv = std::f32::consts::FRAC_1_SQRT_2;
            let u = (dx + dy) * inv;
            let v = (dx - dy) * inv;
            bar(u, v).max(bar(v, u))
        }
        DecoControl::Titlebar => 0.0,
    }
}

/// 1px-wide anti-aliased edge: maps a signed distance (px) to coverage in [0, 1].
fn aa(edge: f32) -> f32 {
    (edge + 0.5).clamp(0.0, 1.0)
}

/// FNV-1a over the decoration-relevant state so we only re-damage on change.
fn signature(windows: &[WindowDeco]) -> u64 {
    let mut h = 1469598103934665603u64;
    let mut mix = |v: i64| {
        h ^= v as u64;
        h = h.wrapping_mul(1099511628211);
    };
    for w in windows {
        mix(w.id as i64);
        mix(w.frame.x as i64);
        mix(w.frame.y as i64);
        mix(w.frame.width as i64);
        mix(w.frame.height as i64);
        mix(w.focused as i64);
        for b in w.title.as_bytes() {
            mix(*b as i64);
        }
        mix(-1);
    }
    h
}

/// Load a UI font for titles: ask fontconfig, then fall back to common paths.
fn load_font() -> Option<Font> {
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(out) = std::process::Command::new("fc-match")
        .args(["-f", "%{file}", "sans"])
        .output()
    {
        if out.status.success() {
            let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !p.is_empty() {
                candidates.push(p.into());
            }
        }
    }
    candidates.extend(
        [
            "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/TTF/DejaVuSans.ttf",
        ]
        .iter()
        .map(std::path::PathBuf::from),
    );

    for path in candidates {
        if let Ok(bytes) = std::fs::read(&path) {
            if let Ok(font) = Font::from_bytes(bytes, fontdue::FontSettings::default()) {
                tracing::info!(path = %path.display(), "decoration: loaded title font");
                return Some(font);
            }
        }
    }
    tracing::warn!("decoration: no title font found; titles will be blank");
    None
}
