mod dropdown;
mod widgets;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::mpsc::Receiver;

use gtk::prelude::*;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::config::{load_bar_config, save_default_bar_config, BarConfig, BarDisplays, BarPosition};
use crate::services::{
    spawn_bar_pollers, spawn_notification_service, spawn_weather_service, workspace_snapshot,
    BarNotification, BarSnapshot, WeatherSnapshot,
};

thread_local! {
    // One bar per output (monitor); see `BarDisplays`. A single-monitor session
    // holds exactly one handle.
    static BARS: RefCell<Vec<BarHandle>> = const { RefCell::new(Vec::new()) };
    // Mirror of the active bar position, kept outside the `BARS` RefCell so
    // `popover_position()` can be read while `BARS` is mutably borrowed (e.g. from
    // within `rebuild_bars`, which builds widgets that query the popover side).
    static BAR_POSITION: Cell<BarPosition> = const { Cell::new(BarPosition::Top) };
    // Set while a coalesced rebuild is queued, so a burst of config/monitor change
    // triggers collapses into a single rebuild pass.
    static REBUILD_SCHEDULED: Cell<bool> = const { Cell::new(false) };
}

struct BarHandle {
    window: gtk::Window,
    outer: gtk::Box,
    column: gtk::Box,
    pill: gtk::Box,
    config: Rc<RefCell<BarConfig>>,
    widget_refs: widgets::WidgetRefs,
    /// Compositor output name this bar is bound to (e.g. `metis-0`), used so its
    /// workspace widget switches/reads that output's own workspaces.
    output: Option<String>,
}


pub fn init_and_show() {
    if let Err(err) = save_default_bar_config() {
        tracing::warn!(%err, "failed to write default bar.json");
    }

    let config = Rc::new(RefCell::new(load_bar_config()));
    let cfg = config.borrow().clone();

    // One bar per target output. `target_monitors` returns at least one entry
    // (`None` = let the compositor pick the output) so the single-monitor path is
    // unchanged.
    let monitors = target_monitors(&cfg);
    let handles: Vec<BarHandle> = monitors
        .iter()
        .map(|m| build_bar(config.clone(), m.as_ref()))
        .collect();
    let count = handles.len();
    BARS.with(|bars| *bars.borrow_mut() = handles);

    // Defer pollers so GTK can finish the first layer-shell commit before subprocess I/O.
    glib::timeout_add_seconds_local(2, move || {
        attach_poll_channel(spawn_bar_pollers());
        attach_weather_channel(spawn_weather_service());
        attach_notification_channel(spawn_notification_service());
        watch_bar_config();
        watch_theme_files();
        watch_compositor_dismiss();
        watch_monitors();
        glib::ControlFlow::Break
    });

    tracing::info!(bars = count, position = ?cfg.position, "Metis edge bar initialized");
}

/// Build a single bar window, optionally bound to `monitor` (None lets the
/// compositor choose the output). Returns the handle without registering it.
fn build_bar(
    config: Rc<RefCell<BarConfig>>,
    monitor: Option<&gtk::gdk::Monitor>,
) -> BarHandle {
    let cfg = config.borrow().clone();
    let (win_w, win_h) = layer_window_size(&cfg);

    let window = gtk::Window::builder()
        .title("Metis Bar")
        .default_width(win_w)
        .default_height(win_h)
        .build();

    // Establish the layer-shell role, anchors, exclusive zone, and output binding
    // *before* the window is realized or any child widgets are built. At startup
    // GTK defers realization so ordering is forgiving, but building and presenting
    // a fresh layer window at runtime (a displays-toggle / hotplug rebuild)
    // realizes immediately — if the role/output aren't set first, gtk4-layer-shell
    // can commit an invalid surface and the compositor drops the connection.
    apply_layer_geometry(&window, &cfg);
    // Bind to a specific output (multi-monitor); must be set before the surface is
    // mapped. Omitted (None) lets the compositor place it on the primary output.
    if let Some(monitor) = monitor {
        window.set_monitor(monitor);
    }
    // The compositor output name this bar lives on (its workspace widget uses it to
    // drive that output's own workspaces).
    let output = monitor.and_then(monitor_output_name);

    let outer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    outer.add_css_class("metis-bar-outer");

    let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
    column.add_css_class("metis-bar-column");

    let pill = gtk::Box::new(orientation_for(&cfg), 4);
    pill.add_css_class("metis-bar-pill");

    configure_surface(&outer, &column, &pill, &cfg);

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
    window.set_child(Some(&outer));

    let widget_refs = widgets::build(&pill, config.clone(), output.clone());
    widget_refs.apply_snapshot(&BarSnapshot {
        workspaces: workspace_snapshot(),
        ..Default::default()
    });

    // Defer map until layer-shell anchors/size are applied (avoids 0-height first commit).
    let show_window = window.clone();
    glib::idle_add_local_once(move || {
        show_window.set_visible(true);
        show_window.present();
    });

    BarHandle {
        window,
        outer,
        column,
        pill,
        config,
        widget_refs,
        output,
    }
}

