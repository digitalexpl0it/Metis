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
use std::time::{Duration, Instant};

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

// Muted, desaturated control colors tuned to the dark slate theme rather than the
// stock bright "traffic light" palette — they read as part of the chrome, not as
// neon dots.
const BTN_CLOSE: [f32; 4] = [0.74, 0.37, 0.36, 1.0];
const BTN_MIN: [f32; 4] = [0.74, 0.58, 0.34, 1.0];
const BTN_MAX: [f32; 4] = [0.40, 0.62, 0.46, 1.0];
/// Traffic-light buttons desaturate to a flat gray when the window is unfocused.
const BTN_INACTIVE: [f32; 4] = [0.30, 0.32, 0.37, 1.0];
/// Glyph (×, +, −) drawn over a focused button — a light translucent symbol so it
/// stays legible on the muted button fills.
const BTN_GLYPH: [f32; 4] = [0.92, 0.94, 0.97, 0.85];
/// Supersample factor for button textures so the circle/glyph edges are smooth.
const BTN_SS: i32 = 3;

const BTN_SIZE: i32 = 17;
const BTN_GAP: i32 = 9;
const BTN_RIGHT_PAD: i32 = 12;
const TITLE_LEFT_PAD: i32 = 12;
const TITLE_FONT_PX: f32 = 14.0;

/// Radius of the rounded top corners on the server-side titlebar.
const CORNER_RADIUS_PX: i32 = 10;
/// Supersample factor for the titlebar texture so the rounded corners are smooth.
const TITLEBAR_SS: i32 = 2;

/// The decoration colors, derived from the active Metis theme (`themes/*.json`) so
/// the server-side chrome tracks light/dark mode like the rest of the DE. Refreshed
/// live (~1s) alongside the titlebar opacity.
#[derive(Clone, PartialEq)]
struct Palette {
    titlebar_active: [f32; 3],
    titlebar_inactive: [f32; 3],
    border_active: [f32; 4],
    border_inactive: [f32; 4],
    text_active: [f32; 4],
    text_inactive: [f32; 4],
}

impl Default for Palette {
    fn default() -> Self {
        load_palette()
    }
}

/// Load the active theme tokens the same way the shell resolves them. The
/// compositor can't query GTK's system-theme setting, so `System` falls back to
/// the dark tokens (an explicit light/dark preference is honored exactly).
fn load_active_theme_tokens() -> metis_config::ThemeTokens {
    use metis_config::ThemeMode;
    match metis_config::load_theme_preference().unwrap_or(ThemeMode::Dark) {
        ThemeMode::Light => metis_config::load_theme_tokens("light"),
        ThemeMode::Dark | ThemeMode::System => metis_config::load_theme_tokens("dark"),
    }
}

/// Build the decoration palette from the active theme: the titlebar uses the
/// raised/base surface tokens, the frame the border token, and the title text the
/// text / muted-text tokens.
fn load_palette() -> Palette {
    let t = load_active_theme_tokens();
    let tb_active = hex_rgb(&t.surface_raised);
    let tb_inactive = hex_rgb(&t.surface);
    let border = hex_rgb(&t.border);
    let border_inactive = [
        lerp(border[0], tb_inactive[0], 0.5),
        lerp(border[1], tb_inactive[1], 0.5),
        lerp(border[2], tb_inactive[2], 0.5),
        1.0,
    ];
    let text = hex_rgb(&t.text);
    let muted = hex_rgb(&t.text_muted);
    Palette {
        titlebar_active: tb_active,
        titlebar_inactive: tb_inactive,
        border_active: [border[0], border[1], border[2], 1.0],
        border_inactive,
        text_active: [text[0], text[1], text[2], 1.0],
        text_inactive: [muted[0], muted[1], muted[2], 1.0],
    }
}

