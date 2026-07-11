//! Native Metis screenshot overlay (Phase 12).

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, SystemTime};

use gtk::gdk;
use gtk::prelude::*;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use metis_capture::{capture_png, CaptureOptions};
use metis_config::{
    expand_save_dir, load_screenshot_config, parse_hex_rgb, save_default_screenshot_config,
    AfterCaptureAction, ScreenshotConfig, ScreenshotMode,
};
use metis_protocol::{CompositorCommand, OutputInfo, PixelRect, WindowInfo};

use crate::ui::theme::active_tokens;

use crate::services::{BarNotification, NotificationKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchMode {
    Interactive,
    InstantFull,
    Window,
}

#[derive(Debug, Clone, Copy, Default)]
struct DragRect {
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
}

impl DragRect {
    fn normalized(&self) -> PixelRect {
        let x = self.x1.min(self.x2).round() as i32;
        let y = self.y1.min(self.y2).round() as i32;
        let width = (self.x2 - self.x1).abs().round() as i32;
        let height = (self.y2 - self.y1).abs().round() as i32;
        PixelRect { x, y, width, height }
    }

    fn valid(&self) -> bool {
        let r = self.normalized();
        r.width >= 4 && r.height >= 4
    }
}

struct Overlay {
    window: gtk::Window,
    canvas: gtk::DrawingArea,
    size_label: gtk::Label,
    toolbar: gtk::Box,
    mode: Cell<ScreenshotMode>,
    draw_cursor: Cell<bool>,
    delay_seconds: Cell<u32>,
    after_capture: Cell<AfterCaptureAction>,
    drag: RefCell<Option<DragRect>>,
    hover_window: RefCell<Option<PixelRect>>,
    /// Last window highlighted in Window mode — kept when the pointer leaves the
    /// canvas (e.g. moving to the Capture button).
    picked_window: RefCell<Option<PixelRect>>,
    /// Click on a window locks the pick until mode changes or empty canvas click.
    window_locked: Cell<bool>,
    monitor_origin: (i32, i32),
    output_index: usize,
    config: ScreenshotConfig,
    windows: RefCell<Vec<WindowInfo>>,
}

thread_local! {
    static OVERLAY: RefCell<Option<Rc<Overlay>>> = const { RefCell::new(None) };
}

pub fn init() {
    if let Err(err) = save_default_screenshot_config() {
        tracing::warn!(%err, "failed to write default screenshot.json");
    }
}

pub fn is_active() -> bool {
    OVERLAY.with(|o| o.borrow().is_some())
}

pub fn on_theme_changed() {
    OVERLAY.with(|o| {
        if let Some(overlay) = o.borrow().as_ref() {
            overlay.canvas.queue_draw();
            overlay.toolbar.queue_draw();
        }
    });
}

pub fn show(mode: LaunchMode) {
    if OVERLAY.with(|o| o.borrow().is_some()) {
        return;
    }
    // Close bar popovers / dashboard, but keep the Notification Center open so
    // it can be included in the capture.
    crate::ui::bar::close_bar_popovers();
    crate::ui::dashboard::request_close();

    let config = load_screenshot_config();
    match mode {
        LaunchMode::InstantFull => {
            run_instant_capture(ScreenshotMode::Screen, config);
            return;
        }
        LaunchMode::Window => {
            show_interactive(ScreenshotMode::Window, config);
            return;
        }
        LaunchMode::Interactive => {
            show_interactive(config.default_mode, config);
        }
    }
}

fn show_interactive(initial_mode: ScreenshotMode, config: ScreenshotConfig) {
    let _ = send_compositor(CompositorCommand::BeginScreenshotOverlay);
    // Keep NC visible for capture, but park it on Top so this Overlay picker
    // paints above it (otherwise the panel covers the selection UI).
    crate::ui::notification_center::set_below_screenshot(true);

    let (monitor_origin, output_index) = monitor_context();
    let windows = list_windows_best_effort();

    let window = gtk::Window::builder()
        .title("Screenshot")
        .decorated(false)
        .build();
    window.add_css_class("metis-screenshot-window");
    window.init_layer_shell();
    window.set_layer(Layer::Overlay);
    window.set_keyboard_mode(KeyboardMode::Exclusive);
    window.set_namespace("metis-screenshot");
    for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
        window.set_anchor(edge, true);
        window.set_exclusive_zone(-1);
        window.set_margin(edge, 0);
    }

    let chrome = gtk::Overlay::new();
    window.set_child(Some(&chrome));

    let canvas = gtk::DrawingArea::new();
    canvas.add_css_class("metis-screenshot-canvas");
    canvas.set_hexpand(true);
    canvas.set_vexpand(true);
    canvas.set_can_focus(true);
    chrome.set_child(Some(&canvas));

    let size_label = gtk::Label::new(None);
    size_label.add_css_class("metis-screenshot-size");
    size_label.set_visible(false);
    chrome.add_overlay(&size_label);

    let toolbar_wrap = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    toolbar_wrap.add_css_class("metis-screenshot-toolbar-wrap");
    toolbar_wrap.set_halign(gtk::Align::Center);
    toolbar_wrap.set_valign(gtk::Align::End);
    toolbar_wrap.set_margin_bottom(28);
    chrome.add_overlay(&toolbar_wrap);

    let (toolbar, mode_buttons, options_widgets, capture_btn) =
        build_toolbar(initial_mode, config.draw_cursor, config.delay_seconds, config.after_capture);
    toolbar_wrap.append(&toolbar);

    let overlay = Rc::new(Overlay {
        window: window.clone(),
        canvas: canvas.clone(),
        size_label: size_label.clone(),
        toolbar: toolbar.clone(),
        mode: Cell::new(initial_mode),
        draw_cursor: Cell::new(config.draw_cursor),
        delay_seconds: Cell::new(config.delay_seconds),
        after_capture: Cell::new(config.after_capture),
        drag: RefCell::new(None),
        hover_window: RefCell::new(None),
        picked_window: RefCell::new(None),
        window_locked: Cell::new(false),
        monitor_origin,
        output_index,
        config,
        windows: RefCell::new(windows),
    });

    wire_canvas(&overlay, &canvas, &size_label);
    wire_toolbar(&overlay, &mode_buttons, &options_widgets, capture_btn);
    wire_keyboard(&window, &chrome);

    canvas.set_draw_func({
        let overlay = overlay.clone();
        move |_area, cr, width, height| {
            draw_scene(&overlay, cr, width, height);
        }
    });

    OVERLAY.with(|o| *o.borrow_mut() = Some(overlay.clone()));

    glib::idle_add_local_once(move || {
        window.set_visible(true);
        window.present();
        if !window.grab_focus() {
            canvas.grab_focus();
        }
    });
}

