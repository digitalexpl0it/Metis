//! Display: arrangement canvas, per-output controls (`outputs.json`).

#[path = "display_arrangement.rs"]
mod display_arrangement;
#[path = "display_confirm.rs"]
mod display_confirm;

use std::cell::RefCell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use metis_config::{
    load_outputs_config, output_prefs, save_outputs_config, DisplayLayoutMode, OutputsConfig,
};
use metis_protocol::{OutputInfo, OutputModeInfo};

use crate::runtime;
use crate::ui;

use display_arrangement::ArrangementCanvas;

pub fn build(parent: &gtk::Window) -> gtk::Widget {
    let (scroller, content) = ui::page_for("display");

    let cfg = Rc::new(RefCell::new(load_outputs_config()));
    let outputs = Rc::new(RefCell::new(runtime::list_outputs()));
    let selected = Rc::new(RefCell::new(0_usize));
    let canvas_slot: Rc<RefCell<Option<Rc<ArrangementCanvas>>>> = Rc::new(RefCell::new(None));
    let display_dirty: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));

    let (global_card, global_body) = ui::section("Night light");
    let night = gtk::Switch::new();
    night.set_active(cfg.borrow().night_light_enabled);
    global_body.append(&ui::row("Enable night light", &night));
    let temp = gtk::SpinButton::with_range(2700.0, 6500.0, 100.0);
    temp.set_digits(0);
    temp.set_value(cfg.borrow().night_light_temperature as f64);
    global_body.append(&ui::row("Colour temperature (K)", &temp));
    let night_note = gtk::Label::new(Some(
        "Saved to outputs.json. Compositor colour shift is not wired yet.",
    ));
    night_note.set_wrap(true);
    night_note.set_xalign(0.0);
    night_note.add_css_class("metis-settings-hint");
    global_body.append(&night_note);
    content.append(&global_card);

    let (mode_card, mode_body) = ui::section("Display mode");
    let duplicate = gtk::Switch::new();
    duplicate.set_active(cfg.borrow().display_mode == DisplayLayoutMode::Mirror);
    mode_body.append(&ui::row("Duplicate displays", &duplicate));

    let source_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    source_row.set_visible(cfg.borrow().display_mode == DisplayLayoutMode::Mirror);
    let source_label = gtk::Label::new(Some("Show on"));
    source_label.set_xalign(0.0);
    source_row.append(&source_label);
    let source_dd = gtk::DropDown::new(
        Some(gtk::StringList::new(&[] as &[&str])),
        gtk::Expression::NONE,
    );
    source_dd.set_hexpand(true);
    source_row.append(&source_dd);
    mode_body.append(&source_row);

    let mirror_hint = gtk::Label::new(Some(
        "Duplicate displays requires a DRM session with two or more active monitors. \
         Nested dev sessions save this preference but the compositor stays in extend mode.",
    ));
    mirror_hint.set_wrap(true);
    mirror_hint.set_xalign(0.0);
    mirror_hint.add_css_class("metis-settings-hint");
    mode_body.append(&mirror_hint);
    content.append(&mode_card);

    let (arrange_card, arrange_body) = ui::section("Arrangement");
    content.append(&arrange_card);

    let detail_host = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.append(&detail_host);

    let save_display_btn = gtk::Button::with_label("Save display settings");
    save_display_btn.add_css_class("suggested-action");
    save_display_btn.set_sensitive(false);
    let revert_display_btn = gtk::Button::with_label("Revert");
    revert_display_btn.add_css_class("metis-settings-secondary");
    revert_display_btn.set_sensitive(false);

    let empty_hint = gtk::Label::new(Some(
        "Compositor not reachable — start a Metis session to configure displays.",
    ));
    empty_hint.set_wrap(true);
    empty_hint.set_xalign(0.0);
    empty_hint.add_css_class("metis-settings-hint");

    let refresh_save_buttons: Rc<dyn Fn()> = {
        let save_display_btn = save_display_btn.clone();
        let revert_display_btn = revert_display_btn.clone();
        let canvas_slot = canvas_slot.clone();
        let display_dirty = display_dirty.clone();
        Rc::new(move || {
            let trialing = canvas_slot
                .borrow()
                .as_ref()
                .is_some_and(|c| c.in_trial());
            let pending = *display_dirty.borrow()
                || canvas_slot
                    .borrow()
                    .as_ref()
                    .is_some_and(|c| c.has_pending());
            save_display_btn.set_sensitive(pending && !trialing);
            revert_display_btn.set_sensitive(pending && !trialing);
        })
    };

    let mark_display_dirty: Rc<dyn Fn()> = {
        let display_dirty = display_dirty.clone();
        let refresh_save_buttons = refresh_save_buttons.clone();
        Rc::new(move || {
            *display_dirty.borrow_mut() = true;
            refresh_save_buttons();
        })
    };

    let set_display_pending: Rc<dyn Fn(bool)> = {
        let display_dirty = display_dirty.clone();
        let refresh_save_buttons = refresh_save_buttons.clone();
        Rc::new(move |pending| {
            if pending {
                *display_dirty.borrow_mut() = true;
            }
            refresh_save_buttons();
        })
    };

    let refresh_source_dropdown: Rc<dyn Fn()> = {
        let cfg = cfg.clone();
        let outputs = outputs.clone();
        let source_dd = source_dd.clone();
        let source_row = source_row.clone();
        let duplicate = duplicate.clone();
        Rc::new(move || {
            let list = outputs.borrow();
            let enabled: Vec<&OutputInfo> = list.iter().filter(|o| o.enabled).collect();
            let mirror_on = cfg.borrow().display_mode == DisplayLayoutMode::Mirror;
            source_row.set_visible(mirror_on && enabled.len() >= 2);
            if !mirror_on || enabled.len() < 2 {
                return;
            }
            let labels: Vec<String> = enabled
                .iter()
                .enumerate()
                .map(|(i, o)| panel_title(o, i))
                .collect();
            let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
            let model = gtk::StringList::new(&refs);
            source_dd.set_model(Some(&model));
            let selected_name = cfg
                .borrow()
                .mirror_source
                .clone()
                .or_else(|| enabled.first().map(|o| o.name.clone()));
            if let Some(name) = selected_name {
                if let Some(idx) = enabled.iter().position(|o| o.name == name) {
                    let idx = idx as u32;
                    if source_dd.selected() != idx {
                        source_dd.set_selected(idx);
                    }
                }
            }
            duplicate.set_sensitive(enabled.len() >= 2);
        })
    };

    let rebuild_detail: Rc<dyn Fn()> = {
        let cfg = cfg.clone();
        let outputs = outputs.clone();
        let selected = selected.clone();
        let detail_host = detail_host.clone();
        let mark_display_dirty = mark_display_dirty.clone();
        Rc::new(move || {
            while let Some(child) = detail_host.first_child() {
                detail_host.remove(&child);
            }
            let list = outputs.borrow();
            if list.is_empty() {
                return;
            }
            let sel = (*selected.borrow()).min(list.len().saturating_sub(1));
            detail_host.append(&build_output_panel(
                &list[sel],
                sel,
                &cfg,
                &outputs,
                &mark_display_dirty,
            ));
        })
    };

    let rebuild_arrangement: Rc<dyn Fn()> = {
        let cfg = cfg.clone();
        let outputs = outputs.clone();
        let selected = selected.clone();
        let arrange_body = arrange_body.clone();
        let arrange_card = arrange_card.clone();
        let canvas_slot = canvas_slot.clone();
        let rebuild_detail = rebuild_detail.clone();
        let empty_hint = empty_hint.clone();
        let set_display_pending = set_display_pending.clone();
        let refresh_source_dropdown = refresh_source_dropdown.clone();
        let duplicate = duplicate.clone();
        Rc::new(move || {
            let mirror_on = cfg.borrow().display_mode == DisplayLayoutMode::Mirror;
            if duplicate.is_active() != mirror_on {
                duplicate.set_active(mirror_on);
            }
            refresh_source_dropdown();
            arrange_card.set_visible(!mirror_on);
            let list = outputs.borrow().clone();
            if list.is_empty() {
                while let Some(child) = arrange_body.first_child() {
                    arrange_body.remove(&child);
                }
                arrange_body.append(&empty_hint);
                *canvas_slot.borrow_mut() = None;
                rebuild_detail();
                return;
            }

            let needs_new = canvas_slot
                .borrow()
                .as_ref()
                .is_none_or(|c| c.output_count() != list.len());

            if needs_new {
                while let Some(child) = arrange_body.first_child() {
                    arrange_body.remove(&child);
                }
                let on_select = {
                    let selected = selected.clone();
                    let rebuild_detail = rebuild_detail.clone();
                    Rc::new(move |idx: usize| {
                        *selected.borrow_mut() = idx;
                        rebuild_detail();
                    })
                };
                let on_pending_changed = {
                    let set_display_pending = set_display_pending.clone();
                    Rc::new(move |pending| set_display_pending(pending))
                };
                let canvas = ArrangementCanvas::new(
                    cfg.clone(),
                    outputs.clone(),
                    selected.clone(),
                    on_select,
                    on_pending_changed,
                );
                arrange_body.append(canvas.widget());
                *canvas_slot.borrow_mut() = Some(canvas);
            } else if let Some(canvas) = canvas_slot.borrow().as_ref() {
                canvas.rebuild_blocks();
            }
            rebuild_detail();
        })
    };

    rebuild_arrangement();

    {
        let cfg = cfg.clone();
        let mark_display_dirty = mark_display_dirty.clone();
        let rebuild_arrangement = rebuild_arrangement.clone();
        let refresh_source_dropdown = refresh_source_dropdown.clone();
        duplicate.connect_active_notify(move |sw| {
            let mut c = cfg.borrow_mut();
            c.display_mode = if sw.is_active() {
                DisplayLayoutMode::Mirror
            } else {
                DisplayLayoutMode::Extend
            };
            if c.display_mode == DisplayLayoutMode::Mirror && c.mirror_source.is_none() {
                if let Some(first) = runtime::list_outputs().into_iter().find(|o| o.enabled) {
                    c.mirror_source = Some(first.name);
                }
            }
            drop(c);
            refresh_source_dropdown();
            mark_display_dirty();
            rebuild_arrangement();
        });
    }
    {
        let cfg = cfg.clone();
        let outputs = outputs.clone();
        let mark_display_dirty = mark_display_dirty.clone();
        source_dd.connect_selected_notify(move |dd| {
            let list = outputs.borrow();
            let enabled: Vec<&OutputInfo> = list.iter().filter(|o| o.enabled).collect();
            let Some(out) = enabled.get(dd.selected() as usize) else {
                return;
            };
            let mut c = cfg.borrow_mut();
            c.mirror_source = Some(out.name.clone());
            mark_display_dirty();
        });
    }

    {
        let parent = parent.clone();
        let canvas_slot = canvas_slot.clone();
        let outputs = outputs.clone();
        let rebuild_detail = rebuild_detail.clone();
        let display_dirty = display_dirty.clone();
        let refresh_save_buttons = refresh_save_buttons.clone();
        save_display_btn.connect_clicked(move |_| {
            let canvas = canvas_slot.borrow().clone();
            let panel_dirty = *display_dirty.borrow();
            let canvas_pending = canvas
                .as_ref()
                .is_some_and(|c| c.has_pending());
            if !panel_dirty && !canvas_pending {
                return;
            }
            if canvas.as_ref().is_some_and(|c| c.in_trial()) {
                return;
            }
            let Some(canvas) = canvas else {
                return;
            };
            if !canvas.begin_trial(panel_dirty) {
                return;
            }
            *display_dirty.borrow_mut() = false;
            refresh_save_buttons();
            runtime::reload_outputs();
            *outputs.borrow_mut() = runtime::list_outputs();
            canvas.sync_positions();

            let on_keep = {
                let canvas = canvas.clone();
                let outputs = outputs.clone();
                let rebuild_detail = rebuild_detail.clone();
                let refresh_save_buttons = refresh_save_buttons.clone();
                Rc::new(move || {
                    canvas.confirm_trial();
                    *outputs.borrow_mut() = runtime::list_outputs();
                    canvas.sync_positions();
                    rebuild_detail();
                    refresh_save_buttons();
                })
            };
            let on_revert = {
                let canvas = canvas.clone();
                let outputs = outputs.clone();
                let rebuild_detail = rebuild_detail.clone();
                let display_dirty = display_dirty.clone();
                let refresh_save_buttons = refresh_save_buttons.clone();
                Rc::new(move || {
                    canvas.cancel_trial();
                    runtime::reload_outputs();
                    *outputs.borrow_mut() = runtime::list_outputs();
                    canvas.sync_positions();
                    *display_dirty.borrow_mut() = false;
                    rebuild_detail();
                    refresh_save_buttons();
                })
            };
            display_confirm::show(&parent, on_keep, on_revert);
        });
    }
    {
        let canvas_slot = canvas_slot.clone();
        let cfg = cfg.clone();
        let display_dirty = display_dirty.clone();
        let refresh_save_buttons = refresh_save_buttons.clone();
        let rebuild_detail = rebuild_detail.clone();
        let rebuild_arrangement = rebuild_arrangement.clone();
        let duplicate = duplicate.clone();
        revert_display_btn.connect_clicked(move |_| {
            *cfg.borrow_mut() = load_outputs_config();
            let mirror_on = cfg.borrow().display_mode == DisplayLayoutMode::Mirror;
            if duplicate.is_active() != mirror_on {
                duplicate.set_active(mirror_on);
            }
            *display_dirty.borrow_mut() = false;
            if let Some(canvas) = canvas_slot.borrow().as_ref() {
                canvas.revert_layout();
            }
            refresh_save_buttons();
            rebuild_detail();
            rebuild_arrangement();
        });
    }

    {
        let cfg = cfg.clone();
        let outputs = outputs.clone();
        let rebuild_arrangement = rebuild_arrangement.clone();
        night.connect_active_notify(move |sw| {
            let mut c = cfg.borrow_mut();
            c.night_light_enabled = sw.is_active();
            save_and_apply(&c);
            refresh_outputs(&outputs, &rebuild_arrangement);
        });
    }
    {
        let cfg = cfg.clone();
        let outputs = outputs.clone();
        let rebuild_arrangement = rebuild_arrangement.clone();
        temp.connect_value_changed(move |spin| {
            let mut c = cfg.borrow_mut();
            c.night_light_temperature = spin.value() as u32;
            save_and_apply(&c);
            refresh_outputs(&outputs, &rebuild_arrangement);
        });
    }

    let btn_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .margin_top(12)
        .build();
    let refresh_btn = gtk::Button::with_label("Detect displays");
    refresh_btn.add_css_class("metis-settings-secondary");
    {
        let outputs = outputs.clone();
        let rebuild_arrangement = rebuild_arrangement.clone();
        refresh_btn.connect_clicked(move |_| {
            refresh_outputs(&outputs, &rebuild_arrangement);
        });
    }
    btn_row.append(&refresh_btn);
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    btn_row.append(&spacer);
    btn_row.append(&revert_display_btn);
    btn_row.append(&save_display_btn);
    content.append(&btn_row);

    scroller.upcast()
}

