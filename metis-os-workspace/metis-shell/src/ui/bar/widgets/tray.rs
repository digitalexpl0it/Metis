//! System tray widget for the edge bar (StatusNotifierItem host UI).

use std::cell::RefCell;
use std::rc::Rc;

use gtk::gdk;
use gtk::prelude::*;

use crate::config::TrayIconMode;
use crate::services::{self, send_command, TrayCommand, TrayItem, TrayMenu, TrayMenuItem, MenuType, TraySnapshot};

const TRAY_ICON_SIZE: i32 = 20;

thread_local! {
    /// Nested context menu opened from a tray icon while the tray popover stays up.
    static TRAY_ITEM_MENU: RefCell<Option<gtk::Popover>> = const { RefCell::new(None) };
}

struct TrayTooltipCtx {
    overlay: gtk::Overlay,
    tip: gtk::Label,
}

pub struct TrayWidget {
    root: gtk::Box,
    pinned_row: gtk::Box,
    panel_flow: gtk::FlowBox,
    mode: Rc<RefCell<TrayIconMode>>,
    refresh: Rc<dyn Fn()>,
}

impl TrayWidget {
    pub fn new(mode: TrayIconMode) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        root.add_css_class("metis-bar-tray");

        let pinned_row = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        pinned_row.add_css_class("metis-bar-tray-pinned");
        root.append(&pinned_row);

        let toggle = gtk::Button::builder().has_frame(false).build();
        toggle.add_css_class("metis-bar-widget");
        toggle.add_css_class("metis-bar-sys-icon");
        toggle.add_css_class("metis-bar-tray-toggle");
        toggle.set_tooltip_text(Some("System tray"));
        let toggle_icon = gtk::Image::from_icon_name("pan-up-symbolic");
        toggle_icon.set_pixel_size(TRAY_ICON_SIZE);
        toggle.set_child(Some(&toggle_icon));
        root.append(&toggle);

        let panel = gtk::Box::new(gtk::Orientation::Vertical, 6);
        panel.add_css_class("metis-bar-dropdown-panel");
        panel.add_css_class("metis-bar-tray-panel");
        let panel_title = gtk::Label::new(Some("Background apps"));
        panel_title.set_xalign(0.0);
        panel_title.add_css_class("metis-bar-section-title");
        panel.append(&panel_title);
        let panel_flow = gtk::FlowBox::new();
        panel_flow.set_selection_mode(gtk::SelectionMode::None);
        panel_flow.set_max_children_per_line(6);
        panel_flow.set_column_spacing(8);
        panel_flow.set_row_spacing(8);
        panel_flow.add_css_class("metis-bar-tray-flow");
        let panel_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .min_content_height(48)
            .max_content_height(240)
            .propagate_natural_width(true)
            .child(&panel_flow)
            .build();
        panel.append(&panel_scroll);

        let tip = gtk::Label::new(None);
        tip.add_css_class("metis-menu-tooltip-label");
        tip.set_visible(false);
        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&panel));
        overlay.add_overlay(&tip);
        tip.set_halign(gtk::Align::Start);
        tip.set_valign(gtk::Align::Start);
        let tooltip_ctx = Rc::new(TrayTooltipCtx {
            overlay: overlay.clone(),
            tip: tip.clone(),
        });

        let mode = Rc::new(RefCell::new(mode));

        let rebuild: Rc<dyn Fn()> = {
            let pinned_row = pinned_row.clone();
            let panel_flow = panel_flow.clone();
            let mode = mode.clone();
            let tooltip_ctx = tooltip_ctx.clone();
            Rc::new(move || {
                rebuild_tray(
                    &pinned_row,
                    &panel_flow,
                    *mode.borrow(),
                    &services::tray_snapshot(),
                    Some(tooltip_ctx.as_ref()),
                );
            })
        };

        services::register_tray_refresh(rebuild.clone());
        wire_tray_toggle(&toggle, &toggle_icon, &overlay, {
            let rebuild = rebuild.clone();
            move || {
                services::sync_tray();
                rebuild();
            }
        });

        Self {
            root,
            pinned_row,
            panel_flow,
            mode,
            refresh: rebuild,
        }
    }

    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    pub fn set_mode(&self, mode: TrayIconMode) {
        *self.mode.borrow_mut() = mode;
        (self.refresh)();
    }

    pub fn update(&self) {
        (self.refresh)();
    }
}