/// The compositor output name (e.g. `metis-0`) backing a GDK monitor. Under the
/// nested Metis session GDK exposes the compositor's `wl_output` name via the
/// monitor connector.
fn monitor_output_name(monitor: &gtk::gdk::Monitor) -> Option<String> {
    monitor
        .connector()
        .map(|c| c.to_string())
        .filter(|c| !c.is_empty())
}

/// Repaint every bar's workspace dots from the current per-output active
/// workspace (called after an optimistic switch or a `WorkspaceChanged` event).
pub fn refresh_workspaces() {
    BARS.with(|bars| {
        for handle in bars.borrow().iter() {
            handle.widget_refs.refresh_workspaces();
        }
    });
}

/// The outputs the bar should appear on, as GDK monitors. Returns at least one
/// entry; `None` means "no specific monitor" (compositor picks the primary).
/// `BarDisplays::Primary` yields a single bar on the first monitor.
fn target_monitors(cfg: &BarConfig) -> Vec<Option<gtk::gdk::Monitor>> {
    let monitors = connected_monitors();
    match cfg.displays {
        BarDisplays::Primary => vec![monitors.into_iter().next()],
        BarDisplays::All => {
            if monitors.is_empty() {
                vec![None]
            } else {
                monitors.into_iter().map(Some).collect()
            }
        }
    }
}

/// Snapshot of the currently connected GDK monitors (first entry is treated as
/// the primary output).
fn connected_monitors() -> Vec<gtk::gdk::Monitor> {
    use gtk::gio::prelude::ListModelExt;
    let Some(display) = gtk::gdk::Display::default() else {
        return Vec::new();
    };
    let list = display.monitors();
    let mut out = Vec::new();
    for i in 0..list.n_items() {
        if let Some(monitor) = list.item(i).and_then(|o| o.downcast::<gtk::gdk::Monitor>().ok()) {
            out.push(monitor);
        }
    }
    out
}

/// Rebuild every bar when monitors are added/removed so the per-output bars track
/// the current display layout.
fn watch_monitors() {
    use gtk::gio::prelude::ListModelExt;
    let Some(display) = gtk::gdk::Display::default() else {
        return;
    };
    display
        .monitors()
        .connect_items_changed(move |_, _, _, _| rebuild_from_config());
}

fn layer_window_size(config: &BarConfig) -> (i32, i32) {
    let thickness = bar_body_thickness(config);
    match config.position {
        BarPosition::Top | BarPosition::Bottom => (-1, thickness),
        BarPosition::Left | BarPosition::Right => (thickness, -1),
    }
}

/// Empty padding kept inside the layer surface around the visible pill so the
/// pill's drop shadow renders fully (and follows its rounded corners) instead of
/// being clipped square at the surface's rectangular edge. Shared with the
/// compositor (via `metis-config`) so backdrop blur can exclude this margin.
const BAR_SHADOW_PAD: i32 = metis_config::bar::SHADOW_PAD;

fn bar_body_thickness(config: &BarConfig) -> i32 {
    config.height as i32 + BAR_SHADOW_PAD
}

/// Visible cross-axis size of the bar pill (height when horizontal, width when
/// vertical). Kept equal so left/right bars match the top/bottom strip thickness.
fn bar_cross_thickness(config: &BarConfig) -> i32 {
    config.height as i32
}