/// Parse a `#rrggbb` color into linear-ish `[r, g, b]` in 0..1 (sRGB bytes / 255,
/// matching how the GL pipeline treats the rest of the chrome). Falls back to a
/// dark slate on malformed input.
fn hex_rgb(hex: &str) -> [f32; 3] {
    let h = hex.trim().trim_start_matches('#');
    if h.len() != 6 {
        return [0.13, 0.14, 0.17];
    }
    let parse = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(0) as f32 / 255.0;
    [parse(0), parse(2), parse(4)]
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

render_elements! {
    pub DecorationElement<=GlesRenderer>;
    Solid=SolidColorRenderElement,
    Text=TextureRenderElement<GlesTexture>,
}

/// One window's decoration geometry + identity, gathered before drawing.
pub struct WindowDeco {
    pub id: u32,
    /// Full tile frame in monitor-logical coordinates. For a normal window this is
    /// the client grown by the chrome; for an `overlay` window it is the client
    /// rect itself (the titlebar overlays its top strip).
    pub frame: PixelRect,
    pub title: String,
    pub focused: bool,
    /// When true this is an auto-hide window's revealed titlebar: draw only the
    /// titlebar + title + controls (no surrounding border) and render it *above*
    /// the client surface as a translucent overlay.
    pub overlay: bool,
}

/// Render elements for a frame, split by stacking layer relative to the client
/// surfaces: `below` chrome draws behind the clients (it only fills the reserved
/// gaps), `overlay` chrome draws on top of them (the auto-hide titlebar reveal).
pub struct DecoElements {
    pub below: Vec<DecorationElement>,
    pub overlay: Vec<DecorationElement>,
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

/// A cached titlebar-background texture with rounded top corners. Re-rasterized
/// when the window's width, opacity, focus, or overlay mode changes (the corners
/// need fully transparent pixels, so a flat `SolidColorRenderElement` can't be
/// used).
#[derive(Clone)]
struct CachedTitlebar {
    width: i32,
    header: i32,
    alpha: f32,
    focused: bool,
    overlay: bool,
    buffer: TextureBuffer<GlesTexture>,
}

/// Persistent decoration resources owned by `MetisState`.
pub struct DecorationRuntime {
    font: Option<Font>,
    ids: HashMap<(u32, u8), Id>,
    titles: HashMap<u32, CachedTitle>,
    buttons: HashMap<(u32, u8), CachedButton>,
    titlebars: HashMap<u32, CachedTitlebar>,
    commit: CommitCounter,
    last_sig: u64,
    /// Configurable titlebar background opacity (title text + buttons stay
    /// opaque). Read from `bar.json` and refreshed live, mirroring the bar blur.
    titlebar_alpha: f32,
    /// Theme-derived chrome colors, refreshed live so light/dark switches apply.
    palette: Palette,
    last_config_check: Instant,
}

impl Default for DecorationRuntime {
    fn default() -> Self {
        Self {
            font: load_font(),
            ids: HashMap::new(),
            titles: HashMap::new(),
            buttons: HashMap::new(),
            titlebars: HashMap::new(),
            commit: CommitCounter::default(),
            last_sig: 0,
            titlebar_alpha: read_titlebar_opacity(),
            palette: load_palette(),
            last_config_check: Instant::now(),
        }
    }
}

/// Read the titlebar background opacity from `~/.config/metis/bar.json`, clamped
/// to a sane range. Defaults are supplied by `metis-config`.
fn read_titlebar_opacity() -> f32 {
    metis_config::load_bar_config()
        .titlebar_opacity
        .clamp(0.0, 1.0)
}

impl DecorationRuntime {
    /// Throttled re-read of `bar.json` + the active theme (~1s) so a Settings app
    /// changing the titlebar opacity or the light/dark theme is picked up live.
    /// Returns true when anything changed (caller flags damage).
    pub fn maybe_refresh(&mut self) -> bool {
        if self.last_config_check.elapsed() < Duration::from_secs(1) {
            return false;
        }
        self.last_config_check = Instant::now();
        let mut changed = false;
        let alpha = read_titlebar_opacity();
        if (alpha - self.titlebar_alpha).abs() > f32::EPSILON {
            self.titlebar_alpha = alpha;
            changed = true;
        }
        let palette = load_palette();
        if palette != self.palette {
            self.palette = palette;
            // The cached textures (titlebar / title / buttons) bake in the old
            // colors, so drop them; bump the commit so the solid border elements
            // re-damage with the new color too.
            self.titles.clear();
            self.buttons.clear();
            self.titlebars.clear();
            self.commit.increment();
            changed = true;
        }
        changed
    }

    /// Build the render elements for every decorated window. Front-to-back order
    /// within the returned vec does not matter — decoration rects never overlap.
    pub fn elements(
        &mut self,
        renderer: &mut GlesRenderer,
        windows: &[WindowDeco],
    ) -> DecoElements {
        // Drop title caches / ids for windows that no longer exist.
        let live: std::collections::HashSet<u32> = windows.iter().map(|w| w.id).collect();
        self.titles.retain(|id, _| live.contains(id));
        self.ids.retain(|(id, _), _| live.contains(id));
        self.buttons.retain(|(id, _), _| live.contains(id));
        self.titlebars.retain(|id, _| live.contains(id));

        // Fold the titlebar alpha into the signature so a live opacity change
        // re-damages the (otherwise unchanged) decoration rects.
        let sig = signature(windows) ^ self.titlebar_alpha.to_bits() as u64;
        if sig != self.last_sig {
            self.last_sig = sig;
            self.commit.increment();
        }
        let commit = self.commit;

        let mut below = Vec::with_capacity(windows.len() * 7);
        let mut overlay = Vec::new();
        for w in windows {
            let frame = w.frame;
            if frame.width <= 2 || frame.height <= APP_TILE_HEADER_PX {
                continue;
            }
            // Overlay (auto-hide reveal) chrome stacks above the client; normal
            // chrome stacks below it.
            let out = if w.overlay { &mut overlay } else { &mut below };
            // Dim only the titlebar fill; the title text and traffic-light buttons
            // are drawn as separate elements and stay fully opaque.
            let titlebar_rgb = if w.focused {
                self.palette.titlebar_active
            } else {
                self.palette.titlebar_inactive
            };
            let titlebar_alpha = self.titlebar_alpha;
            let border = if w.focused {
                self.palette.border_active
            } else {
                self.palette.border_inactive
            };
            let header = APP_TILE_HEADER_PX.min(frame.height);
            let b = APP_TILE_BORDER_PX;

            // Left / right / bottom borders around the client area. Skipped for the
            // overlay reveal — it floats only the titlebar over the client, with no
            // surrounding frame.
            if !w.overlay {
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
            }

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

            // Titlebar background (full width across the top of the frame). Pushed
            // LAST so it sits *behind* the title text and control buttons: the
            // damage renderer draws the first element on top, so an opaque bar would
            // otherwise hide them (and a translucent bar would let them bleed
            // through). Drawing it behind keeps text/buttons solid at any opacity.
            // A normal titlebar gets rounded top corners + a wrapping border; the
            // auto-hide reveal overlay is a plain square strip (no corners/border).
            if let Some(elem) = self.titlebar_element(
                renderer,
                w,
                frame.width,
                header,
                titlebar_rgb,
                titlebar_alpha,
                border,
                w.overlay,
            ) {
                out.push(elem);
            }
        }
        DecoElements { below, overlay }
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

    /// Build (or reuse) the titlebar-background texture (rounded top corners) and
    /// place it across the top of the frame. Re-rasterized only when the window's
    /// width, opacity, or focus changes.
    fn titlebar_element(
        &mut self,
        renderer: &mut GlesRenderer,
        w: &WindowDeco,
        width: i32,
        header: i32,
        color: [f32; 3],
        alpha: f32,
        border: [f32; 4],
        overlay: bool,
    ) -> Option<DecorationElement> {
        if width <= 0 || header <= 0 {
            return None;
        }
        let needs_render = self
            .titlebars
            .get(&w.id)
            .map(|c| {
                c.width != width
                    || c.header != header
                    || c.focused != w.focused
                    || c.overlay != overlay
                    || (c.alpha - alpha).abs() > f32::EPSILON
            })
            .unwrap_or(true);

        if needs_render {
            let (pixels, pw, ph) = rasterize_titlebar(
                color,
                alpha,
                [border[0], border[1], border[2]],
                width,
                header,
                overlay,
            )?;
            let texture = renderer
                .import_memory(&pixels, Fourcc::Abgr8888, Size::from((pw, ph)), false)
                .ok()?;
            let buffer =
                TextureBuffer::from_texture(renderer, texture, TITLEBAR_SS, Transform::Normal, None);
            self.titlebars.insert(
                w.id,
                CachedTitlebar {
                    width,
                    header,
                    alpha,
                    focused: w.focused,
                    overlay,
                    buffer,
                },
            );
        }

        let cached = self.titlebars.get(&w.id)?;
        let src = Rectangle::<f64, Logical>::new(
            Point::from((0.0, 0.0)),
            Size::from((width as f64, header as f64)),
        );
        let loc = Point::<i32, Logical>::from((w.frame.x, w.frame.y)).to_physical(1);
        Some(DecorationElement::Text(
            TextureRenderElement::from_texture_buffer(
                loc.to_f64(),
                &cached.buffer,
                None,
                Some(src),
                Some(Size::from((width, header))),
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
        let color = if w.focused {
            self.palette.text_active
        } else {
            self.palette.text_inactive
        };

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

/// Rasterize the titlebar background. A normal titlebar gets rounded TOP corners
/// (the bottom edge stays square where it meets the client) plus an opaque border
/// ring along the top / left / right edges, so the window's border visually wraps
/// around and continues *under* the titlebar instead of stopping at it. The
/// auto-hide reveal `overlay` is a plain square strip — no corners and no border —
/// since it floats over the client rather than framing it. `color` is the straight
/// titlebar RGB dimmed by `alpha`; `border` is the (opaque) frame color. Pixels
/// outside the rounded corners are fully transparent. Returns premultiplied RGBA
/// at `TITLEBAR_SS`× scale.
fn rasterize_titlebar(
    color: [f32; 3],
    alpha: f32,
    border: [f32; 3],
    width: i32,
    header: i32,
    overlay: bool,
) -> Option<(Vec<u8>, i32, i32)> {
    let ss = TITLEBAR_SS.max(1);
    let w = width * ss;
    let h = header * ss;
    if w <= 0 || h <= 0 {
        return None;
    }
    let mut pixels = vec![0u8; (w * h * 4) as usize];

    // Overlay: a flat, square, borderless strip filled uniformly at `alpha`.
    if overlay {
        let a = alpha.clamp(0.0, 1.0);
        let (pr, pg, pb, pa) = (
            (color[0] * a * 255.0) as u8,
            (color[1] * a * 255.0) as u8,
            (color[2] * a * 255.0) as u8,
            (a * 255.0) as u8,
        );
        for px in pixels.chunks_exact_mut(4) {
            px[0] = pr;
            px[1] = pg;
            px[2] = pb;
            px[3] = pa;
        }
        return Some((pixels, w, h));
    }

    let r = (CORNER_RADIUS_PX * ss) as f32;
    let bw = (APP_TILE_BORDER_PX * ss) as f32;
    for y in 0..h {
        for x in 0..w {
            // Distance to the nearest bordered edge (top/left/right), following the
            // rounded top corners; bottom edge is open (meets the client body).
            let ed = edge_dist(x as f32 + 0.5, y as f32 + 0.5, w as f32, r);
            let outer = aa(ed);
            if outer <= 0.0 {
                continue;
            }
            // Opaque border within `bw` of the edge; translucent fill inside that.
            let bf = aa(bw - ed);
            let fill_a = alpha;
            let pa = (bf + fill_a * (1.0 - bf)) * outer;
            let blend = |fg: f32, bg: f32| (fg * bf + bg * fill_a * (1.0 - bf)) * outer;
            let idx = ((y * w + x) * 4) as usize;
            pixels[idx] = (blend(border[0], color[0]) * 255.0) as u8;
            pixels[idx + 1] = (blend(border[1], color[1]) * 255.0) as u8;
            pixels[idx + 2] = (blend(border[2], color[2]) * 255.0) as u8;
            pixels[idx + 3] = (pa * 255.0) as u8;
        }
    }
    Some((pixels, w, h))
}

/// Signed distance (px, positive = inside) from the nearest *bordered* edge of the
/// titlebar — top, left and right — with the top corners rounded to radius `r`.
/// The bottom edge is intentionally excluded so no border is drawn where the
/// titlebar meets the client body. Values <= 0 are outside the rounded shape.
fn edge_dist(px: f32, py: f32, w: f32, r: f32) -> f32 {
    if r > 0.0 && py < r {
        if px < r {
            let d = ((px - r).powi(2) + (py - r).powi(2)).sqrt();
            return r - d;
        }
        if px > w - r {
            let d = ((px - (w - r)).powi(2) + (py - r).powi(2)).sqrt();
            return r - d;
        }
    }
    px.min(w - px).min(py)
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
