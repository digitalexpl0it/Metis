mod alarms;
mod calendar;
mod stopwatch;
mod timer;
mod world;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration as StdDuration;

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, NaiveTime, TimeZone, Timelike};
use gtk::prelude::*;

use crate::config::{alarm_sound_canberra_id, save_clocks_config, ClockConfig, ClocksConfig};
use crate::services::{spawn_calendar_service, CalCommand, CalendarEvent, LocalEvent};

use alarms::AlarmsPage;
use calendar::{CalendarPage, CreateRequest, EventView};
use stopwatch::StopwatchPage;
use timer::TimerPage;
use world::WorldClocksPage;

/// Shared, persisted clock state (world clocks + alarms) used by the popover pages.
#[derive(Clone)]
pub(crate) struct Store(Rc<RefCell<ClocksConfig>>);

impl Store {
    fn save(&self) {
        let _ = save_clocks_config(&self.0.borrow());
    }
    fn borrow(&self) -> std::cell::Ref<'_, ClocksConfig> {
        self.0.borrow()
    }
    fn borrow_mut(&self) -> std::cell::RefMut<'_, ClocksConfig> {
        self.0.borrow_mut()
    }
}

/// Raise a notification in the bar's own notification popup (bell icon). This
/// keeps timer/alarm/calendar alerts inside Metis instead of relying on an
/// external `notify-send` daemon.
pub(crate) fn notify(title: &str, body: &str) {
    crate::services::push_notification(crate::services::BarNotification {
        kind: crate::services::NotificationKind::Notification,
        title: title.to_string(),
        message: body.to_string(),
    });
}

/// Best-effort alarm sound; degrades silently if no player/sound is present.
pub(crate) fn play_alarm_sound() {
    play_alarm_sound_id("alarm-clock-elapsed");
}

/// Play a specific libcanberra event sound, falling back to a bundled oga file.
pub(crate) fn play_alarm_sound_id(canberra_id: &str) {
    if std::process::Command::new("canberra-gtk-play")
        .args(["-i", canberra_id])
        .spawn()
        .is_ok()
    {
        return;
    }
    let _ = std::process::Command::new("paplay")
        .arg("/usr/share/sounds/freedesktop/stereo/alarm-clock-elapsed.oga")
        .spawn();
}

pub struct ClockWidget {
    root: gtk::Button,
}

