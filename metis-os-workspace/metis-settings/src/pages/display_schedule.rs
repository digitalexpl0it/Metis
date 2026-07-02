//! Night-light schedule time pickers (popover presets + custom entry).

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;
use metis_config::{
    format_schedule_hhmm, format_schedule_minutes, parse_schedule_input, schedule_half_hour_presets,
};

pub struct ScheduleTimePicker {
    pub root: gtk::Box,
    button: gtk::MenuButton,
    popover_entry: gtk::Entry,
    list: gtk::ListBox,
    hhmm: Rc<RefCell<String>>,
    use_12h: Rc<Cell<bool>>,
    on_change: Rc<dyn Fn(String)>,
}

impl ScheduleTimePicker {
    pub fn set_sensitive(&self, sensitive: bool) {
        self.root.set_sensitive(sensitive);
    }

    pub fn refresh(&self) {
        let hhmm = self.hhmm.borrow().clone();
        let label = format_schedule_hhmm(&hhmm, self.use_12h.get())
            .unwrap_or_else(|| hhmm.clone());
        self.button.set_label(&label);
        self.popover_entry.set_text(&label);
        self.rebuild_presets();
    }

    fn rebuild_presets(&self) {
        while let Some(row) = self.list.first_child() {
            self.list.remove(&row);
        }
        let use_12h = self.use_12h.get();
        for minutes in schedule_half_hour_presets() {
            let label = format_schedule_minutes(minutes, use_12h);
            let row = gtk::ListBoxRow::new();
            row.add_css_class("metis-settings-schedule-preset-row");
            let lbl = gtk::Label::new(Some(&label));
            lbl.add_css_class("metis-settings-schedule-preset");
            lbl.set_xalign(0.0);
            lbl.set_margin_start(14);
            lbl.set_margin_end(14);
            lbl.set_margin_top(10);
            lbl.set_margin_bottom(10);
            row.set_child(Some(&lbl));
            self.list.append(&row);
        }
    }
}

