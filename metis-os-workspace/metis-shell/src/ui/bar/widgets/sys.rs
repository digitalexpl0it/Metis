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

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// One labelled row: a mute icon-button on the left, a slider filling the rest.
struct AudioRow {
    scale: gtk::Scale,
    mute_icon: gtk::Image,
    percent: Rc<Cell<u8>>,
    muted: Rc<Cell<bool>>,
}

pub struct VolumeWidget {
    root: gtk::Button,
    icon: gtk::Image,
    output: AudioRow,
    input: AudioRow,
    updating: Rc<Cell<bool>>,
    suppress_until: Rc<Cell<Instant>>,
    last_out: Cell<(u8, bool)>,
    last_in: Cell<(u8, bool)>,
}

/// Hold off poller-driven updates briefly after a user action so optimistic UI
/// state isn't reverted by the lagging pactl read-back (fixes the mute flicker).
fn bump_suppress(cell: &Rc<Cell<Instant>>) {
    cell.set(Instant::now() + Duration::from_millis(700));
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
        panel.set_spacing(12);
        panel.set_width_request(260);

        let title = gtk::Label::builder()
            .label("Audio")
            .halign(gtk::Align::Start)
            .build();
        title.add_css_class("metis-bar-section-title");
        panel.append(&title);

        let updating = Rc::new(Cell::new(false));
        let suppress_until = Rc::new(Cell::new(Instant::now()));

        let output = build_audio_row(
            &panel,
            AudioKind::Output,
            &updating,
            &suppress_until,
        );
        let input = build_audio_row(
            &panel,
            AudioKind::Input,
            &updating,
            &suppress_until,
        );

        super::super::dropdown::wire_toggle(&root, &panel, "volume");

        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
        {
            let suppress_until = suppress_until.clone();
            scroll.connect_scroll(move |_, _, dy| {
                let delta = if dy < 0.0 { 5i8 } else { -5i8 };
                bump_suppress(&suppress_until);
                crate::services::set_volume_relative(delta);
                glib::Propagation::Stop
            });
        }
        root.add_controller(scroll);

        Self {
            root,
            icon,
            output,
            input,
            updating,
            suppress_until,
            last_out: Cell::new((255, false)),
            last_in: Cell::new((255, false)),
        }
    }

    pub fn root(&self) -> &gtk::Button {
        &self.root
    }

    pub fn update(&self, percent: u8, muted: bool, mic_percent: u8, mic_muted: bool) {
        // Don't let the poller stomp optimistic UI right after a user action.
        if Instant::now() < self.suppress_until.get() {
            return;
        }

        if self.last_out.get() != (percent, muted) {
            self.last_out.set((percent, muted));
            self.output.percent.set(percent);
            self.output.muted.set(muted);
            self.root
                .set_tooltip_text(Some(&format!("Volume {percent}%")));
            self.updating.set(true);
            self.output
                .scale
                .set_value(f64::from(if muted { 0 } else { percent }));
            self.updating.set(false);
            icons::set_icon(&self.icon, names::volume(percent, muted));
            icons::set_icon(&self.output.mute_icon, names::volume(percent, muted));
        }

        if self.last_in.get() != (mic_percent, mic_muted) {
            self.last_in.set((mic_percent, mic_muted));
            self.input.percent.set(mic_percent);
            self.input.muted.set(mic_muted);
            self.updating.set(true);
            self.input
                .scale
                .set_value(f64::from(if mic_muted { 0 } else { mic_percent }));
            self.updating.set(false);
            icons::set_icon(&self.input.mute_icon, names::mic(mic_percent, mic_muted));
        }
    }
}

#[derive(Clone, Copy)]
enum AudioKind {
    Output,
    Input,
}

fn build_audio_row(
    panel: &gtk::Box,
    kind: AudioKind,
    updating: &Rc<Cell<bool>>,
    suppress_until: &Rc<Cell<Instant>>,
) -> AudioRow {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .build();

    let percent = Rc::new(Cell::new(0u8));
    let muted = Rc::new(Cell::new(false));

    let initial_icon = match kind {
        AudioKind::Output => names::volume(50, false),
        AudioKind::Input => names::mic(50, false),
    };
    let mute_icon = icons::image(initial_icon);

    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
    scale.set_draw_value(false);
    scale.set_hexpand(true);
    scale.add_css_class("metis-bar-volume-scale");

    let set_mute_icon = {
        let mute_icon = mute_icon.clone();
        move |pct: u8, muted: bool| {
            let name = match kind {
                AudioKind::Output => names::volume(pct, muted),
                AudioKind::Input => names::mic(pct, muted),
            };
            icons::set_icon(&mute_icon, name);
        }
    };

    let mute_btn = gtk::Button::builder().build();
    mute_btn.add_css_class("metis-bar-audio-mute");
    mute_btn.set_child(Some(&mute_icon));
    mute_btn.set_valign(gtk::Align::Center);
    {
        let muted = muted.clone();
        let percent = percent.clone();
        let suppress_until = suppress_until.clone();
        let updating = updating.clone();
        let scale = scale.clone();
        let set_mute_icon = set_mute_icon.clone();
        mute_btn.connect_clicked(move |_| {
            let new_muted = !muted.get();
            muted.set(new_muted);
            bump_suppress(&suppress_until);
            set_mute_icon(percent.get(), new_muted);
            // Reflect mute on the slider immediately (poller is suppressed now).
            updating.set(true);
            scale.set_value(f64::from(if new_muted { 0 } else { percent.get() }));
            updating.set(false);
            match kind {
                AudioKind::Output => crate::services::set_mute(new_muted),
                AudioKind::Input => crate::services::set_mic_mute(new_muted),
            }
        });
    }
    row.append(&mute_btn);

    {
        let updating = updating.clone();
        let suppress_until = suppress_until.clone();
        let percent = percent.clone();
        let muted = muted.clone();
        let set_mute_icon = set_mute_icon.clone();
        scale.connect_value_changed(move |scale| {
            if updating.get() {
                return;
            }
            let pct = scale.value().round() as u8;
            percent.set(pct);
            bump_suppress(&suppress_until);
            // Dragging the slider implies the user wants sound: unmute.
            if muted.get() {
                muted.set(false);
                set_mute_icon(pct, false);
                match kind {
                    AudioKind::Output => crate::services::set_mute(false),
                    AudioKind::Input => crate::services::set_mic_mute(false),
                }
            } else {
                set_mute_icon(pct, false);
            }
            match kind {
                AudioKind::Output => crate::services::set_volume_absolute(pct),
                AudioKind::Input => crate::services::set_mic_volume_absolute(pct),
            }
        });
    }
    row.append(&scale);

    panel.append(&row);

    AudioRow {
        scale,
        mute_icon,
        percent,
        muted,
    }
}
