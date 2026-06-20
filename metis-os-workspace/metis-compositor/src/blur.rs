//! Backdrop blur for the Metis edge bar.
//!
//! The bar reserves an exclusive zone, so the content directly behind it is the
//! wallpaper (windows reflow below the bar). We therefore implement the frosted
//! look by sampling the wallpaper texture under the bar's rectangle through a
//! Gaussian blur shader, drawn just beneath the bar surface. This avoids a true
//! framebuffer capture (and the transform/coordinate hazards that come with it),
//! while still giving a convincing translucent-blur bar over the wallpaper.
//!
//! Failure is always safe: if the wallpaper texture or shader program is
//! missing, the element simply isn't built and the bar renders normally.

use std::time::{Duration, Instant};

use smithay::backend::renderer::element::{Element, Id, Kind, RenderElement};
use smithay::backend::renderer::gles::{
    GlesError, GlesFrame, GlesRenderer, GlesTexProgram, GlesTexture, Uniform, UniformName,
    UniformType,
};
use smithay::backend::renderer::utils::CommitCounter;
use smithay::utils::{
    user_data::UserDataMap, Buffer, Physical, Point, Rectangle, Scale, Size, Transform,
};

/// Custom texture shader: a 7x7 Gaussian sampled around each texel. The sampling
/// span scales with `blur_radius` (in texels). Mirrors Smithay's built-in
/// texture.frag header so the EXTERNAL/NO_ALPHA/DEBUG_FLAGS variants still link.
const BLUR_SHADER: &str = r#"#version 100

//_DEFINES_

#if defined(EXTERNAL)
#extension GL_OES_EGL_image_external : require
#endif

precision highp float;
#if defined(EXTERNAL)
uniform samplerExternalOES tex;
#else
uniform sampler2D tex;
#endif

uniform float alpha;
varying vec2 v_coords;
uniform vec2 tex_size;
uniform float blur_radius;

#if defined(DEBUG_FLAGS)
uniform float tint;
#endif

void main() {
    vec2 px = (blur_radius / 3.0) / tex_size;
    vec4 sum = vec4(0.0);
    float wsum = 0.0;
    for (int i = -3; i <= 3; i++) {
        for (int j = -3; j <= 3; j++) {
            float d2 = float(i * i + j * j);
            float w = exp(-d2 / 8.0);
            sum += texture2D(tex, v_coords + vec2(float(i), float(j)) * px) * w;
            wsum += w;
        }
    }
    vec4 color = sum / wsum;

#if defined(NO_ALPHA)
    color = vec4(color.rgb, 1.0) * alpha;
#else
    color = color * alpha;
#endif

#if defined(DEBUG_FLAGS)
    if (tint == 1.0)
        color = vec4(0.0, 0.2, 0.0, 0.2) + color * 0.8;
#endif

    gl_FragColor = color;
}
"#;

/// Persistent blur configuration + GL resources, owned by `MetisState`.
pub struct BlurRuntime {
    pub enabled: bool,
    pub radius: f32,
    id: Id,
    pub program: Option<GlesTexProgram>,
    commit: CommitCounter,
    last_sig: u64,
    last_check: Instant,
}

impl Default for BlurRuntime {
    fn default() -> Self {
        let (enabled, radius) = read_bar_blur_config();
        Self {
            enabled,
            radius,
            id: Id::new(),
            program: None,
            commit: CommitCounter::default(),
            last_sig: 0,
            last_check: Instant::now(),
        }
    }
}

impl BlurRuntime {
    /// Throttled re-read of `bar.json` (~1s) so a Settings app toggling blur is
    /// picked up live. Returns true when settings changed (caller flags damage).
    pub fn maybe_refresh(&mut self) -> bool {
        if self.last_check.elapsed() < Duration::from_secs(1) {
            return false;
        }
        self.last_check = Instant::now();
        let (enabled, radius) = read_bar_blur_config();
        if enabled != self.enabled || (radius - self.radius).abs() > f32::EPSILON {
            self.enabled = enabled;
            self.radius = radius;
            return true;
        }
        false
    }

    /// Lazily compile the blur shader using the renderer. Safe to call every
    /// frame; only compiles once.
    pub fn ensure_program(&mut self, renderer: &mut GlesRenderer) {
        if self.program.is_some() {
            return;
        }
        match renderer.compile_custom_texture_shader(
            BLUR_SHADER,
            &[
                UniformName::new("tex_size", UniformType::_2f),
                UniformName::new("blur_radius", UniformType::_1f),
            ],
        ) {
            Ok(program) => {
                tracing::info!("blur: compiled backdrop blur shader");
                self.program = Some(program);
            }
            Err(err) => tracing::warn!(?err, "blur: failed to compile shader; disabling"),
        }
    }

