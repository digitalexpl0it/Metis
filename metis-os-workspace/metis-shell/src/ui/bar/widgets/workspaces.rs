use gtk::prelude::*;

use crate::config::BarPosition;
use crate::services::{dispatch_workspace, WorkspaceSnapshot};

const DOT_PX: i32 = 7;

pub struct WorkspacesWidget {
    root: gtk::Box,
    buttons: gtk::Box,
}

impl WorkspacesWidget {
    pub fn new(position: BarPosition) -> Self {
        let axis = match position {
            BarPosition::Top | BarPosition::Bottom => gtk::Orientation::Horizontal,
            BarPosition::Left | BarPosition::Right => gtk::Orientation::Vertical,
        };

        let root = gtk::Box::builder()
            .orientation(axis)
            .spacing(6)
            .build();
        root.add_css_class("metis-bar-widget");
        root.add_css_class("metis-bar-workspaces");
        root.set_vexpand(false);
        root.set_hexpand(false);
        root.set_valign(gtk::Align::Center);
        root.set_halign(gtk::Align::Center);

        let buttons = gtk::Box::builder()
            .orientation(axis)
            .spacing(6)
            .homogeneous(false)
            .build();
        buttons.set_valign(gtk::Align::Center);
        buttons.set_halign(gtk::Align::Center);
        root.append(&buttons);

        Self { root, buttons }
    }

    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    pub fn update(&self, snapshot: &WorkspaceSnapshot) {
        while let Some(child) = self.buttons.first_child() {
            self.buttons.remove(&child);
        }

        let dots = if snapshot.workspaces.is_empty() {
            (0..4).map(|_| (0u32, false)).collect::<Vec<_>>()
        } else {
            snapshot
                .workspaces
                .iter()
                .map(|ws| (ws.id, ws.id == snapshot.active_id))
                .collect()
        };

        for (id, active) in dots {
            let dot = workspace_dot(active);
            if id == 0 {
                dot.add_css_class("metis-bar-ws-dot-idle");
                dot.set_tooltip_text(Some("Metis desktop"));
            } else {
                dot.set_tooltip_text(Some(&format!("Desktop {id}")));
                let gesture = gtk::GestureClick::new();
                gesture.connect_pressed(move |_, _, _, _| {
                    dispatch_workspace(id);
                });
                dot.add_controller(gesture);
            }
            self.buttons.append(&dot);
        }
    }
}

fn workspace_dot(active: bool) -> gtk::Box {
    let dot = gtk::Box::builder().build();
    dot.add_css_class("metis-bar-ws-dot");
    if active {
        dot.add_css_class("metis-bar-ws-dot-active");
    }
    dot.set_size_request(DOT_PX, DOT_PX);
    dot.set_halign(gtk::Align::Center);
    dot.set_valign(gtk::Align::Center);
    dot.set_hexpand(false);
    dot.set_vexpand(false);
    dot
}
