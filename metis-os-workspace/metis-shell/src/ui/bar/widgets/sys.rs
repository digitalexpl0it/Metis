use gtk::prelude::*;

use crate::ui::icons::{self, names};

pub struct BatteryWidget {
    root: gtk::Box,
    icon: gtk::Image,
    last_percent: std::cell::Cell<Option<u8>>,
    last_charging: std::cell::Cell<bool>,
}

impl BatteryWidget {
    pub fn new() -> Self {
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(0)
            .build();
        root.add_css_class("metis-bar-widget");
        root.add_css_class("metis-bar-battery");
        root.add_css_class("metis-bar-sys-icon");

        let icon = icons::image(names::battery(100, false));
        root.append(&icon);

        Self {
            root,
            icon,
            last_percent: std::cell::Cell::new(None),
            last_charging: std::cell::Cell::new(false),
        }
    }

    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    pub fn update(&self, percent: Option<u8>, charging: bool) {
        let pct = percent.unwrap_or(0);
        if self.last_percent.get() == percent && self.last_charging.get() == charging {
            return;
        }
        self.last_percent.set(percent);
        self.last_charging.set(charging);
        self.root
            .set_tooltip_text(Some(&format!("Battery {pct}%")));
        icons::set_icon(&self.icon, names::battery(pct, charging));
    }
}

pub struct NetworkWidget {
    root: gtk::Box,
    icon: gtk::Image,
    connected: std::cell::Cell<bool>,
}

impl NetworkWidget {
    pub fn new() -> Self {
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(0)
            .build();
        root.add_css_class("metis-bar-widget");
        root.add_css_class("metis-bar-network");
        root.add_css_class("metis-bar-sys-icon");

        let icon = icons::image(names::network(true));
        root.append(&icon);

        Self {
            root,
            icon,
            connected: std::cell::Cell::new(true),
        }
    }

    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    pub fn update(&self, label: &str, connected: bool) {
        self.root.set_tooltip_text(Some(label));
        if self.connected.get() != connected {
            self.connected.set(connected);
            icons::set_icon(&self.icon, names::network(connected));
        }
    }
}

pub struct VolumeWidget {
    root: gtk::Button,
    icon: gtk::Image,
    scale: gtk::Scale,
    mute_switch: gtk::Switch,
    updating: std::rc::Rc<std::cell::Cell<bool>>,
    last_percent: std::cell::Cell<u8>,
    last_muted: std::cell::Cell<bool>,
}

impl VolumeWidget {
    pub fn new() -> Self {
        let root = gtk::Button::builder().build();
        root.add_css_class("metis-bar-widget");
        root.add_css_class("metis-bar-volume");
        root.add_css_class("metis-bar-sys-icon");

        let icon = icons::image(names::volume(50, false));
        root.set_child(Some(&icon));

        let panel = super::super::dropdown::build_panel();
        panel.set_spacing(10);
        panel.set_width_request(240);

        let title = gtk::Label::builder()
            .label("Volume")
            .halign(gtk::Align::Start)
            .build();
        title.add_css_class("metis-bar-section-title");
        panel.append(&title);

        let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
        scale.set_draw_value(true);
        scale.set_value_pos(gtk::PositionType::Right);
        scale.add_css_class("metis-bar-volume-scale");
        panel.append(&scale);

        let mute_row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .build();
        let mute_label = gtk::Label::builder()
            .label("Mute")
            .hexpand(true)
            .halign(gtk::Align::Start)
            .build();
        let mute_switch = gtk::Switch::new();
        mute_row.append(&mute_label);
        mute_row.append(&mute_switch);
        panel.append(&mute_row);

        let updating = std::rc::Rc::new(std::cell::Cell::new(false));

        {
            let updating = updating.clone();
            scale.connect_value_changed(move |scale| {
                if updating.get() {
                    return;
                }
                let pct = scale.value().round() as u8;
                crate::services::set_volume_absolute(pct);
            });
        }

        {
            let updating = updating.clone();
            mute_switch.connect_state_set(move |_, state| {
                if updating.get() {
                    return glib::Propagation::Proceed;
                }
                crate::services::set_mute(state);
                glib::Propagation::Proceed
            });
        }

        super::super::dropdown::wire_toggle(&root, &panel, "volume");

        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
        scroll.connect_scroll(move |_, _, dy| {
            let delta = if dy < 0.0 { 5i8 } else { -5i8 };
            crate::services::set_volume_relative(delta);
            glib::Propagation::Stop
        });
        root.add_controller(scroll);

        Self {
            root,
            icon,
            scale,
            mute_switch,
            updating,
            last_percent: std::cell::Cell::new(0),
            last_muted: std::cell::Cell::new(false),
        }
    }

    pub fn root(&self) -> &gtk::Button {
        &self.root
    }

    pub fn update(&self, percent: u8, muted: bool) {
        if self.last_percent.get() == percent && self.last_muted.get() == muted {
            return;
        }
        self.last_percent.set(percent);
        self.last_muted.set(muted);
        self.root
            .set_tooltip_text(Some(&format!("Volume {percent}%")));
        self.updating.set(true);
        self.scale.set_value(f64::from(percent));
        self.mute_switch.set_active(muted);
        self.updating.set(false);
        icons::set_icon(&self.icon, names::volume(percent, muted));
    }
}
