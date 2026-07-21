//! The Metis app-menu popover: an ArcMenu-style panel with a utility/power rail,
//! a Frequent Apps + alphabetical list (with apps-only search), and a Pinned grid.
//!
//! It reuses the bar's non-autohide popover scheme (see `dropdown.rs`): no popup
//! grab (the compositor ignores those), dismissed via toggle and the compositor
//! "close-popovers" signal. The alphabetical app list is filled incrementally on
//! idle; that must not steal keyboard focus from the search entry while typing.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::gdk;
use gtk::prelude::*;
use gtk4_layer_shell::{KeyboardMode, LayerShell};

use crate::services::{applications, AppEntry};

const APP_ICON_SIZE: i32 = 24;
const PIN_ICON_SIZE: i32 = 34;
const FREQUENT_LIMIT: usize = 8;
/// Alphabetical rows appended per idle slice so opening the menu never blocks the
/// GTK main loop (and the nested compositor Wayland socket) for one giant rebuild.
const MENU_ALPHA_CHUNK: usize = 32;

thread_local! {
    /// Menu instances for every output. Weak handles avoid keeping bars alive
    /// across a live bar rebuild.
    static MENU_POPOVERS: RefCell<Vec<glib::WeakRef<gtk::Popover>>> =
        const { RefCell::new(Vec::new()) };
}

/// Toggle the first live Metis menu. Used by the compositor's standalone Super
/// shortcut; pointer-opened menus use the same popover and focus behavior.
pub(crate) fn request_toggle() {
    let target = MENU_POPOVERS.with(|menus| {
        let mut menus = menus.borrow_mut();
        menus.retain(|weak| weak.upgrade().is_some());
        let live: Vec<gtk::Popover> = menus.iter().filter_map(glib::WeakRef::upgrade).collect();
        live.iter()
            .find(|popover| popover.is_visible())
            .cloned()
            .or_else(|| live.into_iter().next())
    });
    let Some(popover) = target else {
        tracing::debug!("menu toggle requested before a bar menu was available");
        return;
    };
    if popover.is_visible() {
        glib::idle_add_local_once(move || popover.popdown());
    } else {
        super::super::dropdown::close_all();
        glib::idle_add_local_once(move || popover.popup());
    }
}

