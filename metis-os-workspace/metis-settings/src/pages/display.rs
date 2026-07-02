//! Display: arrangement canvas, per-output controls (`outputs.json`).

#[path = "display_arrangement.rs"]
mod display_arrangement;
#[path = "display_confirm.rs"]
mod display_confirm;
#[path = "display_schedule.rs"]
mod display_schedule;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;
use metis_config::{
    load_outputs_config, output_prefs, save_outputs_config, DisplayLayoutMode,
};
use metis_protocol::{OutputInfo, OutputModeInfo};

use crate::runtime;
use crate::ui;

use display_arrangement::ArrangementCanvas;
use display_schedule::build_schedule_time_picker;

pub fn build(parent: &gtk::Window) -> gtk::Widget {
    let (scroller, content) = ui::page_for("display");

    let cfg = Rc::new(RefCell::new(load_outputs_config()));
    let outputs = Rc::new(RefCell::new(runtime::list_outputs()));
    let selected_name: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(
        outputs.borrow().first().map(|o| o.name.clone()),
    ));
    let canvas_slot: Rc<RefCell<Option<Rc<ArrangementCanvas>>>> = Rc::new(RefCell::new(None));
    let display_dirty: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));
    let rebuild_arrangement_slot: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));
    // Guards programmatic mirror-toggle sync from re-entering `rebuild_arrangement`.
    let syncing_arrangement = Rc::new(std::cell::Cell::new(false));

    let modes_cache: Rc<RefCell<HashMap<String, (Vec<OutputModeInfo>, Option<OutputModeInfo>)>>> =
        Rc::new(RefCell::new(HashMap::new()));

    let (global_card, global_body) = ui::section("Night light");
    let (night_row, night) = ui::switch_row("Enable night light");
    night.set_active(cfg.borrow().night_light_enabled);
    global_body.append(&night_row);
    let temp = gtk::Scale::with_range(gtk::Orientation::Horizontal, 2700.0, 6500.0, 50.0);
    temp.set_digits(0);
    temp.set_value(cfg.borrow().night_light_temperature as f64);
    temp.set_size_request(220, -1);
    temp.set_draw_value(true);
    temp.set_sensitive(cfg.borrow().night_light_enabled);
    temp.add_css_class("metis-settings-scale");
    ui::forward_wheel_to_page_scroller(&temp);
    let temp_row = gtk::Box::new(gtk::Orientation::Vertical, 4);
    temp_row.append(&temp);
    let temp_hint = gtk::Label::new(Some("Warmer ← drag → cooler"));
    temp_hint.set_xalign(0.0);
    temp_hint.add_css_class("metis-settings-hint");
    temp_row.append(&temp_hint);
    global_body.append(&ui::row("Colour temperature", &temp_row));

    let (schedule_row, schedule_sw) = ui::switch_row("Use schedule");
    schedule_sw.set_active(cfg.borrow().night_light_schedule.enabled);
    global_body.append(&schedule_row);

    let use_12h = Rc::new(Cell::new(cfg.borrow().night_light_schedule_12h));
    let (format_row, format_12h_sw) = ui::switch_row("12-hour time");
    format_12h_sw.set_active(use_12h.get());
    format_12h_sw.set_sensitive(cfg.borrow().night_light_schedule.enabled);
    global_body.append(&format_row);

    let schedule_apply_start = {
        let cfg = cfg.clone();
        Rc::new(move |hhmm: String| {
            cfg.borrow_mut().night_light_schedule.start = hhmm;
            save_and_apply(&cfg.borrow());
        })
    };
    let schedule_apply_end = {
        let cfg = cfg.clone();
        Rc::new(move |hhmm: String| {
            cfg.borrow_mut().night_light_schedule.end = hhmm;
            save_and_apply(&cfg.borrow());
        })
    };

    let start_picker = Rc::new(build_schedule_time_picker(
        "From",
        &cfg.borrow().night_light_schedule.start,
        use_12h.clone(),
        schedule_apply_start,
    ));
    let end_picker = Rc::new(build_schedule_time_picker(
        "To",
        &cfg.borrow().night_light_schedule.end,
        use_12h.clone(),
        schedule_apply_end,
    ));
    start_picker.set_sensitive(cfg.borrow().night_light_schedule.enabled);
    end_picker.set_sensitive(cfg.borrow().night_light_schedule.enabled);

    let schedule_times = gtk::Box::new(gtk::Orientation::Horizontal, 20);
    schedule_times.add_css_class("metis-settings-schedule-times");
    schedule_times.set_margin_start(16);
    schedule_times.set_margin_end(16);
    schedule_times.set_halign(gtk::Align::Start);
    schedule_times.append(&start_picker.root);
    schedule_times.append(&end_picker.root);
    global_body.append(&schedule_times);

    let schedule_hint = gtk::Label::new(Some(
        "Pick a preset or type a custom time at the top of each menu. Overnight ranges \
         work (e.g. 8:00 PM → 7:00 AM). When schedule is on, night light only tints \
         inside that window.",
    ));
    schedule_hint.set_wrap(true);
    schedule_hint.set_xalign(0.0);
    schedule_hint.add_css_class("metis-settings-hint");
    schedule_hint.set_sensitive(cfg.borrow().night_light_schedule.enabled);
    global_body.append(&schedule_hint);

    let night_note = gtk::Label::new(Some(
        "Applies live. Drag left for a warmer evening tint (like GNOME Night Light); \
         drag right toward normal daylight colour.",
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
        let selected_name = selected_name.clone();
        let detail_host = detail_host.clone();
        let mark_display_dirty = mark_display_dirty.clone();
        let rebuild_arrangement_slot = rebuild_arrangement_slot.clone();
        let parent = parent.clone();
        let modes_cache = modes_cache.clone();
        Rc::new(move || {
            while let Some(child) = detail_host.first_child() {
                detail_host.remove(&child);
            }
            let list = outputs.borrow();
            if list.is_empty() {
                return;
            }
            let (out, index) = resolve_selected_output(&list, &selected_name);
            detail_host.append(&build_output_panel(
                out,
                index,
                &cfg,
                &outputs,
                &modes_cache,
                &mark_display_dirty,
                list.len() >= 2,
                &rebuild_arrangement_slot,
                &parent,
            ));
        })
    };

    let rebuild_arrangement: Rc<dyn Fn()> = {
        let cfg = cfg.clone();
        let outputs = outputs.clone();
        let selected_name = selected_name.clone();
        let arrange_body = arrange_body.clone();
        let arrange_card = arrange_card.clone();
        let canvas_slot = canvas_slot.clone();
        let rebuild_detail = rebuild_detail.clone();
        let empty_hint = empty_hint.clone();
        let set_display_pending = set_display_pending.clone();
        let refresh_source_dropdown = refresh_source_dropdown.clone();
        let duplicate = duplicate.clone();
        let syncing_arrangement = syncing_arrangement.clone();
        Rc::new(move || {
            if syncing_arrangement.get() {
                return;
            }
            syncing_arrangement.set(true);

            let mirror_on = cfg.borrow().display_mode == DisplayLayoutMode::Mirror;
            if duplicate.is_active() != mirror_on {
                duplicate.set_active(mirror_on);
            }
            refresh_source_dropdown();
            arrange_card.set_visible(!mirror_on);
            update_arrangement_view(
                &outputs,
                &selected_name,
                &arrange_body,
                &empty_hint,
                &canvas_slot,
                &cfg,
                &set_display_pending,
                &rebuild_detail,
            );
            syncing_arrangement.set(false);
        })
    };

    let refresh_detected_outputs: Rc<dyn Fn()> = {
        let outputs = outputs.clone();
        let selected_name = selected_name.clone();
        let arrange_body = arrange_body.clone();
        let canvas_slot = canvas_slot.clone();
        let cfg = cfg.clone();
        let empty_hint = empty_hint.clone();
        let set_display_pending = set_display_pending.clone();
        let rebuild_detail = rebuild_detail.clone();
        Rc::new(move || {
            update_arrangement_view(
                &outputs,
                &selected_name,
                &arrange_body,
                &empty_hint,
                &canvas_slot,
                &cfg,
                &set_display_pending,
                &rebuild_detail,
            );
        })
    };

    rebuild_arrangement();
    *rebuild_arrangement_slot.borrow_mut() = Some(rebuild_arrangement.clone());

    {
        let cfg = cfg.clone();
        let mark_display_dirty = mark_display_dirty.clone();
        let rebuild_arrangement = rebuild_arrangement.clone();
        let refresh_source_dropdown = refresh_source_dropdown.clone();
        let syncing_arrangement = syncing_arrangement.clone();
        duplicate.connect_active_notify(move |sw| {
            if syncing_arrangement.get() {
                return;
            }
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
            runtime::reload_outputs_async();
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
                    runtime::reload_outputs_async();
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
        let temp = temp.clone();
        ui::defer_switch_active_notify(&night, move |active| {
            temp.set_sensitive(active);
            cfg.borrow_mut().night_light_enabled = active;
            save_and_apply(&cfg.borrow());
        });
    }
    {
        let cfg = cfg.clone();
        let temp_debounce: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
        temp.connect_value_changed(move |scale| {
            let value = scale.value().round() as u32;
            {
                let mut c = cfg.borrow_mut();
                c.night_light_temperature = value;
            }
            let mut slot = temp_debounce.borrow_mut();
            if let Some(id) = slot.take() {
                id.remove();
            }
            let cfg = cfg.clone();
            let temp_debounce = temp_debounce.clone();
            let id = glib::timeout_add_local(Duration::from_millis(120), move || {
                *temp_debounce.borrow_mut() = None;
                save_and_apply(&cfg.borrow());
                glib::ControlFlow::Break
            });
            *slot = Some(id);
        });
    }
    {
        let cfg = cfg.clone();
        let start_picker = start_picker.clone();
        let end_picker = end_picker.clone();
        let format_12h_sw = format_12h_sw.clone();
        let schedule_hint = schedule_hint.clone();
        ui::defer_switch_active_notify(&schedule_sw, move |active| {
            start_picker.set_sensitive(active);
            end_picker.set_sensitive(active);
            format_12h_sw.set_sensitive(active);
            schedule_hint.set_sensitive(active);
            cfg.borrow_mut().night_light_schedule.enabled = active;
            save_and_apply(&cfg.borrow());
        });
    }
    {
        let cfg = cfg.clone();
        let use_12h = use_12h.clone();
        let start_picker = start_picker.clone();
        let end_picker = end_picker.clone();
        ui::defer_switch_active_notify(&format_12h_sw, move |active| {
            use_12h.set(active);
            cfg.borrow_mut().night_light_schedule_12h = active;
            start_picker.refresh();
            end_picker.refresh();
            save_and_apply(&cfg.borrow());
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
        let modes_cache = modes_cache.clone();
        let refresh_detected_outputs = refresh_detected_outputs.clone();
        refresh_btn.connect_clicked(move |_| {
            modes_cache.borrow_mut().clear();
            refresh_outputs(&outputs, &refresh_detected_outputs);
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
    modes_cache: &Rc<RefCell<HashMap<String, (Vec<OutputModeInfo>, Option<OutputModeInfo>)>>>,
    mark_display_dirty: &Rc<dyn Fn()>,
    multi_display: bool,
    rebuild_arrangement: &Rc<RefCell<Option<Rc<dyn Fn()>>>>,
    parent: &gtk::Window,
) -> gtk::Widget {
    let title = panel_title(out, index);
    let (card, body) = ui::section(&title);

    if multi_display {
        if is_primary_output(&cfg.borrow(), out) {
            let primary = gtk::Label::new(Some("Primary display"));
            primary.add_css_class("metis-settings-hint");
            primary.set_xalign(0.0);
            let primary_row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            primary_row.add_css_class("metis-settings-row");
            primary_row.append(&primary);
            body.append(&primary_row);
        } else {
            let set_primary = gtk::Button::with_label("Set as primary display");
            set_primary.add_css_class("metis-settings-secondary");
            let cfg = cfg.clone();
            let name = out.name.clone();
            let outputs = outputs.clone();
            let rebuild_arrangement = rebuild_arrangement.clone();
            set_primary.connect_clicked(move |_| {
                cfg.borrow_mut().primary_output = Some(name.clone());
                save_and_apply(&cfg.borrow());
                if let Some(rebuild) = rebuild_arrangement.borrow().as_ref() {
                    rebuild();
                }
                glib::timeout_add_seconds_local(1, {
                    let outputs = outputs.clone();
                    let rebuild_arrangement = rebuild_arrangement.clone();
                    move || {
                        *outputs.borrow_mut() = runtime::list_outputs();
                        if let Some(rebuild) = rebuild_arrangement.borrow().as_ref() {
                            rebuild();
                        }
                        glib::ControlFlow::Break
                    }
                });
            });
            body.append(&ui::row("Primary", &set_primary));
        }
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

    if out.vrr_available {
        let vrr = gtk::Switch::new();
        vrr.set_active(output_prefs(&cfg.borrow(), &out.name).vrr_enabled);
        live_body.append(&ui::row("Adaptive sync", &vrr));
        let name = out.name.clone();
        let cfg = cfg.clone();
        let outputs = outputs.clone();
        vrr.connect_active_notify({
            let cfg = cfg.clone();
            let name = name.clone();
            let outputs = outputs.clone();
            move |sw| {
                let active = sw.is_active();
                let cfg = cfg.clone();
                let name = name.clone();
                let outputs = outputs.clone();
                glib::idle_add_local_once(move || {
                    {
                        let mut c = cfg.borrow_mut();
                        let entry = c.outputs.entry(name.clone()).or_default();
                        entry.vrr_enabled = active;
                    }
                    save_and_apply(&cfg.borrow());
                    glib::timeout_add_seconds_local(1, {
                        let outputs = outputs.clone();
                        move || {
                            *outputs.borrow_mut() = runtime::list_outputs();
                            glib::ControlFlow::Break
                        }
                    });
                });
            }
        });
    }

    body.append(&live_card);

    let (mode_card, mode_body) = ui::section("Display mode");
    let (modes, current) = cached_output_modes(modes_cache, &out.name);
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

    let (color_card, color_body) = ui::section("Colour profile");
    let profile_path = output_prefs(&cfg.borrow(), &out.name)
        .color_profile
        .clone()
        .unwrap_or_default();
    let profile_label = gtk::Label::new(None);
    profile_label.set_xalign(0.0);
    profile_label.set_wrap(true);
    profile_label.add_css_class("metis-settings-hint");
    profile_label.set_text(if profile_path.is_empty() {
        "Default (sRGB) — compositor colour pipeline apply pending"
    } else {
        &profile_path
    });
    color_body.append(&profile_label);
    let profile_actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let pick_profile = gtk::Button::with_label("Choose ICC profile…");
    pick_profile.add_css_class("metis-settings-secondary");
    let clear_profile = gtk::Button::with_label("Clear");
    clear_profile.add_css_class("metis-settings-secondary");
    clear_profile.set_sensitive(!profile_path.is_empty());
    profile_actions.set_margin_start(16);
    profile_actions.set_margin_end(16);
    profile_actions.set_margin_bottom(4);
    profile_actions.append(&pick_profile);
    profile_actions.append(&clear_profile);
    color_body.append(&profile_actions);
    let color_hint = gtk::Label::new(Some(
        "Saved to outputs.json and exposed to Wayland clients via wp_color_management_v1. \
         GPU colour transforms in the compositor render path are still follow-up work.",
    ));
    color_hint.set_wrap(true);
    color_hint.set_xalign(0.0);
    color_hint.add_css_class("metis-settings-hint");
    color_body.append(&color_hint);
    body.append(&color_card);

    {
        let parent = parent.clone();
        pick_profile.connect_clicked({
            let cfg = cfg.clone();
            let name = out.name.clone();
            let profile_label = profile_label.clone();
            let clear_profile = clear_profile.clone();
            let parent = parent.clone();
            move |_| {
                let dialog = gtk::FileChooserDialog::builder()
                    .title("Choose ICC colour profile")
                    .action(gtk::FileChooserAction::Open)
                    .modal(true)
                    .transient_for(&parent)
                    .build();
                dialog.add_button("Cancel", gtk::ResponseType::Cancel);
                dialog.add_button("Open", gtk::ResponseType::Accept);
                let filter = gtk::FileFilter::new();
                filter.set_name(Some("ICC profiles"));
                filter.add_mime_type("application/vnd.iccprofile");
                filter.add_pattern("*.icc");
                filter.add_pattern("*.icm");
                dialog.add_filter(&filter);
                dialog.connect_response({
                    let cfg = cfg.clone();
                    let name = name.clone();
                    let profile_label = profile_label.clone();
                    let clear_profile = clear_profile.clone();
                    move |d, response| {
                        if response == gtk::ResponseType::Accept {
                            if let Some(file) = d.file() {
                                if let Some(path) = file.path() {
                                    let path = path.display().to_string();
                                    {
                                        let mut c = cfg.borrow_mut();
                                        c.outputs
                                            .entry(name.clone())
                                            .or_default()
                                            .color_profile = Some(path.clone());
                                    }
                                    profile_label.set_text(&path);
                                    clear_profile.set_sensitive(true);
                                    save_and_apply(&cfg.borrow());
                                }
                            }
                        }
                        d.close();
                    }
                });
                dialog.show();
            }
        });
        clear_profile.connect_clicked({
            let cfg = cfg.clone();
            let name = out.name.clone();
            let profile_label = profile_label.clone();
            let clear_profile = clear_profile.clone();
            move |_| {
                {
                    let mut c = cfg.borrow_mut();
                    c.outputs.entry(name.clone()).or_default().color_profile = None;
                }
                profile_label.set_text(
                    "Default (sRGB) — compositor colour pipeline apply pending",
                );
                clear_profile.set_sensitive(false);
                save_and_apply(&cfg.borrow());
            }
        });
    }

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

fn cached_output_modes(
    cache: &Rc<RefCell<HashMap<String, (Vec<OutputModeInfo>, Option<OutputModeInfo>)>>>,
    output: &str,
) -> (Vec<OutputModeInfo>, Option<OutputModeInfo>) {
    if let Some(entry) = cache.borrow().get(output) {
        return entry.clone();
    }
    let modes = runtime::list_output_modes(output);
    cache.borrow_mut().insert(output.to_string(), modes.clone());
    modes
}

thread_local! {
    static SAVE_DEBOUNCE: RefCell<Option<glib::SourceId>> = RefCell::new(None);
}

fn save_and_apply(cfg: &metis_config::OutputsConfig) {
    let cfg = cfg.clone();
    SAVE_DEBOUNCE.with(|slot| {
        let mut slot = slot.borrow_mut();
        if let Some(id) = slot.take() {
            id.remove();
        }
        let id = glib::timeout_add_local(Duration::from_millis(350), move || {
            SAVE_DEBOUNCE.with(|slot| *slot.borrow_mut() = None);
            let cfg = cfg.clone();
            std::thread::spawn(move || {
                if let Err(err) = save_outputs_config(&cfg) {
                    tracing::warn!(%err, "failed to save outputs.json");
                    return;
                }
                runtime::reload_outputs();
            });
            glib::ControlFlow::Break
        });
        *slot = Some(id);
    });
}

fn refresh_outputs(outputs: &Rc<RefCell<Vec<OutputInfo>>>, rebuild: &Rc<dyn Fn()>) {
    let outputs = outputs.clone();
    let rebuild = rebuild.clone();
    // Defer IPC + UI refresh so the click handler returns before compositor I/O.
    glib::idle_add_local_once(move || {
        *outputs.borrow_mut() = runtime::list_outputs();
        rebuild();
    });
}

fn update_arrangement_view(
    outputs: &Rc<RefCell<Vec<OutputInfo>>>,
    selected_name: &Rc<RefCell<Option<String>>>,
    arrange_body: &gtk::Box,
    empty_hint: &gtk::Label,
    canvas_slot: &Rc<RefCell<Option<Rc<ArrangementCanvas>>>>,
    cfg: &Rc<RefCell<metis_config::OutputsConfig>>,
    set_display_pending: &Rc<dyn Fn(bool)>,
    rebuild_detail: &Rc<dyn Fn()>,
) {
    let list = outputs.borrow().clone();
    if list.is_empty() {
        while let Some(child) = arrange_body.first_child() {
            arrange_body.remove(&child);
        }
        arrange_body.append(empty_hint);
        *canvas_slot.borrow_mut() = None;
        *selected_name.borrow_mut() = None;
        rebuild_detail();
        return;
    }
    let reset_selection = {
        let sel = selected_name.borrow();
        sel.is_none() || !list.iter().any(|o| Some(&o.name) == sel.as_ref())
    };
    if reset_selection {
        *selected_name.borrow_mut() = list.first().map(|o| o.name.clone());
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
            let selected_name = selected_name.clone();
            let outputs = outputs.clone();
            let rebuild_detail = rebuild_detail.clone();
            let canvas_slot = canvas_slot.clone();
            Rc::new(move |idx: usize| {
                if let Some(name) = outputs.borrow().get(idx).map(|o| o.name.clone()) {
                    *selected_name.borrow_mut() = Some(name);
                }
                if let Some(canvas) = canvas_slot.borrow().as_ref().cloned() {
                    canvas.set_selected(idx);
                }
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
            selected_name.clone(),
            on_select,
            on_pending_changed,
        );
        arrange_body.append(canvas.widget());
        *canvas_slot.borrow_mut() = Some(canvas);
    } else if let Some(canvas) = canvas_slot.borrow().as_ref().cloned() {
        canvas.rebuild_blocks();
        // Clone selection before set_selected — it borrow_mut's selected_name; holding
        // selected_name.borrow() across that call panics (Detect displays path).
        let sel_idx = selected_name
            .borrow()
            .clone()
            .and_then(|name| list.iter().position(|o| o.name == name));
        if let Some(idx) = sel_idx {
            canvas.set_selected(idx);
        }
    }
    rebuild_detail();
    if let Some(canvas) = canvas_slot.borrow().as_ref().cloned() {
        glib::idle_add_local_once(move || canvas.refresh_layout());
    }
}

fn resolve_selected_output<'a>(
    list: &'a [OutputInfo],
    selected_name: &RefCell<Option<String>>,
) -> (&'a OutputInfo, usize) {
    if let Some(ref name) = *selected_name.borrow() {
        if let Some((idx, out)) = list.iter().enumerate().find(|(_, o)| &o.name == name) {
            return (out, idx);
        }
    }
    list.first()
        .map(|o| (o, 0))
        .expect("non-empty output list")
}

fn is_primary_output(cfg: &metis_config::OutputsConfig, out: &OutputInfo) -> bool {
    if let Some(ref name) = cfg.primary_output {
        return name == &out.name;
    }
    out.primary
}
