//! Free-floating desktop widgets over the wallpaper (Phase 14).
//!
//! One transparent layer-shell surface per output hosts many widget cards on a
//! `GtkFixed` canvas. Master switch defaults off; edit mode enables move/resize
//! for unlocked instances. Empty chrome may still receive pointer hits in v1
//! (imperfect click-through — documented in TODO).

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

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
const RESIZE_HANDLE: i32 = 20;
const SAVE_DEBOUNCE_MS: u64 = 350;
/// Ignore config reloads while dragging and shortly after we write geometry.
const RELOAD_SUPPRESS_AFTER_SAVE: Duration = Duration::from_millis(600);

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
    static SAVE_PENDING: RefCell<Option<glib::SourceId>> = const { RefCell::new(None) };
    /// Active move/resize gestures — file monitor / Settings reload must not
    /// tear down the surface mid-drag (that caused stutter + resize rubberbanding).
    static INTERACTION: Cell<u32> = const { Cell::new(0) };
    static SUPPRESS_RELOAD_UNTIL: Cell<Option<Instant>> = const { Cell::new(None) };
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
    if interaction_blocks_reload() {
        tracing::debug!("desktop widgets reload deferred (edit interaction active)");
        return;
    }
    reload_from_disk();
}

fn interaction_blocks_reload() -> bool {
    if INTERACTION.get() > 0 {
        return true;
    }
    if let Some(until) = SUPPRESS_RELOAD_UNTIL.get() {
        if Instant::now() < until {
            return true;
        }
        SUPPRESS_RELOAD_UNTIL.set(None);
    }
    false
}

fn begin_interaction() {
    INTERACTION.set(INTERACTION.get().saturating_add(1));
}

fn end_interaction() {
    INTERACTION.set(INTERACTION.get().saturating_sub(1));
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
    // Give the header a drag target area without competing with body content.
    header.set_height_request(28);
    let title = gtk::Label::new(Some(inst.kind.label()));
    title.add_css_class("metis-dw-title");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    // Labels steal nothing useful for drag; keep them non-target so the header
    // gesture always wins.
    title.set_can_target(false);
    header.append(&title);
    if inst.locked {
        let lock = gtk::Label::new(Some("Locked"));
        lock.add_css_class("metis-dw-badge");
        lock.set_can_target(false);
        header.append(&lock);
    } else if can_edit {
        let badge = gtk::Label::new(Some("Drag title · resize ↘"));
        badge.add_css_class("metis-dw-badge");
        badge.set_can_target(false);
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
        // Plain box, not a Button — Button + GestureDrag fights and rubberbands.
        let handle = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        handle.add_css_class("metis-dw-resize");
        handle.set_size_request(RESIZE_HANDLE, RESIZE_HANDLE);
        handle.set_tooltip_text(Some("Drag to resize"));
        let grip = gtk::Label::new(Some("↘"));
        grip.set_can_target(false);
        handle.append(&grip);
        footer.append(&handle);
        outer.append(&footer);

        // Move from the header only so resize gestures are not stolen.
        wire_move(&header, &outer, &inst.id);
        wire_resize(&handle, &outer, &inst.id);
    }

    outer.upcast()
}

