//! Small shared widget helpers so the settings pages share a consistent layout.

use gtk::prelude::*;

/// Build a scrollable page with a heading. Returns the outer scroller (add to the
/// stack) and the inner content box (append rows/sections to it).
pub fn page(title: &str) -> (gtk::ScrolledWindow, gtk::Box) {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 16);
    content.set_margin_top(24);
    content.set_margin_bottom(24);
    content.set_margin_start(28);
    content.set_margin_end(28);
    content.add_css_class("metis-settings-page");

    let heading = gtk::Label::new(Some(title));
    heading.set_xalign(0.0);
    heading.add_css_class("metis-settings-title");
    content.append(&heading);

    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .hexpand(true)
        .vexpand(true)
        .child(&content)
        .build();
    (scroller, content)
}

/// A titled card grouping related controls. Returns the body box to fill.
pub fn section(title: &str) -> (gtk::Box, gtk::Box) {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 10);
    card.add_css_class("metis-settings-section");

    let header = gtk::Label::new(Some(title));
    header.set_xalign(0.0);
    header.add_css_class("metis-settings-section-title");
    card.append(&header);

    let body = gtk::Box::new(gtk::Orientation::Vertical, 8);
    card.append(&body);
    (card, body)
}

/// Like [`section`] but with a leading symbolic icon in the header.
pub fn section_with_icon(title: &str, icon: &str) -> (gtk::Box, gtk::Box) {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 10);
    card.add_css_class("metis-settings-section");

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    header.add_css_class("metis-settings-section-header");
    let img = gtk::Image::from_icon_name(icon);
    img.set_pixel_size(16);
    img.add_css_class("metis-settings-section-icon");
    header.append(&img);
    let label = gtk::Label::new(Some(title));
    label.set_xalign(0.0);
    label.add_css_class("metis-settings-section-title");
    header.append(&label);
    card.append(&header);

    let body = gtk::Box::new(gtk::Orientation::Vertical, 8);
    card.append(&body);
    (card, body)
}

/// A leading-icon + label + trailing control row.
pub fn row_with_icon(
    icon: &str,
    label: &str,
    control: &impl IsA<gtk::Widget>,
) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.add_css_class("metis-settings-row");
    let img = gtk::Image::from_icon_name(icon);
    img.set_pixel_size(16);
    img.add_css_class("metis-settings-row-icon");
    row.append(&img);
    let lbl = gtk::Label::new(Some(label));
    lbl.set_xalign(0.0);
    lbl.set_hexpand(true);
    row.append(&lbl);
    row.append(control);
    row
}

/// A label + trailing control row.
pub fn row(label: &str, control: &impl IsA<gtk::Widget>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.add_css_class("metis-settings-row");
    let lbl = gtk::Label::new(Some(label));
    lbl.set_xalign(0.0);
    lbl.set_hexpand(true);
    row.append(&lbl);
    row.append(control);
    row
}
