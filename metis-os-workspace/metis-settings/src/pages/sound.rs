//! Sound: output/input device pickers and volume readout via `pactl`.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use gtk::prelude::*;

use crate::sound::{self, SoundSnapshot};
use crate::ui;

struct Sections {
    output_combo: gtk::DropDown,
    input_combo: gtk::DropDown,
    output_vol: gtk::Label,
    input_vol: gtk::Label,
    sink_names: RefCell<Vec<String>>,
    source_names: RefCell<Vec<String>>,
    /// Suppress handler while programmatically updating combos.
    syncing: RefCell<bool>,
}

pub fn build() -> gtk::Widget {
    let (scroller, content) = ui::page("Sound");

    let (out_card, out_body) = ui::section("Output");
    let output_combo = gtk::DropDown::new(None::<gtk::StringList>, None::<&gtk::Expression>);
    out_body.append(&ui::row("Playback device", &output_combo));
    let output_vol = gtk::Label::new(None);
    output_vol.set_halign(gtk::Align::End);
    out_body.append(&ui::row("Volume", &output_vol));
    content.append(&out_card);

    let (in_card, in_body) = ui::section("Input");
    let input_combo = gtk::DropDown::new(None::<gtk::StringList>, None::<&gtk::Expression>);
    in_body.append(&ui::row("Recording device", &input_combo));
    let input_vol = gtk::Label::new(None);
    input_vol.set_halign(gtk::Align::End);
    in_body.append(&ui::row("Level", &input_vol));
    content.append(&in_card);

    let hint = gtk::Label::new(Some(
        "Use the volume icon on the edge bar for quick mute and level control.",
    ));
    hint.set_wrap(true);
    hint.set_xalign(0.0);
    hint.add_css_class("metis-settings-hint");
    content.append(&hint);

    let sections = Rc::new(Sections {
        output_combo,
        input_combo,
        output_vol,
        input_vol,
        sink_names: RefCell::new(Vec::new()),
        source_names: RefCell::new(Vec::new()),
        syncing: RefCell::new(false),
    });

    let (tx, rx) = mpsc::channel::<SoundSnapshot>();
    let refresh = {
        let tx = tx.clone();
        Rc::new(move || {
            let tx = tx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(sound::load_snapshot());
            });
        })
    };

    {
        let sections_poll = sections.clone();
        let refresh_poll = refresh.clone();
        glib::timeout_add_local(Duration::from_millis(200), move || {
            if let Ok(snap) = rx.try_recv() {
                render(&sections_poll, &snap);
            }
            glib::ControlFlow::Continue
        });
        sections.output_combo.connect_selected_notify({
            let sections = sections.clone();
            let refresh = refresh.clone();
            move |_| {
                if *sections.syncing.borrow() {
                    return;
                }
                let idx = sections.output_combo.selected() as usize;
                if let Some(name) = sections.sink_names.borrow().get(idx) {
                    sound::set_default_sink(name);
                    schedule_refresh(&refresh, 400);
                }
            }
        });
        sections.input_combo.connect_selected_notify({
            let sections = sections.clone();
            let refresh = refresh.clone();
            move |_| {
                if *sections.syncing.borrow() {
                    return;
                }
                let idx = sections.input_combo.selected() as usize;
                if let Some(name) = sections.source_names.borrow().get(idx) {
                    sound::set_default_source(name);
                    schedule_refresh(&refresh, 400);
                }
            }
        });
    }

    refresh();
    scroller.upcast()
}

fn render(sections: &Sections, snap: &SoundSnapshot) {
    *sections.syncing.borrow_mut() = true;
    fill_combo(
        &sections.output_combo,
        &snap.sinks,
        &sections.sink_names,
    );
    fill_combo(
        &sections.input_combo,
        &snap.sources,
        &sections.source_names,
    );
    *sections.syncing.borrow_mut() = false;
    sections.output_vol.set_text(&format!(
        "{}{}%",
        if snap.output_muted { "Muted · " } else { "" },
        snap.output_volume
    ));
    sections.input_vol.set_text(&format!(
        "{}{}%",
        if snap.input_muted { "Muted · " } else { "" },
        snap.input_volume
    ));
}

fn fill_combo(
    dd: &gtk::DropDown,
    devices: &[crate::sound::AudioDevice],
    names: &RefCell<Vec<String>>,
) {
    let list = gtk::StringList::new(&[] as &[&str]);
    let mut selected = 0u32;
    let mut stored = Vec::new();
    for (i, dev) in devices.iter().enumerate() {
        list.append(&dev.description);
        stored.push(dev.name.clone());
        if dev.is_default {
            selected = i as u32;
        }
    }
    *names.borrow_mut() = stored;
    dd.set_model(Some(&list));
    if !devices.is_empty() {
        dd.set_selected(selected);
    }
}

fn schedule_refresh(refresh: &Rc<impl Fn() + 'static>, delay_ms: u64) {
    let refresh = refresh.clone();
    glib::timeout_add_local_once(Duration::from_millis(delay_ms), move || refresh());
}
