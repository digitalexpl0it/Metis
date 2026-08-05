//! Workspace overview overlay: grid of workspaces with live window cards and DnD.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::gdk;
use gtk::prelude::*;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use metis_protocol::WindowInfo;

use crate::services::windows;
use crate::ui::window_thumbs::{self, ThumbSet};

thread_local! {
    static OVERLAY: RefCell<Option<Rc<Overview>>> = const { RefCell::new(None) };
}

struct Overview {
    window: gtk::Window,
    grid: gtk::Grid,
    output: RefCell<Option<String>>,
    thumbs: RefCell<Option<ThumbSet>>,
}

pub fn is_open() -> bool {
    OVERLAY.with(|o| o.borrow().is_some())
}

pub fn toggle() {
    if is_open() {
        dismiss();
    } else {
        show();
    }
}

pub fn dismiss() {
    OVERLAY.with(|o| {
        if let Some(ov) = o.borrow_mut().take() {
            ov.window.set_visible(false);
            ov.window.destroy();
        }
    });
    restore_sibling_layers();
}

fn demote_sibling_layers() {
    crate::ui::notification_center::set_below_screenshot(true);
    crate::ui::dashboard::set_below_screenshot(true);
    crate::ui::bar::widgets::set_menu_below_screenshot(true);
}

fn restore_sibling_layers() {
    crate::ui::notification_center::set_below_screenshot(false);
    crate::ui::dashboard::set_below_screenshot(false);
    crate::ui::bar::widgets::set_menu_below_screenshot(false);
}

pub fn show() {
    if crate::ui::window_switcher::is_open() {
        crate::ui::window_switcher::dismiss();
    }
    if is_open() {
        return;
    }

    windows::reconcile_now();
    let output = windows::focused_output_name();
    let snap = windows::snapshot();
    let thumbs = window_thumbs::load_window_thumbs(&snap.windows);

    demote_sibling_layers();

    let window = gtk::Window::builder()
        .title(metis_i18n::tr("Workspace Overview"))
        .decorated(false)
        .build();
    window.add_css_class("metis-workspace-overview");
    window.init_layer_shell();
    window.set_layer(Layer::Overlay);
    window.set_keyboard_mode(KeyboardMode::Exclusive);
    window.set_namespace("metis-workspace-overview");
    if let Some(monitor) = gdk_monitor_for_output(output.as_deref()) {
        window.set_monitor(&monitor);
    }
    for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
        window.set_anchor(edge, true);
        window.set_exclusive_zone(-1);
        window.set_margin(edge, 0);
    }

    let root = gtk::Box::new(gtk::Orientation::Vertical, 10);
    root.add_css_class("metis-workspace-overview-backdrop");
    root.set_halign(gtk::Align::Fill);
    root.set_valign(gtk::Align::Fill);
    window.set_child(Some(&root));

    let title = gtk::Label::new(Some(&metis_i18n::tr("Workspaces")));
    title.add_css_class("metis-workspace-overview-title");
    title.set_halign(gtk::Align::Center);
    title.set_margin_top(20);
    root.append(&title);

    let hint = gtk::Label::new(Some(&metis_i18n::tr(
        "Click a window to focus · Drag to another workspace · Esc to close",
    )));
    hint.add_css_class("metis-workspace-overview-hint");
    hint.set_halign(gtk::Align::Center);
    root.append(&hint);

    let grid = gtk::Grid::new();
    grid.add_css_class("metis-workspace-overview-grid");
    grid.set_halign(gtk::Align::Center);
    grid.set_valign(gtk::Align::Center);
    grid.set_hexpand(true);
    grid.set_vexpand(true);
    grid.set_row_homogeneous(true);
    grid.set_column_homogeneous(true);
    grid.set_row_spacing(12);
    grid.set_column_spacing(12);
    grid.set_margin_top(8);
    grid.set_margin_bottom(24);
    root.append(&grid);

    let ov = Rc::new(Overview {
        window: window.clone(),
        grid: grid.clone(),
        output: RefCell::new(output),
        thumbs: RefCell::new(thumbs),
    });

    rebuild(&ov);
    wire_keyboard(&ov);

    OVERLAY.with(|o| *o.borrow_mut() = Some(ov.clone()));
    window.present();
    window.grab_focus();
}

