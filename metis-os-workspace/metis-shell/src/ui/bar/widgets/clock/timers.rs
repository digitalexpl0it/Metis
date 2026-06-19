use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk::prelude::*;

use crate::config::Alarm;

use super::{notify, play_alarm_sound, Store};

pub struct TimersPage {
    pub widget: gtk::Widget,
}

impl TimersPage {
    pub fn new(store: Store) -> Self {
        let columns = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(18)
            .homogeneous(false)
            .build();

        columns.append(&build_stopwatch());
        columns.append(&gtk::Separator::new(gtk::Orientation::Vertical));
        columns.append(&build_timer());
        columns.append(&gtk::Separator::new(gtk::Orientation::Vertical));

        let alarms = AlarmsInner::new(store);
        columns.append(&alarms.widget);

        Self {
            widget: columns.upcast(),
        }
    }
}

fn section(title: &str) -> (gtk::Box, gtk::Box) {
    let col = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .build();
    let label = gtk::Label::builder().label(title).halign(gtk::Align::Start).build();
    label.add_css_class("metis-bar-section-title");
    col.append(&label);
    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .vexpand(true)
        .build();
    col.append(&body);
    (col, body)
}

// ---------------------------------------------------------------- Stopwatch

struct StopwatchInner {
    running: Cell<bool>,
    accumulated: Cell<Duration>,
    start: Cell<Option<Instant>>,
    label: gtk::Label,
    generation: Cell<u64>,
}

fn build_stopwatch() -> gtk::Box {
    let (col, body) = section("Stopwatch");

    let label = gtk::Label::new(Some("00:00.0"));
    label.add_css_class("metis-clock-digits");
    label.set_halign(gtk::Align::Center);
    body.append(&label);

    let controls = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .halign(gtk::Align::Center)
        .build();
    let start_btn = gtk::Button::with_label("Start");
    start_btn.add_css_class("metis-clock-btn");
    let lap_btn = gtk::Button::with_label("Lap");
    lap_btn.add_css_class("metis-clock-btn");
    let reset_btn = gtk::Button::with_label("Reset");
    reset_btn.add_css_class("metis-clock-btn");
    controls.append(&start_btn);
    controls.append(&lap_btn);
    controls.append(&reset_btn);
    body.append(&controls);

    let laps = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .build();
    body.append(&laps);

    let inner = Rc::new(StopwatchInner {
        running: Cell::new(false),
        accumulated: Cell::new(Duration::ZERO),
        start: Cell::new(None),
        label,
        generation: Cell::new(0),
    });

    {
        let inner = inner.clone();
        let start_btn = start_btn.clone();
        start_btn.connect_clicked(move |btn| {
            if inner.running.get() {
                inner.pause();
                btn.set_label("Start");
            } else {
                inner.resume();
                btn.set_label("Pause");
            }
        });
    }
    {
        let inner = inner.clone();
        let count = Rc::new(Cell::new(0u32));
        let laps = laps.clone();
        lap_btn.connect_clicked(move |_| {
            count.set(count.get() + 1);
            let row = gtk::Label::builder()
                .label(&format!(
                    "Lap {}   {}",
                    count.get(),
                    fmt_stopwatch(inner.elapsed())
                ))
                .halign(gtk::Align::Start)
                .build();
            row.add_css_class("metis-clock-lap");
            laps.append(&row);
        });
    }
    {
        let inner = inner.clone();
        let start_btn = start_btn.clone();
        reset_btn.connect_clicked(move |_| {
            inner.reset();
            start_btn.set_label("Start");
            while let Some(child) = laps.first_child() {
                laps.remove(&child);
            }
        });
    }

    col
}

impl StopwatchInner {
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
        let generation = self.generation.get();
        let inner = self.clone();
        glib::timeout_add_local(Duration::from_millis(100), move || {
            if inner.generation.get() != generation {
                return glib::ControlFlow::Break;
            }
            inner.label.set_label(&fmt_stopwatch(inner.elapsed()));
            glib::ControlFlow::Continue
        });
    }

    fn pause(&self) {
        self.running.set(false);
        self.generation.set(self.generation.get().wrapping_add(1));
        if let Some(start) = self.start.take() {
            self.accumulated.set(self.accumulated.get() + start.elapsed());
        }
        self.label.set_label(&fmt_stopwatch(self.accumulated.get()));
    }

    fn reset(&self) {
        self.running.set(false);
        self.generation.set(self.generation.get().wrapping_add(1));
        self.start.set(None);
        self.accumulated.set(Duration::ZERO);
        self.label.set_label("00:00.0");
    }
}