    /// Build a render element for the bar rect, or `None` if blur is unavailable.
    /// Bumps the commit counter when the rect/radius/texture changes so the
    /// damage tracker repaints only when needed.
    pub fn element(
        &mut self,
        rect: Rectangle<i32, Physical>,
        texture: GlesTexture,
        tex_size: Size<i32, Buffer>,
    ) -> Option<BlurElement> {
        if !self.enabled || rect.size.is_empty() || tex_size.is_empty() {
            return None;
        }
        let program = self.program.clone()?;

        let sig = signature(rect, self.radius, texture.tex_id(), tex_size);
        if sig != self.last_sig {
            self.last_sig = sig;
            self.commit.increment();
        }

        let src = Rectangle::<f64, Buffer>::new(
            Point::from((rect.loc.x as f64, rect.loc.y as f64)),
            Size::from((rect.size.w as f64, rect.size.h as f64)),
        );

        Some(BlurElement {
            id: self.id.clone(),
            commit: self.commit,
            geometry: rect,
            src,
            texture,
            tex_w: tex_size.w as f32,
            tex_h: tex_size.h as f32,
            radius: self.radius,
            program,
        })
    }
}

fn signature(rect: Rectangle<i32, Physical>, radius: f32, tex_id: u32, tex: Size<i32, Buffer>) -> u64 {
    let mut h = 1469598103934665603u64;
    let mut mix = |v: i64| {
        h ^= v as u64;
        h = h.wrapping_mul(1099511628211);
    };
    mix(rect.loc.x as i64);
    mix(rect.loc.y as i64);
    mix(rect.size.w as i64);
    mix(rect.size.h as i64);
    mix(radius.to_bits() as i64);
    mix(tex_id as i64);
    mix(tex.w as i64);
    mix(tex.h as i64);
    h
}

/// A blurred copy of the wallpaper under the bar, drawn beneath the bar surface.
#[derive(Debug)]
pub struct BlurElement {
    id: Id,
    commit: CommitCounter,
    geometry: Rectangle<i32, Physical>,
    src: Rectangle<f64, Buffer>,
    texture: GlesTexture,
    tex_w: f32,
    tex_h: f32,
    radius: f32,
    program: GlesTexProgram,
}

impl Element for BlurElement {
    fn id(&self) -> &Id {
        &self.id
    }

    fn current_commit(&self) -> CommitCounter {
        self.commit
    }

    fn src(&self) -> Rectangle<f64, Buffer> {
        self.src
    }

    fn geometry(&self, _scale: Scale<f64>) -> Rectangle<i32, Physical> {
        self.geometry
    }

    fn transform(&self) -> Transform {
        Transform::Normal
    }
}

impl RenderElement<GlesRenderer> for BlurElement {
    fn draw(
        &self,
        frame: &mut GlesFrame<'_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque_regions: &[Rectangle<i32, Physical>],
        _cache: Option<&UserDataMap>,
    ) -> Result<(), GlesError> {
        frame.render_texture_from_to(
            &self.texture,
            src,
            dst,
            damage,
            opaque_regions,
            Transform::Normal,
            1.0,
            Some(&self.program),
            &[
                Uniform::new("tex_size", [self.tex_w, self.tex_h]),
                Uniform::new("blur_radius", self.radius),
            ],
        )
    }
}

/// Best-effort read of the bar's blur settings from `~/.config/metis/bar.json`.
/// Defaults to enabled with a moderate radius when the file or fields are
/// missing, matching the shell's defaults.
fn read_bar_blur_config() -> (bool, f32) {
    const DEFAULT_ENABLED: bool = true;
    const DEFAULT_RADIUS: f32 = 18.0;

    let Some(path) = bar_config_path() else {
        return (DEFAULT_ENABLED, DEFAULT_RADIUS);
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return (DEFAULT_ENABLED, DEFAULT_RADIUS);
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return (DEFAULT_ENABLED, DEFAULT_RADIUS);
    };
    let enabled = value
        .get("blur")
        .and_then(|v| v.as_bool())
        .unwrap_or(DEFAULT_ENABLED);
    let radius = value
        .get("blur_radius")
        .and_then(|v| v.as_f64())
        .map(|r| r as f32)
        .unwrap_or(DEFAULT_RADIUS)
        .clamp(1.0, 64.0);
    (enabled, radius)
}

fn bar_config_path() -> Option<std::path::PathBuf> {
    directories::ProjectDirs::from("com", "metis", "metis")
        .map(|dirs| dirs.config_dir().join("bar.json"))
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| std::path::PathBuf::from(h).join(".config/metis/bar.json"))
        })
}