pub fn on_windows_changed() {
    OVERLAY.with(|o| {
        let borrow = o.borrow();
        let Some(ov) = borrow.as_ref() else {
            return;
        };
        rebuild(ov);
    });
}

fn rebuild(ov: &Rc<Overview>) {
    while let Some(child) = ov.grid.first_child() {
        ov.grid.remove(&child);
    }

    let count = crate::services::workspace_count().max(1);
    let output = ov.output.borrow().clone();
    let active = crate::services::active_workspace_for(output.as_deref());
    let snap = windows::snapshot();

    let cols = ((count as f64).sqrt().ceil() as u32).max(1).min(4);
    for ws in 1..=count {
        let idx = ws - 1;
        let col = (idx % cols) as i32;
        let row = (idx / cols) as i32;
        let tile = build_tile(ov, ws, active, &snap.windows, output.as_deref());
        ov.grid.attach(&tile, col, row, 1, 1);
    }
}

fn windows_on_workspace(
    windows: &[WindowInfo],
    output: Option<&str>,
    workspace: u32,
) -> Vec<WindowInfo> {
    windows
        .iter()
        .filter(|w| {
            let out_ok = match output {
                Some(o) if !o.is_empty() => w.output.is_empty() || w.output == o,
                _ => true,
            };
            let ws_ok = w.workspace == 0 || w.workspace == workspace;
            out_ok && ws_ok && !w.minimized
        })
        .cloned()
        .collect()
}

const TILE_W: i32 = 220;
const TILE_H: i32 = 140;
const CARD_W: i32 = 88;
const CARD_H: i32 = 72;

fn build_tile(
    ov: &Rc<Overview>,
    workspace: u32,
    active: u32,
    windows: &[WindowInfo],
    output: Option<&str>,
) -> gtk::Overlay {
    let frame = gtk::Overlay::new();
    frame.add_css_class("metis-workspace-overview-tile");
    if workspace == active {
        frame.add_css_class("active");
    }
    frame.set_size_request(TILE_W, TILE_H);
    frame.set_hexpand(false);
    frame.set_vexpand(false);

    let body = gtk::Box::new(gtk::Orientation::Vertical, 4);
    body.set_margin_start(6);
    body.set_margin_end(6);
    body.set_margin_top(4);
    body.set_margin_bottom(6);
    frame.set_child(Some(&body));

    let header = gtk::Button::new();
    header.add_css_class("metis-workspace-overview-tile-num-btn");
    header.set_label(&format!("{}", workspace));
    header.set_halign(gtk::Align::Start);
    let out_hdr = ov.output.borrow().clone();
    header.connect_clicked(move |_| {
        switch_and_dismiss(out_hdr.clone(), workspace);
    });
    body.append(&header);

    let on_ws = windows_on_workspace(windows, output, workspace);
    let thumbs = ov.thumbs.borrow();

    if on_ws.is_empty() {
        let empty = gtk::Button::new();
        empty.add_css_class("metis-workspace-overview-empty-btn");
        empty.set_label(&workspace.to_string());
        empty.set_hexpand(true);
        empty.set_vexpand(true);
        let out_empty = ov.output.borrow().clone();
        empty.connect_clicked(move |_| {
            switch_and_dismiss(out_empty.clone(), workspace);
        });
        body.append(&empty);
    } else {
        let flow = gtk::FlowBox::new();
        flow.add_css_class("metis-workspace-overview-flow");
        flow.set_selection_mode(gtk::SelectionMode::None);
        flow.set_max_children_per_line(3);
        flow.set_min_children_per_line(1);
        flow.set_column_spacing(6);
        flow.set_row_spacing(6);
        flow.set_homogeneous(true);
        flow.set_halign(gtk::Align::Center);
        flow.set_valign(gtk::Align::Center);
        flow.set_hexpand(true);
        flow.set_vexpand(true);
        body.append(&flow);

        const MAX_CARDS: usize = 6;
        for w in on_ws.iter().take(MAX_CARDS) {
            let card = build_window_card(w, thumbs.as_ref(), CARD_W, CARD_H);
            let win_id = w.id;
            let out_name = ov.output.borrow().clone();
            let click = gtk::GestureClick::new();
            click.set_button(1);
            click.connect_released(move |_, _, _, _| {
                switch_and_dismiss(out_name.clone(), workspace);
                glib::idle_add_local_once(move || {
                    if let Err(err) = crate::compositor::activate_window(win_id) {
                        tracing::warn!(win_id, %err, "overview activate failed");
                    }
                });
            });
            card.add_controller(click);

            let drag = gtk::DragSource::new();
            drag.set_actions(gdk::DragAction::MOVE);
            let id_str = win_id.to_string();
            drag.connect_prepare(move |_, _, _| {
                Some(gdk::ContentProvider::for_value(&glib::Value::from(
                    id_str.clone(),
                )))
            });
            card.add_controller(drag);
            flow.insert(&card, -1);
        }
    }

    let drop = gtk::DropTarget::new(String::static_type(), gdk::DragAction::MOVE);
    drop.connect_drop(move |_, value, _, _| {
        let Ok(id_str) = value.get::<String>() else {
            return false;
        };
        let Ok(window_id) = id_str.parse::<u32>() else {
            return false;
        };
        if let Err(err) = crate::compositor::move_window_to_workspace(window_id, workspace) {
            tracing::warn!(window_id, workspace, %err, "overview move failed");
            return false;
        }
        glib::idle_add_local_once(|| {
            windows::reconcile_now();
            on_windows_changed();
        });
        true
    });
    drop.connect_enter(|target, _, _| {
        target.widget().add_css_class("drop-hover");
        gdk::DragAction::MOVE
    });
    drop.connect_leave(|target| {
        target.widget().remove_css_class("drop-hover");
    });
    frame.add_controller(drop);

    frame
}