#[derive(Clone)]
struct ModeButtons {
    selection: gtk::ToggleButton,
    screen: gtk::ToggleButton,
    window: gtk::ToggleButton,
}

struct OptionsWidgets {
    pointer: gtk::Switch,
    delay: gtk::SpinButton,
    after_copy: gtk::ToggleButton,
    after_save: gtk::ToggleButton,
    after_both: gtk::ToggleButton,
    after_open: gtk::ToggleButton,
}

fn build_toolbar(
    initial_mode: ScreenshotMode,
    draw_cursor: bool,
    delay_seconds: u32,
    after_capture: AfterCaptureAction,
) -> (gtk::Box, ModeButtons, OptionsWidgets, gtk::Button) {
    let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    toolbar.add_css_class("metis-screenshot-toolbar");

    let mode_box = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    mode_box.add_css_class("metis-screenshot-mode");

    let selection_btn = mode_button("select-rect-symbolic", "Selection");
    let screen_btn = mode_button("view-fullscreen-symbolic", "Full screen");
    let window_btn = mode_button("window-symbolic", "Window");
    mode_box.append(&selection_btn);
    mode_box.append(&screen_btn);
    mode_box.append(&window_btn);
    toolbar.append(&mode_box);

    let options_btn = gtk::MenuButton::new();
    options_btn.set_icon_name("preferences-system-symbolic");
    options_btn.add_css_class("metis-screenshot-icon");
    options_btn.set_tooltip_text(Some("Options"));

    let pointer_switch = gtk::Switch::new();
    pointer_switch.set_active(draw_cursor);
    let delay_spin = gtk::SpinButton::with_range(0.0, 30.0, 1.0);
    delay_spin.set_value(delay_seconds as f64);
    delay_spin.set_digits(0);
    let after_seg = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    after_seg.add_css_class("metis-screenshot-after-seg");
    after_seg.add_css_class("linked");

    let (after_copy, after_save, after_both, after_open) = {
        let labels = ["Copy", "Save", "Both", "Open"];
        let mut buttons = Vec::new();
        let mut leader: Option<gtk::ToggleButton> = None;
        for label in labels {
            let btn = gtk::ToggleButton::with_label(label);
            btn.add_css_class("metis-screenshot-after-btn");
            if let Some(ref head) = leader {
                btn.set_group(Some(head));
            } else {
                leader = Some(btn.clone());
            }
            after_seg.append(&btn);
            buttons.push(btn);
        }
        (
            buttons[0].clone(),
            buttons[1].clone(),
            buttons[2].clone(),
            buttons[3].clone(),
        )
    };
    sync_after_buttons(
        &after_copy,
        &after_save,
        &after_both,
        &after_open,
        after_capture,
    );

    let options_widgets = OptionsWidgets {
        pointer: pointer_switch.clone(),
        delay: delay_spin.clone(),
        after_copy: after_copy.clone(),
        after_save: after_save.clone(),
        after_both: after_both.clone(),
        after_open: after_open.clone(),
    };

    let popover = build_options_popover(
        &pointer_switch,
        &delay_spin,
        &after_seg,
    );
    options_btn.set_popover(Some(&popover));
    toolbar.append(&options_btn);

    let capture_btn = gtk::Button::with_label("Capture");
    capture_btn.add_css_class("metis-screenshot-capture");
    toolbar.append(&capture_btn);

    let mode_buttons = ModeButtons {
        selection: selection_btn,
        screen: screen_btn,
        window: window_btn,
    };
    sync_mode_buttons(&mode_buttons, initial_mode);

    (toolbar, mode_buttons, options_widgets, capture_btn)
}

