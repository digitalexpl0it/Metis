//! Shared helpers for the Appearance-family pages (Appearance, Background, Edge
//! bar, Windows). These pages all read/write `themes/*.json`, `wallpaper.json`,
//! and `bar.json` and share the same colour-button + persistence plumbing.

use std::collections::HashSet;
use std::path::PathBuf;

use gtk::gdk;

use crate::runtime;

/// A colour button with no alpha channel (the design tokens are opaque hex).
pub fn color_dialog_button() -> gtk::ColorDialogButton {
    let dialog = gtk::ColorDialog::new();
    dialog.set_with_alpha(false);
    gtk::ColorDialogButton::new(Some(dialog))
}

pub fn hex_to_rgba(hex: &str) -> gdk::RGBA {
    gdk::RGBA::parse(hex).unwrap_or_else(|_| gdk::RGBA::new(0.0, 0.95, 1.0, 1.0))
}

pub fn rgba_to_hex(rgba: &gdk::RGBA) -> String {
    let to_u8 = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}",
        to_u8(rgba.red()),
        to_u8(rgba.green()),
        to_u8(rgba.blue())
    )
}

/// Set the `idx`-th gradient stop in a stop list, growing it if needed so a sparse
/// config still accepts edits to later stops.
pub fn set_stops(stops: &mut Vec<String>, idx: usize, hex: String) {
    while stops.len() <= idx {
        stops.push(hex.clone());
    }
    stops[idx] = hex;
}

/// Read `bar.json` fresh, let `apply` overwrite just the caller's fields, then
/// persist and nudge a live reload.
///
/// The Edge bar and Windows pages each own a *disjoint* subset of `bar.json`
/// and hold independent in-memory copies. Re-reading the on-disk config here (and
/// mutating only the fields the caller touches) means neither page — nor other
/// writers like the dock's `taskbar_pinned` pin/unpin — clobbers the others.
pub fn update_bar<F>(apply: F)
where
    F: FnOnce(&mut metis_config::BarConfig),
{
    let mut on_disk = metis_config::load_bar_config();
    apply(&mut on_disk);
    if let Err(err) = metis_config::save_bar_config(&on_disk) {
        tracing::warn!(%err, "failed to save bar.json");
    }
    // bar.json is watched by the shell (and re-read by the compositor for blur),
    // but nudge a reload so the change is instant.
    runtime::send("reload-bar");
}

/// The wallpaper currently in use (falls back to the first discoverable one),
/// used both for the Style previews and the Background picker's selection state.
pub fn current_wallpaper() -> Option<PathBuf> {
    if let Some(p) = metis_config::load_wallpaper_config().path {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    list_wallpapers().into_iter().next()
}

/// Collect selectable wallpapers: user-imported pictures first, then bundled.
pub fn list_wallpapers() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    metis_config::collect_wallpaper_images(
        &metis_config::wallpaper_store_dir(),
        &mut out,
        &mut seen,
    );
    for dir in metis_config::bundled_wallpaper_dirs() {
        metis_config::collect_wallpaper_images(&dir, &mut out, &mut seen);
    }
    out
}
