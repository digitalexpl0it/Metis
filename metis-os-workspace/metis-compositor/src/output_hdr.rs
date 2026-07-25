//! Per-output HDR signaling via DRM `Colorspace`, `HDR_OUTPUT_METADATA`, and
//! `max_bpc` (Phase 5 H1).
//!
//! Enabling HDR puts the connector into an HDR10-capable signaling state
//! (`HDR_OUTPUT_METADATA` with PQ EOTF) and bumps `max_bpc`. For SDR→PQ desktop
//! encode (H2) we intentionally keep connector `Colorspace` at Default / RGB and
//! advertise Rec.709 mastering primaries — forcing BT.2020 without a gamut
//! convert makes many laptop panels look warm/muddy ("tan overlay").

use std::ffi::CStr;

use metis_config::{output_prefs, OutputsConfig};
use smithay::backend::allocator::Fourcc;
use smithay::reexports::drm::control::{
    connector, property, Device as DrmControlDevice,
};

use crate::state::MetisState;
use crate::udev::UdevOutputId;

/// CTA-861 EOTF: SMPTE ST 2084 (PQ).
const EOTF_ST2084: u8 = 2;
/// `hdr_output_metadata.metadata_type` — HDMI static metadata type 1.
const HDR_METADATA_TYPE1: u32 = 0;

