//! Taskbar / running-apps dock for the edge bar.
//!
//! Shows one button per running application (windows grouped by resolved app
//! identity), plus any apps the user has pinned to the dock (`taskbar_pinned` in
//! `bar.json`). A running indicator dot and a focus highlight track live window
//! state fed from the compositor via `services::windows`. Clicking toggles
//! focus/minimize for single-window apps, opens a window picker for multi-window
//! apps, and launches pinned-but-not-running apps. Right-click pins/unpins or
//! closes. Overflow scrolls horizontally.

use std::collections::HashMap;
use std::rc::Rc;

use gio::prelude::*;
use gtk::gdk;
use gtk::prelude::*;

use crate::config::{load_bar_config, save_bar_config};
use crate::services::windows::{self, WindowsSnapshot};
use crate::services::{applications, AppEntry};

use metis_protocol::WindowInfo;

const TASK_ICON_SIZE: i32 = 20;

pub struct TasksWidget {
    root: gtk::ScrolledWindow,
    refresh: Rc<dyn Fn()>,
}

impl TasksWidget {
    pub fn new() -> Self {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        row.add_css_class("metis-bar-tasks-row");

        let root = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .child(&row)
            .build();
        root.add_css_class("metis-bar-tasks");
        // Size the dock to its content (up to the available bar space) instead of
        // claiming a fixed slice of the bar. Both axes must propagate or the
        // scrolled window collapses to its 0-px minimum inside the bar strip.
        root.set_propagate_natural_width(true);
        root.set_propagate_natural_height(true);

        let refresh: Rc<dyn Fn()> = {
            let row = row.clone();
            Rc::new(move || {
                let snap = windows::snapshot();
                let pinned = load_bar_config().taskbar_pinned;
                rebuild(&row, &snap, &pinned);
            })
        };

        windows::register_refresh(refresh.clone());
        refresh();

        Self { root, refresh }
    }

    pub fn root(&self) -> &gtk::ScrolledWindow {
        &self.root
    }

    /// Repaint from the current window store (used by the bar's fan-out).
    pub fn update(&self, _snapshot: &WindowsSnapshot) {
        (self.refresh)();
    }
}

/// One dock entry: a pinned app and/or a set of running windows for one app.
struct Group {
    /// Id stored in `taskbar_pinned` when pinnable (resolved desktop id or app_id).
    pin_id: Option<String>,
    name: String,
    icon: gio::Icon,
    exec: Option<String>,
    windows: Vec<WindowInfo>,
    pinned: bool,
    /// True when `name`/`icon` came from a real `.desktop` entry. When false the
    /// group is showing a best-effort placeholder, so a live window's title/icon
    /// should override it on merge.
    resolved: bool,
}

/// Turn a reverse-DNS Wayland `app_id` into a friendly label when no `.desktop`
/// entry exists (e.g. `com.metis.Settings` -> `Settings`), so tooltips never
/// surface the raw id.
fn prettify_app_id(id: &str) -> String {
    let base = id.strip_suffix(".desktop").unwrap_or(id);
    let last = base.rsplit('.').next().unwrap_or(base);
    let cleaned = last.replace(['-', '_'], " ");
    let mut out = String::with_capacity(cleaned.len());
    for (i, word) in cleaned.split_whitespace().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    if out.is_empty() {
        id.to_string()
    } else {
        out
    }
}

fn fallback_icon() -> gio::Icon {
    gio::ThemedIcon::new(applications::FALLBACK_ICON_NAME).upcast()
}

fn icon_or_fallback(entry: &AppEntry) -> gio::Icon {
    entry.icon.clone().unwrap_or_else(fallback_icon)
}

fn matches_app_id(entry: &AppEntry, needle: &str) -> bool {
    let base = entry
        .id
        .strip_suffix(".desktop")
        .unwrap_or(&entry.id)
        .to_lowercase();
    base == needle
        || entry.id.to_lowercase() == needle
        || entry
            .wm_class
            .as_deref()
            .is_some_and(|w| w.to_lowercase() == needle)
}