fn fmt_stopwatch(d: Duration) -> String {
    let total = d.as_secs_f64();
    let mins = (total / 60.0).floor() as u64;
    let secs = total - (mins as f64) * 60.0;
    if mins >= 60 {
        let h = mins / 60;
        let m = mins % 60;
        format!("{h}:{m:02}:{:04.1}", secs)
    } else {
        format!("{mins:02}:{:04.1}", secs)
    }
}

// -------------------------------------------------------------------- Timer

struct TimerInner {
    end: Cell<Option<Instant>>,
    remaining: Cell<Duration>,
    label: gtk::Label,
    min: gtk::SpinButton,
    sec: gtk::SpinButton,
    generation: Cell<u64>,
}

fn build_timer() -> gtk::Box {
    let (col, body) = section("Timer");

    let label = gtk::Label::new(Some("00:00"));
    label.add_css_class("metis-clock-digits");
    label.set_halign(gtk::Align::Center);
    body.append(&label);

    let spins = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .halign(gtk::Align::Center)
        .build();
    let min = gtk::SpinButton::with_range(0.0, 999.0, 1.0);
    min.set_value(5.0);
    let sec = gtk::SpinButton::with_range(0.0, 59.0, 1.0);
    spins.append(&gtk::Label::new(Some("min")));
    spins.append(&min);
    spins.append(&gtk::Label::new(Some("sec")));
    spins.append(&sec);
    body.append(&spins);

    let controls = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .halign(gtk::Align::Center)
        .build();
    let start_btn = gtk::Button::with_label("Start");
    start_btn.add_css_class("metis-clock-btn");
    let reset_btn = gtk::Button::with_label("Reset");
    reset_btn.add_css_class("metis-clock-btn");
    controls.append(&start_btn);
    controls.append(&reset_btn);
    body.append(&controls);

    let inner = Rc::new(TimerInner {
        end: Cell::new(None),
        remaining: Cell::new(Duration::ZERO),
        label,
        min,
        sec,
        generation: Cell::new(0),
    });

    {
        let inner = inner.clone();
        let start_btn = start_btn.clone();
        start_btn.connect_clicked(move |btn| {
            if inner.end.get().is_some() {
                inner.pause();
                btn.set_label("Start");
            } else if inner.start() {
                btn.set_label("Pause");
            }
        });
    }
    {
        let inner = inner.clone();
        let start_btn = start_btn.clone();
        reset_btn.connect_clicked(move |_| {
            inner.reset();
            start_btn.set_label("Start");
        });
    }

    col
}

impl TimerInner {
    fn start(self: &Rc<Self>) -> bool {
        let remaining = if self.remaining.get() > Duration::ZERO {
            self.remaining.get()
        } else {
            let secs = self.min.value() as u64 * 60 + self.sec.value() as u64;
            Duration::from_secs(secs)
        };
        if remaining.is_zero() {
            return false;
        }
        self.end.set(Some(Instant::now() + remaining));
        let generation = self.generation.get();
        let inner = self.clone();
        glib::timeout_add_local(Duration::from_millis(200), move || {
            if inner.generation.get() != generation {
                return glib::ControlFlow::Break;
            }
            inner.tick()
        });
        true
    }

    fn tick(self: &Rc<Self>) -> glib::ControlFlow {
        let Some(end) = self.end.get() else {
            return glib::ControlFlow::Break;
        };
        let now = Instant::now();
        if now >= end {
            self.end.set(None);
            self.remaining.set(Duration::ZERO);
            self.generation.set(self.generation.get().wrapping_add(1));
            self.label.set_label("00:00");
            notify("Timer finished", "Your Metis timer is up.");
            play_alarm_sound();
            return glib::ControlFlow::Break;
        }
        let left = end - now;
        self.label.set_label(&fmt_timer(left));
        glib::ControlFlow::Continue
    }

