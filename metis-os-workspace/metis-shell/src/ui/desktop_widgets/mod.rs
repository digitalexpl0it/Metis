//! Free-floating desktop widgets over the wallpaper (Phase 14).
//!
//! One transparent layer-shell surface per output hosts many widget cards on a
//! `GtkFixed` canvas. Master switch defaults off; edit mode enables move/resize
//! for unlocked instances. Empty chrome may still receive pointer hits in v1
//! (imperfect click-through — documented in TODO).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::gdk;
use gtk::gio;
use gtk::prelude::*;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use metis_config::{
    desktop_widgets_config_path, load_desktop_widgets_config, save_desktop_widgets_config,
    DesktopWidgetInstance, DesktopWidgetKind, DesktopWidgetsConfig,
};

const MIN_W: i32 = 160;
const MIN_H: i32 = 120;
const RESIZE_HANDLE: i32 = 18;
const SAVE_DEBOUNCE_MS: u64 = 200;

struct HostSurface {
    window: gtk::Window,
    canvas: gtk::Fixed,
    /// Compositor / GDK connector name (`DP-1`, `metis-0`, …).
    output: String,
    /// True when this host is bound to the session's first monitor (primary).
    is_primary: bool,
}

thread_local! {
    static HOSTS: RefCell<Vec<HostSurface>> = const { RefCell::new(Vec::new()) };
    static CFG: RefCell<DesktopWidgetsConfig> = RefCell::new(DesktopWidgetsConfig::default());
    static INITED: Cell<bool> = const { Cell::new(false) };
    static FILE_MONITOR: RefCell<Option<gio::FileMonitor>> = const { RefCell::new(None) };
    /// Skip one file-monitor reload after we write geometry ourselves.
    static SKIP_RELOAD: Cell<bool> = const { Cell::new(false) };
    static SAVE_PENDING: RefCell<Option<glib::SourceId>> = const { RefCell::new(None) };
}

/// Start the desktop-widgets host (idempotent). Called once from bar startup.
pub fn init() {
    if INITED.replace(true) {
        return;
    }
    reload_from_disk();
    watch_config_file();
    watch_monitors();
}

/// Re-read `desktop-widgets.json` and rebuild host surfaces.
pub fn reload() {
    if SKIP_RELOAD.replace(false) {
        return;
    }
    reload_from_disk();
}

fn reload_from_disk() {
    let cfg = load_desktop_widgets_config();
    CFG.with(|cell| *cell.borrow_mut() = cfg);
    rebuild_hosts();
}

fn current_cfg() -> DesktopWidgetsConfig {
    CFG.with(|cell| cell.borrow().clone())
}

fn rebuild_hosts() {
    let cfg = current_cfg();
    tear_down_hosts();

    if !cfg.enabled {
        tracing::debug!("desktop widgets disabled — no host surfaces");
        return;
    }

    let monitors = connected_monitors();
    if monitors.is_empty() {
        tracing::warn!("desktop widgets enabled but no GDK monitors");
        return;
    }

    let mut hosts = Vec::with_capacity(monitors.len());
    for (idx, monitor) in monitors.iter().enumerate() {
        let output = monitor_output_name(monitor).unwrap_or_default();
        let is_primary = idx == 0;
        let host = build_host(monitor, output, is_primary);
        populate_host(&host, &cfg);
        hosts.push(host);
    }
    HOSTS.with(|cell| *cell.borrow_mut() = hosts);
    tracing::info!(
        outputs = monitors.len(),
        instances = cfg.instances.len(),
        edit = cfg.edit_mode,
        "desktop widgets host ready"
    );
}

fn tear_down_hosts() {
    let old = HOSTS.with(|cell| std::mem::take(&mut *cell.borrow_mut()));
    for host in old {
        host.window.destroy();
    }
}

fn build_host(monitor: &gdk::Monitor, output: String, is_primary: bool) -> HostSurface {
    let geo = monitor.geometry();
    let window = gtk::Window::builder()
        .title("Metis Desktop Widgets")
        .default_width(geo.width().max(1))
        .default_height(geo.height().max(1))
        .build();
    window.add_css_class("metis-desktop-widgets-window");
    window.init_layer_shell();
    // Bottom sits above the wallpaper (Background) and below normal windows / Top bar.
    window.set_layer(Layer::Bottom);
    window.set_namespace("metis-desktop-widgets");
    window.set_keyboard_mode(KeyboardMode::None);
    window.set_exclusive_zone(0);
    for edge in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
        window.set_anchor(edge, true);
        window.set_margin(edge, 0);
    }
    window.set_monitor(monitor);

    let canvas = gtk::Fixed::new();
    canvas.add_css_class("metis-desktop-widgets-canvas");
    canvas.set_hexpand(true);
    canvas.set_vexpand(true);
    window.set_child(Some(&canvas));
    window.present();

    HostSurface {
        window,
        canvas,
        output,
        is_primary,
    }
}

