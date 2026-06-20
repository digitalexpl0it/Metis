mod dropdown;
mod widgets;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::Receiver;

use gtk::prelude::*;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::config::{load_bar_config, save_default_bar_config, BarConfig, BarPosition};
use crate::services::{
    spawn_bar_pollers, spawn_weather_service, workspace_snapshot, BarSnapshot, WeatherSnapshot,
};

thread_local! {
    static BAR: RefCell<Option<BarHandle>> = const { RefCell::new(None) };
}

struct BarHandle {
    window: gtk::ApplicationWindow,
    pill: gtk::Box,
    config: Rc<RefCell<BarConfig>>,
    widget_refs: widgets::WidgetRefs,
}


pub fn init_and_show(app: &gtk::Application) {
    if let Err(err) = save_default_bar_config() {
        tracing::warn!(%err, "failed to write default bar.json");
    }

    let config = Rc::new(RefCell::new(load_bar_config()));
    let cfg = config.borrow().clone();
    let (win_w, win_h) = layer_window_size(&cfg);
    let thickness = bar_body_thickness(&cfg);

    let is_vertical = matches!(cfg.position, BarPosition::Left | BarPosition::Right);

    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Metis Bar")
        .default_width(win_w)
        .default_height(win_h)
        .build();

    let outer = gtk::Box::builder()
        .orientation(if is_vertical {
            gtk::Orientation::Vertical
        } else {
            gtk::Orientation::Horizontal
        })
        .build();
    outer.add_css_class("metis-bar-outer");
    if is_vertical {
        outer.add_css_class("metis-bar-outer-vertical");
        outer.set_size_request(thickness, win_h);
    } else {
        outer.set_size_request(win_w, win_h);
    }

    let column = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .build();
    column.add_css_class("metis-bar-column");
    column.set_hexpand(!is_vertical);
    column.set_vexpand(is_vertical);
    column.set_halign(gtk::Align::Fill);
    column.set_valign(gtk::Align::Start);
    if is_vertical {
        column.set_size_request(thickness, win_h);
    } else {
        column.set_size_request(win_w, win_h);
    }

    let pill = gtk::Box::builder()
        .orientation(orientation_for(&cfg))
        .spacing(4)
        .build();
    pill.add_css_class("metis-bar-pill");
    apply_pill_layout(&pill, &cfg);
    if is_vertical {
        pill.set_size_request(cfg.width as i32, -1);
        pill.add_css_class("metis-bar-pill-vertical");
    } else {
        pill.set_size_request(-1, cfg.height as i32);
    }

    column.append(&pill);

    // Click on empty bar space (not an icon button, not the popover) dismisses any
    // open popover. Bubble phase means child buttons that claim the press are
    // skipped, so this never fires when toggling/opening an icon.
    let dismiss = gtk::GestureClick::builder()
        .button(0)
        .propagation_phase(gtk::PropagationPhase::Bubble)
        .build();
    let pill_for_dismiss = pill.clone();
    dismiss.connect_pressed(move |_, _, x, y| {
        // Popover presses bubble up here because the popover is a widget-tree
        // child of its icon button. Ignore anything outside the pill's own strip
        // so interacting with the popover doesn't dismiss it.
        let w = pill_for_dismiss.width() as f64;
        let h = pill_for_dismiss.height() as f64;
        if x < 0.0 || y < 0.0 || x > w || y > h {
            return;
        }
        // If the press landed on (or inside) one of the bar's own icon buttons,
        // let that button's own click handler toggle its popover. Dismissing here
        // would race the toggle and re-open the popover on the second click.
        if let Some(target) = pill_for_dismiss.pick(x, y, gtk::PickFlags::DEFAULT) {
            let mut node = Some(target);
            while let Some(w) = node {
                if w.has_css_class("metis-bar-widget") {
                    return;
                }
                node = w.parent();
            }
        }
        dropdown::request_close_all();
    });
    pill.add_controller(dismiss);

    outer.append(&column);
    outer.set_hexpand(true);
    outer.set_vexpand(false);
    outer.set_halign(gtk::Align::Fill);
    outer.set_valign(gtk::Align::Start);
    window.set_child(Some(&outer));

    let widget_refs = widgets::build(&pill, config.clone());
    widget_refs.apply_snapshot(&BarSnapshot {
        workspaces: workspace_snapshot(),
        ..Default::default()
    });
    apply_layer_geometry(&window, &cfg);

    // Defer map until layer-shell anchors/size are applied (avoids 0-height first commit).
    let show_window = window.clone();
    glib::idle_add_local_once(move || {
        show_window.set_visible(true);
        show_window.present();
    });

    BAR.with(|bar| {
        *bar.borrow_mut() = Some(BarHandle {
            window: window.clone(),
            pill,
            config: config.clone(),
            widget_refs,
        });
    });

    // Defer pollers so GTK can finish the first layer-shell commit before subprocess I/O.
    glib::timeout_add_seconds_local(2, {
        let config = config.clone();
        move || {
            attach_poll_channel(spawn_bar_pollers());
            attach_weather_channel(spawn_weather_service());
            watch_bar_config(config.clone());
            watch_compositor_dismiss();
            glib::ControlFlow::Break
        }
    });

    tracing::info!(
        win_w,
        win_h,
        position = ?cfg.position,
        "Metis edge bar initialized"
    );
}

