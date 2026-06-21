//! The Metis app-menu popover: an ArcMenu-style panel with a utility/power rail,
//! a Frequent Apps + alphabetical list (with apps-only search), and a Pinned grid.
//!
//! It reuses the bar's non-autohide popover scheme (see `dropdown.rs`): no popup
//! grab (the compositor ignores those), dismissed via toggle and the compositor
//! "close-popovers" signal. Search keyboard focus is grabbed synchronously on
//! `popover.connect_map`, the same pattern the network/clock entries rely on.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::gdk;
use gtk::prelude::*;

use crate::services::{applications, AppEntry};

const APP_ICON_SIZE: i32 = 24;
const PIN_ICON_SIZE: i32 = 34;
const FREQUENT_LIMIT: usize = 8;

/// Build the menu popover and wire it to `button` (the brand launcher button).
pub fn install(button: &gtk::Button) {
    let panel = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    panel.add_css_class("metis-bar-dropdown-panel");
    panel.add_css_class("metis-menu-panel");

    // ---- Left rail: quick launchers + power actions ----
    let rail = build_rail();
    panel.append(&rail);

    // ---- Center column: header + scrollable app list + search ----
    let center = gtk::Box::new(gtk::Orientation::Vertical, 8);
    center.add_css_class("metis-menu-center");

    let header = gtk::Label::builder()
        .label("Frequent Apps")
        .halign(gtk::Align::Start)
        .build();
    header.add_css_class("metis-bar-section-title");
    center.append(&header);

    let apps_container = gtk::Box::new(gtk::Orientation::Vertical, 2);
    apps_container.add_css_class("metis-menu-list");
    let apps_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .vexpand(true)
        .child(&apps_container)
        .build();
    apps_scroll.add_css_class("metis-menu-scroll");
    center.append(&apps_scroll);

    let search = gtk::SearchEntry::builder()
        .placeholder_text("Search applications…")
        .build();
    search.add_css_class("metis-menu-search");
    center.append(&search);

    panel.append(&center);

    // ---- Vertical divider ----
    let divider = gtk::Separator::new(gtk::Orientation::Vertical);
    divider.add_css_class("metis-menu-divider");
    panel.append(&divider);

    // ---- Right column: pinned grid ----
    let pinned_col = gtk::Box::new(gtk::Orientation::Vertical, 8);
    pinned_col.add_css_class("metis-menu-pinned");
    let pinned_header = gtk::Label::builder()
        .label("Pinned")
        .halign(gtk::Align::Start)
        .build();
    pinned_header.add_css_class("metis-bar-section-title");
    pinned_col.append(&pinned_header);

    let pinned_hint = gtk::Label::new(Some("Right-click an app to pin it here."));
    pinned_hint.add_css_class("metis-menu-empty");
    pinned_hint.set_wrap(true);
    pinned_hint.set_halign(gtk::Align::Start);
    pinned_hint.set_valign(gtk::Align::Start);
    pinned_hint.set_xalign(0.0);
    pinned_hint.set_visible(false);
    pinned_col.append(&pinned_hint);

    let pinned_flow = gtk::FlowBox::builder()
        .orientation(gtk::Orientation::Horizontal)
        .selection_mode(gtk::SelectionMode::None)
        .min_children_per_line(3)
        .max_children_per_line(3)
        .homogeneous(true)
        .row_spacing(6)
        .column_spacing(6)
        .build();
    pinned_flow.add_css_class("metis-menu-pinned-flow");
    pinned_flow.set_valign(gtk::Align::Start);
    // Wrap the grid in a plain Box: a Box never stretches its non-expanding
    // children, so the tiles keep their natural height and pack at the top
    // instead of the FlowBox absorbing the tall column's extra vertical space
    // (which otherwise inflates each row and spreads them far apart).
    let pinned_wrap = gtk::Box::new(gtk::Orientation::Vertical, 0);
    pinned_wrap.set_valign(gtk::Align::Start);
    pinned_wrap.append(&pinned_flow);
    let pinned_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .vexpand(true)
        .child(&pinned_wrap)
        .build();
    pinned_scroll.add_css_class("metis-menu-scroll");
    pinned_col.append(&pinned_scroll);
    panel.append(&pinned_col);

    // ---- Rebuild plumbing ----
    // A shared `refresh` handle lets row/tile context actions (pin/unpin) trigger
    // a full repopulate. It dispatches through a slot so it can reference the
    // rebuild closure that is defined just below it.
    let rebuild_slot: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));
    let refresh: Rc<dyn Fn()> = {
        let slot = rebuild_slot.clone();
        Rc::new(move || {
            let f = slot.borrow().clone();
            if let Some(f) = f {
                f();
            }
        })
    };

    let rebuild: Rc<dyn Fn()> = {
        let apps_container = apps_container.clone();
        let pinned_flow = pinned_flow.clone();
        let pinned_hint = pinned_hint.clone();
        let header = header.clone();
        let search = search.clone();
        let refresh = refresh.clone();
        Rc::new(move || {
            let query = search.text().to_string();
            populate_center(&apps_container, &header, &query, &refresh);
            populate_pinned(&pinned_flow, &pinned_hint, &refresh);
        })
    };
    *rebuild_slot.borrow_mut() = Some(rebuild.clone());

    {
        let rebuild = rebuild.clone();
        search.connect_search_changed(move |_| rebuild());
    }

    // ---- Popover (non-autohide; mirrors dropdown::wire_toggle) ----
    let popover = gtk::Popover::builder()
        .autohide(false)
        .has_arrow(true)
        .position(gtk::PositionType::Bottom)
        .child(&panel)
        .build();
    popover.add_css_class("metis-bar-popover");
    popover.add_css_class("metis-menu-popover");
    popover.set_parent(button);

    {
        let btn = button.clone();
        let rebuild = rebuild.clone();
        let search = search.clone();
        popover.connect_map(move |_| {
            btn.add_css_class("metis-bar-dropdown-active");
            // Reset to the default (Frequent) view each open, then focus search so
            // typing filters immediately. The entry is realized by map time, so a
            // synchronous grab_focus lands on this OnDemand layer surface.
            search.set_text("");
            rebuild();
            search.grab_focus();
        });
    }
    {
        let btn = button.clone();
        popover.connect_unmap(move |_| {
            btn.remove_css_class("metis-bar-dropdown-active");
        });
    }

    super::super::dropdown::register(&popover);

    let popover_weak = popover.downgrade();
    button.connect_clicked(move |_| {
        let Some(popover) = popover_weak.upgrade() else {
            return;
        };
        if popover.is_visible() {
            glib::idle_add_local_once(move || popover.popdown());
            return;
        }
        super::super::dropdown::close_all();
        glib::idle_add_local_once(move || popover.popup());
    });
}