fn rebuild(row: &gtk::Box, snap: &WindowsSnapshot, pinned: &[String]) {
    while let Some(child) = row.first_child() {
        row.remove(&child);
    }

    // Enumerate installed apps once per rebuild for icon/name/exec resolution.
    let apps = applications::list_apps();
    let resolve = |app_id: &str| -> Option<&AppEntry> {
        let needle = app_id.trim().to_lowercase();
        if needle.is_empty() {
            return None;
        }
        apps.iter().find(|e| matches_app_id(e, &needle))
    };
    // When a window never reports an `app_id` (GTK sets it late, X11 apps, etc.)
    // fall back to matching its title against an installed app name so it still
    // gets a real icon where possible.
    let resolve_by_title = |title: &str| -> Option<&AppEntry> {
        let needle = title.trim().to_lowercase();
        if needle.is_empty() {
            return None;
        }
        apps.iter().find(|e| e.name.to_lowercase() == needle)
    };

    tracing::info!(
        windows = snap.windows.len(),
        pinned = pinned.len(),
        "tasks: rebuilding dock"
    );

    let mut groups: Vec<Group> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();

    // Pinned entries first, in saved order (even when not currently running).
    for id in pinned {
        let key = id.to_lowercase();
        if index.contains_key(&key) {
            continue;
        }
        let entry = apps.iter().find(|e| matches_app_id(e, &key));
        let (name, icon, exec, resolved) = match entry {
            Some(e) => (
                e.name.clone(),
                icon_or_fallback(e),
                Some(e.exec.clone()),
                true,
            ),
            None => (prettify_app_id(id), fallback_icon(), None, false),
        };
        index.insert(key, groups.len());
        groups.push(Group {
            pin_id: Some(id.clone()),
            name,
            icon,
            exec,
            windows: Vec::new(),
            pinned: true,
            resolved,
        });
    }

    // Then running windows, merged into pinned groups or appended as new ones.
    for w in &snap.windows {
        let (pin_id, key, name, icon, exec, resolved) = match w.app_id.as_deref() {
            Some(app_id) if !app_id.is_empty() => {
                if let Some(e) = resolve(app_id) {
                    (
                        Some(e.id.clone()),
                        e.id.to_lowercase(),
                        e.name.clone(),
                        icon_or_fallback(e),
                        Some(e.exec.clone()),
                        true,
                    )
                } else {
                    // No matching desktop entry: prefer the human-readable window
                    // title over the raw app_id (e.g. "Metis Settings" rather than
                    // "com.metis.Settings") for the label and tooltip.
                    (
                        Some(app_id.to_string()),
                        app_id.to_lowercase(),
                        window_label(w),
                        fallback_icon(),
                        None,
                        false,
                    )
                }
            }
            _ => {
                if let Some(e) = resolve_by_title(&w.title) {
                    (
                        Some(e.id.clone()),
                        e.id.to_lowercase(),
                        e.name.clone(),
                        icon_or_fallback(e),
                        Some(e.exec.clone()),
                        true,
                    )
                } else {
                    (
                        None,
                        format!("win:{}", w.id),
                        window_label(w),
                        fallback_icon(),
                        None,
                        false,
                    )
                }
            }
        };

        if let Some(&i) = index.get(&key) {
            // A pinned group created from an unresolved app_id shows a placeholder
            // name; once its window is live, adopt the real window title (and a
            // title-resolved icon) so the tooltip reads the app, not the raw id.
            if !groups[i].resolved {
                groups[i].name = window_label(w);
                if let Some(e) = resolve_by_title(&w.title) {
                    groups[i].icon = icon_or_fallback(e);
                    groups[i].resolved = true;
                }
            }
            groups[i].windows.push(w.clone());
        } else {
            index.insert(key, groups.len());
            groups.push(Group {
                pin_id,
                name,
                icon,
                exec,
                windows: vec![w.clone()],
                pinned: false,
                resolved,
            });
        }
    }

    let mut shown = 0;
    for group in &groups {
        if !group.pinned && group.windows.is_empty() {
            continue;
        }
        row.append(&task_button(group, snap.focused));
        shown += 1;
    }
    tracing::info!(buttons = shown, "tasks: dock rebuilt");
}