/// The side bar popovers/menus should open toward, derived from the bar's anchored
/// edge: a top bar opens downward, a bottom bar upward, a left bar to the right,
/// a right bar to the left. Falls back to `Bottom` before the bar is initialized.
pub(crate) fn popover_position() -> gtk::PositionType {
    match BAR_POSITION.with(Cell::get) {
        BarPosition::Bottom => gtk::PositionType::Top,
        BarPosition::Left => gtk::PositionType::Right,
        BarPosition::Right => gtk::PositionType::Left,
        BarPosition::Top => gtk::PositionType::Bottom,
    }
}

/// Layer-shell surfaces do not receive outside-click events; the compositor
/// tells us to pop down when the pointer hits bare desktop.
fn watch_compositor_dismiss() {
    glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
        let path = metis_protocol::runtime_command_path();
        if let Ok(cmd) = std::fs::read_to_string(&path) {
            let cmd = cmd.trim();
            let (verb, arg) = cmd.split_once(char::is_whitespace).unwrap_or((cmd, ""));
            match verb {
                "close-popovers" => dropdown::request_close_all(),
                "reload-bar" => rebuild_from_config(),
                "reload-theme" => {
                    let _ = crate::ui::theme::init_theme();
                }
                "reload-weather" => crate::services::weather::weather_refresh(),
                "reload-calendars" => crate::services::reload_calendars(),
                "settings" => {
                    let program = if arg.trim().is_empty() {
                        "metis-settings".to_string()
                    } else {
                        format!("metis-settings --page {}", arg.trim())
                    };
                    if let Err(err) = crate::compositor::launch_program(&program) {
                        tracing::warn!(%err, "failed to launch metis-settings");
                    }
                }
                _ => {}
            }
            let _ = std::fs::remove_file(&path);
        }
        glib::ControlFlow::Continue
    });
}

/// Configure the bar's surface widget tree (outer box, column, pill) for the
/// current position: orientation, expansion, alignment (so the pill sits flush
/// against the anchored edge), size requests, and the vertical-bar CSS classes.
/// Shared by the initial build and the live `rebuild_bar` path so switching
/// between horizontal and vertical layouts at runtime re-sizes correctly.
fn configure_surface(outer: &gtk::Box, column: &gtk::Box, pill: &gtk::Box, config: &BarConfig) {
    // Publish the position for `popover_position()` before any widgets (which read
    // it) are built; reads must not borrow `BAR`, which is held during rebuilds.
    BAR_POSITION.with(|p| p.set(config.position));
    let is_vertical = matches!(config.position, BarPosition::Left | BarPosition::Right);
    let thickness = bar_body_thickness(config);

    outer.set_orientation(if is_vertical {
        gtk::Orientation::Vertical
    } else {
        gtk::Orientation::Horizontal
    });
    // Stretch along the bar's long axis only (width for top/bottom, height for
    // left/right). Expanding on both axes makes a horizontal pill collapse to its
    // natural content width.
    outer.set_hexpand(!is_vertical);
    outer.set_vexpand(is_vertical);
    outer.set_halign(edge_halign(config.position));
    outer.set_valign(edge_valign(config.position));
    outer.remove_css_class("metis-bar-outer-vertical");
    if is_vertical {
        outer.add_css_class("metis-bar-outer-vertical");
        outer.set_size_request(thickness, -1);
    } else {
        outer.set_size_request(-1, thickness);
    }

    column.set_orientation(gtk::Orientation::Vertical);
    column.set_hexpand(!is_vertical);
    column.set_vexpand(is_vertical);
    column.set_halign(edge_halign(config.position));
    column.set_valign(edge_valign(config.position));
    if is_vertical {
        column.set_size_request(thickness, -1);
    } else {
        column.set_size_request(-1, thickness);
    }

    pill.set_orientation(orientation_for(config));
    pill.remove_css_class("metis-bar-pill-vertical");
    pill.remove_css_class("metis-bar-pill-vertical-right");
    if is_vertical {
        pill.set_size_request(bar_cross_thickness(config), -1);
        pill.add_css_class("metis-bar-pill-vertical");
        if matches!(config.position, BarPosition::Right) {
            pill.add_css_class("metis-bar-pill-vertical-right");
        }
    } else {
        pill.set_size_request(-1, config.height as i32);
    }
    apply_pill_layout(pill, config);
}