fn mode_button(icon_name: &str, tooltip: &str) -> gtk::ToggleButton {
    let btn = gtk::ToggleButton::new();
    btn.set_child(Some(&crate::ui::icons::image(icon_name)));
    btn.set_tooltip_text(Some(tooltip));
    btn.add_css_class("metis-screenshot-mode-btn");
    btn
}

fn sync_mode_buttons(buttons: &ModeButtons, mode: ScreenshotMode) {
    buttons.selection.set_active(mode == ScreenshotMode::Selection);
    buttons.screen.set_active(mode == ScreenshotMode::Screen);
    buttons.window.set_active(mode == ScreenshotMode::Window);
}

fn sync_after_buttons(
    copy: &gtk::ToggleButton,
    save: &gtk::ToggleButton,
    both: &gtk::ToggleButton,
    open: &gtk::ToggleButton,
    action: AfterCaptureAction,
) {
    copy.set_active(action == AfterCaptureAction::Copy);
    save.set_active(action == AfterCaptureAction::Save);
    both.set_active(action == AfterCaptureAction::CopyAndSave);
    open.set_active(action == AfterCaptureAction::Open);
}

fn after_from_buttons(
    copy: &gtk::ToggleButton,
    save: &gtk::ToggleButton,
    both: &gtk::ToggleButton,
    open: &gtk::ToggleButton,
) -> AfterCaptureAction {
    if save.is_active() {
        AfterCaptureAction::Save
    } else if both.is_active() {
        AfterCaptureAction::CopyAndSave
    } else if open.is_active() {
        AfterCaptureAction::Open
    } else {
        AfterCaptureAction::Copy
    }
}