/// Kernel `struct hdr_metadata_infoframe` (CTA-861 static metadata).
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct HdrMetadataInfoframe {
    eotf: u8,
    metadata_type: u8,
    display_primaries: [HdrXy; 3],
    white_point: HdrXy,
    max_display_mastering_luminance: u16,
    min_display_mastering_luminance: u16,
    max_cll: u16,
    max_fall: u16,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct HdrXy {
    x: u16,
    y: u16,
}

/// Kernel `struct hdr_output_metadata`.
#[repr(C)]
#[derive(Clone, Copy)]
struct HdrOutputMetadata {
    metadata_type: u32,
    hdmi_metadata_type1: HdrMetadataInfoframe,
}

/// Preferred Colorspace when enabling HDR for SDR→PQ desktop content.
/// Stay on Default/RGB — do **not** switch to BT.2020 without a gamut matrix.
const HDR_SDR_COLORSPACE_PREFS: &[&str] = &["Default", "DEFAULT", "RGB", "RGB_FULL"];

const SDR_COLORSPACE_PREFS: &[&str] = &["Default", "DEFAULT", "RGB", "RGB_FULL"];

pub fn query_hdr_available(state: &MetisState, name: &str) -> bool {
    let Some(udev) = state.udev.as_ref() else {
        return false;
    };
    let Some(surface) = udev.surfaces().find(|s| s.output.name() == name) else {
        return false;
    };
    let Some(backend) = udev.backends.get(&surface.device) else {
        return false;
    };
    connector_hdr_capable(backend.drm_output_manager.device(), surface.connector, name)
}

pub fn query_hdr_active(state: &MetisState, name: &str) -> bool {
    let Some(udev) = state.udev.as_ref() else {
        return false;
    };
    udev.surfaces()
        .find(|s| s.output.name() == name)
        .is_some_and(|s| s.hdr_active)
}

/// True when HDR signaling is applied on this output (prefs + last successful apply).
pub fn hdr_active_for_output(state: &MetisState, name: &str) -> bool {
    query_hdr_active(state, name)
}

pub fn apply_output_hdrs(state: &mut MetisState, cfg: &OutputsConfig) -> bool {
    apply_output_hdrs_inner(state, cfg, false)
}

/// Re-apply saved HDR prefs after mode-set / VT resume (blob + Colorspace reset).
pub fn reapply_output_hdrs(state: &mut MetisState) {
    let cfg = state.output_runtime.cached().clone();
    let _ = apply_output_hdrs_inner(state, &cfg, true);
}

fn apply_output_hdrs_inner(state: &mut MetisState, cfg: &OutputsConfig, force: bool) -> bool {
    let mut changed = false;
    let names: Vec<String> = state
        .connected_outputs()
        .into_iter()
        .map(|o| o.name())
        .collect();
    for name in names {
        if !state.is_output_enabled(&name) {
            continue;
        }
        let want = output_prefs(cfg, &name).hdr_enabled;
        if sync_hdr_for_output(state, &name, want, force) {
            changed = true;
        }
    }
    changed
}

fn sync_hdr_for_output(state: &mut MetisState, name: &str, want: bool, force: bool) -> bool {
    let id = state
        .udev
        .as_ref()
        .and_then(|u| u.output_id_by_name(name));
    let Some(id) = id else {
        return false;
    };
    sync_hdr_for_crtc(state, id, want, force)
}

fn sync_hdr_for_crtc(state: &mut MetisState, id: UdevOutputId, want: bool, force: bool) -> bool {
    let Some(udev) = state.udev.as_ref() else {
        return false;
    };
    let Some(surface) = udev.surface(id) else {
        return false;
    };
    let name = surface.output.name();
    let connector = surface.connector;
    let device_node = surface.device;
    let already = surface.hdr_active;
    let old_blob = surface.hdr_metadata_blob;

    let Some(backend) = udev.backends.get(&device_node) else {
        return false;
    };
    let device = backend.drm_output_manager.device();

    if want {
        if !connector_hdr_capable(device, connector, &name) {
            if already {
                clear_hdr_signaling(device, connector, &name, old_blob);
                if let Some(udev) = state.udev.as_mut() {
                    if let Some(surface) = udev.surface_mut(id) {
                        surface.hdr_active = false;
                        surface.hdr_metadata_blob = None;
                    }
                }
                return true;
            }
            return false;
        }
        if already && !force {
            return false;
        }
        match apply_hdr_signaling(device, connector, &name, old_blob) {
            Ok(new_blob) => {
                tracing::info!(output = %name, "applied HDR output signaling");
                if let Some(udev) = state.udev.as_mut() {
                    if let Some(surface) = udev.surface_mut(id) {
                        surface.hdr_active = true;
                        surface.hdr_metadata_blob = new_blob;
                    }
                }
                true
            }
            Err(err) => {
                tracing::warn!(output = %name, %err, "failed to apply HDR signaling");
                false
            }
        }
    } else {
        if !already && old_blob.is_none() {
            return false;
        }
        clear_hdr_signaling(device, connector, &name, old_blob);
        tracing::info!(output = %name, "cleared HDR output signaling");
        if let Some(udev) = state.udev.as_mut() {
            if let Some(surface) = udev.surface_mut(id) {
                surface.hdr_active = false;
                surface.hdr_metadata_blob = None;
            }
        }
        true
    }
}

fn connector_hdr_capable(
    device: &impl DrmControlDevice,
    conn: connector::Handle,
    name: &str,
) -> bool {
    let Ok(props) = device.get_properties(conn) else {
        return false;
    };
    let (handles, values) = props.as_props_and_values();
    let mut has_metadata_prop = false;
    let mut edid_blob: Option<u64> = None;
    for (handle, value) in handles.iter().zip(values.iter()) {
        let Ok(info) = device.get_property(*handle) else {
            continue;
        };
        let Ok(prop_name) = info.name().to_str() else {
            continue;
        };
        match prop_name {
            "HDR_OUTPUT_METADATA" => has_metadata_prop = true,
            "EDID" => {
                if let property::Value::Blob(id) = info.value_type().convert_value(*value) {
                    if id != 0 {
                        edid_blob = Some(id);
                    }
                }
            }
            _ => {}
        }
    }

    // Driver props alone are not enough — Intel/AMD often expose
    // HDR_OUTPUT_METADATA on eDP even when the panel is SDR. PQ on SDR looks
    // washed out. Require the monitor EDID to advertise ST.2084 (HDR10).
    let edid_hdr10 = edid_blob
        .and_then(|blob| device.get_property_blob(blob).ok())
        .is_some_and(|data| edid_supports_st2084(&data));

    let capable = has_metadata_prop && edid_hdr10;
    if has_metadata_prop && !edid_hdr10 {
        tracing::info!(
            output = %name,
            "HDR toggle hidden — connector has HDR_OUTPUT_METADATA but EDID has no ST.2084/HDR10"
        );
    } else if capable {
        tracing::debug!(
            output = %name,
            "HDR capability detected (DRM metadata + EDID ST.2084)"
        );
    }
    capable
}

/// CTA-861 HDR Static Metadata Data Block (extended tag 6): EOTF bit 2 = ST.2084.
fn edid_supports_st2084(edid: &[u8]) -> bool {
    if edid.len() < 128 {
        return false;
    }
    let n_ext = edid[126] as usize;
    let mut off = 128usize;
    for _ in 0..n_ext {
        if off + 128 > edid.len() {
            break;
        }
        // CTA / CEA extension
        if edid[off] == 0x02 {
            let dtd_offset = edid[off + 2] as usize;
            let end = if dtd_offset == 0 {
                off + 127
            } else {
                off + dtd_offset.min(127)
            };
            let mut i = off + 4;
            while i < end && i < edid.len() {
                let byte = edid[i];
                if byte == 0 {
                    break;
                }
                let tag = (byte >> 5) & 0x07;
                let length = (byte & 0x1f) as usize;
                if i + 1 + length > edid.len() {
                    break;
                }
                let payload = &edid[i + 1..i + 1 + length];
                // Extended tag block (tag 7), extended tag code 6 = HDR static metadata
                if tag == 7 && !payload.is_empty() && payload[0] == 6 {
                    let eotf = payload.get(1).copied().unwrap_or(0);
                    // Bit 0 Traditional gamma SDR, 1 Traditional HDR, 2 ST2084, 3 HLG
                    if eotf & 0x04 != 0 {
                        return true;
                    }
                }
                i += 1 + length;
            }
        }
        off += 128;
    }
    false
}

fn apply_hdr_signaling(
    device: &impl DrmControlDevice,
    conn: connector::Handle,
    name: &str,
    old_blob: Option<u64>,
) -> Result<Option<u64>, String> {
    let mut new_blob = None;

    if let Some(handle) = find_prop(device, conn, "HDR_OUTPUT_METADATA") {
        let meta = default_hdr_metadata();
        let blob_val = device
            .create_property_blob(&meta)
            .map_err(|e| format!("create HDR blob: {e}"))?;
        let blob_id = match blob_val {
            property::Value::Blob(id) => id,
            _ => return Err("create_property_blob did not return a Blob".into()),
        };
        if let Err(err) = device.set_property(conn, handle, blob_id) {
            let _ = device.destroy_property_blob(blob_id);
            return Err(format!("set HDR_OUTPUT_METADATA: {err}"));
        }
        if let Some(old) = old_blob {
            if old != blob_id {
                let _ = device.destroy_property_blob(old);
            }
        }
        new_blob = Some(blob_id);
        tracing::debug!(output = %name, blob = blob_id, "HDR_OUTPUT_METADATA set");
    }

    if let Some(handle) = find_prop(device, conn, "Colorspace")
        .or_else(|| find_prop(device, conn, "COLOR_ENCODING"))
    {
        // Keep Default/RGB for SDR→PQ; BT.2020 without gamut convert looks wrong.
        if let Some(value) = pick_enum_value(device, handle, HDR_SDR_COLORSPACE_PREFS) {
            if let Err(err) = device.set_property(conn, handle, value) {
                tracing::warn!(output = %name, ?err, "failed to set Colorspace for HDR");
            } else {
                tracing::debug!(output = %name, value, "Colorspace kept Default/RGB for SDR→PQ");
            }
        }
    }

    if let Some(handle) = find_prop(device, conn, "max bpc")
        .or_else(|| find_prop(device, conn, "max_bpc"))
    {
        if let Err(err) = device.set_property(conn, handle, 10) {
            tracing::debug!(output = %name, ?err, "max_bpc=10 rejected (optional)");
        }
    }

    Ok(new_blob)
}

fn clear_hdr_signaling(
    device: &impl DrmControlDevice,
    conn: connector::Handle,
    name: &str,
    old_blob: Option<u64>,
) {
    if let Some(handle) = find_prop(device, conn, "HDR_OUTPUT_METADATA") {
        if let Err(err) = device.set_property(conn, handle, 0u64) {
            tracing::warn!(output = %name, ?err, "failed to clear HDR_OUTPUT_METADATA");
        }
    }
    if let Some(old) = old_blob {
        let _ = device.destroy_property_blob(old);
    }
    if let Some(handle) = find_prop(device, conn, "Colorspace")
        .or_else(|| find_prop(device, conn, "COLOR_ENCODING"))
    {
        if let Some(value) = pick_enum_value(device, handle, SDR_COLORSPACE_PREFS) {
            if let Err(err) = device.set_property(conn, handle, value) {
                tracing::warn!(output = %name, ?err, "failed to restore SDR Colorspace");
            }
        } else if let Err(err) = device.set_property(conn, handle, 0u64) {
            tracing::debug!(output = %name, ?err, "Colorspace reset to 0 failed");
        }
    }
}

fn find_prop(
    device: &impl DrmControlDevice,
    conn: connector::Handle,
    want: &str,
) -> Option<property::Handle> {
    let props = device.get_properties(conn).ok()?;
    let (handles, _) = props.as_props_and_values();
    for handle in handles {
        let Ok(info) = device.get_property(*handle) else {
            continue;
        };
        if prop_name_eq(info.name(), want) {
            return Some(*handle);
        }
    }
    None
}

fn prop_name_eq(name: &CStr, want: &str) -> bool {
    name.to_str()
        .map(|n| n.eq_ignore_ascii_case(want))
        .unwrap_or(false)
}

fn pick_enum_value(
    device: &impl DrmControlDevice,
    handle: property::Handle,
    prefs: &[&str],
) -> Option<u64> {
    let info = device.get_property(handle).ok()?;
    let property::ValueType::Enum(values) = info.value_type() else {
        return None;
    };
    let (_, enums) = values.values();
    for pref in prefs {
        for e in enums {
            let Ok(n) = e.name().to_str() else {
                continue;
            };
            if n.eq_ignore_ascii_case(pref) {
                return Some(e.value());
            }
        }
    }
    None
}

/// Rec.709 / D65 primaries with PQ EOTF — matches SDR→PQ desktop content
/// (BT.2408 reference white ~203 nits). Wide-gamut mastering is H3.
fn default_hdr_metadata() -> HdrOutputMetadata {
    // CTA-861 chromaticity units: coordinate / 0.00002
    fn xy(x: f64, y: f64) -> HdrXy {
        HdrXy {
            x: (x / 0.00002).round().clamp(0.0, 65535.0) as u16,
            y: (y / 0.00002).round().clamp(0.0, 65535.0) as u16,
        }
    }
    HdrOutputMetadata {
        metadata_type: HDR_METADATA_TYPE1,
        hdmi_metadata_type1: HdrMetadataInfoframe {
            eotf: EOTF_ST2084,
            metadata_type: 0,
            display_primaries: [
                xy(0.640, 0.330), // R Rec.709
                xy(0.300, 0.600), // G
                xy(0.150, 0.060), // B
            ],
            white_point: xy(0.3127, 0.3290),
            // cd/m² — honest for SDR-mapped desktop, not a fake 1000-nit master
            max_display_mastering_luminance: 400,
            // 0.0001 cd/m² units → 0.05 nits
            min_display_mastering_luminance: 500,
            max_cll: 203,
            max_fall: 100,
        },
    }
}

/// Log the negotiated primary-plane swapchain format once per surface.
pub fn maybe_log_scanout_format(state: &mut MetisState, id: UdevOutputId) {
    let Some(udev) = state.udev.as_mut() else {
        return;
    };
    let Some(surface) = udev.surface_mut(id) else {
        return;
    };
    if surface.scanout_format_logged {
        return;
    }
    let format = surface.drm_output.with_compositor(|c| c.format());
    let ten_bit = matches!(
        format,
        Fourcc::Abgr2101010
            | Fourcc::Argb2101010
            | Fourcc::Xbgr2101010
            | Fourcc::Xrgb2101010
    );
    tracing::info!(
        output = %surface.output.name(),
        ?format,
        ten_bit,
        "DRM primary plane scanout format"
    );
    surface.scanout_format_logged = true;
}

#[cfg(test)]
mod tests {
    use super::edid_supports_st2084;

    #[test]
    fn base_edid_without_cta_is_not_hdr() {
        let mut edid = vec![0u8; 128];
        edid[126] = 0; // no extensions
        assert!(!edid_supports_st2084(&edid));
    }

    #[test]
    fn cta_hdr_static_metadata_st2084_is_detected() {
        let mut edid = vec![0u8; 256];
        edid[126] = 1; // one extension
        edid[128] = 0x02; // CTA
        edid[130] = 0; // dtd offset 0 → scan until 127
        // Data block at 132: tag=7 (extended), length=3, ext_tag=6, eotf=0x04 (ST2084)
        edid[132] = (7 << 5) | 3;
        edid[133] = 6;
        edid[134] = 0x04;
        edid[135] = 0;
        assert!(edid_supports_st2084(&edid));
    }

    #[test]
    fn cta_hdr_block_without_st2084_is_rejected() {
        let mut edid = vec![0u8; 256];
        edid[126] = 1;
        edid[128] = 0x02;
        edid[130] = 0;
        edid[132] = (7 << 5) | 3;
        edid[133] = 6;
        edid[134] = 0x01; // traditional gamma only
        edid[135] = 0;
        assert!(!edid_supports_st2084(&edid));
    }
}