/// Horizontal alignment of the bar strip within its layer surface (flush to the
/// anchored screen edge; shadow pad sits on the inner side).
fn edge_halign(position: BarPosition) -> gtk::Align {
    match position {
        BarPosition::Right => gtk::Align::End,
        BarPosition::Left => gtk::Align::Start,
        // Top/bottom bars fill the full monitor width.
        BarPosition::Top | BarPosition::Bottom => gtk::Align::Fill,
    }
}

/// Vertical alignment of the bar strip within its layer surface.
fn edge_valign(position: BarPosition) -> gtk::Align {
    match position {
        BarPosition::Bottom => gtk::Align::End,
        BarPosition::Top => gtk::Align::Start,
        // Left/right bars fill the full monitor height.
        BarPosition::Left | BarPosition::Right => gtk::Align::Fill,
    }
}

fn apply_pill_layout(pill: &gtk::Box, config: &BarConfig) {
    pill.remove_css_class("metis-bar-full");
    pill.remove_css_class("metis-bar-floating");
    let vertical = matches!(config.position, BarPosition::Left | BarPosition::Right);
    // Inset the pill within the (larger) layer surface so the rounded drop shadow
    // has breathing room along the bar's long edges (the pill's rounded ends).
    let side_pad = metis_config::bar::PILL_SIDE_INSET;
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
        if vertical {
            // Keep the pill at `height` px wide; the layer surface is wider only
            // for the inner-edge shadow pad — do not stretch the pill into it.
            pill.set_hexpand(false);
            pill.set_vexpand(true);
            pill.set_halign(edge_halign(config.position));
            pill.set_valign(gtk::Align::Fill);
        } else {
            pill.set_hexpand(true);
            pill.set_vexpand(false);
            pill.set_halign(gtk::Align::Fill);
            pill.set_valign(gtk::Align::Fill);
        }
    } else {
        pill.add_css_class("metis-bar-floating");
        pill.set_hexpand(false);
        pill.set_vexpand(false);
        pill.set_halign(if matches!(config.position, BarPosition::Right) {
            gtk::Align::End
        } else {
            gtk::Align::Center
        });
        // Flush the floating pill against the anchored edge so the shadow pad sits
        // on the inner side (below a top bar, above a bottom bar).
        pill.set_valign(if matches!(config.position, BarPosition::Bottom) {
            gtk::Align::End
        } else {
            gtk::Align::Start
        });
    }
}

fn orientation_for(config: &BarConfig) -> gtk::Orientation {
    match config.position {
        BarPosition::Top | BarPosition::Bottom => gtk::Orientation::Horizontal,
        BarPosition::Left | BarPosition::Right => gtk::Orientation::Vertical,
    }
}

