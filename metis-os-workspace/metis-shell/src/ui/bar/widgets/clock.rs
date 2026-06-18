use chrono::{DateTime, Datelike, Local, Months, Utc};
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
        panel.set_width_request(320);

        // Custom, non-grabbing calendar. GtkCalendar requests an xdg_popup grab
        // that our layer-parented popup can't satisfy, which locks the bar, so we
        // render a plain label grid with month navigation instead.
        let calendar = build_calendar();
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

fn build_calendar() -> gtk::Widget {
    let container = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .build();
    container.add_css_class("metis-bar-calendar");

    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .build();
    let prev = gtk::Button::builder().label("\u{2039}").build();
    prev.add_css_class("metis-cal-nav");
    let title = gtk::Label::builder().hexpand(true).build();
    title.add_css_class("metis-cal-title");
    let next = gtk::Button::builder().label("\u{203A}").build();
    next.add_css_class("metis-cal-nav");
    header.append(&prev);
    header.append(&title);
    header.append(&next);
    container.append(&header);

    let grid = gtk::Grid::builder()
        .row_spacing(2)
        .column_spacing(2)
        .column_homogeneous(true)
        .build();
    container.append(&grid);

    // Anchored to the first day of the currently displayed month.
    let shown = std::rc::Rc::new(std::cell::RefCell::new(
        Local::now()
            .date_naive()
            .with_day(1)
            .unwrap_or_else(|| Local::now().date_naive()),
    ));

    let rebuild: std::rc::Rc<dyn Fn()> = {
        let grid = grid.clone();
        let title = title.clone();
        let shown = shown.clone();
        std::rc::Rc::new(move || {
            while let Some(child) = grid.first_child() {
                grid.remove(&child);
            }

            let anchor = *shown.borrow();
            let today = Local::now().date_naive();
            title.set_label(&anchor.format("%B %Y").to_string());

            for (i, wd) in ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"]
                .iter()
                .enumerate()
            {
                let label = gtk::Label::new(Some(wd));
                label.add_css_class("metis-cal-weekday");
                grid.attach(&label, i as i32, 0, 1, 1);
            }

            let first_col = anchor.weekday().num_days_from_sunday() as i32;
            let days_in_month = anchor
                .checked_add_months(Months::new(1))
                .map(|next| next.signed_duration_since(anchor).num_days() as u32)
                .unwrap_or(30);

            let mut col = first_col;
            let mut row = 1;
            for day in 1..=days_in_month {
                let label = gtk::Label::new(Some(&day.to_string()));
                label.add_css_class("metis-cal-day");
                if anchor.with_day(day) == Some(today) {
                    label.add_css_class("metis-cal-today");
                }
                grid.attach(&label, col, row, 1, 1);
                col += 1;
                if col > 6 {
                    col = 0;
                    row += 1;
                }
            }
        })
    };

    {
        let rebuild = rebuild.clone();
        let shown = shown.clone();
        prev.connect_clicked(move |_| {
            let current = *shown.borrow();
            if let Some(prev_month) = current.checked_sub_months(Months::new(1)) {
                *shown.borrow_mut() = prev_month;
                rebuild();
            }
        });
    }
    {
        let rebuild = rebuild.clone();
        let shown = shown.clone();
        next.connect_clicked(move |_| {
            let current = *shown.borrow();
            if let Some(next_month) = current.checked_add_months(Months::new(1)) {
                *shown.borrow_mut() = next_month;
                rebuild();
            }
        });
    }

    rebuild();
    container.upcast()
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