fn layer_window_size(config: &BarConfig) -> (i32, i32) {
    let thickness = bar_body_thickness(config);
    match config.position {
        BarPosition::Top => (-1, thickness),
        BarPosition::Left | BarPosition::Right => (thickness, -1),
    }
}

/// Empty padding kept inside the layer surface around the visible pill so the
/// pill's drop shadow renders fully (and follows its rounded corners) instead of
/// being clipped square at the surface's rectangular edge.
const BAR_SHADOW_PAD: i32 = 16;

fn bar_body_thickness(config: &BarConfig) -> i32 {
    match config.position {
        BarPosition::Top => config.height as i32 + BAR_SHADOW_PAD,
        BarPosition::Left | BarPosition::Right => config.width as i32 + BAR_SHADOW_PAD,
    }
}

fn bar_body_height(config: &BarConfig) -> i32 {
    bar_body_thickness(config)
}

/// Layer-shell popovers use GtkPopover — no layer window resize needed.
pub(crate) fn sync_layer_window_height(_dropdown_open: bool) {}

/// Layer-shell surfaces do not receive outside-click events; the compositor
/// tells us to pop down when the pointer hits bare desktop.
fn watch_compositor_dismiss() {
    glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
        let path = metis_protocol::runtime_command_path();
        if let Ok(cmd) = std::fs::read_to_string(&path) {
            let cmd = cmd.trim();
            if cmd == "close-popovers" {
                dropdown::request_close_all();
            } else if cmd == "reload-bar" {
                rebuild_from_config();
            }
            let _ = std::fs::remove_file(&path);
        }
        glib::ControlFlow::Continue
    });
}

fn apply_pill_layout(pill: &gtk::Box, config: &BarConfig) {
    pill.remove_css_class("metis-bar-full");
    pill.remove_css_class("metis-bar-floating");
    let vertical = matches!(config.position, BarPosition::Left | BarPosition::Right);
    // Inset the pill within the (larger) layer surface so the rounded drop shadow
    // has breathing room along the bar's long edges (the pill's rounded ends).
    let side_pad = BAR_SHADOW_PAD - 4;
    if vertical {
        pill.set_margin_top(side_pad);
        pill.set_margin_bottom(side_pad);
        pill.set_margin_start(0);
        pill.set_margin_end(0);
    } else {
        pill.set_margin_start(side_pad);
        pill.set_margin_end(side_pad);
        pill.set_margin_top(0);
        pill.set_margin_bottom(0);
    }
    if config.full_width {
        pill.add_css_class("metis-bar-full");
        pill.set_hexpand(!vertical);
        pill.set_vexpand(vertical);
        pill.set_halign(gtk::Align::Fill);
        pill.set_valign(gtk::Align::Fill);
    } else {
        pill.add_css_class("metis-bar-floating");
        pill.set_hexpand(false);
        pill.set_vexpand(false);
        pill.set_halign(gtk::Align::Center);
        pill.set_valign(gtk::Align::Start);
    }
}

fn orientation_for(config: &BarConfig) -> gtk::Orientation {
    match config.position {
        BarPosition::Top => gtk::Orientation::Horizontal,
        BarPosition::Left | BarPosition::Right => gtk::Orientation::Vertical,
    }
}

