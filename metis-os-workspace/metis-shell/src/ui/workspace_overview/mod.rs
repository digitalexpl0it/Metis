//! Workspace overview overlay: grid of workspaces with window cards and DnD.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::gdk;
use gtk::prelude::*;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use metis_protocol::{OutputInfo, PixelRect, WindowInfo};

use crate::services::applications;
use crate::services::windows;

thread_local! {
    static OVERLAY: RefCell<Option<Rc<Overview>>> = const { RefCell::new(None) };
}

struct Overview {
    window: gtk::Window,
    grid: gtk::Grid,
    output: RefCell<Option<String>>,
    output_rect: Cell<PixelRect>,
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

    let output = windows::focused_output_name();
    let output_rect = output_geometry(output.as_deref());

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

    let root = gtk::Box::new(gtk::Orientation::Vertical, 16);
    root.add_css_class("metis-workspace-overview-backdrop");
    root.set_halign(gtk::Align::Fill);
    root.set_valign(gtk::Align::Fill);
    window.set_child(Some(&root));

    let title = gtk::Label::new(Some(&metis_i18n::tr("Workspaces")));
    title.add_css_class("metis-workspace-overview-title");
    title.set_halign(gtk::Align::Center);
    title.set_margin_top(32);
    root.append(&title);

    let grid = gtk::Grid::new();
    grid.add_css_class("metis-workspace-overview-grid");
    grid.set_halign(gtk::Align::Center);
    grid.set_valign(gtk::Align::Center);
    grid.set_hexpand(true);
    grid.set_vexpand(true);
    grid.set_row_spacing(16);
    grid.set_column_spacing(16);
    grid.set_margin_bottom(32);
    root.append(&grid);