fn populate_host(host: &HostSurface, cfg: &DesktopWidgetsConfig) {
    while let Some(child) = host.canvas.first_child() {
        host.canvas.remove(&child);
    }

    for inst in &cfg.instances {
        if !instance_belongs(inst, host) {
            continue;
        }
        let card = build_card(inst, cfg.edit_mode);
        host.canvas
            .put(&card, inst.x as f64, inst.y as f64);
    }
}

fn instance_belongs(inst: &DesktopWidgetInstance, host: &HostSurface) -> bool {
    if inst.output.is_empty() {
        return host.is_primary;
    }
    inst.output == host.output
}

fn build_card(inst: &DesktopWidgetInstance, edit_mode: bool) -> gtk::Widget {
    let can_edit = edit_mode && !inst.locked;

    let outer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    outer.add_css_class("metis-dw-card");
    if can_edit {
        outer.add_css_class("metis-dw-edit");
    }
    if inst.locked {
        outer.add_css_class("metis-dw-locked");
    }
    outer.set_size_request(inst.w as i32, inst.h as i32);
    outer.set_overflow(gtk::Overflow::Hidden);

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    header.add_css_class("metis-dw-header");
    let title = gtk::Label::new(Some(inst.kind.label()));
    title.add_css_class("metis-dw-title");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    header.append(&title);
    if inst.locked {
        let lock = gtk::Label::new(Some("Locked"));
        lock.add_css_class("metis-dw-badge");
        header.append(&lock);
    } else if can_edit {
        let badge = gtk::Label::new(Some("Edit"));
        badge.add_css_class("metis-dw-badge");
        header.append(&badge);
    }
    outer.append(&header);

    let body = gtk::Box::new(gtk::Orientation::Vertical, 6);
    body.add_css_class("metis-dw-body");
    body.set_hexpand(true);
    body.set_vexpand(true);
    body.append(&content_for_kind(inst));
    outer.append(&body);

    if can_edit {
        let footer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        footer.set_halign(gtk::Align::End);
        let handle = gtk::Button::new();
        handle.set_label("↘");
        handle.set_tooltip_text(Some("Drag to resize"));
        handle.add_css_class("metis-dw-resize");
        handle.set_size_request(RESIZE_HANDLE, RESIZE_HANDLE);
        footer.append(&handle);
        outer.append(&footer);

        wire_move(&outer, &inst.id);
        wire_resize(&handle, &outer, &inst.id);
    }

    outer.upcast()
}

fn content_for_kind(inst: &DesktopWidgetInstance) -> gtk::Widget {
    match inst.kind {
        DesktopWidgetKind::Placeholder => {
            let col = gtk::Box::new(gtk::Orientation::Vertical, 8);
            let hint = gtk::Label::new(Some(
                "Placeholder — use Edit mode to move and resize. \
                 Folders, Apps, Clock, System, and Weather come next.",
            ));
            hint.set_wrap(true);
            hint.set_xalign(0.0);
            hint.add_css_class("metis-dw-hint");
            col.append(&hint);
            col.upcast()
        }
        other => {
            let col = gtk::Box::new(gtk::Orientation::Vertical, 8);
            let hint = gtk::Label::new(Some(&format!(
                "{} widget — content lands in a later Phase 14 step.",
                other.label()
            )));
            hint.set_wrap(true);
            hint.set_xalign(0.0);
            hint.add_css_class("metis-dw-hint");
            col.append(&hint);
            col.upcast()
        }
    }
}

fn wire_move(card: &gtk::Box, id: &str) {
    let drag = gtk::GestureDrag::new();
    drag.set_button(1);
    drag.set_propagation_phase(gtk::PropagationPhase::Capture);

    let id = id.to_string();
    let start = Rc::new(Cell::new((0.0_f64, 0.0_f64)));
    let origin = Rc::new(Cell::new((0.0_f64, 0.0_f64)));

    {
        let start = start.clone();
        let origin = origin.clone();
        let card = card.clone();
        drag.connect_drag_begin(move |_, _x, _y| {
            start.set((0.0, 0.0));
            if let Some(fixed) = card.parent().and_then(|p| p.downcast::<gtk::Fixed>().ok()) {
                let (fx, fy) = fixed.child_position(&card);
                origin.set((fx, fy));
            }
        });
    }
    {
        let origin = origin.clone();
        let card = card.clone();
        drag.connect_drag_update(move |gesture, _x, _y| {
            let Some((dx, dy)) = gesture.offset() else {
                return;
            };
            let (ox, oy) = origin.get();
            let nx = (ox + dx).max(0.0);
            let ny = (oy + dy).max(0.0);
            if let Some(fixed) = card.parent().and_then(|p| p.downcast::<gtk::Fixed>().ok()) {
                fixed.move_(&card, nx, ny);
            }
        });
    }
    {
        let card = card.clone();
        drag.connect_drag_end(move |gesture, _x, _y| {
            let Some((dx, dy)) = gesture.offset() else {
                return;
            };
            // Ignore tiny clicks.
            if dx.abs() < 1.0 && dy.abs() < 1.0 {
                return;
            }
            let Some(fixed) = card.parent().and_then(|p| p.downcast::<gtk::Fixed>().ok()) else {
                return;
            };
            let (fx, fy) = fixed.child_position(&card);
            update_instance_geometry(&id, |inst| {
                inst.x = fx.round() as i32;
                inst.y = fy.round() as i32;
            });
        });
    }

    card.add_controller(drag);
}

