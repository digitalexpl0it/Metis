//! Shared helpers for desktop-widget body content.

use gtk::pango;
use gtk::prelude::*;

/// Apply an optional Pango font description string to a label.
/// Empty / invalid strings leave the theme CSS font alone.
pub fn apply_font(label: &gtk::Label, font: &str) {
    let font = font.trim();
    if font.is_empty() {
        label.set_attributes(None::<&pango::AttrList>);
        return;
    }
    let desc = pango::FontDescription::from_string(font);
    let attrs = pango::AttrList::new();
    attrs.insert(pango::AttrFontDesc::new(&desc));
    label.set_attributes(Some(&attrs));
}