fn rebuild_tray(
    pinned_row: &gtk::Box,
    panel_flow: &gtk::FlowBox,
    mode: TrayIconMode,
    snap: &TraySnapshot,
    panel_tooltip: Option<&TrayTooltipCtx>,
) {
    dismiss_tray_item_menu();
    while let Some(child) = pinned_row.first_child() {
        pinned_row.remove(&child);
    }
    while let Some(child) = panel_flow.first_child() {
        panel_flow.remove(&child);
    }

    pinned_row.set_visible(mode == TrayIconMode::Pinned && !snap.items.is_empty());

    for item in &snap.items {
        if mode == TrayIconMode::Pinned {
            pinned_row.append(&build_tray_button(item, TRAY_ICON_SIZE, None));
        }
        panel_flow.append(&build_tray_button(
            item,
            TRAY_ICON_SIZE + 4,
            panel_tooltip,
        ));
    }

    if snap.items.is_empty() {
        let hint = gtk::Label::new(Some("No background apps"));
        hint.add_css_class("dim-label");
        panel_flow.append(&hint);
    }
}

fn build_tray_button(
    item: &TrayItem,
    icon_size: i32,
    panel_tooltip: Option<&TrayTooltipCtx>,
) -> gtk::Button {
    let btn = gtk::Button::builder().has_frame(false).build();
    btn.add_css_class("metis-bar-widget");
    btn.add_css_class("metis-bar-tray-item");

    let image = gtk::Image::new();
    image.set_pixel_size(icon_size);
    if let Some(texture) = pixmap_texture(item) {
        image.set_from_paintable(Some(&texture));
    } else if let Some(name) = &item.icon_name {
        image.set_from_icon_name(Some(name));
    } else {
        image.set_from_icon_name(Some("application-x-executable-symbolic"));
    }
    btn.set_child(Some(&image));

    match panel_tooltip {
        Some(ctx) => attach_tray_tooltip(&btn, &item.title, &ctx.overlay, &ctx.tip),
        None => btn.set_tooltip_text(Some(&item.title)),
    }

    wire_tray_button(&btn, item);
    btn
}

fn wire_tray_button(btn: &gtk::Button, item: &TrayItem) {
    let item = item.clone();

    {
        let item = item.clone();
        btn.connect_clicked(move |b| {
            let (x, y) = button_coords(b);
            send_command(TrayCommand::Activate {
                bus_name: item.bus_name.clone(),
                object_path: item.object_path.clone(),
                x,
                y,
            });
        });
    }

    let gesture = gtk::GestureClick::builder()
        .button(gdk::BUTTON_SECONDARY)
        .build();
    {
        let item = item.clone();
        let btn = btn.clone();
        gesture.connect_pressed(move |gesture, _, _, _| {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            if let Some(menu) = &item.menu {
                if !menu.submenus.is_empty() {
                    show_context_menu(&btn, &item, menu);
                    return;
                }
            }
            let (x, y) = button_coords(&btn);
            send_command(TrayCommand::SecondaryActivate {
                bus_name: item.bus_name.clone(),
                object_path: item.object_path.clone(),
                x,
                y,
            });
        });
    }
    btn.add_controller(gesture);
}

fn button_coords(btn: &gtk::Button) -> (i32, i32) {
    let alloc = btn.allocation();
    (alloc.x() + alloc.width() / 2, alloc.y() + alloc.height() / 2)
}

fn show_context_menu(anchor: &gtk::Button, item: &TrayItem, menu: &TrayMenu) {
    dismiss_tray_item_menu();

    let popover = gtk::Popover::builder()
        .autohide(false)
        .has_arrow(true)
        .position(super::super::popover_position())
        .build();
    popover.add_css_class("metis-bar-popover");
    popover.add_css_class("metis-bar-tray-menu");
    popover.set_parent(anchor);

    let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
    list.add_css_class("metis-bar-tray-menu-list");
    append_menu_items(&list, item, &menu.submenus);
    popover.set_child(Some(&list));

    super::super::dropdown::register(&popover);
    TRAY_ITEM_MENU.with(|cell| *cell.borrow_mut() = Some(popover.clone()));

    let weak = popover.downgrade();
    popover.connect_closed(move |_| {
        let weak = weak.clone();
        glib::idle_add_local_once(move || {
            if let Some(p) = weak.upgrade() {
                if p.parent().is_some() {
                    p.unparent();
                }
            }
        });
    });

    glib::idle_add_local_once(move || popover.popup());
}