fn window_label(w: &WindowInfo) -> String {
    if w.title.trim().is_empty() {
        "Application".to_string()
    } else {
        w.title.clone()
    }
}

fn task_button(group: &Group, focused: Option<u32>) -> gtk::Button {
    let btn = gtk::Button::builder().has_frame(false).build();
    btn.add_css_class("metis-bar-widget");
    btn.add_css_class("metis-bar-task");

    let running = !group.windows.is_empty();
    let is_focused = focused.is_some_and(|f| group.windows.iter().any(|w| w.id == f));
    let all_minimized = running && group.windows.iter().all(|w| w.minimized);
    if running {
        btn.add_css_class("running");
    }
    if is_focused {
        btn.add_css_class("focused");
    }
    if all_minimized {
        btn.add_css_class("minimized");
    }

    let overlay = gtk::Overlay::new();
    let image = gtk::Image::from_gicon(&group.icon);
    image.set_pixel_size(TASK_ICON_SIZE);
    overlay.set_child(Some(&image));

    let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    dot.add_css_class("metis-bar-task-dot");
    dot.set_halign(gtk::Align::Center);
    dot.set_valign(gtk::Align::End);
    dot.set_can_target(false);
    dot.set_visible(running);
    overlay.add_overlay(&dot);

    btn.set_child(Some(&overlay));

    let tip = if group.windows.len() > 1 {
        format!("{} ({})", group.name, group.windows.len())
    } else {
        group.name.clone()
    };
    btn.set_tooltip_text(Some(&tip));

    {
        let windows = group.windows.clone();
        let exec = group.exec.clone();
        let name = group.name.clone();
        let btn_weak = btn.downgrade();
        btn.connect_clicked(move |_| match windows.len() {
            0 => {
                if let Some(exec) = &exec {
                    if let Err(err) = crate::compositor::launch_program(exec) {
                        tracing::warn!(%err, "failed to launch pinned app");
                    }
                }
            }
            1 => toggle_window(&windows[0], focused),
            _ => {
                if let Some(btn) = btn_weak.upgrade() {
                    show_picker(&btn, &windows, &name, focused);
                }
            }
        });
    }

    attach_context_menu(&btn, group);
    btn
}

fn toggle_window(w: &WindowInfo, focused: Option<u32>) {
    let is_focused = focused == Some(w.id);
    let result = if is_focused && !w.minimized {
        crate::compositor::set_minimized(w.id, true)
    } else {
        crate::compositor::activate_window(w.id)
    };
    if let Err(err) = result {
        tracing::warn!(%err, id = w.id, "failed to toggle window");
    }
}

/// Window picker for a multi-window app: one row per window, click toggles it.
fn show_picker(parent: &gtk::Button, windows: &[WindowInfo], name: &str, focused: Option<u32>) {
    let panel = super::super::dropdown::build_panel();
    panel.add_css_class("metis-bar-tasks-picker");
    panel.set_spacing(2);
    panel.set_width_request(240);

    let title = gtk::Label::builder()
        .label(name)
        .halign(gtk::Align::Start)
        .build();
    title.add_css_class("metis-bar-section-title");
    panel.append(&title);

    let popover = transient_popover(parent, &panel);

    for w in windows {
        let row = gtk::Button::builder().has_frame(false).build();
        row.add_css_class("metis-bar-task-pick");
        let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let label = gtk::Label::new(Some(&window_label(w)));
        label.set_halign(gtk::Align::Start);
        label.set_xalign(0.0);
        label.set_hexpand(true);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_max_width_chars(28);
        if focused == Some(w.id) {
            row.add_css_class("focused");
        }
        if w.minimized {
            row.add_css_class("minimized");
        }
        hbox.append(&label);
        row.set_child(Some(&hbox));

        let w = w.clone();
        let popover = popover.clone();
        row.connect_clicked(move |_| {
            popover.popdown();
            toggle_window(&w, focused);
        });
        panel.append(&row);
    }

    glib::idle_add_local_once(move || popover.popup());
}