fn switch_and_dismiss(output: Option<String>, workspace: u32) {
    if let Some(o) = output {
        crate::services::dispatch_workspace(Some(o), workspace);
    } else {
        crate::services::dispatch_workspace(None, workspace);
    }
    dismiss();
}

fn build_window_card(
    w: &WindowInfo,
    thumbs: Option<&ThumbSet>,
    width: i32,
    height: i32,
) -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 2);
    card.add_css_class("metis-workspace-overview-card");
    card.set_overflow(gtk::Overflow::Hidden);
    card.set_size_request(width, height);
    card.set_halign(gtk::Align::Center);
    card.set_valign(gtk::Align::Center);

    let thumb = window_thumbs::thumb_or_icon_widget(thumbs, w, 32);
    thumb.set_halign(gtk::Align::Center);
    thumb.set_valign(gtk::Align::Center);
    thumb.set_size_request(width - 8, height - 22);
    card.append(&thumb);

    let title = gtk::Label::new(Some(w.title.as_str()));
    title.add_css_class("metis-workspace-overview-card-title");
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.set_max_width_chars(10);
    title.set_halign(gtk::Align::Center);
    card.append(&title);

    card
}

fn wire_keyboard(ov: &Rc<Overview>) {
    let controller = gtk::EventControllerKey::new();
    controller.connect_key_pressed(move |_, key, _, _| {
        use gdk::Key;
        if key == Key::Escape {
            dismiss();
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    ov.window.add_controller(controller);
}

fn gdk_monitor_for_output(connector: Option<&str>) -> Option<gdk::Monitor> {
    let display = gdk::Display::default()?;
    let monitors = display.monitors();
    let n = monitors.n_items();
    if let Some(name) = connector {
        for i in 0..n {
            let Ok(monitor) = monitors.item(i)?.downcast::<gdk::Monitor>() else {
                continue;
            };
            if monitor
                .connector()
                .map(|c| c.eq_ignore_ascii_case(name))
                .unwrap_or(false)
            {
                return Some(monitor);
            }
        }
    }
    for i in 0..n {
        let Ok(monitor) = monitors.item(i)?.downcast::<gdk::Monitor>() else {
            continue;
        };
        return Some(monitor);
    }
    None
}