impl ClockWidget {
    pub fn new(config: &ClockConfig) -> Self {
        // Plain Button + non-autohide popover (same proven pattern as the volume
        // and notification widgets). A MenuButton's autohide popover tears itself
        // down when it holds complex/interactive content on our layer-shell bar.
        let root = gtk::Button::builder().build();
        root.add_css_class("metis-bar-widget");
        root.add_css_class("metis-bar-clock");

        let row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .build();
        let time_label = gtk::Label::new(None);
        time_label.add_css_class("metis-bar-clock-time");
        let date_label = gtk::Label::new(None);
        date_label.add_css_class("metis-bar-clock-date");
        row.append(&time_label);
        row.append(&date_label);
        root.set_child(Some(&row));

        let store = Store(Rc::new(RefCell::new(crate::config::load_clocks_config(
            &config.timezones,
        ))));

        // ---- Popover content: pill-tabbed stack ----
        let panel = super::super::dropdown::build_panel();
        panel.set_spacing(10);

        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .transition_duration(150)
            .build();

        let calendar = CalendarPage::new();
        let world = WorldClocksPage::new(store.clone());
        let stopwatch = StopwatchPage::new();
        let timer = TimerPage::new();
        let alarms_page = AlarmsPage::new(store.clone());

        stack.add_named(&calendar.widget, Some("calendar"));
        stack.add_named(&world.widget, Some("clocks"));
        stack.add_named(&stopwatch.widget, Some("stopwatch"));
        stack.add_named(&timer.widget, Some("timer"));
        stack.add_named(&alarms_page.widget, Some("alarms"));

        // ---- Calendar event service wiring ----
        // Account management moved to the settings app (Calendars page); the clock
        // popover keeps only the calendar/clock/stopwatch/timer/alarm views.
        let (cal_tx, cal_rx) = spawn_calendar_service();

        let switcher = build_pill_switcher(
            &stack,
            &[
                ("calendar", "Calendar", "x-office-calendar-symbolic"),
                ("clocks", "World Clocks", "preferences-system-time-symbolic"),
                ("stopwatch", "Stopwatch", "media-playback-start-symbolic"),
                ("timer", "Timer", "alarm-symbolic"),
                ("alarms", "Alarms", "alarm-symbolic"),
            ],
        );
        {
            let tx = cal_tx.clone();
            calendar.set_on_month_change(move |a, b| {
                let _ = tx.send(CalCommand::SetRange {
                    since: day_start(a),
                    until: day_end(b),
                });
            });
        }
        {
            let tx = cal_tx.clone();
            calendar.set_on_dismiss(move |ev| {
                let _ = tx.send(CalCommand::Dismiss(ev.uid.clone()));
            });
        }
        {
            let tx = cal_tx.clone();
            calendar.set_on_delete(move |ev| {
                let _ = tx.send(CalCommand::Delete(ev.uid.clone()));
            });
        }
        {
            let tx = cal_tx.clone();
            calendar.set_on_create(move |req| {
                let _ = tx.send(CalCommand::AddLocal(local_event_from(req)));
            });
        }
        {
            let tx = cal_tx.clone();
            calendar.set_on_refresh(move || {
                let _ = tx.send(CalCommand::Refresh);
            });
        }
        {
            let (a, b) = calendar.visible_range();
            let _ = cal_tx.send(CalCommand::SetRange {
                since: day_start(a),
                until: day_end(b),
            });
        }
        glib::timeout_add_local(StdDuration::from_millis(500), move || {
            let mut latest = None;
            while let Ok(events) = cal_rx.try_recv() {
                latest = Some(events);
            }
            if let Some(events) = latest {
                let views: Vec<EventView> = events.iter().map(event_to_view).collect();
                calendar.set_events(views);
            }
            glib::ControlFlow::Continue
        });

        panel.append(&switcher);
        // Hard caps on the popover size. `max_content_width` keeps the popover
        // narrower than the (≈1280px) compositor output even if a page reports a
        // large natural width — without this, an over-wide page is repeatedly
        // clamped by the compositor and GTK tears the popup down and recreates
        // it in a tight loop (the "surface missing from known popups" flicker).
        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .max_content_width(1040)
            .max_content_height(560)
            .propagate_natural_height(true)
            .propagate_natural_width(true)
            .child(&stack)
            .build();
        panel.append(&scroller);

        // Grab-based popover driven by the MenuButton. `autohide(true)` makes GTK
        // request the xdg_popup grab BEFORE mapping (the protocol-correct order),
        // which our compositor now honors so text entries inside the popover work.
        // Non-autohide popover toggled by the button click, dismissed via the
        // compositor "close-popovers" signal. Refresh calendar events on open.
        let refresh_tx = cal_tx.clone();
        super::super::dropdown::wire_toggle_prepare(&root, &panel, move || {
            let _ = refresh_tx.send(CalCommand::Refresh);
        });

        let widget = Self { root };
        widget.refresh_bar_labels(config, &time_label, &date_label);

        // ---- Per-second tick: bar labels, world times, alarm scheduling ----
        let cfg = config.clone();
        let last_minute = Rc::new(std::cell::Cell::new(Local::now().timestamp() / 60));
        glib::timeout_add_local(std::time::Duration::from_secs(1), move || {
            update_bar_labels(&time_label, &date_label, &cfg);
            world.refresh();

            let minute = Local::now().timestamp() / 60;
            if minute != last_minute.get() {
                last_minute.set(minute);
                check_alarms(&store);
            }
            glib::ControlFlow::Continue
        });

        widget
    }

    pub fn root(&self) -> &gtk::Button {
        &self.root
    }

    fn refresh_bar_labels(
        &self,
        config: &ClockConfig,
        time_label: &gtk::Label,
        date_label: &gtk::Label,
    ) {
        update_bar_labels(time_label, date_label, config);
    }
}