pub fn build_schedule_time_picker(
    label: &str,
    initial_hhmm: &str,
    use_12h: Rc<Cell<bool>>,
    on_change: Rc<dyn Fn(String)>,
) -> ScheduleTimePicker {
    let hhmm = Rc::new(RefCell::new(initial_hhmm.to_string()));
    let root = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let title = gtk::Label::new(Some(label));
    title.set_xalign(0.0);
    title.set_width_chars(3);
    root.append(&title);

    let button = gtk::MenuButton::new();
    button.add_css_class("metis-settings-secondary");
    root.append(&button);

    let popover = gtk::Popover::new();
    popover.add_css_class("metis-settings-schedule-popover");
    popover.set_parent(&button);
    let pop_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
    pop_box.set_size_request(268, -1);

    let custom_hint = gtk::Label::new(Some("Custom time"));
    custom_hint.set_xalign(0.0);
    custom_hint.set_margin_start(14);
    custom_hint.set_margin_top(12);
    custom_hint.add_css_class("metis-settings-hint");
    pop_box.append(&custom_hint);

    let popover_entry = gtk::Entry::new();
    popover_entry.add_css_class("metis-settings-schedule-entry");
    popover_entry.set_margin_start(14);
    popover_entry.set_margin_end(14);
    popover_entry.set_margin_bottom(8);
    popover_entry.set_placeholder_text(Some(if use_12h.get() {
        "e.g. 8:30 PM"
    } else {
        "e.g. 20:30"
    }));
    pop_box.append(&popover_entry);

    let sep = gtk::Separator::new(gtk::Orientation::Horizontal);
    pop_box.append(&sep);

    let presets_hint = gtk::Label::new(Some("Presets"));
    presets_hint.set_xalign(0.0);
    presets_hint.set_margin_start(14);
    presets_hint.set_margin_top(10);
    presets_hint.add_css_class("metis-settings-hint");
    pop_box.append(&presets_hint);

    let scrolled = gtk::ScrolledWindow::new();
    scrolled.add_css_class("metis-settings-schedule-presets");
    scrolled.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scrolled.set_propagate_natural_width(false);
    scrolled.set_propagate_natural_height(false);
    scrolled.set_min_content_height(300);
    scrolled.set_max_content_height(300);
    scrolled.set_margin_start(8);
    scrolled.set_margin_end(8);
    scrolled.set_margin_bottom(10);
    let list = gtk::ListBox::new();
    list.add_css_class("boxed-list");
    list.add_css_class("metis-settings-schedule-list");
    list.set_selection_mode(gtk::SelectionMode::Single);
    scrolled.set_child(Some(&list));
    pop_box.append(&scrolled);

    popover.set_child(Some(&pop_box));
    button.set_popover(Some(&popover));

    {
        let list = list.clone();
        let hhmm = hhmm.clone();
        let use_12h = use_12h.clone();
        popover.connect_notify_local(Some("visible"), move |popover, _| {
            if !popover.is_visible() {
                return;
            }
            select_list_hhmm(&list, &hhmm.borrow(), use_12h.get());
        });
    }

    {
        let hhmm = hhmm.clone();
        let use_12h = use_12h.clone();
        let on_change = on_change.clone();
        let button = button.clone();
        let popover_entry = popover_entry.clone();
        let debounce: Rc<std::cell::RefCell<Option<glib::SourceId>>> =
            Rc::new(std::cell::RefCell::new(None));
        let apply_custom = {
            let hhmm = hhmm.clone();
            let use_12h = use_12h.clone();
            let on_change = on_change.clone();
            let button = button.clone();
            let popover_entry = popover_entry.clone();
            Rc::new(move || {
                let text = popover_entry.text().to_string();
                let Some(parsed) = parse_schedule_input(&text, use_12h.get()) else {
                    return;
                };
                *hhmm.borrow_mut() = parsed.clone();
                if let Some(display) = format_schedule_hhmm(&parsed, use_12h.get()) {
                    button.set_label(&display);
                    popover_entry.set_text(&display);
                }
                on_change(parsed);
            })
        };
        popover_entry.connect_activate({
            let apply_custom = apply_custom.clone();
            move |_| apply_custom()
        });
        popover_entry.connect_changed({
            let apply_custom = apply_custom.clone();
            let debounce = debounce.clone();
            move |_| {
                let mut slot = debounce.borrow_mut();
                if let Some(id) = slot.take() {
                    id.remove();
                }
                let apply_custom = apply_custom.clone();
                let debounce = debounce.clone();
                let id = glib::timeout_add_local(Duration::from_millis(450), move || {
                    *debounce.borrow_mut() = None;
                    apply_custom();
                    glib::ControlFlow::Break
                });
                *slot = Some(id);
            }
        });
    }

    {
        let hhmm = hhmm.clone();
        let use_12h = use_12h.clone();
        let on_change = on_change.clone();
        let button = button.clone();
        let popover = popover.clone();
        let popover_entry = popover_entry.clone();
        list.connect_row_activated(move |_, row| {
            let Some(child) = row.child() else {
                return;
            };
            let Ok(lbl) = child.downcast::<gtk::Label>() else {
                return;
            };
            let text = lbl.label().to_string();
            let Some(parsed) = parse_schedule_input(&text, use_12h.get()) else {
                return;
            };
            *hhmm.borrow_mut() = parsed.clone();
            button.set_label(&text);
            popover_entry.set_text(&text);
            popover.popdown();
            on_change(parsed);
        });
    }

    let picker = ScheduleTimePicker {
        root,
        button,
        popover_entry,
        list,
        hhmm,
        use_12h,
        on_change,
    };
    picker.refresh();

    picker
}

fn list_row_hhmm(row: &gtk::ListBoxRow, use_12h: bool) -> Option<String> {
    let child = row.child()?;
    let lbl = child.downcast::<gtk::Label>().ok()?;
    parse_schedule_input(&lbl.label(), use_12h)
}

fn select_list_hhmm(list: &gtk::ListBox, hhmm: &str, use_12h: bool) {
    let mut child = list.first_child();
    while let Some(node) = child {
        child = node.next_sibling();
        if let Ok(row) = node.downcast::<gtk::ListBoxRow>() {
            if list_row_hhmm(&row, use_12h).as_deref() == Some(hhmm) {
                list.select_row(Some(&row));
                return;
            }
        }
    }
}