fn build_rail() -> gtk::Box {
    let rail = gtk::Box::new(gtk::Orientation::Vertical, 6);
    rail.add_css_class("metis-menu-rail");

    rail.append(&rail_button("system-file-manager-symbolic", "Files", || {
        launch_first(FILE_MANAGERS)
    }));
    rail.append(&rail_button("utilities-terminal-symbolic", "Terminal", || {
        launch_first(TERMINALS)
    }));
    rail.append(&rail_button("preferences-system-symbolic", "Settings", || {
        if let Err(err) = crate::compositor::launch_program("metis-settings") {
            tracing::warn!(%err, "failed to launch metis-settings");
        }
        super::super::dropdown::request_close_all();
    }));

    let spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    spacer.set_vexpand(true);
    rail.append(&spacer);

    rail.append(&rail_button("system-lock-screen-symbolic", "Suspend", || {
        run_detached("systemctl", &["suspend"]);
        super::super::dropdown::request_close_all();
    }));
    rail.append(&rail_button("system-log-out-symbolic", "Log Out", || {
        if let Err(err) = crate::compositor::end_session() {
            tracing::warn!(%err, "failed to end session");
        }
    }));
    rail.append(&rail_button("system-reboot-symbolic", "Restart", || {
        run_detached("systemctl", &["reboot"]);
        super::super::dropdown::request_close_all();
    }));
    rail.append(&rail_button("system-shutdown-symbolic", "Shut Down", || {
        run_detached("systemctl", &["poweroff"]);
        super::super::dropdown::request_close_all();
    }));

    rail
}