fn apply_layer_geometry(window: &gtk::Window, config: &BarConfig) {
    if !window.is_layer_window() {
        window.init_layer_shell();
    }
    window.set_layer(Layer::Top);
    window.set_namespace("metis-bar");
    window.add_css_class("metis-bar-window");
    // OnDemand (not None) so popovers spawned from the bar can receive keyboard
    // focus via their xdg_popup grab (text entries in the clock/calendar popover).
    window.set_keyboard_mode(KeyboardMode::OnDemand);

    for edge in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
        window.set_anchor(edge, false);
        window.set_margin(edge, 0);
    }

    let thickness = bar_body_thickness(config);
    // Reserve only the *visible* bar (margin + body), not the extra shadow padding
    // baked into the surface thickness. This lets windows tuck right up under the
    // bar's bottom edge (the shadow pad region is transparent) instead of leaving a
    // chunk of dead space below the bar.
    let visible_thickness = bar_cross_thickness(config);
    // Only the top bar reserves screen space (windows reflow below it). Bottom
    // and side bars overlay the desktop; maximize/snap insets come from config.
    let exclusive = match config.position {
        BarPosition::Top => config.margin_top as i32 + visible_thickness,
        BarPosition::Bottom | BarPosition::Left | BarPosition::Right => 0,
    };
    window.set_exclusive_zone(exclusive);

    match config.position {
        BarPosition::Top => {
            window.set_anchor(Edge::Top, true);
            window.set_anchor(Edge::Left, true);
            window.set_anchor(Edge::Right, true);
            window.set_margin(Edge::Top, config.margin_top as i32);
            window.set_margin(Edge::Left, config.margin_h as i32);
            window.set_margin(Edge::Right, config.margin_h as i32);
            window.set_height_request(thickness);
            window.set_width_request(-1);
        }
        BarPosition::Bottom => {
            window.set_anchor(Edge::Bottom, true);
            window.set_anchor(Edge::Left, true);
            window.set_anchor(Edge::Right, true);
            window.set_margin(Edge::Bottom, config.margin_top as i32);
            window.set_margin(Edge::Left, config.margin_h as i32);
            window.set_margin(Edge::Right, config.margin_h as i32);
            window.set_height_request(thickness);
            window.set_width_request(-1);
        }
        BarPosition::Left => {
            window.set_anchor(Edge::Left, true);
            window.set_anchor(Edge::Top, true);
            window.set_anchor(Edge::Bottom, true);
            window.set_margin(Edge::Left, config.margin_top as i32);
            window.set_margin(Edge::Top, config.margin_h as i32);
            window.set_margin(Edge::Bottom, config.margin_h as i32);
            window.set_width_request(thickness);
            window.set_height_request(-1);
        }
        BarPosition::Right => {
            window.set_anchor(Edge::Right, true);
            window.set_anchor(Edge::Top, true);
            window.set_anchor(Edge::Bottom, true);
            window.set_margin(Edge::Right, config.margin_top as i32);
            window.set_margin(Edge::Top, config.margin_h as i32);
            window.set_margin(Edge::Bottom, config.margin_h as i32);
            window.set_width_request(thickness);
            window.set_height_request(-1);
        }
    }

    // The configurable opacity dims only the bar's background surface (applied via
    // a CSS provider in the theme loader), so icons/text stay fully opaque. The
    // window itself must remain at full opacity.
    window.set_opacity(1.0);
    crate::ui::theme::apply_bar_appearance(config.opacity, &config.bar_border, config.position);
    crate::ui::theme::apply_menu_opacity(config.menu_opacity);

    // Anchor/margin changes do not always trigger a GTK relayout on their own;
    // queue a resize so gtk-layer-shell commits the new surface dimensions.
    window.queue_resize();
}

/// Re-apply geometry/widgets to every existing bar in place (keeps the layer
/// surfaces — used for live theme/opacity/position edits that don't change the
/// set of outputs).
fn rebuild_bars_in_place(config: Rc<RefCell<BarConfig>>) {
    dropdown::close_all();
    BARS.with(|bars| {
        let mut bars = bars.borrow_mut();
        let cfg = config.borrow();
        let (win_w, win_h) = layer_window_size(&cfg);
        for handle in bars.iter_mut() {
            configure_surface(&handle.outer, &handle.column, &handle.pill, &cfg);
            apply_layer_geometry(&handle.window, &cfg);
            handle.window.set_default_size(win_w, win_h);
            handle.outer.queue_resize();
            handle.column.queue_resize();
            handle.pill.queue_resize();
            while let Some(child) = handle.pill.first_child() {
                handle.pill.remove(&child);
            }
            handle.widget_refs = widgets::build(&handle.pill, config.clone(), handle.output.clone());
        }
    });
}

/// Tear down all bars and rebuild from scratch for the current monitor set —
/// used when the number of target outputs changes (monitor hotplug or toggling
/// the `displays` option).
fn rebuild_all_bars(config: Rc<RefCell<BarConfig>>) {
    dropdown::close_all();
    let cfg = config.borrow().clone();
    // Build the new bars *before* destroying the old ones to avoid a one-frame
    // flash with no bar on screen. (The shell runs its own GLib main loop, so an
    // empty window set never quits it.)
    let new_handles: Vec<BarHandle> = target_monitors(&cfg)
        .iter()
        .map(|m| build_bar(config.clone(), m.as_ref()))
        .collect();
    let old = BARS.with(|bars| std::mem::replace(&mut *bars.borrow_mut(), new_handles));
    for handle in old {
        handle.window.destroy();
    }
}