fn update_bar_labels(time_label: &gtk::Label, date_label: &gtk::Label, config: &ClockConfig) {
    let now = Local::now();
    time_label.set_label(&now.format(&config.time_format).to_string());
    date_label.set_label(&now.format(&config.date_format).to_string());
}

fn day_start(date: NaiveDate) -> DateTime<Local> {
    date.and_hms_opt(0, 0, 0)
        .and_then(|naive| Local.from_local_datetime(&naive).single())
        .unwrap_or_else(Local::now)
}

fn day_end(date: NaiveDate) -> DateTime<Local> {
    date.and_hms_opt(23, 59, 59)
        .and_then(|naive| Local.from_local_datetime(&naive).single())
        .unwrap_or_else(Local::now)
}

fn event_to_view(event: &CalendarEvent) -> EventView {
    let start_date = event.start.date_naive();
    let end_date = if event.all_day {
        (event.end - Duration::days(1)).date_naive().max(start_date)
    } else {
        event.end.date_naive()
    };
    EventView {
        uid: event.id.clone(),
        date: start_date,
        end_date,
        start: if event.all_day {
            None
        } else {
            Some(event.start.time())
        },
        all_day: event.all_day,
        title: event.summary.clone(),
        location: event.location.clone(),
        color: event.color.clone(),
        can_delete: event.can_delete,
    }
}

fn local_event_from(req: CreateRequest) -> LocalEvent {
    let start = if req.all_day {
        day_start(req.date)
    } else {
        let time = req
            .start
            .unwrap_or_else(|| NaiveTime::from_hms_opt(9, 0, 0).unwrap_or_default());
        Local
            .from_local_datetime(&req.date.and_time(time))
            .single()
            .unwrap_or_else(Local::now)
    };
    let end = if req.all_day {
        start + Duration::days(1)
    } else {
        start + Duration::hours(1)
    };
    LocalEvent {
        id: new_event_id(),
        summary: req.title,
        start,
        end,
        all_day: req.all_day,
        location: None,
    }
}

fn new_event_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("evt-{nanos}")
}

fn check_alarms(store: &Store) {
    let now = Local::now();
    let weekday = now.weekday().num_days_from_sunday() as u8;
    let hour = now.hour() as u8;
    let minute = now.minute() as u8;
    let due: Vec<(String, &'static str)> = store
        .borrow()
        .alarms
        .iter()
        .filter(|a| a.enabled && a.hour == hour && a.minute == minute)
        .filter(|a| a.days.is_empty() || a.days.contains(&weekday))
        .map(|a| {
            let label = if a.label.is_empty() {
                format!("Alarm {:02}:{:02}", a.hour, a.minute)
            } else {
                a.label.clone()
            };
            (label, alarm_sound_canberra_id(a.sound.as_deref()))
        })
        .collect();
    for (label, sound) in due {
        notify(&label, "Metis alarm");
        play_alarm_sound_id(sound);
    }
}

/// Build a horizontal strip of linked pill toggle buttons (icon + label) that
/// switch the given `stack`. The first entry starts selected.
fn build_pill_switcher(stack: &gtk::Stack, tabs: &[(&str, &str, &str)]) -> gtk::Box {
    let bar = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .halign(gtk::Align::Center)
        .build();
    bar.add_css_class("metis-clock-tabs");

    let mut group: Option<gtk::ToggleButton> = None;
    for (i, (name, title, icon)) in tabs.iter().enumerate() {
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        content.set_halign(gtk::Align::Center);
        let image = gtk::Image::from_icon_name(icon);
        let label = gtk::Label::new(Some(title));
        content.append(&image);
        content.append(&label);

        let pill = gtk::ToggleButton::builder().child(&content).build();
        pill.add_css_class("metis-clock-tab");
        if let Some(ref leader) = group {
            pill.set_group(Some(leader));
        } else {
            group = Some(pill.clone());
        }
        if i == 0 {
            pill.set_active(true);
        }
        {
            let stack = stack.clone();
            let name = name.to_string();
            pill.connect_toggled(move |b| {
                if b.is_active() {
                    stack.set_visible_child_name(&name);
                }
            });
        }
        bar.append(&pill);
    }
    bar
}