    fn pause(&self) {
        self.generation.set(self.generation.get().wrapping_add(1));
        if let Some(end) = self.end.take() {
            self.remaining
                .set(end.saturating_duration_since(Instant::now()));
        }
    }

    fn reset(&self) {
        self.generation.set(self.generation.get().wrapping_add(1));
        self.end.set(None);
        self.remaining.set(Duration::ZERO);
        self.label.set_label("00:00");
    }
}

fn fmt_timer(d: Duration) -> String {
    let secs = d.as_secs() + if d.subsec_millis() > 0 { 1 } else { 0 };
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

// ------------------------------------------------------------------- Alarms

struct AlarmsInner {
    store: Store,
    widget: gtk::Box,
    list: gtk::Box,
}

impl AlarmsInner {
    fn new(store: Store) -> Rc<Self> {
        let (col, body) = section("Alarms");

        let add_row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(6)
            .build();
        let hour = gtk::SpinButton::with_range(0.0, 23.0, 1.0);
        let minute = gtk::SpinButton::with_range(0.0, 59.0, 1.0);
        let label = gtk::Entry::builder()
            .placeholder_text("Label")
            .width_chars(8)
            .max_width_chars(10)
            .hexpand(true)
            .build();
        let add = gtk::Button::with_label("Add");
        add.add_css_class("metis-cal-add-btn");
        add_row.append(&hour);
        add_row.append(&gtk::Label::new(Some(":")));
        add_row.append(&minute);
        add_row.append(&label);
        add_row.append(&add);
        body.append(&add_row);

        let list = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .build();
        body.append(&list);

        let inner = Rc::new(Self {
            store,
            widget: col,
            list,
        });

        {
            let inner = inner.clone();
            let hour = hour.clone();
            let minute = minute.clone();
            let label = label.clone();
            add.connect_clicked(move |_| {
                let alarm = Alarm {
                    id: new_id(),
                    hour: hour.value() as u8,
                    minute: minute.value() as u8,
                    label: label.text().to_string(),
                    enabled: true,
                    days: Vec::new(),
                };
                inner.store.borrow_mut().alarms.push(alarm);
                inner.store.save();
                label.set_text("");
                inner.rebuild();
            });
        }

        inner.rebuild();
        inner
    }

    fn rebuild(self: &Rc<Self>) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        let alarms = self.store.borrow().alarms.clone();
        if alarms.is_empty() {
            let empty = gtk::Label::builder().label("No alarms").build();
            empty.add_css_class("metis-cal-empty");
            self.list.append(&empty);
            return;
        }
        for alarm in alarms {
            let row = gtk::Box::builder()
                .orientation(gtk::Orientation::Horizontal)
                .spacing(8)
                .build();
            row.add_css_class("metis-clock-alarm");

            let time = gtk::Label::builder()
                .label(&format!("{:02}:{:02}", alarm.hour, alarm.minute))
                .build();
            time.add_css_class("metis-clock-alarm-time");
            row.append(&time);

            let text = gtk::Label::builder()
                .label(if alarm.label.is_empty() {
                    "Alarm"
                } else {
                    &alarm.label
                })
                .halign(gtk::Align::Start)
                .hexpand(true)
                .build();
            row.append(&text);

            let toggle = gtk::Switch::new();
            toggle.set_active(alarm.enabled);
            toggle.set_valign(gtk::Align::Center);
            {
                let inner = self.clone();
                let id = alarm.id.clone();
                toggle.connect_state_set(move |_, state| {
                    if let Some(a) = inner
                        .store
                        .borrow_mut()
                        .alarms
                        .iter_mut()
                        .find(|a| a.id == id)
                    {
                        a.enabled = state;
                    }
                    inner.store.save();
                    glib::Propagation::Proceed
                });
            }
            row.append(&toggle);

            let remove = gtk::Button::from_icon_name("window-close-symbolic");
            remove.add_css_class("metis-cal-event-action");
            remove.set_valign(gtk::Align::Center);
            {
                let inner = self.clone();
                let id = alarm.id.clone();
                remove.connect_clicked(move |_| {
                    inner.store.borrow_mut().alarms.retain(|a| a.id != id);
                    inner.store.save();
                    inner.rebuild();
                });
            }
            row.append(&remove);

            self.list.append(&row);
        }
    }
}

fn new_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("alarm-{nanos}")
}