fn wire_resize(handle: &gtk::Button, card: &gtk::Box, id: &str) {
    let drag = gtk::GestureDrag::new();
    drag.set_button(1);
    drag.set_propagation_phase(gtk::PropagationPhase::Capture);

    let id = id.to_string();
    let start_size = Rc::new(Cell::new((0_i32, 0_i32)));

    {
        let start_size = start_size.clone();
        let card = card.clone();
        drag.connect_drag_begin(move |_, _, _| {
            start_size.set((card.width().max(MIN_W), card.height().max(MIN_H)));
        });
    }
    {
        let start_size = start_size.clone();
        let card = card.clone();
        drag.connect_drag_update(move |gesture, _, _| {
            let Some((dx, dy)) = gesture.offset() else {
                return;
            };
            let (sw, sh) = start_size.get();
            let nw = (sw as f64 + dx).round() as i32;
            let nh = (sh as f64 + dy).round() as i32;
            card.set_size_request(nw.max(MIN_W), nh.max(MIN_H));
        });
    }
    {
        let card = card.clone();
        drag.connect_drag_end(move |_, _, _| {
            let w = card.width().max(MIN_W) as u32;
            let h = card.height().max(MIN_H) as u32;
            update_instance_geometry(&id, |inst| {
                inst.w = w.clamp(160, 2400);
                inst.h = h.clamp(120, 1800);
            });
        });
    }

    handle.add_controller(drag);
}

fn update_instance_geometry(id: &str, f: impl FnOnce(&mut DesktopWidgetInstance)) {
    let mut cfg = current_cfg();
    let Some(inst) = cfg.instances.iter_mut().find(|i| i.id == id) else {
        return;
    };
    f(inst);
    CFG.with(|cell| *cell.borrow_mut() = cfg.clone());
    schedule_save(cfg);
}

fn schedule_save(cfg: DesktopWidgetsConfig) {
    if let Some(id) = SAVE_PENDING.with(|cell| cell.borrow_mut().take()) {
        id.remove();
    }
    let id = glib::timeout_add_local(
        std::time::Duration::from_millis(SAVE_DEBOUNCE_MS),
        move || {
            SAVE_PENDING.with(|cell| *cell.borrow_mut() = None);
            SKIP_RELOAD.set(true);
            if let Err(err) = save_desktop_widgets_config(&cfg) {
                tracing::warn!(%err, "failed to persist desktop widget geometry");
                SKIP_RELOAD.set(false);
            }
            // Refresh Settings list labels eventually via file monitor; we skip
            // our own rebuild so drag feels smooth.
            glib::ControlFlow::Break
        },
    );
    SAVE_PENDING.with(|cell| *cell.borrow_mut() = Some(id));
}

fn watch_config_file() {
    let path = desktop_widgets_config_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let file = gio::File::for_path(&path);
    let Ok(monitor) = file.monitor_file(gio::FileMonitorFlags::NONE, None::<&gio::Cancellable>)
    else {
        tracing::warn!(
            path = %path.display(),
            "desktop-widgets.json file monitor unavailable"
        );
        return;
    };
    monitor.connect_changed(move |_, _, _, _| {
        glib::timeout_add_local_once(std::time::Duration::from_millis(250), || {
            reload();
        });
    });
    FILE_MONITOR.with(|cell| *cell.borrow_mut() = Some(monitor));
}

fn watch_monitors() {
    use gtk::gio::prelude::ListModelExt;
    let Some(display) = gdk::Display::default() else {
        return;
    };
    display.monitors().connect_items_changed(move |_, _, _, _| {
        glib::timeout_add_local_once(std::time::Duration::from_millis(200), || {
            rebuild_hosts();
        });
    });
}

fn connected_monitors() -> Vec<gdk::Monitor> {
    use gtk::gio::prelude::ListModelExt;
    let Some(display) = gdk::Display::default() else {
        return Vec::new();
    };
    let list = display.monitors();
    let mut out = Vec::new();
    for i in 0..list.n_items() {
        if let Some(monitor) = list.item(i).and_then(|o| o.downcast::<gdk::Monitor>().ok()) {
            out.push(monitor);
        }
    }
    out
}

fn monitor_output_name(monitor: &gdk::Monitor) -> Option<String> {
    monitor
        .connector()
        .map(|c| c.to_string())
        .filter(|c| !c.is_empty())
}