/// Build the menu popover and wire it to `button` (the brand launcher button).
pub fn install(button: &gtk::Button) {
    let panel = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    panel.add_css_class("metis-bar-dropdown-panel");
    panel.add_css_class("metis-menu-panel");

    // The rail's icon tooltips render as a label inside this overlay (part of the
    // menu's own surface) rather than a child popup, so they always paint on top of
    // the panel — a separate popup gets stacked *behind* the translucent menu.
    let overlay = gtk::Overlay::new();
    let tip = gtk::Label::new(None);
    tip.add_css_class("metis-menu-tooltip-label");
    tip.set_halign(gtk::Align::Start);
    tip.set_valign(gtk::Align::Start);
    tip.set_can_target(false);
    tip.set_visible(false);

    // ---- Left rail: quick launchers + power actions ----
    let rail = build_rail(&overlay, &tip);
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
    apps_container.set_hexpand(true);
    apps_container.set_halign(gtk::Align::Fill);
    let apps_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .vexpand(true)
        .hexpand(true)
        .child(&apps_container)
        .build();
    apps_scroll.add_css_class("metis-menu-scroll");
    // A single Capture-phase controller on the scrolled window intercepts wheel
    // events for its whole subtree (rows + transparent gutters) before the row
    // buttons can swallow them — no per-widget wiring needed.
    wire_vertical_scroll(&apps_scroll, &apps_scroll);
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
    wire_vertical_scroll(&pinned_scroll, &pinned_scroll);
    pinned_col.append(&pinned_scroll);
    panel.append(&pinned_col);

    overlay.set_child(Some(&panel));
    overlay.add_overlay(&tip);

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

    let list_generation = Rc::new(Cell::new(0_u64));
    let search_debounce: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));

    let rebuild: Rc<dyn Fn()> = {
        let apps_container = apps_container.clone();
        let pinned_flow = pinned_flow.clone();
        let pinned_hint = pinned_hint.clone();
        let header = header.clone();
        let search = search.clone();
        let refresh = refresh.clone();
        let list_generation = list_generation.clone();
        Rc::new(move || {
            let keep_search_focus = search.has_focus();
            let query = search.text().to_string();
            let apps = applications::list_apps();
            populate_center(
                &apps_container,
                &header,
                &query,
                &apps,
                &refresh,
                &search,
                &list_generation,
            );
            if query.trim().is_empty() {
                populate_pinned(&pinned_flow, &pinned_hint, &apps, &refresh);
            }
            restore_search_focus(&search, keep_search_focus);
        })
    };
    *rebuild_slot.borrow_mut() = Some(rebuild.clone());
    applications::register_refresh(rebuild.clone());

    let search_changed = {
        let rebuild = rebuild.clone();
        let search_debounce = search_debounce.clone();
        search.connect_search_changed(move |_| {
            if let Some(id) = search_debounce.borrow_mut().take() {
                id.remove();
            }
            let rebuild = rebuild.clone();
            let debounce_slot = search_debounce.clone();
            let id = glib::timeout_add_local(std::time::Duration::from_millis(40), move || {
                *debounce_slot.borrow_mut() = None;
                rebuild();
                glib::ControlFlow::Break
            });
            *search_debounce.borrow_mut() = Some(id);
        })
    };

    // ---- Popover (non-autohide; mirrors dropdown::wire_toggle) ----
    let popover = gtk::Popover::builder()
        .autohide(false)
        .has_arrow(true)
        .position(super::super::popover_position())
        .child(&overlay)
        .build();
    popover.add_css_class("metis-bar-popover");
    popover.add_css_class("metis-menu-popover");
    popover.set_parent(button);
    MENU_POPOVERS.with(|menus| menus.borrow_mut().push(popover.downgrade()));

    // Type-to-search without forcing focus on open: key capture routes typing
    // anywhere in the popover to the search entry (which only grabs focus once you
    // start typing). Scroll is positional, not focus-based, so this never steals
    // wheel events from the app list.
    search.set_key_capture_widget(Some(&popover));

    {
        let btn = button.clone();
        let rebuild = rebuild.clone();
        let search = search.clone();
        popover.connect_map(move |popover| {
            btn.add_css_class("metis-bar-dropdown-active");
            if let Some(window) = popover.root().and_downcast::<gtk::Window>() {
                window.set_keyboard_mode(KeyboardMode::Exclusive);
            }
            // Clearing the search entry fires `search_changed`, which would
            // synchronously rebuild the entire app list during `map` and freeze
            // the nested session — block it and defer one rebuild on idle instead.
            search.block_signal(&search_changed);
            search.set_text("");
            search.unblock_signal(&search_changed);
            applications::invalidate_app_cache();
            let rebuild = rebuild.clone();
            glib::idle_add_local_once(move || rebuild());
            // Super-key opens never get a pointer click to claim OnDemand focus;
            // Exclusive is set above and the compositor routes keys to this layer.
            // Grab (and re-grab once after a tick) so SearchEntry is the GTK focus
            // target as soon as the layer owns the seat.
            search.grab_focus();
            let search_again = search.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(50), move || {
                if !search_again.has_focus() {
                    search_again.grab_focus();
                }
            });
        });
    }
    {
        let btn = button.clone();
        popover.connect_unmap(move |popover| {
            btn.remove_css_class("metis-bar-dropdown-active");
            if let Some(window) = popover.root().and_downcast::<gtk::Window>() {
                window.set_keyboard_mode(KeyboardMode::OnDemand);
            }
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

fn build_rail(overlay: &gtk::Overlay, tip: &gtk::Label) -> gtk::Box {
    let rail = gtk::Box::new(gtk::Orientation::Vertical, 6);
    rail.add_css_class("metis-menu-rail");

    rail.append(&rail_button(overlay, tip, "system-file-manager-symbolic", "Files", || {
        launch_quick_action(launch_file_manager_snippet())
    }));
    rail.append(&rail_button(overlay, tip, "utilities-terminal-symbolic", "Terminal", || {
        launch_quick_action(launch_terminal_snippet())
    }));
    rail.append(&rail_button(overlay, tip, "preferences-system-symbolic", "Settings", || {
        super::super::dropdown::close_all();
        activate_or_launch_settings();
    }));

    // Controller-friendly Steam Big Picture, shown only when Steam is installed
    // (native on PATH or Flatpak). Absent entirely on non-gaming setups.
    if let Some(cmd) = applications::steam_big_picture_command() {
        rail.append(&rail_button(
            overlay,
            tip,
            "input-gaming-symbolic",
            "Big Picture",
            move || launch_quick_action(cmd.clone()),
        ));
    }

    let spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    spacer.set_vexpand(true);
    rail.append(&spacer);

    rail.append(&rail_button(overlay, tip, "system-lock-screen-symbolic", "Lock", || {
        if let Err(err) = crate::compositor::lock_session() {
            tracing::warn!(%err, "compositor lock_session failed");
        }
        super::super::dropdown::request_close_all();
    }));
    rail.append(&rail_button(overlay, tip, "weather-clear-night-symbolic", "Suspend", || {
        run_detached("systemctl", &["suspend"]);
        super::super::dropdown::request_close_all();
    }));
    rail.append(&rail_button(overlay, tip, "system-log-out-symbolic", "Log Out", || {
        if let Err(err) = crate::compositor::end_session() {
            tracing::warn!(%err, "compositor end_session failed — falling back to loginctl");
            if std::env::var_os("XDG_SESSION_ID").is_some() {
                run_detached("loginctl", &["terminate-session", "self"]);
            } else if let Ok(user) = std::env::var("USER") {
                run_detached("loginctl", &["terminate-user", &user]);
            }
        }
        super::super::dropdown::request_close_all();
    }));
    rail.append(&rail_button(overlay, tip, "system-reboot-symbolic", "Restart", || {
        run_detached("systemctl", &["reboot"]);
        super::super::dropdown::request_close_all();
    }));
    rail.append(&rail_button(overlay, tip, "system-shutdown-symbolic", "Shut Down", || {
        run_detached("systemctl", &["poweroff"]);
        super::super::dropdown::request_close_all();
    }));

    rail
}

fn activate_or_launch_settings() {
    const SETTINGS_APP_ID: &str = "com.metis.Settings";
    let existing = match crate::compositor::list_windows() {
        Ok(windows) => windows
            .into_iter()
            .filter(|window| {
                window
                    .app_id
                    .as_deref()
                    .is_some_and(|app_id| app_id.eq_ignore_ascii_case(SETTINGS_APP_ID))
            })
            .min_by_key(|window| (!window.focused, window.id)),
        Err(err) => {
            tracing::debug!(%err, "could not query open windows before launching Settings");
            None
        }
    };

    if let Some(window) = existing {
        match crate::compositor::activate_window(window.id) {
            Ok(()) => return,
            Err(err) => {
                tracing::warn!(%err, id = window.id, "failed to restore existing Metis Settings");
            }
        }
    }
    if let Err(err) = crate::compositor::launch_program("metis-settings") {
        tracing::warn!(%err, "failed to launch metis-settings");
    }
}

fn rail_button(
    overlay: &gtk::Overlay,
    tip: &gtk::Label,
    icon: &str,
    label: &str,
    on_click: impl Fn() + 'static,
) -> gtk::Button {
    let btn = gtk::Button::builder().has_frame(false).build();
    btn.add_css_class("metis-menu-rail-btn");
    let image = gtk::Image::from_icon_name(icon);
    image.set_pixel_size(18);
    btn.set_child(Some(&image));
    btn.connect_clicked(move |_| on_click());
    attach_tooltip(&btn, label, overlay, tip);
    btn
}

/// Tooltip for an icon-only rail control.
///
/// GTK's built-in tooltips don't behave on this non-autohide, grab-less
/// layer-shell popover (and in the nested session they now present as a separate
/// window the compositor stacks *behind* the translucent menu), so we drive our
/// own using a single shared `Label` living in the menu's `GtkOverlay`. Because it
/// is part of the menu's own surface it always paints on top of the panel; we just
/// move it next to the hovered button after a short delay. The accessible label is
/// set directly so screen readers still get the name.
fn attach_tooltip(
    widget: &impl IsA<gtk::Widget>,
    text: &str,
    overlay: &gtk::Overlay,
    tip: &gtk::Label,
) {
    widget
        .as_ref()
        .update_property(&[gtk::accessible::Property::Label(text)]);

    let timer: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
    let motion = gtk::EventControllerMotion::new();
    {
        let widget_weak = widget.clone().upcast::<gtk::Widget>().downgrade();
        let overlay_weak = overlay.downgrade();
        let tip = tip.clone();
        let text = text.to_string();
        let timer = timer.clone();
        motion.connect_enter(move |_, _, _| {
            if let Some(id) = timer.borrow_mut().take() {
                id.remove();
            }
            let widget_weak = widget_weak.clone();
            let overlay_weak = overlay_weak.clone();
            let tip = tip.clone();
            let text = text.clone();
            let timer_inner = timer.clone();
            let id = glib::timeout_add_local_once(std::time::Duration::from_millis(450), move || {
                *timer_inner.borrow_mut() = None;
                let (Some(w), Some(ov)) = (widget_weak.upgrade(), overlay_weak.upgrade()) else {
                    return;
                };
                tip.set_label(&text);
                // Position the tooltip just to the right of the button, vertically
                // centered, in the overlay's coordinate space.
                if let Some((x, y)) =
                    w.translate_coordinates(&ov, w.width() as f64, w.height() as f64 / 2.0)
                {
                    tip.set_margin_start((x as i32 + 8).max(0));
                    tip.set_margin_top((y as i32 - 14).max(0));
                }
                tip.set_visible(true);
            });
            *timer.borrow_mut() = Some(id);
        });
    }
    {
        let tip = tip.clone();
        let timer = timer.clone();
        motion.connect_leave(move |_| {
            if let Some(id) = timer.borrow_mut().take() {
                id.remove();
            }
            tip.set_visible(false);
        });
    }
    widget.add_controller(motion);
}

fn restore_search_focus(search: &gtk::SearchEntry, had_focus: bool) {
    if !had_focus {
        return;
    }
    let search = search.clone();
    glib::idle_add_local_once(move || {
        if !search.has_focus() {
            search.grab_focus();
        }
    });
}

fn populate_center(
    container: &gtk::Box,
    header: &gtk::Label,
    query: &str,
    apps: &[AppEntry],
    refresh: &Rc<dyn Fn()>,
    search: &gtk::SearchEntry,
    list_generation: &Rc<Cell<u64>>,
) {
    let generation = list_generation.get().wrapping_add(1);
    list_generation.set(generation);
    let keep_search_focus = search.has_focus();
    clear_box(container);
    let q = query.trim();
    if q.is_empty() {
        header.set_text("Frequent Apps");
        for entry in applications::frequent_from(apps, FREQUENT_LIMIT) {
            container.append(&app_row(&entry, refresh));
        }
        append_alpha_chunk(
            container,
            apps,
            refresh,
            list_generation,
            generation,
            0,
            '\0',
        );
    } else {
        header.set_text("Search Results");
        let results = applications::search_in(apps, q);
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
    restore_search_focus(search, keep_search_focus);
}

/// Append a slice of the alphabetical app list, scheduling the rest on idle.
fn append_alpha_chunk(
    container: &gtk::Box,
    apps: &[AppEntry],
    refresh: &Rc<dyn Fn()>,
    list_generation: &Rc<Cell<u64>>,
    generation: u64,
    start: usize,
    mut last_letter: char,
) {
    if list_generation.get() != generation {
        return;
    }
    if start >= apps.len() {
        return;
    }
    let end = (start + MENU_ALPHA_CHUNK).min(apps.len());
    for entry in &apps[start..end] {
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
        container.append(&app_row(entry, refresh));
    }
    if end < apps.len() {
        let container = container.clone();
        let apps = apps.to_vec();
        let refresh = refresh.clone();
        let list_generation = list_generation.clone();
        glib::idle_add_local_once(move || {
            append_alpha_chunk(
                &container,
                &apps,
                &refresh,
                &list_generation,
                generation,
                end,
                last_letter,
            );
        });
    }
}

fn populate_pinned(flow: &gtk::FlowBox, hint: &gtk::Label, apps: &[AppEntry], refresh: &Rc<dyn Fn()>) {
    while let Some(child) = flow.first_child() {
        flow.remove(&child);
    }
    let pinned = applications::pinned_entries_from(apps);
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

/// Drive a scrolled window's vertical adjustment from wheel events. Attached in
/// Capture phase on the `ScrolledWindow` so it intercepts the whole subtree
/// before child row buttons (which otherwise swallow scroll) and covers the blank
/// gutter beside row labels.
fn wire_vertical_scroll(widget: &impl IsA<gtk::Widget>, scroll: &gtk::ScrolledWindow) {
    let ctrl = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
    ctrl.set_propagation_phase(gtk::PropagationPhase::Capture);
    let vadj = scroll.vadjustment();
    ctrl.connect_scroll(move |_, _, dy| {
        let page = vadj.page_size();
        let upper = vadj.upper();
        let lower = vadj.lower();
        if upper - lower <= page {
            return glib::Propagation::Proceed;
        }
        let max = (upper - page).max(lower);
        let new_val = (vadj.value() + dy).clamp(lower, max);
        if (new_val - vadj.value()).abs() > f64::EPSILON {
            vadj.set_value(new_val);
        }
        glib::Propagation::Stop
    });
    widget.add_controller(ctrl);
}

fn app_row(entry: &AppEntry, refresh: &Rc<dyn Fn()>) -> gtk::Button {
    let row = gtk::Button::builder().has_frame(false).build();
    row.add_css_class("metis-menu-row");
    row.set_hexpand(true);
    row.set_halign(gtk::Align::Fill);

    let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    hbox.set_hexpand(true);
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
            // Close synchronously *before* launching: the new window grabs focus,
            // which otherwise swallows the deferred (idle) popdown and leaves the
            // menu hanging open over the app.
            super::super::dropdown::close_all();
            applications::launch(&entry);
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
            // See `app_row`: pop down before the launched window steals focus.
            super::super::dropdown::close_all();
            applications::launch(&entry);
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
    applications::set_app_icon(&image, entry, size);
    image
}

fn letter_header(letter: char) -> gtk::Label {
    let label = gtk::Label::new(Some(&letter.to_string()));
    label.add_css_class("metis-menu-letter");
    label.set_halign(gtk::Align::Start);
    label.set_xalign(0.0);
    label.set_hexpand(true);
    label
}

fn clear_box(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

/// Escape a value for safe interpolation inside a double-quoted shell word.
fn shell_dquote(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
        .replace('`', "\\`")
}

/// Build a launch snippet that tries (in order) the user's chosen program, then
/// the environment hint, then each known candidate, then `final_fallback`. Passed
/// straight to the compositor, which runs space-containing programs via `sh -lc`,
/// so `$VAR` expansion and `command -v` probing work as written.
///
/// The chosen value is probed on its own line so a custom path containing spaces
/// stays a single argument (the candidate loop can only hold whitespace-free names).
fn build_launch_snippet(
    chosen: Option<&str>,
    env_hint: &str,
    candidates: &[(&str, &str)],
    args: &str,
    final_fallback: &str,
) -> String {
    let mut snippet = String::new();
    if let Some(chosen) = chosen.map(str::trim).filter(|s| !s.is_empty()) {
        let c = shell_dquote(chosen);
        snippet.push_str(&format!(
            "if command -v \"{c}\" >/dev/null 2>&1; then exec \"{c}\"{args}; fi; "
        ));
    }
    snippet.push_str(&format!("for x in \"{env_hint}\""));
    for (bin, _) in candidates {
        snippet.push(' ');
        snippet.push_str(bin);
    }
    snippet.push_str(&format!(
        "; do command -v \"$x\" >/dev/null 2>&1 && exec \"$x\"{args}; done"
    ));
    if !final_fallback.is_empty() {
        snippet.push_str("; ");
        snippet.push_str(final_fallback);
    }
    snippet
}

fn launch_terminal_snippet() -> String {
    let cfg = metis_config::load_menu_config();
    build_launch_snippet(
        cfg.terminal.as_deref(),
        "$TERMINAL",
        metis_config::KNOWN_TERMINALS,
        "",
        "",
    )
}

fn launch_file_manager_snippet() -> String {
    let cfg = metis_config::load_menu_config();
    build_launch_snippet(
        cfg.file_manager.as_deref(),
        "$FILE_MANAGER",
        metis_config::KNOWN_FILE_MANAGERS,
        " \"$HOME\"",
        "exec xdg-open \"$HOME\"",
    )
}

fn launch_quick_action(snippet: String) {
    // Close before the launched window grabs focus (see `app_row`).
    super::super::dropdown::close_all();
    if let Err(err) = crate::compositor::launch_program(&snippet) {
        tracing::warn!(%err, "failed to launch quick action");
    }
}

fn run_detached(cmd: &str, args: &[&str]) {
    if let Err(err) = std::process::Command::new(cmd).args(args).spawn() {
        tracing::warn!(%err, cmd, "failed to spawn power action");
    }
}