fn apply_layer_geometry(window: &gtk::ApplicationWindow, config: &BarConfig) {
    if !window.is_layer_window() {
        window.init_layer_shell();
    }
    window.set_layer(Layer::Top);
    window.set_namespace("metis-bar");
    window.add_css_class("metis-bar-window");
    // OnDemand (not None) so popovers spawned from the bar can receive keyboard
    // focus via their xdg_popup grab (text entries in the clock/calendar popover).
    window.set_keyboard_mode(KeyboardMode::OnDemand);
    window.auto_exclusive_zone_enable();

    for edge in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
        window.set_anchor(edge, false);
        window.set_margin(edge, 0);
    }

    let thickness = bar_body_thickness(config);

    match config.position {
        BarPosition::Top => {
            window.set_anchor(Edge::Top, true);
            window.set_anchor(Edge::Left, true);
            window.set_anchor(Edge::Right, true);
            window.set_margin(Edge::Top, config.margin_top as i32);
            window.set_margin(Edge::Left, config.margin_h as i32);
            window.set_margin(Edge::Right, config.margin_h as i32);
            window.set_height_request(thickness);
        }
        BarPosition::Left => {
            window.set_anchor(Edge::Left, true);
            window.set_anchor(Edge::Top, true);
            window.set_anchor(Edge::Bottom, true);
            window.set_margin(Edge::Left, config.margin_top as i32);
            window.set_margin(Edge::Top, config.margin_h as i32);
            window.set_margin(Edge::Bottom, config.margin_h as i32);
            window.set_width_request(thickness);
        }
        BarPosition::Right => {
            window.set_anchor(Edge::Right, true);
            window.set_anchor(Edge::Top, true);
            window.set_anchor(Edge::Bottom, true);
            window.set_margin(Edge::Right, config.margin_top as i32);
            window.set_margin(Edge::Top, config.margin_h as i32);
            window.set_margin(Edge::Bottom, config.margin_h as i32);
            window.set_width_request(thickness);
        }
    }

    window.set_opacity(config.opacity as f64);
}

fn rebuild_bar(config: Rc<RefCell<BarConfig>>) {
    dropdown::close_all();
    BAR.with(|bar| {
        let mut slot = bar.borrow_mut();
        let Some(handle) = slot.as_mut() else {
            return;
        };
        let cfg = config.borrow();
        apply_layer_geometry(&handle.window, &cfg);
        handle.pill.set_orientation(orientation_for(&cfg));
        apply_pill_layout(&handle.pill, &cfg);
        handle.pill.set_size_request(-1, cfg.height as i32);
        while let Some(child) = handle.pill.first_child() {
            handle.pill.remove(&child);
        }
        handle.widget_refs = widgets::build(&handle.pill, config.clone());
    });
}

fn watch_bar_config(config: Rc<RefCell<BarConfig>>) {
    let path = crate::config::bar_config_path();
    if !path.exists() {
        if let Err(err) = crate::config::save_default_bar_config() {
            tracing::warn!(%err, "failed to create default bar.json");
        }
    }
    let file = gio::File::for_path(&path);
    let Ok(monitor) = file.monitor_file(gio::FileMonitorFlags::NONE, None::<&gio::Cancellable>)
    else {
        tracing::warn!(path = %path.display(), "bar.json file monitor unavailable");
        return;
    };

    monitor.connect_changed(move |_, _, _event, _| {
        let config = config.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(250), move || {
            let cfg = load_bar_config();
            *config.borrow_mut() = cfg;
            rebuild_bar(config.clone());
            crate::ui::theme::reload_stylesheet();
        });
    });
}

fn attach_weather_channel(rx: Receiver<WeatherSnapshot>) {
    glib::timeout_add_local(std::time::Duration::from_millis(1000), move || {
        while let Ok(snapshot) = rx.try_recv() {
            tracing::debug!(
                locations = snapshot.locations.len(),
                error = ?snapshot.error,
                "weather: UI received snapshot"
            );
            BAR.with(|bar| {
                if let Some(handle) = bar.borrow().as_ref() {
                    handle.widget_refs.apply_weather(&snapshot);
                }
            });
        }
        glib::ControlFlow::Continue
    });
}

fn attach_poll_channel(rx: Receiver<BarSnapshot>) {
    let mut last = BarSnapshot::default();
    glib::timeout_add_local(std::time::Duration::from_millis(400), move || {
        while let Ok(snapshot) = rx.try_recv() {
            if snapshot == last {
                continue;
            }
            last = snapshot.clone();
            BAR.with(|bar| {
                if let Some(handle) = bar.borrow().as_ref() {
                    handle.widget_refs.apply_snapshot(&snapshot);
                }
            });
        }
        glib::ControlFlow::Continue
    });
}

pub fn rebuild_from_config() {
    BAR.with(|bar| {
        if let Some(handle) = bar.borrow().as_ref() {
            rebuild_bar(handle.config.clone());
        }
    });
}