fn rail_button(icon: &str, label: &str, on_click: impl Fn() + 'static) -> gtk::Button {
    let btn = gtk::Button::builder().has_frame(false).build();
    btn.add_css_class("metis-menu-rail-btn");
    let image = gtk::Image::from_icon_name(icon);
    image.set_pixel_size(18);
    btn.set_child(Some(&image));
    btn.connect_clicked(move |_| on_click());
    attach_tooltip(&btn, label, gtk::PositionType::Right);
    btn
}

/// Floating tooltip for an icon-only control.
///
/// GTK's built-in tooltips don't present on this non-autohide, grab-less
/// layer-shell popover, so we drive our own: a small non-autohide child
/// `Popover` (the same popup mechanism the menu itself uses, which the
/// compositor renders) popped after a short hover and popped down on leave.
/// `set_tooltip_text` is kept for accessibility.
fn attach_tooltip(widget: &impl IsA<gtk::Widget>, text: &str, side: gtk::PositionType) {
    widget.set_tooltip_text(Some(text));

    let tip: Rc<RefCell<Option<gtk::Popover>>> = Rc::new(RefCell::new(None));
    let timer: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));

    let hide = {
        let tip = tip.clone();
        let timer = timer.clone();
        Rc::new(move || {
            if let Some(id) = timer.borrow_mut().take() {
                id.remove();
            }
            if let Some(pop) = tip.borrow_mut().take() {
                pop.popdown();
                pop.unparent();
            }
        })
    };

    let motion = gtk::EventControllerMotion::new();
    {
        let widget_weak = widget.clone().upcast::<gtk::Widget>().downgrade();
        let text = text.to_string();
        let tip = tip.clone();
        let timer = timer.clone();
        let hide = hide.clone();
        motion.connect_enter(move |_, _, _| {
            hide();
            let widget_weak = widget_weak.clone();
            let text = text.clone();
            let tip = tip.clone();
            let timer_inner = timer.clone();
            let id = glib::timeout_add_local_once(std::time::Duration::from_millis(450), move || {
                *timer_inner.borrow_mut() = None;
                let Some(w) = widget_weak.upgrade() else {
                    return;
                };
                let pop = gtk::Popover::builder()
                    .autohide(false)
                    .has_arrow(false)
                    .position(side)
                    .can_focus(false)
                    .build();
                pop.add_css_class("metis-menu-tooltip");
                pop.set_child(Some(&gtk::Label::new(Some(&text))));
                pop.set_parent(&w);
                pop.popup();
                *tip.borrow_mut() = Some(pop);
            });
            *timer.borrow_mut() = Some(id);
        });
    }
    {
        let hide = hide.clone();
        motion.connect_leave(move |_| hide());
    }
    widget.add_controller(motion);
}

fn populate_center(
    container: &gtk::Box,
    header: &gtk::Label,
    query: &str,
    refresh: &Rc<dyn Fn()>,
) {
    clear_box(container);
    let q = query.trim();
    if q.is_empty() {
        header.set_text("Frequent Apps");
        for entry in applications::frequent(FREQUENT_LIMIT) {
            container.append(&app_row(&entry, refresh));
        }
        let mut last_letter = '\0';
        for entry in applications::list_apps() {
            let letter = entry
                .name
                .chars()
                .next()
                .map(|c| c.to_ascii_uppercase())
                .unwrap_or('#');
            if letter != last_letter {
                last_letter = letter;
                container.append(&letter_header(letter));
            }
            container.append(&app_row(&entry, refresh));
        }
    } else {
        header.set_text("Search Results");
        let results = applications::search(q);
        if results.is_empty() {
            let empty = gtk::Label::new(Some("No matching applications"));
            empty.add_css_class("metis-menu-empty");
            empty.set_halign(gtk::Align::Start);
            container.append(&empty);
        }
        for entry in results {
            container.append(&app_row(&entry, refresh));
        }
    }
}

fn populate_pinned(flow: &gtk::FlowBox, hint: &gtk::Label, refresh: &Rc<dyn Fn()>) {
    while let Some(child) = flow.first_child() {
        flow.remove(&child);
    }
    let pinned = applications::pinned_entries();
    if pinned.is_empty() {
        hint.set_visible(true);
        flow.set_visible(false);
        return;
    }
    hint.set_visible(false);
    flow.set_visible(true);
    for entry in pinned {
        flow.append(&pinned_tile(&entry, refresh));
    }
}

