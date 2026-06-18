pub mod clock;
mod notifications;
pub mod sys;
pub mod workspaces;

use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;

use crate::config::{BarConfig, BarWidgetId};
use crate::services::BarSnapshot;

use clock::ClockWidget;
use notifications::NotificationsWidget;
use sys::{BatteryWidget, NetworkWidget, VolumeWidget};
use workspaces::WorkspacesWidget;

pub struct WidgetRefs {
    workspaces: RefCell<Option<WorkspacesWidget>>,
    clock: RefCell<Option<ClockWidget>>,
    battery: RefCell<Option<BatteryWidget>>,
    network: RefCell<Option<NetworkWidget>>,
    volume: RefCell<Option<VolumeWidget>>,
    notifications: RefCell<Option<NotificationsWidget>>,
}

impl WidgetRefs {
    pub fn apply_snapshot(&self, snapshot: &BarSnapshot) {
        if let Some(w) = self.workspaces.borrow().as_ref() {
            w.update(&snapshot.workspaces);
        }
        if let Some(w) = self.battery.borrow().as_ref() {
            w.update(snapshot.battery_percent, snapshot.battery_charging);
        }
        if let Some(w) = self.network.borrow().as_ref() {
            w.update(&snapshot.network_label, snapshot.network_connected);
        }
        if let Some(w) = self.volume.borrow().as_ref() {
            w.update(
                snapshot.volume_percent,
                snapshot.volume_muted,
                snapshot.mic_percent,
                snapshot.mic_muted,
            );
        }
        if let Some(w) = self.notifications.borrow().as_ref() {
            w.update(&snapshot.notifications);
        }
    }
}

pub fn build(root: &gtk::Box, config: Rc<RefCell<BarConfig>>) -> WidgetRefs {
    let refs = WidgetRefs {
        workspaces: RefCell::new(None),
        clock: RefCell::new(None),
        battery: RefCell::new(None),
        network: RefCell::new(None),
        volume: RefCell::new(None),
        notifications: RefCell::new(None),
    };

    let cfg = config.borrow().clone();
    let bar_orientation = match cfg.position {
        crate::config::BarPosition::Top => gtk::Orientation::Horizontal,
        crate::config::BarPosition::Left | crate::config::BarPosition::Right => {
            gtk::Orientation::Vertical
        }
    };

    for widget in &cfg.widgets {
        match widget {
            BarWidgetId::Spacer => {
                let spacer = gtk::Box::new(bar_orientation, 0);
                spacer.set_hexpand(true);
                spacer.set_vexpand(true);
                spacer.add_css_class("metis-bar-spacer");
                root.append(&spacer);
            }
            BarWidgetId::Workspaces => {
                let w = WorkspacesWidget::new(cfg.position);
                append_bar_widget(root, w.root(), bar_orientation);
                w.update(&crate::services::workspace_snapshot());
                *refs.workspaces.borrow_mut() = Some(w);
            }
            BarWidgetId::Clock => {
                let w = ClockWidget::new(&cfg.clock);
                append_bar_widget(root, w.root(), bar_orientation);
                *refs.clock.borrow_mut() = Some(w);
            }
            BarWidgetId::Battery => {
                let w = BatteryWidget::new();
                append_bar_widget(root, w.root(), bar_orientation);
                *refs.battery.borrow_mut() = Some(w);
            }
            BarWidgetId::Network => {
                let w = NetworkWidget::new();
                append_bar_widget(root, w.root(), bar_orientation);
                *refs.network.borrow_mut() = Some(w);
            }
            BarWidgetId::Volume => {
                let w = VolumeWidget::new();
                append_bar_widget(root, w.root(), bar_orientation);
                *refs.volume.borrow_mut() = Some(w);
            }
            BarWidgetId::Notifications => {
                let w = NotificationsWidget::new();
                append_bar_widget(root, w.root(), bar_orientation);
                *refs.notifications.borrow_mut() = Some(w);
            }
        }
    }

    refs
}

fn append_bar_widget(
    root: &gtk::Box,
    widget: &impl IsA<gtk::Widget>,
    orientation: gtk::Orientation,
) {
    let w = widget.upcast_ref::<gtk::Widget>();
    // Fill the bar's cross axis so the hover gradient + underline reach the bar's
    // edge instead of floating around the icon. Only stretch the cross axis.
    if orientation == gtk::Orientation::Horizontal {
        w.set_valign(gtk::Align::Fill);
        w.set_vexpand(true);
    } else {
        w.set_halign(gtk::Align::Fill);
        w.set_hexpand(true);
    }
    root.append(w);
}