fn build_options_popover(
    pointer_switch: &gtk::Switch,
    delay_spin: &gtk::SpinButton,
    after_seg: &gtk::Box,
) -> gtk::Popover {
    let popover = gtk::Popover::new();
    popover.add_css_class("metis-screenshot-popover");
    popover.set_has_arrow(true);

    let panel = gtk::Box::new(gtk::Orientation::Vertical, 10);
    panel.add_css_class("metis-screenshot-options");
    panel.set_margin_start(12);
    panel.set_margin_end(12);
    panel.set_margin_top(10);
    panel.set_margin_bottom(10);
    panel.set_size_request(280, -1);

    panel.append(&option_row("Include pointer", pointer_switch));
    panel.append(&option_row("Delay (seconds)", delay_spin));

    let after_label = gtk::Label::new(Some("After capture"));
    after_label.set_halign(gtk::Align::Start);
    after_label.add_css_class("metis-screenshot-option-label");
    panel.append(&after_label);
    panel.append(after_seg);

    popover.set_child(Some(&panel));
    popover
}

fn option_row(label: &str, control: &impl IsA<gtk::Widget>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let title = gtk::Label::new(Some(label));
    title.set_hexpand(true);
    title.set_halign(gtk::Align::Start);
    title.add_css_class("metis-screenshot-option-label");
    row.append(&title);
    row.append(control);
    row
}

fn wire_after_buttons(
    overlay: &Rc<Overlay>,
    copy: &gtk::ToggleButton,
    save: &gtk::ToggleButton,
    both: &gtk::ToggleButton,
    open: &gtk::ToggleButton,
) {
    let sync = {
        let overlay = overlay.clone();
        let copy = copy.clone();
        let save = save.clone();
        let both = both.clone();
        let open = open.clone();
        move || {
            overlay
                .after_capture
                .set(after_from_buttons(&copy, &save, &both, &open));
        }
    };
    copy.connect_toggled({
        let sync = sync.clone();
        move |_| sync()
    });
    save.connect_toggled({
        let sync = sync.clone();
        move |_| sync()
    });
    both.connect_toggled({
        let sync = sync.clone();
        move |_| sync()
    });
    open.connect_toggled({
        let sync = sync.clone();
        move |_| sync()
    });
}

fn wire_toolbar(
    overlay: &Rc<Overlay>,
    mode_buttons: &ModeButtons,
    options: &OptionsWidgets,
    capture_btn: gtk::Button,
) {
    options.pointer.connect_active_notify({
        let overlay = overlay.clone();
        move |sw| overlay.draw_cursor.set(sw.is_active())
    });
    options.delay.connect_value_changed({
        let overlay = overlay.clone();
        move |spin| {
            overlay
                .delay_seconds
                .set(spin.value().max(0.0).round() as u32);
        }
    });
    wire_after_buttons(
        overlay,
        &options.after_copy,
        &options.after_save,
        &options.after_both,
        &options.after_open,
    );

    let apply_mode = {
        let overlay = overlay.clone();
        let mode_buttons = ModeButtons {
            selection: mode_buttons.selection.clone(),
            screen: mode_buttons.screen.clone(),
            window: mode_buttons.window.clone(),
        };
        move |mode: ScreenshotMode| {
            overlay.mode.set(mode);
            sync_mode_buttons(&mode_buttons, mode);
            overlay.drag.replace(None);
            overlay.hover_window.replace(None);
            overlay.picked_window.replace(None);
            overlay.window_locked.set(false);
            overlay.canvas.queue_draw();
            overlay.size_label.set_visible(false);
        }
    };

    mode_buttons.selection.connect_toggled({
        let overlay = overlay.clone();
        let mode_buttons = ModeButtons {
            selection: mode_buttons.selection.clone(),
            screen: mode_buttons.screen.clone(),
            window: mode_buttons.window.clone(),
        };
        let apply_mode = apply_mode.clone();
        move |btn| {
            if btn.is_active() {
                apply_mode(ScreenshotMode::Selection);
            } else {
                sync_mode_buttons(&mode_buttons, overlay.mode.get());
            }
        }
    });
    mode_buttons.screen.connect_toggled({
        let overlay = overlay.clone();
        let mode_buttons = ModeButtons {
            selection: mode_buttons.selection.clone(),
            screen: mode_buttons.screen.clone(),
            window: mode_buttons.window.clone(),
        };
        let apply_mode = apply_mode.clone();
        move |btn| {
            if btn.is_active() {
                apply_mode(ScreenshotMode::Screen);
            } else {
                sync_mode_buttons(&mode_buttons, overlay.mode.get());
            }
        }
    });
    mode_buttons.window.connect_toggled({
        let overlay = overlay.clone();
        let mode_buttons = ModeButtons {
            selection: mode_buttons.selection.clone(),
            screen: mode_buttons.screen.clone(),
            window: mode_buttons.window.clone(),
        };
        let apply_mode = apply_mode.clone();
        move |btn| {
            if btn.is_active() {
                apply_mode(ScreenshotMode::Window);
            } else {
                sync_mode_buttons(&mode_buttons, overlay.mode.get());
            }
        }
    });

    capture_btn.connect_clicked({
        let overlay = overlay.clone();
        move |_| overlay.clone().request_capture()
    });
}