fn dismiss_tray_item_menu() {
    TRAY_ITEM_MENU.with(|cell| {
        if let Some(p) = cell.borrow_mut().take() {
            p.popdown();
            if p.parent().is_some() {
                p.unparent();
            }
        }
    });
}

/// In-popover tooltips: GTK's native tooltips render behind our layer-shell popover.
fn attach_tray_tooltip(
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
                if let Some((x, y)) =
                    w.translate_coordinates(&ov, w.width() as f64 / 2.0, w.height() as f64)
                {
                    tip.set_margin_start((x as i32 - 24).max(0));
                    tip.set_margin_top((y as i32 + 6).max(0));
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

fn append_menu_items(list: &gtk::Box, item: &TrayItem, entries: &[TrayMenuItem]) {
    for entry in entries {
        if !entry.visible {
            continue;
        }
        match entry.menu_type {
            MenuType::Separator => {
                let sep = gtk::Separator::new(gtk::Orientation::Horizontal);
                sep.set_margin_top(4);
                sep.set_margin_bottom(4);
                list.append(&sep);
            }
            MenuType::Standard => {
                let row = gtk::Button::builder()
                    .label(&entry.label)
                    .has_frame(false)
                    .build();
                row.add_css_class("metis-bar-tray-menu-item");
                row.set_sensitive(entry.enabled);
                row.set_halign(gtk::Align::Fill);
                let click_item = item.clone();
                let menu_path = click_item.menu_path.clone().unwrap_or_default();
                let submenu_id = entry.id;
                row.connect_clicked(move |_| {
                    dismiss_tray_item_menu();
                    send_command(TrayCommand::MenuClicked {
                        bus_name: click_item.bus_name.clone(),
                        menu_path: menu_path.clone(),
                        submenu_id,
                    });
                });
                list.append(&row);
                if !entry.submenu.is_empty() {
                    append_menu_items(list, item, &entry.submenu);
                }
            }
        }
    }
}

fn pixmap_texture(item: &TrayItem) -> Option<gdk::Texture> {
    item.icon_pixmap.as_ref().map(|pixmap| pixmap_to_texture(pixmap))
}

fn pixmap_to_texture(pixmap: &crate::services::IconPixmap) -> gdk::Texture {
    let w = pixmap.width as usize;
    let h = pixmap.height as usize;
    if w == 0 || h == 0 {
        return gdk::MemoryTexture::new(
            1,
            1,
            gdk::MemoryFormat::R8g8b8a8,
            &glib::Bytes::from_static(&[0, 0, 0, 0]),
            4,
        ).into();
    }
    let mut rgba = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let src = (y * w + x) * 4;
            if src + 3 >= pixmap.pixels.len() {
                continue;
            }
            let a = pixmap.pixels[src];
            let r = pixmap.pixels[src + 1];
            let g = pixmap.pixels[src + 2];
            let b = pixmap.pixels[src + 3];
            let dst = (y * w + x) * 4;
            rgba[dst] = r;
            rgba[dst + 1] = g;
            rgba[dst + 2] = b;
            rgba[dst + 3] = a;
        }
    }
    gdk::MemoryTexture::new(
        pixmap.width,
        pixmap.height,
        gdk::MemoryFormat::R8g8b8a8,
        &glib::Bytes::from(&rgba),
        (w * 4) as usize,
    )
    .into()
}

fn wire_tray_toggle(
    button: &gtk::Button,
    icon: &gtk::Image,
    panel: &gtk::Overlay,
    prepare: impl Fn() + 'static,
) {
    let popover = gtk::Popover::builder()
        .autohide(false)
        .has_arrow(true)
        .position(super::super::popover_position())
        .child(panel)
        .build();
    popover.add_css_class("metis-bar-popover");
    popover.set_parent(button);

    {
        let btn = button.clone();
        let icon = icon.clone();
        popover.connect_map(move |_| {
            btn.add_css_class("metis-bar-dropdown-active");
            icon.set_from_icon_name(Some("pan-down-symbolic"));
        });
    }
    {
        let btn = button.clone();
        let icon = icon.clone();
        popover.connect_unmap(move |_| {
            dismiss_tray_item_menu();
            btn.remove_css_class("metis-bar-dropdown-active");
            icon.set_from_icon_name(Some("pan-up-symbolic"));
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
        prepare();
        glib::idle_add_local_once(move || popover.popup());
    });
}
