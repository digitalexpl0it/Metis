//! HDR encode at scanout — SDR desktop → PQ or HLG.
//!
//! When an output has HDR signaling active, the DRM path composites the usual
//! sRGB desktop into an offscreen buffer, then blits it through a texture
//! shader that:
//!   1. decodes sRGB → linear light
//!   2. converts Rec.709 / sRGB primaries → BT.2020
//!   3. scales by a reference-white luminance (BT.2408: 203 nits)
//!   4. encodes either SMPTE ST 2084 (PQ) or HLG (ARIB STD-B67)
//!
//! so the panel (already in HDR mode) displays SDR content at a sensible
//! brightness. PQ is preferred when EDID advertises ST.2084; HLG is used for
//! HLG-only panels.

use smithay::backend::renderer::gles::{
    GlesRenderer, GlesTexProgram, UniformName, UniformType,
};

/// BT.2408 reference white for mapping SDR peak to HDR (nits).
pub const REFERENCE_WHITE_NITS: f32 = 203.0;

/// Transfer function used for HDR encode + matching `HDR_OUTPUT_METADATA` EOTF.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HdrTransfer {
    #[default]
    Pq,
    Hlg,
}

/// Custom texture shader: sRGB → linear → BT.2020 → scale nits → ST.2084 PQ.
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

vec3 rec709_to_bt2020(vec3 c) {
    return vec3(
        0.627404 * c.r + 0.329282 * c.g + 0.043314 * c.b,
        0.069097 * c.r + 0.919540 * c.g + 0.011362 * c.b,
        0.016391 * c.r + 0.088013 * c.g + 0.895595 * c.b
    );
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

    vec3 lin = vec3(
        srgb_to_linear(srgb.r),
        srgb_to_linear(srgb.g),
        srgb_to_linear(srgb.b)
    );
    lin = rec709_to_bt2020(lin);

    // Map SDR 1.0 → reference_white nits, then normalize to PQ's 10 000 nits.
    float scale = reference_white / 10000.0;
    vec3 pq = vec3(
        linear_to_pq(lin.r * scale),
        linear_to_pq(lin.g * scale),
        linear_to_pq(lin.b * scale)
    );

    vec4 color = vec4(pq, srgb.a) * alpha;

#if defined(DEBUG_FLAGS)
    if (tint == 1.0)
        color = vec4(0.0, 0.2, 0.0, 0.2) + color * 0.8;
#endif

    gl_FragColor = color;
}
"#;

/// Custom texture shader: sRGB → linear → BT.2020 → HLG OETF.
/// Linear is scaled so SDR peak ≈ reference_white on a 1000-nit HLG system
/// (BT.2408-style), then ARIB STD-B67 / Rec.2100 HLG OETF is applied.
const HLG_ENCODE_SHADER: &str = r#"#version 100

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

vec3 rec709_to_bt2020(vec3 c) {
    return vec3(
        0.627404 * c.r + 0.329282 * c.g + 0.043314 * c.b,
        0.069097 * c.r + 0.919540 * c.g + 0.011362 * c.b,
        0.016391 * c.r + 0.088013 * c.g + 0.895595 * c.b
    );
}

float linear_to_hlg(float x) {
    // Rec.2100 HLG OETF; x is scene-referred linear in [0, 1] (1.0 = peak).
    x = max(x, 0.0);
    float a = 0.17883277;
    float b = 1.0 - 4.0 * a;
    float c = 0.5 - a * log(4.0 * a);
    if (x <= 1.0 / 12.0) {
        return sqrt(3.0 * x);
    }
    return a * log(12.0 * x - b) + c;
}

void main() {
    vec4 srgb = texture2D(tex, v_coords);
#if defined(NO_ALPHA)
    srgb.a = 1.0;
#endif

    vec3 lin = vec3(
        srgb_to_linear(srgb.r),
        srgb_to_linear(srgb.g),
        srgb_to_linear(srgb.b)
    );
    lin = rec709_to_bt2020(lin);

    // Map SDR 1.0 → reference_white / 1000 peak (typical HLG system).
    float scale = reference_white / 1000.0;
    vec3 hlg = vec3(
        linear_to_hlg(lin.r * scale),
        linear_to_hlg(lin.g * scale),
        linear_to_hlg(lin.b * scale)
    );

    vec4 color = vec4(hlg, srgb.a) * alpha;

#if defined(DEBUG_FLAGS)
    if (tint == 1.0)
        color = vec4(0.0, 0.2, 0.0, 0.2) + color * 0.8;
#endif

    gl_FragColor = color;
}
"#;

/// Persistent HDR-encode GL resources, owned by `MetisState`.
#[derive(Default)]
pub struct HdrEncodeRuntime {
    pub pq_program: Option<GlesTexProgram>,
    pub hlg_program: Option<GlesTexProgram>,
}

impl HdrEncodeRuntime {
    /// Drop compiled shaders so they recompile after VT resume / context loss.
    pub fn invalidate_gl(&mut self) {
        self.pq_program = None;
        self.hlg_program = None;
    }

    pub fn ensure_program(&mut self, renderer: &mut GlesRenderer, transfer: HdrTransfer) {
        match transfer {
            HdrTransfer::Pq => {
                if self.pq_program.is_some() {
                    return;
                }
                match renderer.compile_custom_texture_shader(
                    PQ_ENCODE_SHADER,
                    &[UniformName::new("reference_white", UniformType::_1f)],
                ) {
                    Ok(program) => {
                        tracing::info!("hdr: compiled SDR→BT.2020→PQ encode shader");
                        self.pq_program = Some(program);
                    }
                    Err(err) => {
                        tracing::warn!(?err, "hdr: failed to compile PQ encode shader; leaving SDR")
                    }
                }
            }
            HdrTransfer::Hlg => {
                if self.hlg_program.is_some() {
                    return;
                }
                match renderer.compile_custom_texture_shader(
                    HLG_ENCODE_SHADER,
                    &[UniformName::new("reference_white", UniformType::_1f)],
                ) {
                    Ok(program) => {
                        tracing::info!("hdr: compiled SDR→BT.2020→HLG encode shader");
                        self.hlg_program = Some(program);
                    }
                    Err(err) => {
                        tracing::warn!(?err, "hdr: failed to compile HLG encode shader; leaving SDR")
                    }
                }
            }
        }
    }
}

/// Clear colour for the final HDR scanout pass (fullscreen encode element covers it).
pub const HDR_CLEAR: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