    let ov = Rc::new(Overview {
        window: window.clone(),
        grid: grid.clone(),
        output: RefCell::new(output),
        output_rect: Cell::new(output_rect),
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
    let out_rect = ov.output_rect.get();

    let cols = ((count as f64).sqrt().ceil() as u32).max(1).min(4);
    for ws in 1..=count {
        let idx = ws - 1;
        let col = (idx % cols) as i32;
        let row = (idx / cols) as i32;
        let tile = build_tile(ov, ws, active, &snap.windows, output.as_deref(), out_rect);
        ov.grid.attach(&tile, col, row, 1, 1);
    }
}

fn build_tile(
    ov: &Rc<Overview>,
    workspace: u32,
    active: u32,
    windows: &[WindowInfo],
    output: Option<&str>,
    out_rect: PixelRect,
) -> gtk::Overlay {
    let frame = gtk::Overlay::new();
    frame.add_css_class("metis-workspace-overview-tile");
    if workspace == active {
        frame.add_css_class("active");
    }
    frame.set_size_request(280, 180);

    let body = gtk::Box::new(gtk::Orientation::Vertical, 8);
    body.set_hexpand(true);
    body.set_vexpand(true);
    frame.set_child(Some(&body));

    let header = gtk::Button::new();
    header.add_css_class("metis-workspace-overview-tile-num-btn");
    header.set_label(&format!("{}", workspace));
    header.set_halign(gtk::Align::Start);
    header.set_margin_start(6);
    header.set_margin_top(4);
    let out_hdr = ov.output.borrow().clone();
    header.connect_clicked(move |_| {
        if let Some(ref o) = out_hdr {
            crate::services::dispatch_workspace(Some(o.clone()), workspace);
        } else {
            crate::services::dispatch_workspace(None, workspace);
        }
        dismiss();
    });
    body.append(&header);

    let on_ws: Vec<&WindowInfo> = windows
        .iter()
        .filter(|w| {
            let out_ok = match output {
                Some(o) if !o.is_empty() => w.output.is_empty() || w.output == o,
                _ => true,
            };
            let ws_ok = w.workspace == 0 || w.workspace == workspace;
            out_ok && ws_ok && !w.minimized
        })
        .collect();

    if on_ws.is_empty() {
        let empty = gtk::Button::new();
        empty.add_css_class("metis-workspace-overview-empty-btn");
        empty.set_label(&workspace.to_string());
        empty.set_hexpand(true);
        empty.set_vexpand(true);
        let out_empty = ov.output.borrow().clone();
        empty.connect_clicked(move |_| {
            if let Some(ref o) = out_empty {
                crate::services::dispatch_workspace(Some(o.clone()), workspace);
            } else {
                crate::services::dispatch_workspace(None, workspace);
            }
            dismiss();
        });
        body.append(&empty);
    } else {
        let stage = gtk::Fixed::new();
        stage.add_css_class("metis-workspace-overview-stage");
        stage.set_hexpand(true);
        stage.set_vexpand(true);
        stage.set_size_request(260, 140);
        body.append(&stage);

        let stage_w = 260.0_f64;
        let stage_h = 140.0_f64;
        let ox = out_rect.x as f64;
        let oy = out_rect.y as f64;
        let ow = (out_rect.width.max(1) as f64).max(1.0);
        let oh = (out_rect.height.max(1) as f64).max(1.0);
        let scale = (stage_w / ow).min(stage_h / oh);

        const MAX_CARDS: usize = 12;
        for w in on_ws.iter().take(MAX_CARDS) {
            let card = build_window_card(w);
            let rx = ((w.rect.x as f64 - ox) * scale).clamp(0.0, stage_w - 48.0);
            let ry = ((w.rect.y as f64 - oy) * scale).clamp(0.0, stage_h - 40.0);
            let rw = ((w.rect.width as f64) * scale).clamp(48.0, stage_w);
            let rh = ((w.rect.height as f64) * scale).clamp(40.0, stage_h);
            card.set_size_request(rw as i32, rh as i32);
            stage.put(&card, rx, ry);

            let win_id = w.id;
            let out_name = ov.output.borrow().clone();
            let click = gtk::GestureClick::new();
            click.set_button(1);
            click.connect_released(move |_, _, _, _| {
                if let Some(ref o) = out_name {
                    crate::services::dispatch_workspace(Some(o.clone()), workspace);
                } else {
                    crate::services::dispatch_workspace(None, workspace);
                }
                dismiss();
                if let Err(err) = crate::compositor::activate_window(win_id) {
                    tracing::warn!(win_id, %err, "overview activate failed");
                }
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

fn build_window_card(w: &WindowInfo) -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 4);
    card.add_css_class("metis-workspace-overview-card");
    card.set_overflow(gtk::Overflow::Hidden);

    let icon = gtk::Image::from_gicon(&applications::resolve_icon_for_app_id(w.app_id.as_deref()));
    icon.set_pixel_size(24);
    icon.set_halign(gtk::Align::Center);
    card.append(&icon);

    let title = gtk::Label::new(Some(w.title.as_str()));
    title.add_css_class("metis-workspace-overview-card-title");
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.set_max_width_chars(12);
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

fn output_geometry(name: Option<&str>) -> PixelRect {
    let outputs = list_outputs_best_effort();
    let mon = if let Some(n) = name {
        outputs
            .iter()
            .find(|o| o.name.eq_ignore_ascii_case(n))
            .or_else(|| outputs.iter().find(|o| o.primary))
            .or_else(|| outputs.first())
    } else {
        outputs
            .iter()
            .find(|o| o.primary)
            .or_else(|| outputs.first())
    };
    mon.map(|o| PixelRect {
        x: o.rect.x,
        y: o.rect.y,
        width: o.rect.width,
        height: o.rect.height,
    })
    .unwrap_or(PixelRect {
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
    })
}

fn list_outputs_best_effort() -> Vec<OutputInfo> {
    match metis_protocol::send_compositor_command(&metis_protocol::CompositorCommand::ListOutputs) {
        Ok(metis_protocol::CompositorEvent::OutputList { outputs }) => outputs,
        _ => Vec::new(),
    }
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
