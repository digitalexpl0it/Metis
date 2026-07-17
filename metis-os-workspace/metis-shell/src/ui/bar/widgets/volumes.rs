//! Edge-bar removable volumes: USB / SD / optical / ISO icons next to the tray.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::gdk;
use gtk::prelude::*;

use crate::services::{self, VolumeEntry, VolumeKind};

pub struct VolumesWidget {
    root: gtk::Box,
}

impl VolumesWidget {
    pub fn new() -> Self {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        root.add_css_class("metis-bar-volumes");
        root.set_visible(false);

        let widget = Rc::new(RefCell::new(VolumesWidgetInner {
            root: root.clone(),
        }));

        let refresh = {
            let widget = widget.clone();
            Rc::new(move || {
                widget.borrow().rebuild();
            }) as Rc<dyn Fn()>
        };
        services::register_volumes_refresh(refresh.clone());
        refresh();

        Self { root }
    }

    pub fn root(&self) -> &gtk::Box {
        &self.root
    }
}

struct VolumesWidgetInner {
    root: gtk::Box,
}

impl VolumesWidgetInner {
    fn rebuild(&self) {
        while let Some(child) = self.root.first_child() {
            self.root.remove(&child);
        }
        let entries = services::volumes_snapshot();
        self.root.set_visible(!entries.is_empty());
        for entry in entries {
            self.root.append(&build_volume_button(&entry));
        }
    }
}

fn build_volume_button(entry: &VolumeEntry) -> gtk::Button {
    let btn = gtk::Button::builder().has_frame(false).build();
    btn.add_css_class("metis-bar-widget");
    btn.add_css_class("metis-bar-sys-icon");
    btn.add_css_class("metis-bar-volume-item");
    btn.set_tooltip_text(Some(&entry.tooltip));

    let icon = gtk::Image::from_icon_name(services::volumes_icon_name(entry.kind));
    icon.set_pixel_size(18);
    if entry.kind == VolumeKind::Locked {
        if let Some(display) = gdk::Display::default() {
            let theme = gtk::IconTheme::for_display(&display);
            if !theme.has_icon("drive-harddisk-encrypted-symbolic") {
                icon.set_icon_name(Some("changes-prevent-symbolic"));
            }
        }
    }
    btn.set_child(Some(&icon));

    let id = entry.id.clone();
    btn.connect_clicked(move |_| {
        services::volumes_activate(&id);
    });

    let entry = entry.clone();
    let btn_for_menu = btn.clone();
    let gesture = gtk::GestureClick::new();
    gesture.set_button(gdk::BUTTON_SECONDARY);
    gesture.connect_released(move |g, _, _x, _y| {
        g.set_state(gtk::EventSequenceState::Claimed);
        show_context_menu(&btn_for_menu, &entry);
    });
    btn.add_controller(gesture);

    btn
}

fn show_context_menu(anchor: &gtk::Button, entry: &VolumeEntry) {
    let popover = gtk::Popover::new();
    popover.set_parent(anchor);
    popover.set_has_arrow(true);
    popover.add_css_class("metis-bar-volumes-menu");

    let box_ = gtk::Box::new(gtk::Orientation::Vertical, 2);
    box_.set_margin_top(6);
    box_.set_margin_bottom(6);
    box_.set_margin_start(6);
    box_.set_margin_end(6);

    let title = gtk::Label::new(Some(&entry.label));
    title.set_xalign(0.0);
    title.add_css_class("metis-bar-section-title");
    title.set_margin_bottom(4);
    box_.append(&title);

    if entry.mount_path.is_some() {
        let open = menu_btn("Open");
        let id = entry.id.clone();
        let pop = popover.clone();
        open.connect_clicked(move |_| {
            pop.popdown();
            if let Some(path) = services::volumes_snapshot()
                .into_iter()
                .find(|e| e.id == id)
                .and_then(|e| e.mount_path)
            {
                services::open_in_file_manager(&path);
            }
        });
        box_.append(&open);
    }

    if entry.needs_mount {
        let label = if entry.is_encrypted_locked {
            "Unlock…"
        } else {
            "Mount"
        };
        let mount = menu_btn(label);
        let id = entry.id.clone();
        let pop = popover.clone();
        mount.connect_clicked(move |_| {
            pop.popdown();
            services::volumes_mount(&id);
        });
        box_.append(&mount);
    }

    if entry.can_unmount {
        let unmount = menu_btn("Unmount");
        let id = entry.id.clone();
        let pop = popover.clone();
        unmount.connect_clicked(move |_| {
            pop.popdown();
            services::volumes_unmount(&id);
        });
        box_.append(&unmount);
    }

    if entry.can_eject {
        let eject = menu_btn("Eject");
        let id = entry.id.clone();
        let pop = popover.clone();
        eject.connect_clicked(move |_| {
            pop.popdown();
            services::volumes_eject(&id);
        });
        box_.append(&eject);
    }

    popover.set_child(Some(&box_));
    popover.popup();
}

fn menu_btn(label: &str) -> gtk::Button {
    let btn = gtk::Button::with_label(label);
    btn.add_css_class("flat");
    btn.add_css_class("metis-bar-volumes-menu-item");
    btn.set_halign(gtk::Align::Fill);
    btn
}
