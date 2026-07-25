//! HDR H2 — SDR→PQ encode at scanout.
//!
//! When an output has HDR signaling active (H1), the DRM path composites the
//! usual sRGB desktop into an offscreen buffer, then blits it through a
//! texture shader that:
//!   1. decodes sRGB → linear light
//!   2. scales by a reference-white luminance (BT.2408: 203 nits)
//!   3. encodes SMPTE ST 2084 (PQ)
//!
//! so the panel (already in HDR10 mode) displays SDR content at a sensible
//! brightness instead of treating 1.0 as 10 000 nits.

use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::backend::renderer::element::texture::{TextureBuffer, TextureRenderElement};
use smithay::backend::renderer::element::Kind;
use smithay::backend::renderer::gles::element::TextureShaderElement;
use smithay::backend::renderer::gles::{
    GlesRenderer, GlesTexProgram, GlesTexture, Uniform, UniformName, UniformType,
};
use smithay::backend::renderer::{Bind, Offscreen};
use smithay::utils::{Buffer, Logical, Physical, Point, Rectangle, Scale, Size, Transform};

use crate::render::{OutputStack, CLEAR_COLOR};

/// BT.2408 reference white for mapping SDR peak to HDR (nits).
pub const REFERENCE_WHITE_NITS: f32 = 203.0;

/// Custom texture shader: sRGB → linear → scale nits → ST.2084 PQ.
/// Header mirrors Smithay's texture.frag so EXTERNAL/NO_ALPHA/DEBUG_FLAGS link.
const PQ_ENCODE_SHADER: &str = r#"#version 100

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
uniform float reference_white;

#if defined(DEBUG_FLAGS)
uniform float tint;
#endif

float srgb_to_linear(float c) {
    if (c <= 0.04045) {
        return c / 12.92;
    }
    return pow((c + 0.055) / 1.055, 2.4);
}

float linear_to_pq(float y) {
    // ST 2084; y is luminance in [0, 1] where 1.0 = 10 000 nits.
    y = max(y, 0.0);
    float m1 = 2610.0 / 16384.0;
    float m2 = 2523.0 / 32.0;
    float c1 = 3424.0 / 4096.0;
    float c2 = 2413.0 / 128.0;
    float c3 = 2392.0 / 128.0;
    float ym = pow(y, m1);
    return pow((c1 + c2 * ym) / (1.0 + c3 * ym), m2);
}

void main() {
    vec4 srgb = texture2D(tex, v_coords);
#if defined(NO_ALPHA)
    srgb.a = 1.0;
#endif

    float r = srgb_to_linear(srgb.r);
    float g = srgb_to_linear(srgb.g);
    float b = srgb_to_linear(srgb.b);

    // Map SDR 1.0 → reference_white nits, then normalize to PQ's 10 000 nits.
    float scale = reference_white / 10000.0;
    vec3 pq = vec3(
        linear_to_pq(r * scale),
        linear_to_pq(g * scale),
        linear_to_pq(b * scale)
    );

    vec4 color = vec4(pq, srgb.a) * alpha;

#if defined(DEBUG_FLAGS)
    if (tint == 1.0)
        color = vec4(0.0, 0.2, 0.0, 0.2) + color * 0.8;
#endif

    gl_FragColor = color;
}
"#;

/// Persistent PQ-encode GL resources, owned by `MetisState`.
#[derive(Default)]
pub struct HdrEncodeRuntime {
    pub program: Option<GlesTexProgram>,
}

impl HdrEncodeRuntime {
    pub fn ensure_program(&mut self, renderer: &mut GlesRenderer) {
        if self.program.is_some() {
            return;
        }
        match renderer.compile_custom_texture_shader(
            PQ_ENCODE_SHADER,
            &[UniformName::new("reference_white", UniformType::_1f)],
        ) {
            Ok(program) => {
                tracing::info!("hdr: compiled SDR→PQ encode shader");
                self.program = Some(program);
            }
            Err(err) => {
                tracing::warn!(?err, "hdr: failed to compile PQ encode shader; leaving SDR")
            }
        }
    }
}

/// Clear colour for the final HDR scanout pass (fullscreen PQ element covers it).
pub const HDR_CLEAR: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/// Composite `elements` (SDR) into an offscreen buffer, then wrap as a PQ blit
/// element covering the whole output. Returns `None` on GL failure (caller
/// should fall back to a direct SDR `render_frame`).
pub fn encode_output_stack(
    runtime: &mut HdrEncodeRuntime,
    renderer: &mut GlesRenderer,
    elements: &[OutputStack],
    size: Size<i32, Physical>,
    scale: Scale<f64>,
) -> Option<TextureShaderElement> {
    if size.w <= 0 || size.h <= 0 {
        return None;
    }
    runtime.ensure_program(renderer);
    let program = runtime.program.clone()?;

    let size_buf: Size<i32, Buffer> = Size::from((size.w, size.h));
    let mut offscreen =
        match Offscreen::<GlesTexture>::create_buffer(renderer, Fourcc::Abgr8888, size_buf) {
            Ok(buf) => buf,
            Err(err) => {
                tracing::warn!(?err, "hdr: offscreen buffer creation failed");
                return None;
            }
        };

    {
        let mut framebuffer = match renderer.bind(&mut offscreen) {
            Ok(fb) => fb,
            Err(err) => {
                tracing::warn!(?err, "hdr: offscreen bind failed");
                return None;
            }
        };
        let mut damage_tracker = OutputDamageTracker::new(size, scale, Transform::Normal);
        if let Err(err) = damage_tracker.render_output(
            renderer,
            &mut framebuffer,
            0,
            elements,
            CLEAR_COLOR,
        ) {
            tracing::warn!(?err, "hdr: SDR offscreen composite failed");
            return None;
        }
    }

    // TextureBuffer scale 1: buffer pixels == physical pixels for this pass.
    let buffer = TextureBuffer::from_texture(renderer, offscreen, 1, Transform::Normal, None);
    let src_rect = Rectangle::<f64, Logical>::new(
        Point::from((0.0, 0.0)),
        Size::from((size.w as f64, size.h as f64)),
    );
    let inner = TextureRenderElement::from_texture_buffer(
        Point::<f64, Physical>::from((0.0, 0.0)),
        &buffer,
        None,
        Some(src_rect),
        Some(Size::from((size.w, size.h))),
        Kind::Unspecified,
    );

    Some(TextureShaderElement::new(
        inner,
        program,
        vec![Uniform::new("reference_white", REFERENCE_WHITE_NITS)],
    ))
}