fn on_overlay_key(key: gdk::Key, modifier: gdk::ModifierType) -> glib::Propagation {
    if key == gdk::Key::Escape {
        dismiss();
        glib::Propagation::Stop
    } else if key == gdk::Key::Return || key == gdk::Key::KP_Enter {
        if !modifier.contains(gdk::ModifierType::SHIFT_MASK) {
            if let Some(overlay) = OVERLAY.with(|o| o.borrow().clone()) {
                overlay.request_capture();
            }
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    } else {
        glib::Propagation::Proceed
    }
}

fn wire_keyboard(window: &gtk::Window, root: &gtk::Overlay) {
    for widget in [window.upcast_ref::<gtk::Widget>(), root.upcast_ref::<gtk::Widget>()] {
        let controller = gtk::EventControllerKey::new();
        controller.set_propagation_phase(gtk::PropagationPhase::Capture);
        controller.connect_key_pressed(|_, key, _, modifier| on_overlay_key(key, modifier));
        widget.add_controller(controller);
    }
}

fn wire_canvas(overlay: &Rc<Overlay>, canvas: &gtk::DrawingArea, size_label: &gtk::Label) {
    let gesture = gtk::GestureDrag::new();
    gesture.set_button(0);
    gesture.connect_drag_begin({
        let overlay = overlay.clone();
        move |_, x, y| {
            if overlay.mode.get() != ScreenshotMode::Selection {
                return;
            }
            overlay.drag.replace(Some(DragRect {
                x1: x,
                y1: y,
                x2: x,
                y2: y,
            }));
            overlay.size_label.set_visible(false);
        }
    });
    gesture.connect_drag_update({
        let overlay = overlay.clone();
        let size_label = size_label.clone();
        move |_, offset_x, offset_y| {
            if overlay.mode.get() != ScreenshotMode::Selection {
                return;
            }
            let Some(mut drag) = overlay.drag.borrow().clone() else {
                return;
            };
            drag.x2 = drag.x1 + offset_x;
            drag.y2 = drag.y1 + offset_y;
            overlay.drag.replace(Some(drag));
            if drag.valid() {
                let rect = drag.normalized();
                size_label.set_text(&format!("{} × {}", rect.width, rect.height));
                size_label.set_visible(true);
                position_size_label(&size_label, &drag);
            }
            overlay.canvas.queue_draw();
        }
    });
    canvas.add_controller(gesture);

    let motion = gtk::EventControllerMotion::new();
    motion.connect_motion({
        let overlay = overlay.clone();
        move |_, x, y| {
            if overlay.mode.get() == ScreenshotMode::Window {
                let hit = window_at(&overlay, x, y);
                if overlay.window_locked.get() {
                    overlay.hover_window.replace(hit);
                } else {
                    overlay.hover_window.replace(hit);
                    if hit.is_some() {
                        overlay.picked_window.replace(hit);
                    }
                }
                overlay.canvas.queue_draw();
            }
        }
    });
    canvas.add_controller(motion);

    let click = gtk::GestureClick::new();
    click.set_button(0);
    click.connect_pressed({
        let overlay = overlay.clone();
        move |_, _n, x, y| {
            if overlay.mode.get() == ScreenshotMode::Window {
                if let Some(rect) = window_at(&overlay, x, y) {
                    overlay.picked_window.replace(Some(rect));
                    overlay.hover_window.replace(Some(rect));
                    overlay.window_locked.set(true);
                    overlay.canvas.queue_draw();
                } else {
                    overlay.window_locked.set(false);
                    overlay.picked_window.replace(None);
                    overlay.hover_window.replace(None);
                    overlay.canvas.queue_draw();
                }
            }
        }
    });
    canvas.add_controller(click);

    let right = gtk::GestureClick::new();
    right.set_button(3);
    right.connect_pressed(|_, _, _, _| dismiss());
    canvas.add_controller(right);
}

impl Overlay {
    fn request_capture(self: Rc<Self>) {
        let crop = self.capture_rect();
        let Some(crop) = crop else {
            let msg = match self.mode.get() {
                ScreenshotMode::Window => "Select a window to capture",
                _ => "Select an area to capture",
            };
            toast_message(msg);
            return;
        };
        let draw_cursor = self.draw_cursor.get();
        let delay = self.delay_seconds.get();
        let after_capture = self.after_capture.get();
        let config = self.config.clone();
        let output_index = self.output_index;
        self.window.set_visible(false);

        glib::timeout_add_local_once(Duration::from_millis(80), move || {
            std::thread::spawn(move || {
                if delay > 0 {
                    std::thread::sleep(Duration::from_secs(delay as u64));
                }
                let result = perform_capture(crop, draw_cursor, output_index, &config);
                glib::idle_add_once(move || {
                    dismiss();
                    match result {
                        Ok(path) => after_capture_action(&config, &path, after_capture),
                        Err(err) => {
                            tracing::warn!(%err, "screenshot capture failed");
                            toast_message("Screenshot failed");
                        }
                    }
                });
            });
        });
    }

    fn capture_rect(&self) -> Option<PixelRect> {
        match self.mode.get() {
            ScreenshotMode::Selection => {
                let drag = self.drag.borrow().clone()?;
                if drag.valid() {
                    let local = drag.normalized();
                    Some(PixelRect {
                        x: self.monitor_origin.0 + local.x,
                        y: self.monitor_origin.1 + local.y,
                        width: local.width,
                        height: local.height,
                    })
                } else {
                    None
                }
            }
            ScreenshotMode::Screen => {
                let (mx, my) = self.monitor_origin;
                let w = self.canvas.width() as i32;
                let h = self.canvas.height() as i32;
                Some(PixelRect {
                    x: mx,
                    y: my,
                    width: w,
                    height: h,
                })
            }
            ScreenshotMode::Window => {
                if self.window_locked.get() {
                    self.picked_window.borrow().clone()
                } else {
                    self.picked_window
                        .borrow()
                        .clone()
                        .or_else(|| self.hover_window.borrow().clone())
                }
            }
        }
    }
}

fn draw_scene(overlay: &Overlay, cr: &gtk::cairo::Context, width: i32, height: i32) {
    let tokens = active_tokens();
    let is_light = tokens.mode.eq_ignore_ascii_case("light");
    cr.set_source_rgba(
        0.0,
        0.0,
        0.0,
        if is_light { 0.28 } else { 0.45 },
    );
    cr.paint().ok();

    let highlight = match overlay.mode.get() {
        ScreenshotMode::Selection => overlay
            .drag
            .borrow()
            .clone()
            .filter(|d| d.valid())
            .map(|d| d.normalized()),
        ScreenshotMode::Screen => Some(PixelRect {
            x: 0,
            y: 0,
            width,
            height,
        }),
        ScreenshotMode::Window => window_highlight_rect(overlay),
    };

    if let Some(rect) = highlight {
        cr.set_operator(gtk::cairo::Operator::Clear);
        cr.rectangle(rect.x as f64, rect.y as f64, rect.width as f64, rect.height as f64);
        cr.fill().ok();
        cr.set_operator(gtk::cairo::Operator::Over);

        let (ar, ag, ab) = {
            let rgb = parse_hex_rgb(tokens.accent_primary());
            (
                rgb[0] as f64 / 255.0,
                rgb[1] as f64 / 255.0,
                rgb[2] as f64 / 255.0,
            )
        };
        cr.set_source_rgb(ar, ag, ab);
        cr.set_line_width(2.0);
        cr.set_dash(&[8.0, 6.0], 0.0);
        cr.rectangle(rect.x as f64, rect.y as f64, rect.width as f64, rect.height as f64);
        cr.stroke().ok();
        cr.set_dash(&[], 0.0);
    }
}

fn position_size_label(label: &gtk::Label, drag: &DragRect) {
    let rect = drag.normalized();
    label.set_margin_start((rect.x + 8).max(0));
    label.set_margin_top((rect.y + 8).max(0));
    label.set_halign(gtk::Align::Start);
    label.set_valign(gtk::Align::Start);
}

fn window_highlight_rect(overlay: &Overlay) -> Option<PixelRect> {
    let global = if overlay.window_locked.get() {
        overlay.picked_window.borrow().clone()
    } else {
        overlay
            .picked_window
            .borrow()
            .clone()
            .or_else(|| overlay.hover_window.borrow().clone())
    }?;
    Some(PixelRect {
        x: global.x - overlay.monitor_origin.0,
        y: global.y - overlay.monitor_origin.1,
        width: global.width,
        height: global.height,
    })
}

fn window_at(overlay: &Overlay, x: f64, y: f64) -> Option<PixelRect> {
    let gx = overlay.monitor_origin.0 + x.round() as i32;
    let gy = overlay.monitor_origin.1 + y.round() as i32;
    overlay
        .windows
        .borrow()
        .iter()
        .filter(|w| !w.minimized)
        .find(|w| point_in_rect(gx, gy, w.rect))
        .map(|w| w.rect)
}

fn point_in_rect(x: i32, y: i32, rect: PixelRect) -> bool {
    x >= rect.x && y >= rect.y && x < rect.x + rect.width && y < rect.y + rect.height
}

fn primary_monitor_geometry() -> gdk::Rectangle {
    if let Some(display) = gdk::Display::default() {
        if let Some(obj) = display.monitors().item(0) {
            if let Ok(monitor) = obj.downcast::<gdk::Monitor>() {
                return monitor.geometry();
            }
        }
    }
    gdk::Rectangle::new(0, 0, 1920, 1080)
}

fn run_instant_capture(mode: ScreenshotMode, config: ScreenshotConfig) {
    let (origin, output_index) = monitor_context();
    let crop = match mode {
        ScreenshotMode::Screen => {
            let g = primary_monitor_geometry();
            PixelRect {
                x: g.x(),
                y: g.y(),
                width: g.width(),
                height: g.height(),
            }
        }
        _ => PixelRect {
            x: origin.0,
            y: origin.1,
            width: 1920,
            height: 1080,
        },
    };
    let draw_cursor = config.draw_cursor;
    std::thread::spawn(move || {
        if let Ok(path) = perform_capture(crop, draw_cursor, output_index, &config) {
            glib::idle_add_once(move || after_capture_action(&config, &path, config.after_capture));
        } else {
            tracing::warn!("instant screenshot failed");
            glib::idle_add_once(|| toast_message("Screenshot failed"));
        }
    });
}

fn perform_capture(
    crop: PixelRect,
    draw_cursor: bool,
    output_index: usize,
    config: &ScreenshotConfig,
) -> Result<PathBuf, String> {
    let path = capture_path(config);
    let (ox, oy) = output_origin(output_index);
    let local = PixelRect {
        x: crop.x - ox,
        y: crop.y - oy,
        width: crop.width,
        height: crop.height,
    };
    capture_png(
        CaptureOptions {
            draw_cursor,
            output_index,
        },
        Some(local),
        &path,
    )?;
    Ok(path)
}

fn after_capture_action(config: &ScreenshotConfig, path: &PathBuf, action: AfterCaptureAction) {
    match action {
        AfterCaptureAction::Copy | AfterCaptureAction::CopyAndSave => {
            if let Err(err) = crate::compositor::set_clipboard(
                "image/png".into(),
                None,
                Some(path.display().to_string()),
            ) {
                tracing::warn!(%err, "failed to copy screenshot to clipboard");
            }
        }
        _ => {}
    }
    if matches!(action, AfterCaptureAction::Save | AfterCaptureAction::CopyAndSave) {
        let save_dir = expand_save_dir(&config.save_dir);
        if let Err(err) = std::fs::create_dir_all(&save_dir) {
            tracing::warn!(%err, "failed to create screenshot save dir");
        } else {
            let dest = save_dir.join(path.file_name().unwrap_or_default());
            if dest != *path {
                if let Err(err) = std::fs::copy(path, &dest) {
                    tracing::warn!(%err, "failed to save screenshot copy");
                }
            }
        }
    }
    if matches!(action, AfterCaptureAction::Open) {
        let program = format!("xdg-open {}", shell_escape(path));
        let _ = crate::compositor::launch_program(&program);
    }
    toast_message("Screenshot captured");
}

fn toast_message(message: &str) {
    crate::ui::toast::show(&BarNotification::internal(
        NotificationKind::Information,
        "Screenshot",
        message,
    ));
}

fn capture_path(config: &ScreenshotConfig) -> PathBuf {
    let dir = expand_save_dir(&config.save_dir);
    let _ = std::fs::create_dir_all(&dir);
    let stamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    dir.join(format!("metis-{stamp}.png"))
}

fn shell_escape(path: &PathBuf) -> String {
    let text = path.display().to_string();
    if text.contains(' ') || text.contains('\'') {
        format!("'{text}'")
    } else {
        text
    }
}

pub fn dismiss() {
    OVERLAY.with(|o| {
        if let Some(overlay) = o.borrow_mut().take() {
            overlay.window.set_visible(false);
            overlay.window.destroy();
        }
    });
    let _ = send_compositor(CompositorCommand::EndScreenshotOverlay);
    crate::ui::notification_center::set_below_screenshot(false);
}

fn monitor_context() -> ((i32, i32), usize) {
    let g = primary_monitor_geometry();
    let outputs = list_outputs_best_effort();
    let index = outputs
        .iter()
        .position(|o| o.rect.x == g.x() && o.rect.y == g.y())
        .unwrap_or(0);
    ((g.x(), g.y()), index)
}

fn output_origin(output_index: usize) -> (i32, i32) {
    list_outputs_best_effort()
        .get(output_index)
        .map(|o| (o.rect.x, o.rect.y))
        .unwrap_or((0, 0))
}

fn list_outputs_best_effort() -> Vec<OutputInfo> {
    match send_compositor(CompositorCommand::ListOutputs) {
        Ok(metis_protocol::CompositorEvent::OutputList { outputs }) => outputs,
        _ => Vec::new(),
    }
}

fn list_windows_best_effort() -> Vec<WindowInfo> {
    crate::compositor::list_windows().unwrap_or_default()
}

fn send_compositor(cmd: CompositorCommand) -> Result<metis_protocol::CompositorEvent, std::io::Error> {
    metis_protocol::send_compositor_command(&cmd)
}
