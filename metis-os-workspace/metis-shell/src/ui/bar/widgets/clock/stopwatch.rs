use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk::prelude::*;

pub struct StopwatchPage {
    pub widget: gtk::Widget,
}

struct Inner {
    running: Cell<bool>,
    accumulated: Cell<Duration>,
    start: Cell<Option<Instant>>,
    generation: Cell<u64>,
    last_lap: Cell<Duration>,
    lap_count: Cell<u32>,
    label: gtk::Label,
    laps: gtk::Box,
    primary: gtk::Button,
    secondary: gtk::Button,
}

impl StopwatchPage {
    pub fn new() -> Self {
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .halign(gtk::Align::Fill)
            .hexpand(true)
            .build();
        root.add_css_class("metis-sw-page");
        root.set_width_request(-1);

        let label = gtk::Label::new(Some("00:00:00.0"));
        label.add_css_class("metis-sw-digits");
        label.set_halign(gtk::Align::Center);
        root.append(&label);

        let controls = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(16)
            .halign(gtk::Align::Center)
            .build();
        let primary = gtk::Button::with_label(&metis_i18n::tr("Start"));
        primary.add_css_class("metis-sw-btn");
        primary.add_css_class("metis-sw-btn-go");
        let secondary = gtk::Button::with_label(&metis_i18n::tr("Lap"));
        secondary.add_css_class("metis-sw-btn");
        secondary.add_css_class("metis-sw-btn-stop");
        secondary.set_sensitive(false);
        controls.append(&primary);
        controls.append(&secondary);
        root.append(&controls);

        let laps = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(2)
            .build();
        laps.add_css_class("metis-sw-laps");
        let laps_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .min_content_height(0)
            .max_content_height(200)
            .propagate_natural_height(true)
            .overlay_scrolling(false)
            .child(&laps)
            .build();
        laps_scroll.add_css_class("metis-sw-laps-scroll");
        root.append(&laps_scroll);

        let inner = Rc::new(Inner {
            running: Cell::new(false),
            accumulated: Cell::new(Duration::ZERO),
            start: Cell::new(None),
            generation: Cell::new(0),
            last_lap: Cell::new(Duration::ZERO),
            lap_count: Cell::new(0),
            label,
            laps,
            primary: primary.clone(),
            secondary: secondary.clone(),
        });

        {
            let inner = inner.clone();
            primary.connect_clicked(move |_| {
                if inner.running.get() {
                    inner.pause();
                } else {
                    inner.resume();
                }
            });
        }
        {
            let inner = inner.clone();
            secondary.connect_clicked(move |_| {
                if inner.running.get() {
                    inner.lap();
                } else {
                    inner.reset();
                }
            });
        }

        Self {
            widget: root.upcast(),
        }
    }
}

impl Inner {
    fn elapsed(&self) -> Duration {
        let mut total = self.accumulated.get();
        if let Some(start) = self.start.get() {
            total += start.elapsed();
        }
        total
    }

    fn resume(self: &Rc<Self>) {
        self.running.set(true);
        self.start.set(Some(Instant::now()));
        self.primary.set_label(&metis_i18n::tr("Pause"));
        self.secondary.set_label(&metis_i18n::tr("Lap"));
        self.secondary.set_sensitive(true);

        let generation = self.generation.get();
        let inner = self.clone();
        glib::timeout_add_local(Duration::from_millis(50), move || {
            if inner.generation.get() != generation {
                return glib::ControlFlow::Break;
            }
            inner.label.set_label(&fmt(inner.elapsed()));
            glib::ControlFlow::Continue
        });
    }

    fn pause(&self) {
        self.running.set(false);
        self.generation.set(self.generation.get().wrapping_add(1));
        if let Some(start) = self.start.take() {
            self.accumulated.set(self.accumulated.get() + start.elapsed());
        }
        self.label.set_label(&fmt(self.accumulated.get()));
        self.primary.set_label(&metis_i18n::tr("Resume"));
        self.secondary.set_label(&metis_i18n::tr("Reset"));
    }

    fn reset(&self) {
        self.running.set(false);
        self.generation.set(self.generation.get().wrapping_add(1));
        self.start.set(None);
        self.accumulated.set(Duration::ZERO);
        self.last_lap.set(Duration::ZERO);
        self.lap_count.set(0);
        self.label.set_label("00:00:00.0");
        self.primary.set_label(&metis_i18n::tr("Start"));
        self.secondary.set_label(&metis_i18n::tr("Lap"));
        self.secondary.set_sensitive(false);
        while let Some(child) = self.laps.first_child() {
            self.laps.remove(&child);
        }
    }

    fn lap(&self) {
        let total = self.elapsed();
        let delta = total.saturating_sub(self.last_lap.get());
        self.last_lap.set(total);
        let n = self.lap_count.get() + 1;
        self.lap_count.set(n);

        let row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(10)
            .build();
        row.add_css_class("metis-sw-lap");

        let total_lbl = gtk::Label::builder().label(&fmt(total)).halign(gtk::Align::Start).build();
        total_lbl.add_css_class("metis-sw-lap-total");
        let delta_lbl = gtk::Label::builder()
            .label(&format!("+{}", fmt(delta)))
            .halign(gtk::Align::Start)
            .hexpand(true)
            .build();
        delta_lbl.add_css_class("metis-sw-lap-delta");
        let name = gtk::Label::builder()
            .label(metis_i18n::tr("Lap %1").replace("%1", &n.to_string()))
            .halign(gtk::Align::End)
            .build();
        name.add_css_class("metis-sw-lap-name");
        row.append(&total_lbl);
        row.append(&delta_lbl);
        row.append(&name);
        self.laps.prepend(&row);
    }
}

fn fmt(d: Duration) -> String {
    let total = d.as_secs_f64();
    let h = (total / 3600.0).floor() as u64;
    let m = ((total % 3600.0) / 60.0).floor() as u64;
    let s = total % 60.0;
    format!("{h:02}:{m:02}:{:04.1}", s)
}