fn content_for_kind(inst: &DesktopWidgetInstance) -> gtk::Widget {
    match inst.kind {
        DesktopWidgetKind::Placeholder => {
            let col = gtk::Box::new(gtk::Orientation::Vertical, 8);
            let hint = gtk::Label::new(Some(
                "Placeholder — drag the title bar to move, corner to resize. \
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

/// Pointer position in the native surface's coordinate space.
///
/// `GestureDrag::offset()` is widget-local. Moving/resizing that widget under the
/// cursor collapses the offset toward zero (rubberband / jitter). Surface coords
/// stay stable for the lifetime of the gesture.
fn gesture_surface_pos(gesture: &gtk::GestureDrag) -> Option<(f64, f64)> {
    gesture.current_event()?.position()
}

fn css_class_for_id(id: &str) -> String {
    let mut out = String::from("metis-dw-t-");
    for ch in id.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    out
}

/// Live translate while dragging — layout position stays put so gestures stay
/// stable and we avoid full Fixed relayout every pointer event.
fn apply_live_translate(provider: &gtk::CssProvider, class: &str, dx: f64, dy: f64) {
    let css = format!(
        ".{class} {{ transform: translate({dx:.1}px, {dy:.1}px); }}"
    );
    provider.load_from_data(&css);
}

fn clear_live_transform(provider: &gtk::CssProvider, class: &str) {
    provider.load_from_data(&format!(".{class} {{ }}"));
}

fn wire_move(header: &gtk::Box, card: &gtk::Box, id: &str) {
    let drag = gtk::GestureDrag::new();
    drag.set_button(gdk::BUTTON_PRIMARY);
    drag.set_propagation_phase(gtk::PropagationPhase::Capture);
    drag.set_exclusive(true);

    let id = id.to_string();
    let class = css_class_for_id(&id);
    card.add_css_class(&class);

    let provider = gtk::CssProvider::new();
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 50,
        );
    }

    let card_origin = Rc::new(Cell::new((0.0_f64, 0.0_f64)));
    let ptr_origin = Rc::new(Cell::new((0.0_f64, 0.0_f64)));
    let last_delta = Rc::new(Cell::new((0.0_f64, 0.0_f64)));

    {
        let card = card.clone();
        let card_origin = card_origin.clone();
        let ptr_origin = ptr_origin.clone();
        let last_delta = last_delta.clone();
        let provider = provider.clone();
        let class = class.clone();
        drag.connect_drag_begin(move |gesture, _x, _y| {
            begin_interaction();
            last_delta.set((0.0, 0.0));
            clear_live_transform(&provider, &class);
            if let Some(fixed) = card.parent().and_then(|p| p.downcast::<gtk::Fixed>().ok()) {
                card_origin.set(fixed.child_position(&card));
            }
            if let Some(pos) = gesture_surface_pos(gesture) {
                ptr_origin.set(pos);
            }
        });
    }
    {
        let ptr_origin = ptr_origin.clone();
        let last_delta = last_delta.clone();
        let provider = provider.clone();
        let class = class.clone();
        drag.connect_drag_update(move |gesture, _ox, _oy| {
            let Some((px, py)) = gesture_surface_pos(gesture) else {
                return;
            };
            let (sx, sy) = ptr_origin.get();
            let dx = px - sx;
            let dy = py - sy;
            last_delta.set((dx, dy));
            apply_live_translate(&provider, &class, dx, dy);
        });
    }
    {
        let card = card.clone();
        let card_origin = card_origin.clone();
        let last_delta = last_delta.clone();
        let provider = provider.clone();
        let class = class.clone();
        drag.connect_drag_end(move |gesture, _, _| {
            let (mut dx, mut dy) = last_delta.get();
            if let Some((px, py)) = gesture_surface_pos(gesture) {
                let (sx, sy) = ptr_origin.get();
                dx = px - sx;
                dy = py - sy;
            }
            clear_live_transform(&provider, &class);
            let (ox, oy) = card_origin.get();
            let nx = (ox + dx).max(0.0);
            let ny = (oy + dy).max(0.0);
            if let Some(fixed) = card.parent().and_then(|p| p.downcast::<gtk::Fixed>().ok()) {
                fixed.move_(&card, nx, ny);
            }
            if dx.abs() >= 1.0 || dy.abs() >= 1.0 {
                update_instance_geometry(&id, |inst| {
                    inst.x = nx.round() as i32;
                    inst.y = ny.round() as i32;
                });
            }
            end_interaction();
        });
    }
    {
        let provider = provider.clone();
        let class = class.clone();
        drag.connect_cancel(move |_, _| {
            clear_live_transform(&provider, &class);
            end_interaction();
        });
    }

    header.add_controller(drag);
}

fn wire_resize(handle: &gtk::Box, card: &gtk::Box, id: &str) {
    let drag = gtk::GestureDrag::new();
    drag.set_button(gdk::BUTTON_PRIMARY);
    drag.set_propagation_phase(gtk::PropagationPhase::Capture);
    drag.set_exclusive(true);

    let id = id.to_string();
    let start_size = Rc::new(Cell::new((0_i32, 0_i32)));
    let ptr_origin = Rc::new(Cell::new((0.0_f64, 0.0_f64)));
    let pending = Rc::new(Cell::new((0_i32, 0_i32)));

    {
        let start_size = start_size.clone();
        let ptr_origin = ptr_origin.clone();
        let pending = pending.clone();
        let card = card.clone();
        drag.connect_drag_begin(move |gesture, _, _| {
            begin_interaction();
            let w = card.width().max(card.width_request()).max(MIN_W);
            let h = card.height().max(card.height_request()).max(MIN_H);
            start_size.set((w, h));
            pending.set((w, h));
            if let Some(pos) = gesture_surface_pos(gesture) {
                ptr_origin.set(pos);
            }
        });
    }
    {
        let start_size = start_size.clone();
        let ptr_origin = ptr_origin.clone();
        let pending = pending.clone();
        let card = card.clone();
        drag.connect_drag_update(move |gesture, _, _| {
            let Some((px, py)) = gesture_surface_pos(gesture) else {
                return;
            };
            let (sx, sy) = ptr_origin.get();
            let (sw, sh) = start_size.get();
            let nw = ((sw as f64 + (px - sx)).round() as i32).max(MIN_W);
            let nh = ((sh as f64 + (py - sy)).round() as i32).max(MIN_H);
            pending.set((nw, nh));
            // Apply immediately from surface deltas (not widget-local offset) so
            // growing the card under the grip cannot collapse the gesture.
            card.set_size_request(nw, nh);
        });
    }
    {
        let card = card.clone();
        let pending = pending.clone();
        drag.connect_drag_end(move |_, _, _| {
            let (w, h) = pending.get();
            let w = w.max(MIN_W) as u32;
            let h = h.max(MIN_H) as u32;
            card.set_size_request(w as i32, h as i32);
            update_instance_geometry(&id, |inst| {
                inst.w = w.clamp(160, 2400);
                inst.h = h.clamp(120, 1800);
            });
            end_interaction();
        });
    }
    {
        drag.connect_cancel(move |_, _| {
            end_interaction();
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
        Duration::from_millis(SAVE_DEBOUNCE_MS),
        move || {
            SAVE_PENDING.with(|cell| *cell.borrow_mut() = None);
            SUPPRESS_RELOAD_UNTIL.set(Some(Instant::now() + RELOAD_SUPPRESS_AFTER_SAVE));
            if let Err(err) = save_desktop_widgets_config(&cfg) {
                tracing::warn!(%err, "failed to persist desktop widget geometry");
            }
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
        glib::timeout_add_local_once(Duration::from_millis(250), || {
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
        glib::timeout_add_local_once(Duration::from_millis(200), || {
            if interaction_blocks_reload() {
                return;
            }
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