fn build_output_panel(
    out: &OutputInfo,
    index: usize,
    cfg: &Rc<RefCell<metis_config::OutputsConfig>>,
    outputs: &Rc<RefCell<Vec<OutputInfo>>>,
    mark_display_dirty: &Rc<dyn Fn()>,
) -> gtk::Widget {
    let title = panel_title(out, index);
    let (card, body) = ui::section(&title);

    if out.primary {
        let primary = gtk::Label::new(Some("Primary display"));
        primary.add_css_class("metis-settings-hint");
        primary.set_xalign(0.0);
        let primary_row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        primary_row.add_css_class("metis-settings-row");
        primary_row.append(&primary);
        body.append(&primary_row);
    }

    if out.mirror_source {
        let label = gtk::Label::new(Some("Mirror source — other displays duplicate this one"));
        label.add_css_class("metis-settings-hint");
        label.set_xalign(0.0);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        row.add_css_class("metis-settings-row");
        row.append(&label);
        body.append(&row);
    } else if out.mirrored {
        let label = gtk::Label::new(Some("Duplicating another display"));
        label.add_css_class("metis-settings-hint");
        label.set_xalign(0.0);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        row.add_css_class("metis-settings-row");
        row.append(&label);
        body.append(&row);
    }

    let (live_card, live_body) = ui::section("Applies live");
    live_body.add_css_class("metis-display-live-section");

    let scale = gtk::DropDown::from_strings(&["100%", "125%", "150%", "175%", "200%"]);
    let prefs = output_prefs(&cfg.borrow(), &out.name);
    scale.set_selected(scale_index(prefs.scale));
    live_body.append(&ui::row("Scale", &scale));

    let enabled = gtk::Switch::new();
    enabled.set_active(out.enabled);
    live_body.append(&ui::row("Active", &enabled));
    body.append(&live_card);

    let (mode_card, mode_body) = ui::section("Display mode");
    let (modes, current) = runtime::list_output_modes(&out.name);
    if modes.is_empty() {
        let hint = gtk::Label::new(Some(
            "No DRM mode list available — connect displays in a Metis session on real hardware.",
        ));
        hint.set_wrap(true);
        hint.set_xalign(0.0);
        hint.add_css_class("metis-settings-hint");
        mode_body.append(&hint);
    } else {
        let labels: Vec<String> = modes
            .iter()
            .map(|m| mode_dropdown_label(m))
            .collect();
        let label_refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let mode_dd = gtk::DropDown::from_strings(&label_refs);
        let prefs = output_prefs(&cfg.borrow(), &out.name);
        let selected = mode_index_for_prefs(&modes, &prefs).or_else(|| {
            current
                .as_ref()
                .and_then(|c| modes.iter().position(|m| modes_equal(m, c)))
        });
        if let Some(idx) = selected {
            mode_dd.set_selected(idx as u32);
        }
        mode_body.append(&ui::row("Resolution & refresh", &mode_dd));

        let name = out.name.clone();
        let cfg = cfg.clone();
        let modes = modes.clone();
        let mark_display_dirty = mark_display_dirty.clone();
        mode_dd.connect_selected_notify(move |dd| {
            let Some(mode) = modes.get(dd.selected() as usize) else {
                return;
            };
            let mut c = cfg.borrow_mut();
            let entry = c.outputs.entry(name.clone()).or_default();
            entry.mode_width = Some(mode.width);
            entry.mode_height = Some(mode.height);
            entry.mode_refresh_millihz = Some(mode.refresh_millihz);
            mark_display_dirty();
        });
    }

    let rotation = gtk::Label::new(Some("Rotation — coming soon"));
    rotation.set_xalign(0.0);
    rotation.add_css_class("metis-settings-hint");
    mode_body.append(&ui::row("Rotation", &rotation));
    body.append(&mode_card);

    let applied = gtk::Label::new(Some(&format!(
        "Compositor: {}% scale · desktop position ({}, {})",
        (out.scale * 100.0).round() as i32,
        out.rect.x,
        out.rect.y
    )));
    applied.set_xalign(0.0);
    applied.add_css_class("metis-settings-hint");
    let applied_row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    applied_row.add_css_class("metis-settings-row");
    applied_row.append(&applied);
    body.append(&applied_row);

    let name = out.name.clone();
    let cfg = cfg.clone();
    let outputs = outputs.clone();
    scale.connect_selected_notify({
        let cfg = cfg.clone();
        let name = name.clone();
        let outputs = outputs.clone();
        move |dd| {
            let mut c = cfg.borrow_mut();
            let entry = c.outputs.entry(name.clone()).or_default();
            entry.scale = scale_from_index(dd.selected());
            save_and_apply(&c);
            glib::timeout_add_seconds_local(1, {
                let outputs = outputs.clone();
                move || {
                    *outputs.borrow_mut() = runtime::list_outputs();
                    glib::ControlFlow::Break
                }
            });
        }
    });
    enabled.connect_active_notify({
        let cfg = cfg.clone();
        let name = name.clone();
        let outputs = outputs.clone();
        move |sw| {
            let mut c = cfg.borrow_mut();
            let entry = c.outputs.entry(name.clone()).or_default();
            entry.enabled = sw.is_active();
            save_and_apply(&c);
            glib::timeout_add_seconds_local(1, {
                let outputs = outputs.clone();
                move || {
                    *outputs.borrow_mut() = runtime::list_outputs();
                    glib::ControlFlow::Break
                }
            });
        }
    });

    card.upcast()
}

