//! Desktop widgets: enable the wallpaper widget layer, edit mode, and manage
//! instances. Persists to `desktop-widgets.json`; the shell live-reloads.
//!
//! Geometry is owned by the shell while the user drags/resizes. Every Settings
//! write reloads from disk first so toggles (edit mode, lock, …) cannot clobber
//! positions the shell already saved.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;
use metis_config::{
    load_desktop_widgets_config, save_desktop_widgets_config, DesktopWidgetInstance,
    DesktopWidgetKind, DesktopWidgetsConfig,
};

use crate::ui;

pub fn build() -> gtk::Widget {
    let (scroller, content) = ui::page_for("desktop_widgets");
    let cfg = Rc::new(RefCell::new(load_desktop_widgets_config()));

    let (panel_card, panel_body) =
        ui::section_with_icon("Desktop widgets", "view-grid-symbolic");

    let enabled = gtk::Switch::new();
    enabled.set_active(cfg.borrow().enabled);
    enabled.set_halign(gtk::Align::End);
    panel_body.append(&ui::row_with_icon(
        "preferences-desktop-wallpaper-symbolic",
        "Show desktop widgets",
        &enabled,
    ));

    let edit_mode = gtk::Switch::new();
    edit_mode.set_active(cfg.borrow().edit_mode);
    edit_mode.set_halign(gtk::Align::End);
    panel_body.append(&ui::row_with_icon(
        "document-edit-symbolic",
        "Edit mode (move / resize)",
        &edit_mode,
    ));

    let hint = gtk::Label::new(Some(
        "Widgets float over the wallpaper (not classic desktop icons). Off by \
         default. In edit mode, drag unlocked widgets to reposition them; use the \
         corner handle to resize. Turn edit mode off when the layout looks right. \
         Add a Placeholder first to try placement; Folders / Apps / Clock / System \
         / Weather fill in as Phase 14 continues.",
    ));
    hint.set_xalign(0.0);
    hint.set_wrap(true);
    hint.add_css_class("metis-settings-hint");
    panel_body.append(&hint);
    content.append(&panel_card);

    let (list_card, list_body) =
        ui::section_with_icon("Widgets on this desktop", "view-list-symbolic");

    let list = gtk::Box::new(gtk::Orientation::Vertical, 6);
    list.add_css_class("metis-settings-list");
    list_body.append(&list);

    let empty = gtk::Label::new(Some(
        "No widgets yet. Add a Placeholder to try move / resize on the desktop.",
    ));
    empty.set_xalign(0.0);
    empty.add_css_class("metis-settings-hint");
    list_body.append(&empty);

    let add_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    add_row.set_halign(gtk::Align::Start);
    let kind_labels: Vec<&str> = DesktopWidgetKind::addable()
        .iter()
        .map(|k| k.label())
        .collect();
    let kind_dd = gtk::DropDown::from_strings(&kind_labels);
    kind_dd.set_selected(0);
    let add_btn = gtk::Button::with_label("Add widget");
    add_btn.add_css_class("suggested-action");
    add_row.append(&kind_dd);
    add_row.append(&add_btn);
    list_body.append(&add_row);

    content.append(&list_card);

    let refresh_list: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));
    {
        let cfg = cfg.clone();
        let list = list.clone();
        let empty = empty.clone();
        let refresh_slot = refresh_list.clone();
        let refresh = Rc::new(move || {
            while let Some(child) = list.first_child() {
                list.remove(&child);
            }
            let instances = cfg.borrow().instances.clone();
            empty.set_visible(instances.is_empty());
            for inst in &instances {
                let row = instance_row(inst, cfg.clone(), refresh_slot.clone());
                list.append(&row);
            }
        });
        *refresh_list.borrow_mut() = Some(refresh.clone());
        refresh();
    }

    {
        let cfg = cfg.clone();
        let refresh_list = refresh_list.clone();
        enabled.connect_active_notify(move |sw| {
            // Reload disk first so we keep shell-saved geometry.
            mutate_from_disk(&cfg, |disk| {
                disk.enabled = sw.is_active();
            });
            if let Some(refresh) = refresh_list.borrow().as_ref() {
                refresh();
            }
        });
    }
    {
        let cfg = cfg.clone();
        let refresh_list = refresh_list.clone();
        edit_mode.connect_active_notify(move |sw| {
            mutate_from_disk(&cfg, |disk| {
                disk.edit_mode = sw.is_active();
            });
            if let Some(refresh) = refresh_list.borrow().as_ref() {
                refresh();
            }
        });
    }
    {
        let cfg = cfg.clone();
        let refresh_list = refresh_list.clone();
        add_btn.connect_clicked(move |_| {
            let idx = kind_dd.selected() as usize;
            let kind = DesktopWidgetKind::addable()
                .get(idx)
                .copied()
                .unwrap_or(DesktopWidgetKind::Placeholder);
            mutate_from_disk(&cfg, |disk| {
                disk.instances
                    .push(DesktopWidgetInstance::new(kind));
            });
            if let Some(refresh) = refresh_list.borrow().as_ref() {
                refresh();
            }
        });
    }

    scroller.upcast()
}

fn instance_row(
    inst: &DesktopWidgetInstance,
    cfg: Rc<RefCell<DesktopWidgetsConfig>>,
    refresh_list: Rc<RefCell<Option<Rc<dyn Fn()>>>>,
) -> gtk::Widget {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("metis-settings-row");

    let label = gtk::Label::new(Some(&format!(
        "{}  ·  {}×{}  @ ({}, {}){}",
        inst.kind.label(),
        inst.w,
        inst.h,
        inst.x,
        inst.y,
        if inst.output.is_empty() {
            String::new()
        } else {
            format!("  ·  {}", inst.output)
        }
    )));
    label.set_xalign(0.0);
    label.set_hexpand(true);
    row.append(&label);

    let locked = gtk::CheckButton::with_label("Locked");
    locked.set_active(inst.locked);
    let id = inst.id.clone();
    {
        let cfg = cfg.clone();
        locked.connect_toggled(move |btn| {
            let locked = btn.is_active();
            mutate_from_disk(&cfg, |disk| {
                if let Some(inst) = disk.instances.iter_mut().find(|i| i.id == id) {
                    inst.locked = locked;
                }
            });
        });
    }
    row.append(&locked);

    let remove = gtk::Button::with_label("Remove");
    remove.add_css_class("destructive-action");
    let id = inst.id.clone();
    remove.connect_clicked(move |_| {
        mutate_from_disk(&cfg, |disk| {
            disk.instances.retain(|i| i.id != id);
        });
        if let Some(refresh) = refresh_list.borrow().as_ref() {
            refresh();
        }
    });
    row.append(&remove);

    row.upcast()
}

/// Re-read `desktop-widgets.json`, apply `f`, write back, and ask the shell to
/// reload. Preserves geometry the shell saved after drag/resize.
fn mutate_from_disk(
    cfg: &RefCell<DesktopWidgetsConfig>,
    f: impl FnOnce(&mut DesktopWidgetsConfig),
) {
    let mut disk = load_desktop_widgets_config();
    f(&mut disk);
    *cfg.borrow_mut() = disk.clone();
    if let Err(err) = save_desktop_widgets_config(&disk) {
        tracing::warn!(%err, "failed to save desktop-widgets.json");
    }
    crate::runtime::send("reload-desktop-widgets");
}
