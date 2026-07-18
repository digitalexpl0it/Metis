//! Builtin desktop-widget body content (Folders, Apps, Clock, System, Weather, Equalizer).

mod apps;
mod clock;
mod equalizer;
mod folders;
mod font;
mod system;
pub(crate) mod weather;

use gtk::prelude::*;
use metis_config::{DesktopWidgetInstance, DesktopWidgetKind};

pub fn build(inst: &DesktopWidgetInstance) -> gtk::Widget {
    match inst.kind {
        DesktopWidgetKind::Folders => folders::build(inst),
        DesktopWidgetKind::Apps => apps::build(inst),
        DesktopWidgetKind::Clock => clock::build(inst),
        DesktopWidgetKind::System => system::build(inst),
        DesktopWidgetKind::Weather => weather::build(inst),
        DesktopWidgetKind::Equalizer => equalizer::build(inst),
        DesktopWidgetKind::Placeholder => {
            let col = gtk::Box::new(gtk::Orientation::Vertical, 8);
            let hint = gtk::Label::new(Some(
                "Placeholder — drag the title bar to move, corner to resize.",
            ));
            hint.set_wrap(true);
            hint.set_xalign(0.0);
            hint.add_css_class("metis-dw-hint");
            col.append(&hint);
            col.upcast()
        }
    }
}