fn panel_title(out: &OutputInfo, index: usize) -> String {
    let name = if !out.make.is_empty() || !out.model.is_empty() {
        format!("{} {}", out.make.trim(), out.model.trim())
            .trim()
            .to_string()
    } else {
        out.name.clone()
    };
    if name.is_empty() {
        format!("Display {}", index + 1)
    } else {
        name
    }
}

fn scale_index(scale: f64) -> u32 {
    match (scale * 100.0).round() as i32 {
        n if n <= 112 => 0,
        n if n <= 137 => 1,
        n if n <= 162 => 2,
        n if n <= 187 => 3,
        _ => 4,
    }
}

fn scale_from_index(idx: u32) -> f64 {
    match idx {
        1 => 1.25,
        2 => 1.5,
        3 => 1.75,
        4 => 2.0,
        _ => 1.0,
    }
}

fn refresh_hz_label(millihz: i32) -> String {
    format!("{:.2} Hz", millihz as f64 / 1000.0)
}

fn mode_dropdown_label(mode: &OutputModeInfo) -> String {
    let recommended = if mode.preferred { " · recommended" } else { "" };
    format!(
        "{} × {} @ {}{}",
        mode.width,
        mode.height,
        refresh_hz_label(mode.refresh_millihz),
        recommended
    )
}

fn modes_equal(a: &OutputModeInfo, b: &OutputModeInfo) -> bool {
    a.width == b.width && a.height == b.height && a.refresh_millihz == b.refresh_millihz
}

fn mode_index_for_prefs(
    modes: &[OutputModeInfo],
    prefs: &metis_config::OutputPrefs,
) -> Option<usize> {
    let (w, h, r) = (
        prefs.mode_width?,
        prefs.mode_height?,
        prefs.mode_refresh_millihz?,
    );
    modes.iter().position(|m| m.width == w && m.height == h && m.refresh_millihz == r)
}

fn save_and_apply(cfg: &metis_config::OutputsConfig) {
    if let Err(err) = save_outputs_config(cfg) {
        tracing::warn!(%err, "failed to save outputs.json");
    }
    runtime::reload_outputs();
}

fn refresh_outputs(outputs: &Rc<RefCell<Vec<OutputInfo>>>, rebuild: &Rc<dyn Fn()>) {
    *outputs.borrow_mut() = runtime::list_outputs();
    let rebuild = rebuild.clone();
    glib::idle_add_local_once(move || {
        rebuild();
    });
}