fn watch_bar_config() {
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
        glib::timeout_add_local_once(std::time::Duration::from_millis(250), || {
            // Re-reads bar.json and rebuilds in place, or recreates the surfaces if
            // the `displays` option changed the number of bars.
            rebuild_from_config();
        });
    });
}

/// Live-reload the active theme when any `themes/*.json` changes. Mirrors
/// `watch_bar_config`: the GFileMonitor stays alive via the main-context source,
/// and the debounced callback re-runs `init_theme()` (which re-reads the active
/// mode + on-disk token file and re-applies the CssProvider).
fn watch_theme_files() {
    let dir = crate::config::config_dir().join("themes");
    if let Err(err) = crate::config::ensure_config_dirs() {
        tracing::warn!(%err, "failed to ensure themes dir");
    }
    let file = gio::File::for_path(&dir);
    let Ok(monitor) =
        file.monitor_directory(gio::FileMonitorFlags::NONE, None::<&gio::Cancellable>)
    else {
        tracing::warn!(path = %dir.display(), "themes dir monitor unavailable");
        return;
    };

    monitor.connect_changed(move |_, _, _event, _| {
        glib::timeout_add_local_once(std::time::Duration::from_millis(250), || {
            let _ = crate::ui::theme::init_theme();
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
            BARS.with(|bars| {
                for handle in bars.borrow().iter() {
                    handle.widget_refs.apply_weather(&snapshot);
                }
            });
        }
        glib::ControlFlow::Continue
    });
}

/// Drain notifications delivered by the freedesktop D-Bus daemon thread and push
/// them into the (thread-local) in-bar notification store on the UI thread.
fn attach_notification_channel(rx: Receiver<BarNotification>) {
    glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
        while let Ok(note) = rx.try_recv() {
            crate::services::push_notification(note);
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
            BARS.with(|bars| {
                for handle in bars.borrow().iter() {
                    handle.widget_refs.apply_snapshot(&snapshot);
                }
            });
        }
        glib::ControlFlow::Continue
    });
}

/// Mirror a user audio change (volume/mic/mute) onto every bar immediately, so a
/// multi-monitor session doesn't wait for the pactl poll round-trip to update the
/// other displays' volume icons/sliders.
pub fn broadcast_audio(percent: u8, muted: bool, mic_percent: u8, mic_muted: bool) {
    BARS.with(|bars| {
        for handle in bars.borrow().iter() {
            handle
                .widget_refs
                .apply_volume_optimistic(percent, muted, mic_percent, mic_muted);
        }
    });
}

pub fn rebuild_from_config() {
    // Coalesce bursts: one settings change writes bar.json (which can emit several
    // file-change events) *and* sends a `reload-bar` runtime command, so multiple
    // rebuild triggers land within a few hundred ms. Collapsing them into a single
    // deferred rebuild avoids overlapping teardown/rebuild passes racing each
    // other (and the deferred surface present) while bars are being recreated.
    if REBUILD_SCHEDULED.with(|f| f.replace(true)) {
        return;
    }
    glib::timeout_add_local_once(std::time::Duration::from_millis(80), || {
        REBUILD_SCHEDULED.with(|f| f.set(false));
        rebuild_from_config_now();
    });
}

fn rebuild_from_config_now() {
    // Clone the config Rc out and drop the BARS borrow *before* calling the
    // rebuild helpers, which re-borrow BARS mutably. Holding the borrow across the
    // call panics with "RefCell already borrowed".
    let (config, cur_count) = BARS.with(|bars| {
        let bars = bars.borrow();
        (bars.first().map(|h| h.config.clone()), bars.len())
    });
    let Some(config) = config else {
        return;
    };
    // Pull the latest on-disk bar config so live theme/opacity edits apply.
    *config.borrow_mut() = load_bar_config();
    let target_count = target_monitors(&config.borrow()).len();
    if target_count != cur_count {
        // The set of outputs changed (hotplug, or `displays` toggled) — recreate
        // the bar surfaces so each output gets (or loses) its bar.
        rebuild_all_bars(config.clone());
    } else {
        rebuild_bars_in_place(config.clone());
    }
    crate::ui::theme::reload_stylesheet();
}