/// Right-click menu: pin/unpin from the dock and close the app's windows.
fn attach_context_menu(btn: &gtk::Button, group: &Group) {
    let gesture = gtk::GestureClick::builder()
        .button(gdk::BUTTON_SECONDARY)
        .build();

    let pin_id = group.pin_id.clone();
    let pinned = group.pinned;
    let window_ids: Vec<u32> = group.windows.iter().map(|w| w.id).collect();
    let btn_weak = btn.downgrade();

    gesture.connect_pressed(move |_, _, _, _| {
        let Some(btn) = btn_weak.upgrade() else {
            return;
        };
        let panel = super::super::dropdown::build_panel();
        panel.add_css_class("metis-bar-tasks-menu");
        panel.set_spacing(2);
        panel.set_width_request(180);

        let popover = transient_popover(&btn, &panel);

        if let Some(id) = pin_id.clone() {
            let label = if pinned {
                "Unpin from taskbar"
            } else {
                "Pin to taskbar"
            };
            let item = menu_item(label);
            let popover_c = popover.clone();
            item.connect_clicked(move |_| {
                popover_c.popdown();
                toggle_taskbar_pin(&id);
            });
            panel.append(&item);
        }

        if !window_ids.is_empty() {
            let label = if window_ids.len() > 1 {
                "Close all windows"
            } else {
                "Close window"
            };
            let item = menu_item(label);
            let ids = window_ids.clone();
            let popover_c = popover.clone();
            item.connect_clicked(move |_| {
                popover_c.popdown();
                for id in &ids {
                    if let Err(err) = crate::compositor::close_window(*id) {
                        tracing::warn!(%err, id, "failed to close window");
                    }
                }
            });
            panel.append(&item);
        }

        glib::idle_add_local_once(move || popover.popup());
    });

    btn.add_controller(gesture);
}

fn menu_item(label: &str) -> gtk::Button {
    let item = gtk::Button::builder()
        .label(label)
        .has_frame(false)
        .build();
    item.add_css_class("metis-bar-task-menu-item");
    if let Some(child) = item.child() {
        if let Ok(lbl) = child.downcast::<gtk::Label>() {
            lbl.set_halign(gtk::Align::Start);
            lbl.set_xalign(0.0);
        }
    }
    item
}

/// Build a non-autohide popover parented to `parent` that unparents itself when
/// closed (task buttons are rebuilt on every window change, so popovers must not
/// outlive their button). Registered with the dropdown manager so the
/// compositor "close-popovers" signal and single-open behavior still apply.
fn transient_popover(parent: &impl IsA<gtk::Widget>, panel: &gtk::Box) -> gtk::Popover {
    super::super::dropdown::close_all();
    let popover = gtk::Popover::builder()
        .autohide(false)
        .has_arrow(true)
        .position(gtk::PositionType::Bottom)
        .child(panel)
        .build();
    popover.add_css_class("metis-bar-popover");
    popover.set_parent(parent);
    super::super::dropdown::register(&popover);

    let weak = popover.downgrade();
    popover.connect_closed(move |_| {
        let weak = weak.clone();
        glib::idle_add_local_once(move || {
            if let Some(p) = weak.upgrade() {
                p.unparent();
            }
        });
    });

    popover
}

fn toggle_taskbar_pin(id: &str) {
    let mut cfg = load_bar_config();
    let needle = id.to_lowercase();
    if let Some(pos) = cfg
        .taskbar_pinned
        .iter()
        .position(|p| p.to_lowercase() == needle)
    {
        cfg.taskbar_pinned.remove(pos);
    } else {
        cfg.taskbar_pinned.push(id.to_string());
    }
    if let Err(err) = save_bar_config(&cfg) {
        tracing::warn!(%err, "failed to persist bar.json after taskbar pin toggle");
    }
}