fn app_row(entry: &AppEntry, refresh: &Rc<dyn Fn()>) -> gtk::Button {
    let row = gtk::Button::builder().has_frame(false).build();
    row.add_css_class("metis-menu-row");

    let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    hbox.append(&app_image(entry, APP_ICON_SIZE));

    let label = gtk::Label::new(Some(&entry.name));
    label.set_halign(gtk::Align::Start);
    label.set_xalign(0.0);
    label.set_hexpand(true);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    hbox.append(&label);
    row.set_child(Some(&hbox));

    {
        let entry = entry.clone();
        row.connect_clicked(move |_| {
            applications::launch(&entry);
            super::super::dropdown::request_close_all();
        });
    }
    attach_pin_gesture(&row, &entry.id, refresh);
    row
}

fn pinned_tile(entry: &AppEntry, refresh: &Rc<dyn Fn()>) -> gtk::Button {
    let tile = gtk::Button::builder().has_frame(false).build();
    tile.add_css_class("metis-menu-tile");

    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 4);
    vbox.set_halign(gtk::Align::Center);
    vbox.append(&app_image(entry, PIN_ICON_SIZE));

    let label = gtk::Label::new(Some(&entry.name));
    label.add_css_class("metis-menu-tile-label");
    label.set_justify(gtk::Justification::Center);
    label.set_wrap(true);
    label.set_lines(2);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label.set_max_width_chars(10);
    vbox.append(&label);
    tile.set_child(Some(&vbox));

    {
        let entry = entry.clone();
        tile.connect_clicked(move |_| {
            applications::launch(&entry);
            super::super::dropdown::request_close_all();
        });
    }
    attach_pin_gesture(&tile, &entry.id, refresh);
    tile
}

/// Right-click toggles the app's pinned state and refreshes the menu in place.
fn attach_pin_gesture(widget: &impl IsA<gtk::Widget>, id: &str, refresh: &Rc<dyn Fn()>) {
    let gesture = gtk::GestureClick::builder()
        .button(gdk::BUTTON_SECONDARY)
        .build();
    let id = id.to_string();
    let refresh = refresh.clone();
    gesture.connect_pressed(move |_, _, _, _| {
        applications::toggle_pin(&id);
        refresh();
    });
    widget.add_controller(gesture);
}

fn app_image(entry: &AppEntry, size: i32) -> gtk::Image {
    let image = gtk::Image::new();
    image.set_pixel_size(size);
    if let Some(icon) = &entry.icon {
        image.set_from_gicon(icon);
    } else {
        image.set_from_icon_name(Some("application-x-executable-symbolic"));
    }
    image
}

fn letter_header(letter: char) -> gtk::Label {
    let label = gtk::Label::new(Some(&letter.to_string()));
    label.add_css_class("metis-menu-letter");
    label.set_halign(gtk::Align::Start);
    label.set_xalign(0.0);
    label
}

fn clear_box(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

/// File managers / terminals to try, in order. Passed straight to the compositor,
/// which runs any space-containing program via `sh -lc`, so `$VAR` expansion and
/// `command -v` probing work as written.
const FILE_MANAGERS: &str =
    r#"for f in "$FILE_MANAGER" nautilus dolphin nemo thunar pcmanfm caja; do command -v "$f" >/dev/null 2>&1 && exec "$f" "$HOME"; done; exec xdg-open "$HOME""#;
const TERMINALS: &str =
    r#"for t in "$TERMINAL" x-terminal-emulator kgx gnome-terminal konsole foot alacritty kitty xterm; do command -v "$t" >/dev/null 2>&1 && exec "$t"; done"#;

fn launch_first(snippet: &str) {
    if let Err(err) = crate::compositor::launch_program(snippet) {
        tracing::warn!(%err, "failed to launch quick action");
    }
    super::super::dropdown::request_close_all();
}

fn run_detached(cmd: &str, args: &[&str]) {
    if let Err(err) = std::process::Command::new(cmd).args(args).spawn() {
        tracing::warn!(%err, cmd, "failed to spawn power action");
    }
}
