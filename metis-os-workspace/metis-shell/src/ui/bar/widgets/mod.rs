pub mod clock;
mod launcher;
mod menu;
mod notifications;
pub mod sys;
mod tasks;
mod weather;
pub mod workspaces;

use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;

use crate::config::{BarConfig, BarWidgetId};
use crate::services::BarSnapshot;

use clock::ClockWidget;
use launcher::LauncherWidget;
use notifications::NotificationsWidget;
use sys::{BatteryWidget, NetworkWidget, VolumeWidget};
use tasks::TasksWidget;
use weather::WeatherWidget;
use workspaces::WorkspacesWidget;

pub struct WidgetRefs {
    workspaces: RefCell<Option<WorkspacesWidget>>,
    tasks: RefCell<Option<TasksWidget>>,
    clock: RefCell<Option<ClockWidget>>,
    battery: RefCell<Option<BatteryWidget>>,
    network: RefCell<Option<NetworkWidget>>,
    volume: RefCell<Option<VolumeWidget>>,
    notifications: RefCell<Option<NotificationsWidget>>,
    weather: RefCell<Option<WeatherWidget>>,
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
            w.update(&snapshot.ethernet, &snapshot.wifi, snapshot.wifi_enabled);
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

    /// Repaint the taskbar from the latest window store snapshot. The tasks
    /// widget also self-refreshes via the window store's `register_refresh` hook;
    /// this fan-out is the explicit driver for callers that hold the snapshot.
    pub fn apply_tasks(&self, snapshot: &crate::services::windows::WindowsSnapshot) {
        if let Some(w) = self.tasks.borrow().as_ref() {
            w.update(snapshot);
        }
    }

    /// Weather arrives on its own (slow) channel, separate from the poll snapshot.
    pub fn apply_weather(&self, snapshot: &crate::services::WeatherSnapshot) {
        if let Some(w) = self.weather.borrow().as_ref() {
            w.update(snapshot);
        }
    }
}

pub fn build(root: &gtk::Box, config: Rc<RefCell<BarConfig>>) -> WidgetRefs {
    let refs = WidgetRefs {
        workspaces: RefCell::new(None),
        tasks: RefCell::new(None),
        clock: RefCell::new(None),
        battery: RefCell::new(None),
        network: RefCell::new(None),
        volume: RefCell::new(None),
        notifications: RefCell::new(None),
        weather: RefCell::new(None),
    };

    let cfg = config.borrow().clone();
    let bar_orientation = match cfg.position {
        crate::config::BarPosition::Top => gtk::Orientation::Horizontal,
        crate::config::BarPosition::Left | crate::config::BarPosition::Right => {
            gtk::Orientation::Vertical
        }
    };

    // Pinned brand/launcher icon, always first so it sits at the far-leading edge
    // of the bar (left for a top bar, top for a vertical bar).
    let launcher = LauncherWidget::new();
    append_bar_widget(root, launcher.root(), bar_orientation);

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
            BarWidgetId::Tasks => {
                let w = TasksWidget::new();
                append_bar_widget(root, w.root(), bar_orientation);
                *refs.tasks.borrow_mut() = Some(w);
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
            BarWidgetId::Weather => {
                let w = WeatherWidget::new();
                append_bar_widget(root, w.root(), bar_orientation);
                *refs.weather.borrow_mut() = Some(w);
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
