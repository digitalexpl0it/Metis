use chrono::{DateTime, Local, Utc};
use chrono_tz::Tz;
use gtk::prelude::*;

use crate::config::ClockConfig;

pub struct ClockWidget {
    root: gtk::Button,
    time_label: gtk::Label,
    date_label: gtk::Label,
    timezone_box: gtk::Box,
}

impl ClockWidget {
    pub fn new(config: &ClockConfig) -> Self {
        let root = gtk::Button::builder().build();
        root.add_css_class("metis-bar-widget");
        root.add_css_class("metis-bar-clock");

        let row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .build();

        let time_label = gtk::Label::builder().build();
        time_label.add_css_class("metis-bar-clock-time");

        let date_label = gtk::Label::builder().build();
        date_label.add_css_class("metis-bar-clock-date");

        row.append(&time_label);
        row.append(&date_label);
        root.set_child(Some(&row));

        let panel = super::super::dropdown::build_panel();
        panel.set_spacing(8);
        panel.set_margin_top(10);
        panel.set_margin_bottom(10);
        panel.set_margin_start(12);
        panel.set_margin_end(12);
        panel.set_width_request(320);

        let calendar = gtk::Calendar::new();
        calendar.add_css_class("metis-bar-calendar");
        panel.append(&calendar);

        let tz_title = gtk::Label::builder()
            .label("Time zones")
            .halign(gtk::Align::Start)
            .build();
        tz_title.add_css_class("metis-bar-section-title");
        panel.append(&tz_title);

        let timezone_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(4)
            .build();
        panel.append(&timezone_box);

        super::super::dropdown::wire_toggle(&root, &panel, "clock");

        let timezones = if config.timezones.is_empty() {
            vec!["UTC".into()]
        } else {
            config.timezones.clone()
        };

        let widget = Self {
            root,
            time_label,
            date_label,
            timezone_box,
        };
        widget.refresh_labels(config);
        widget.refresh_timezones(&timezones);

        let tick_ref = std::rc::Rc::new(std::cell::RefCell::new((
            widget.time_label.clone(),
            widget.date_label.clone(),
            widget.timezone_box.clone(),
            timezones,
            config.clone(),
        )));

        glib::timeout_add_local(std::time::Duration::from_secs(1), move || {
            let (time_l, date_l, tz_box, tzs, cfg) = tick_ref.borrow().clone();
            update_clock_labels(&time_l, &date_l, &cfg);
            update_timezone_list(&tz_box, &tzs);
            glib::ControlFlow::Continue
        });

        widget
    }

    pub fn root(&self) -> &gtk::Button {
        &self.root
    }

    fn refresh_labels(&self, config: &ClockConfig) {
        update_clock_labels(&self.time_label, &self.date_label, config);
    }

    fn refresh_timezones(&self, zones: &[String]) {
        update_timezone_list(&self.timezone_box, zones);
    }
}

fn update_clock_labels(time_label: &gtk::Label, date_label: &gtk::Label, config: &ClockConfig) {
    let now = Local::now();
    time_label.set_label(&now.format(&config.time_format).to_string());
    date_label.set_label(&now.format(&config.date_format).to_string());
}

fn update_timezone_list(box_: &gtk::Box, zones: &[String]) {
    while let Some(child) = box_.first_child() {
        box_.remove(&child);
    }
    for zone in zones {
        let row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .build();
        let name = gtk::Label::builder()
            .label(zone)
            .halign(gtk::Align::Start)
            .hexpand(true)
            .build();
        name.add_css_class("metis-bar-tz-name");
        let time = gtk::Label::builder().build();
        time.add_css_class("metis-bar-tz-time");
        if let Ok(tz) = zone.parse::<Tz>() {
            let now: DateTime<Tz> = Utc::now().with_timezone(&tz);
            time.set_label(&now.format("%I:%M %p").to_string());
        } else {
            time.set_label("—");
        }
        row.append(&name);
        row.append(&time);
        box_.append(&row);
    }
}
