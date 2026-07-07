//! Pull-down / pull-aside system dashboard (Phase 10).
//!
//! The control center is embedded in the bar's layer window (below the pill) so
//! there is no gap between the edge bar and the panel.

mod charts;
mod views;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::mpsc::Receiver;

use gtk::prelude::*;
use gtk4_layer_shell::LayerShell;
use metis_config::{load_bar_config, load_dashboard_config, BarPosition};

use crate::services::{
    format_bytes, format_rate, format_uptime, kill_process, short_kernel_version,
    DashboardSnapshot, GpuTempReading, ProcessClass, ProcessRow,
};
use crate::ui::bar::{resize_bar_for_dashboard, BarShell};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum ProcessClassFilter {
    #[default]
    All,
    UserApps,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum ProcessSortColumn {
    #[default]
    Cpu,
    Name,
    Pid,
    User,
    Kind,
    Memory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum SortDirection {
    #[default]
    Desc,
    Asc,
}

const SNAP_MS: u32 = 220;
const OPEN_THRESHOLD: f64 = 72.0;
const PULL_START_SLOP: f64 = 6.0;

thread_local! {
    static DASHBOARD: RefCell<Option<Rc<Dashboard>>> = const { RefCell::new(None) };
    static POLL_ATTACHED: Cell<bool> = const { Cell::new(false) };
}

struct Dashboard {
    shell: BarShell,
    root: gtk::Box,
    header: gtk::Box,
    tab_switcher: gtk::StackSwitcher,
    stack: gtk::Stack,
    overview: views::OverviewPage,
    processes: views::ProcessPage,
    cpu_hist: Rc<RefCell<Vec<f32>>>,
    cpu_core_hist: Rc<RefCell<Vec<Vec<f32>>>>,
    mem_hist: Rc<RefCell<Vec<f32>>>,
    swap_hist: Rc<RefCell<Vec<f32>>>,
    has_swap: Rc<Cell<bool>>,
    rx_hist: Rc<RefCell<Vec<f64>>>,
    tx_hist: Rc<RefCell<Vec<f64>>>,
    disk_read_hist: Rc<RefCell<Vec<f64>>>,
    disk_write_hist: Rc<RefCell<Vec<f64>>>,
    gpu_gauges: RefCell<Vec<views::TempGaugeCard>>,
    open: Cell<bool>,
    pulling: Cell<bool>,
    animating: Cell<bool>,
    current_extent: Cell<i32>,
    max_extent: Cell<i32>,
    snapshot: RefCell<DashboardSnapshot>,
    text_filter: RefCell<String>,
    class_filter: RefCell<ProcessClassFilter>,
    sort_column: RefCell<ProcessSortColumn>,
    sort_direction: RefCell<SortDirection>,
}

pub fn init() {
    if let Err(err) = metis_config::save_default_dashboard_config() {
        tracing::warn!(%err, "failed to write default dashboard.json");
    }
}

/// Press on the bar pill and drag toward the desktop to pull the dashboard open.
pub fn wire_bar_pull(pill: &gtk::Box, shell: &BarShell) {
    if !load_dashboard_config().enabled {
        return;
    }

    let pill_weak = pill.downgrade();
    let _shell_pull = shell.clone();

    let drag = gtk::GestureDrag::new();
    drag.set_button(0);
    drag.set_touch_only(false);

    drag.connect_drag_begin(move |gesture, start_x, start_y| {
        let Some(pill) = pill_weak.upgrade() else {
            return;
        };
        if press_on_bar_widget(&pill, start_x, start_y) {
            gesture.set_state(gtk::EventSequenceState::Denied);
        }
    });

    let shell_update = shell.clone();
    drag.connect_drag_update(move |gesture, offset_x, offset_y| {
        let position = load_bar_config().position;
        let delta = pull_delta(position, offset_x, offset_y);
        if delta < PULL_START_SLOP {
            return;
        }
        gesture.set_state(gtk::EventSequenceState::Claimed);
        let dash = ensure_dashboard(&shell_update);
        if dash.open.get() && !dash.pulling.get() {
            return;
        }
        if !dash.pulling.get() {
            dropdown::request_close_all();
            dash.pulling.set(true);
            dash.max_extent.set(compute_max_extent(position, dash.shell.window.monitor().as_ref()));
        }
        let max = dash.max_extent.get().max(1);
        let extent = (delta as i32).clamp(0, max);
        dash.set_pull_preview(extent);
    });

    let shell_end = shell.clone();
    drag.connect_drag_end(move |_, offset_x, offset_y| {
        let position = load_bar_config().position;
        let delta = pull_delta(position, offset_x, offset_y);
        let Some(dash) = DASHBOARD.with(|d| d.borrow().clone()) else {
            return;
        };
        dash.pulling.set(false);
        if dash.open.get() {
            return;
        }
        let monitor = shell_end.window.monitor();
        let max = compute_max_extent(position, monitor.as_ref());
        dash.max_extent.set(max);
        if delta >= OPEN_THRESHOLD {
            dash.snap_open();
        } else {
            dash.snap_closed();
        }
    });

    pill.add_controller(drag);
}

pub fn on_theme_changed() {
    DASHBOARD.with(|d| {
        if let Some(dash) = d.borrow().as_ref() {
            dash.header.queue_draw();
            dash.tab_switcher.queue_draw();
            dash.redraw_for_theme();
        }
    });
}

pub fn on_bar_config_changed() {
    DASHBOARD.with(|d| {
        if let Some(dash) = d.borrow().as_ref() {
            dash.relayout_for_bar();
            if !dash.open.get() && !dash.pulling.get() {
                dash.set_closed_state();
            } else if dash.open.get() {
                dash.max_extent.set(compute_max_extent(
                    load_bar_config().position,
                    dash.shell.window.monitor().as_ref(),
                ));
                dash.apply_extent(dash.max_extent.get());
                dash.relayout_for_size(dash.shell.host.width(), dash.current_extent.get());
            }
        }
    });
}

pub fn attach_poll_channel(rx: Receiver<DashboardSnapshot>) {
    glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
        while let Ok(snapshot) = rx.try_recv() {
            apply_snapshot(&snapshot);
        }
        glib::ControlFlow::Continue
    });
}

pub fn request_close() {
    DASHBOARD.with(|d| {
        if let Some(dash) = d.borrow().as_ref() {
            dash.snap_closed();
        }
    });
}

/// Open or close the control center with the same slide animation as a bar pull.
pub fn request_toggle(shell: &BarShell) {
    if !load_dashboard_config().enabled {
        return;
    }
    let dash = ensure_dashboard(shell);
    if dash.open.get() || dash.current_extent.get() > OPEN_THRESHOLD as i32 / 2 {
        dash.snap_closed();
    } else {
        dash.snap_open();
    }
}

fn ensure_dashboard(shell: &BarShell) -> Rc<Dashboard> {
    DASHBOARD.with(|d| {
        if let Some(dash) = d.borrow().as_ref() {
            if dash.shell.window != shell.window {
                teardown_dashboard();
            } else {
                return dash.clone();
            }
        }
        tracing::debug!("lazy-starting system dashboard");
        let dash = Rc::new(build_dashboard(shell));
        dash.set_closed_state();
        *d.borrow_mut() = Some(dash.clone());
        start_polling();
        dash
    })
}

fn start_polling() {
    if POLL_ATTACHED.get() {
        crate::services::set_polling_active(true);
        return;
    }
    POLL_ATTACHED.set(true);
    attach_poll_channel(crate::services::spawn_dashboard_pollers());
}

fn teardown_dashboard() {
    crate::services::set_polling_active(false);
    DASHBOARD.with(|d| {
        if let Some(dash) = d.borrow().as_ref() {
            dash.set_closed_state();
            while let Some(child) = dash.shell.host.first_child() {
                dash.shell.host.remove(&child);
            }
        }
        *d.borrow_mut() = None;
    });
    tracing::debug!("system dashboard torn down (idle)");
}

fn apply_snapshot(snapshot: &DashboardSnapshot) {
    DASHBOARD.with(|d| {
        if let Some(dash) = d.borrow().as_ref() {
            dash.update(snapshot);
        }
    });
}

fn press_on_bar_widget(pill: &gtk::Box, x: f64, y: f64) -> bool {
    let Some(target) = pill.pick(x, y, gtk::PickFlags::DEFAULT) else {
        return false;
    };
    let mut node = Some(target);
    while let Some(w) = node {
        if w.has_css_class("metis-bar-widget") {
            return true;
        }
        node = w.parent();
    }
    false
}

fn pull_delta(position: BarPosition, offset_x: f64, offset_y: f64) -> f64 {
    match position {
        BarPosition::Top => offset_y,
        BarPosition::Bottom => -offset_y,
        BarPosition::Left => offset_x,
        BarPosition::Right => -offset_x,
    }
}

fn close_delta(position: BarPosition, offset_x: f64, offset_y: f64) -> f64 {
    match position {
        BarPosition::Top => -offset_y,
        BarPosition::Bottom => offset_y,
        BarPosition::Left => -offset_x,
        BarPosition::Right => offset_x,
    }
}

fn build_dashboard(shell: &BarShell) -> Dashboard {
    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .build();
    root.add_css_class("metis-dashboard-root");
    root.set_overflow(gtk::Overflow::Hidden);
    root.set_hexpand(true);
    root.set_vexpand(true);

    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    header.add_css_class("metis-dashboard-header");

    let title = gtk::Label::new(Some("Control Center"));
    title.add_css_class("metis-dashboard-title");
    title.set_halign(gtk::Align::Start);
    title.set_hexpand(false);

    let stack = gtk::Stack::new();
    stack.add_css_class("metis-dashboard-stack");
    let switcher = gtk::StackSwitcher::new();
    switcher.set_stack(Some(&stack));
    switcher.add_css_class("metis-dash-tabs");
    switcher.set_halign(gtk::Align::Start);
    switcher.set_hexpand(true);

    header.append(&title);
    header.append(&switcher);

    let close_btn = gtk::Button::from_icon_name("window-close-symbolic");
    close_btn.add_css_class("metis-dashboard-close");
    close_btn.connect_clicked(|_| request_close());
    header.append(&close_btn);

    let cpu_hist = Rc::new(RefCell::new(Vec::new()));
    let cpu_core_hist = Rc::new(RefCell::new(Vec::new()));
    let mem_hist = Rc::new(RefCell::new(Vec::new()));
    let swap_hist = Rc::new(RefCell::new(Vec::new()));
    let has_swap = Rc::new(Cell::new(false));
    let rx_hist = Rc::new(RefCell::new(Vec::new()));
    let tx_hist = Rc::new(RefCell::new(Vec::new()));
    let disk_read_hist = Rc::new(RefCell::new(Vec::new()));
    let disk_write_hist = Rc::new(RefCell::new(Vec::new()));

    let overview = views::build_overview();
    charts::wire_multi_core_chart(&overview.cpu_chart, cpu_core_hist.clone(), cpu_hist.clone());
    charts::wire_memory_chart(
        &overview.mem_chart,
        mem_hist.clone(),
        swap_hist.clone(),
        has_swap.clone(),
    );
    charts::wire_dual_rate_chart(
        &overview.net_chart,
        rx_hist.clone(),
        tx_hist.clone(),
    );
    charts::wire_dual_rate_chart(
        &overview.disk_io_chart,
        disk_read_hist.clone(),
        disk_write_hist.clone(),
    );

    let overview_scroll = gtk::ScrolledWindow::new();
    overview_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    overview_scroll.set_vexpand(true);
    overview_scroll.set_hexpand(true);
    overview_scroll.set_child(Some(&overview.widget));
    overview_scroll.add_css_class("metis-dashboard-scroll");

    let processes = views::build_processes();

    stack.add_titled(&overview_scroll, Some("overview"), "Overview");
    stack.add_titled(&processes.widget, Some("processes"), "Processes");
    stack.set_visible_child_name("overview");
    stack.set_vexpand(true);
    stack.set_hexpand(true);

    let dash = Dashboard {
        shell: shell.clone(),
        root,
        header,
        tab_switcher: switcher,
        stack,
        overview,
        processes,
        cpu_hist,
        cpu_core_hist,
        mem_hist,
        swap_hist,
        has_swap,
        rx_hist,
        tx_hist,
        disk_read_hist,
        disk_write_hist,
        gpu_gauges: RefCell::new(Vec::new()),
        open: Cell::new(false),
        pulling: Cell::new(false),
        animating: Cell::new(false),
        current_extent: Cell::new(0),
        max_extent: Cell::new(480),
        snapshot: RefCell::new(DashboardSnapshot::default()),
        text_filter: RefCell::new(String::new()),
        class_filter: RefCell::new(ProcessClassFilter::All),
        sort_column: RefCell::new(ProcessSortColumn::Cpu),
        sort_direction: RefCell::new(SortDirection::Desc),
    };

    dash.relayout_for_bar();
    dash.shell.host.append(&dash.root);

    let key = gtk::EventControllerKey::new();
    key.connect_key_pressed(|_, key, _, _| {
        if key == gtk::gdk::Key::Escape {
            request_close();
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    dash.root.add_controller(key);

    let root_alloc = dash.root.clone();
    let slf_weak = dash.root.downgrade();
    root_alloc.connect_map(move |_| {
        let Some(root) = slf_weak.upgrade() else {
            return;
        };
        DASHBOARD.with(|d| {
            if let Some(dash) = d.borrow().as_ref() {
                if dash.root == root && dash.current_extent.get() > 0 {
                    dash.relayout_for_size(dash.shell.host.width(), dash.current_extent.get());
                }
            }
        });
    });

    let filter_entry = dash.processes.search.clone();
    let list = dash.processes.list.clone();
    filter_entry.connect_search_changed(move |entry| {
        let text = entry.text().to_string();
        DASHBOARD.with(|d| {
            if let Some(dash) = d.borrow().as_ref() {
                *dash.text_filter.borrow_mut() = text;
                dash.rebuild_process_list(&list);
            }
        });
    });

    let filter_dd = dash.processes.filter.clone();
    let list = dash.processes.list.clone();
    filter_dd.connect_selected_notify(move |dd| {
        let filter = match dd.selected() {
            1 => ProcessClassFilter::UserApps,
            2 => ProcessClassFilter::System,
            _ => ProcessClassFilter::All,
        };
        DASHBOARD.with(|d| {
            if let Some(dash) = d.borrow().as_ref() {
                *dash.class_filter.borrow_mut() = filter;
                dash.rebuild_process_list(&list);
            }
        });
    });

    wire_process_sort(&dash.processes.headers, &dash.processes.list);

    let header_drag = gtk::GestureDrag::new();
    header_drag.set_button(0);
    header_drag.connect_drag_update(|gesture, offset_x, offset_y| {
        let position = load_bar_config().position;
        if close_delta(position, offset_x, offset_y) > OPEN_THRESHOLD {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            request_close();
        }
    });
    dash.header.add_controller(header_drag);

    dash.refresh_sort_headers();
    dash
}

impl Dashboard {
    fn relayout_for_bar(&self) {
        let position = load_bar_config().position;
        self.max_extent.set(compute_max_extent(
            position,
            self.shell.window.monitor().as_ref(),
        ));
        while let Some(child) = self.root.first_child() {
            self.root.remove(&child);
        }
        self.root.remove_css_class("metis-dashboard-root-bottom");
        match position {
            BarPosition::Bottom => {
                self.root.add_css_class("metis-dashboard-root-bottom");
                self.root.append(&self.stack);
                self.root.append(&self.header);
            }
            _ => {
                self.root.append(&self.header);
                self.root.append(&self.stack);
            }
        }
        self.apply_extent(self.current_extent.get());
    }

    fn set_closed_state(&self) {
        self.open.set(false);
        self.pulling.set(false);
        self.current_extent.set(0);
        self.root.set_opacity(0.0);
        self.root.set_sensitive(false);
        self.apply_extent(0);
    }

    fn set_pull_preview(&self, extent: i32) {
        let max = self.max_extent.get().max(1);
        let e = extent.clamp(0, max);
        self.current_extent.set(e);
        let fade = (e as f64 / 96.0).clamp(0.0, 1.0);
        self.root.set_opacity(fade);
        self.root.set_sensitive(false);
        self.apply_extent(e);
    }

    fn snap_open(&self) {
        if self.open.get() && self.animating.get() {
            return;
        }
        if self.open.get() && self.current_extent.get() >= self.max_extent.get() {
            return;
        }
        dropdown::request_close_all();
        self.pulling.set(false);
        self.max_extent.set(compute_max_extent(
            load_bar_config().position,
            self.shell.window.monitor().as_ref(),
        ));
        if self.current_extent.get() == 0 {
            self.root.set_opacity(0.0);
            self.root.set_sensitive(false);
        }
        self.open.set(false);
        self.animate_to(self.max_extent.get());
    }

    fn snap_closed(&self) {
        if !self.open.get() && self.current_extent.get() == 0 {
            self.set_closed_state();
            teardown_dashboard();
            return;
        }
        self.open.set(false);
        self.animate_to(0);
    }

    fn redraw_for_theme(&self) {
        self.overview.cpu_chart.queue_draw();
        self.overview.mem_chart.queue_draw();
        self.overview.net_chart.queue_draw();
        self.overview.disk_io_chart.queue_draw();
        self.overview.cpu_temp.gauge.queue_draw();
        for gauge in self.gpu_gauges.borrow().iter() {
            gauge.gauge.queue_draw();
        }
        let snapshot = self.snapshot.borrow();
        while let Some(child) = self.overview.cpu_legend.first_child() {
            self.overview.cpu_legend.remove(&child);
        }
        let core_count = snapshot.cpu_per_core.len();
        let legend_cap = core_count.min(16);
        for i in 0..legend_cap {
            let label = if core_count <= 16 {
                format!("C{i}")
            } else if i == 15 {
                format!("+{}", core_count - 15)
            } else {
                format!("C{i}")
            };
            self.overview
                .cpu_legend
                .append(&views::legend_chip(i, &label));
        }
        if core_count > 0 {
            self.overview
                .cpu_legend
                .append(&views::aggregate_legend_chip("Σ total"));
        }
    }

    fn apply_extent(&self, extent: i32) {
        resize_bar_for_dashboard(&self.shell, extent);
        let width = self.shell.host.width();
        if width > 0 {
            self.relayout_for_size(width, extent);
        } else {
            let slf = DASHBOARD.with(|d| d.borrow().clone());
            glib::idle_add_local_once(move || {
                if let Some(dash) = slf {
                    if dash.current_extent.get() > 0 {
                        dash.relayout_for_size(dash.shell.host.width(), dash.current_extent.get());
                    }
                }
            });
        }
    }

    fn relayout_for_size(&self, width: i32, height: i32) {
        if width <= 0 || height <= 0 {
            return;
        }

        let (_, nat_h, _, _) = self.header.measure(gtk::Orientation::Vertical, -1);
        let header_h = nat_h;
        let content_h = (height - header_h).max(120);

        let session_w = ((width as f64 * 0.32).round() as i32).clamp(220, 320);
        self.overview.session_card.set_size_request(session_w, -1);

        let cpu_h = ((content_h as f64 * 0.36).round() as i32).clamp(96, 200);
        let row_h = ((content_h as f64 * 0.22).round() as i32).clamp(72, 120);
        self.overview.cpu_chart.set_content_height(cpu_h);
        self.overview.mem_chart.set_content_height(cpu_h);
        self.overview.net_chart.set_content_height(row_h);
        self.overview.disk_io_chart.set_content_height(row_h);
    }

    fn animate_to(&self, target: i32) {
        self.animating.set(true);
        let start = self.current_extent.get();
        let delta = target - start;
        let start_at = glib::monotonic_time();
        let duration_us = (SNAP_MS as i64) * 1000;
        let slf = DASHBOARD.with(|d| d.borrow().clone()).unwrap();

        glib::timeout_add_local(std::time::Duration::from_millis(16), move || {
            let elapsed = glib::monotonic_time() - start_at;
            let t = (elapsed as f64 / duration_us as f64).clamp(0.0, 1.0);
            let eased = 1.0 - (1.0 - t).powi(3);
            let e = start + (delta as f64 * eased) as i32;
            slf.current_extent.set(e);
            if e > 0 {
                let max_extent = slf.max_extent.get().max(1) as f64;
                let fade = (e as f64 / max_extent).clamp(0.0, 1.0);
                slf.root.set_opacity(if t >= 1.0 && target > 0 {
                    1.0
                } else {
                    fade.max(0.05)
                });
            }
            slf.apply_extent(e);
            if t >= 1.0 {
                slf.animating.set(false);
                if target == 0 {
                    slf.set_closed_state();
                    teardown_dashboard();
                } else {
                    slf.open.set(true);
                    slf.root.set_sensitive(true);
                    slf.root.set_opacity(1.0);
                }
                return glib::ControlFlow::Break;
            }
            glib::ControlFlow::Continue
        });
    }

    fn update(&self, snapshot: &DashboardSnapshot) {
        *self.snapshot.borrow_mut() = snapshot.clone();
        *self.cpu_hist.borrow_mut() = snapshot.cpu_history.clone();
        *self.cpu_core_hist.borrow_mut() = snapshot.cpu_core_histories.clone();
        *self.mem_hist.borrow_mut() = snapshot.mem_percent_history.clone();
        *self.swap_hist.borrow_mut() = snapshot.swap_percent_history.clone();
        self.has_swap.set(snapshot.swap_total_bytes > 0);
        *self.rx_hist.borrow_mut() = snapshot.net_rx_history.clone();
        *self.tx_hist.borrow_mut() = snapshot.net_tx_history.clone();
        *self.disk_read_hist.borrow_mut() = snapshot.disk_read_history.clone();
        *self.disk_write_hist.borrow_mut() = snapshot.disk_write_history.clone();
        self.has_swap.set(snapshot.swap_total_bytes > 0);

        self.overview.cpu_value.set_text(&format!(
            "{:.0}% total · {} cores",
            snapshot.cpu_percent,
            snapshot.cpu_per_core.len().max(1)
        ));
        self.overview.cpu_chart.queue_draw();

        while let Some(child) = self.overview.cpu_legend.first_child() {
            self.overview.cpu_legend.remove(&child);
        }
        let core_count = snapshot.cpu_per_core.len();
        let legend_cap = core_count.min(16);
        for i in 0..legend_cap {
            let label = if core_count <= 16 {
                format!("C{i}")
            } else if i == 15 {
                format!("+{}", core_count - 15)
            } else {
                format!("C{i}")
            };
            self.overview
                .cpu_legend
                .append(&views::legend_chip(i, &label));
        }
        if core_count > 0 {
            self.overview
                .cpu_legend
                .append(&views::aggregate_legend_chip("Σ total"));
        }

        let mem_pct = pct(snapshot.memory_used_bytes, snapshot.memory_total_bytes);
        self.overview.mem_value.set_text(&format!(
            "{} / {} ({:.0}%)",
            format_bytes(snapshot.memory_used_bytes),
            format_bytes(snapshot.memory_total_bytes),
            mem_pct
        ));
        self.overview.mem_chart.queue_draw();
        self.overview.mem_legend.set_visible(snapshot.swap_total_bytes > 0);

        self.overview.load_label.set_text(&format!(
            "{:.2}  {:.2}  {:.2}",
            snapshot.load_avg[0], snapshot.load_avg[1], snapshot.load_avg[2]
        ));
        self.overview.uptime_label.set_text(&format_uptime(snapshot.uptime_secs));

        self.overview.eth_down.set_text(&format!(
            "Ethernet ↓ {}",
            format_rate(snapshot.ethernet_rx_bps)
        ));
        self.overview.eth_up.set_text(&format!(
            "Ethernet ↑ {}",
            format_rate(snapshot.ethernet_tx_bps)
        ));
        self.overview.wifi_down.set_text(&format!(
            "Wi‑Fi ↓ {}",
            format_rate(snapshot.wifi_rx_bps)
        ));
        self.overview.wifi_up.set_text(&format!(
            "Wi‑Fi ↑ {}",
            format_rate(snapshot.wifi_tx_bps)
        ));
        self.overview.net_chart.queue_draw();

        let fw = &snapshot.firewall;
        if fw.active {
            self.overview
                .firewall_status
                .set_text(&format!("Firewall active · {}", fw.backend));
        } else {
            self.overview
                .firewall_status
                .set_text(&format!("Firewall inactive · {}", fw.backend));
        }

        self.overview.disk_io_value.set_text(&format!(
            "↓ {}  ↑ {}",
            format_rate(snapshot.disk_read_bps),
            format_rate(snapshot.disk_write_bps)
        ));
        self.overview.disk_io_chart.queue_draw();

        while let Some(child) = self.overview.disk_box.first_child() {
            self.overview.disk_box.remove(&child);
        }
        for disk in &snapshot.disks {
            let disk_pct = pct(disk.used_bytes, disk.total_bytes);
            let tile = views::disk_mount_card(
                &disk.mount_point,
                disk_pct,
                &format_bytes(disk.used_bytes),
                &format_bytes(disk.total_bytes),
            );
            self.overview.disk_box.append(&tile);
        }

        let hw = &snapshot.hardware;
        self.overview.hostname.set_text(&hw.hostname);
        self.overview.cpu_model.set_text(&hw.cpu_model);
        self.overview.cpu_cores.set_text(&hw.cpu_cores.to_string());
        self.overview.system_memory.set_text(&format!(
            "{} total",
            format_bytes(snapshot.memory_total_bytes)
        ));
        let kernel_short = short_kernel_version(&hw.kernel);
        self.overview.kernel.set_text(&kernel_short);
        self.overview.kernel.set_tooltip_text(Some(&hw.kernel));
        self.overview.cpu_model.set_tooltip_text(Some(&hw.cpu_model));

        set_temp_label(
            &self.overview.cpu_temp.value,
            snapshot.cpu_temp_celsius,
        );
        *self.overview.cpu_temp.temp.borrow_mut() = snapshot.cpu_temp_celsius;
        self.overview.cpu_temp.gauge.queue_draw();
        self.sync_gpu_gauges(&snapshot.gpu_temps);

        self.rebuild_process_list(&self.processes.list);
        self.refresh_sort_headers();

        if self.open.get() {
            self.relayout_for_size(self.shell.host.width(), self.current_extent.get());
        }
    }

    fn sync_gpu_gauges(&self, readings: &[GpuTempReading]) {
        let desired = readings.len();
        let mut slots = self.gpu_gauges.borrow_mut();

        while slots.len() > desired {
            let slot = slots.pop().expect("slot count");
            self.overview.temp_gauges.remove(&slot.card);
        }

        while slots.len() < desired {
            let (card, gauge_card) = views::build_temp_gauge_card(
                "GPU",
                &[
                    "video-display-symbolic",
                    "display-brightness-symbolic",
                    "computer-symbolic",
                ],
            );
            self.overview.temp_gauges.append(&card);
            slots.push(gauge_card);
        }

        for (slot, reading) in slots.iter_mut().zip(readings.iter()) {
            slot.title.set_text(&reading.label);
            slot.value
                .set_text(&format!("{:.0}°C", reading.temp_celsius));
            *slot.temp.borrow_mut() = Some(reading.temp_celsius);
            slot.gauge.queue_draw();
        }
    }

    fn refresh_sort_headers(&self) {
        let col = *self.sort_column.borrow();
        let dir = *self.sort_direction.borrow();
        let h = &self.processes.headers;
        set_sort_label(&h.name, "Name", col == ProcessSortColumn::Name, dir);
        set_sort_label(&h.pid, "PID", col == ProcessSortColumn::Pid, dir);
        set_sort_label(&h.user, "User", col == ProcessSortColumn::User, dir);
        set_sort_label(&h.kind, "Type", col == ProcessSortColumn::Kind, dir);
        set_sort_label(&h.cpu, "CPU", col == ProcessSortColumn::Cpu, dir);
        set_sort_label(&h.memory, "Memory", col == ProcessSortColumn::Memory, dir);
    }

    fn rebuild_process_list(&self, list: &gtk::ListBox) {
        while let Some(child) = list.first_child() {
            list.remove(&child);
        }
        let text_filter = self.text_filter.borrow().to_lowercase();
        let class_filter = *self.class_filter.borrow();
        let sort_col = *self.sort_column.borrow();
        let sort_dir = *self.sort_direction.borrow();
        let processes: Vec<ProcessRow> = {
            let snapshot = self.snapshot.borrow();
            let mut rows: Vec<ProcessRow> = snapshot
                .processes
                .iter()
                .filter(|proc| {
                    if !text_filter.is_empty()
                        && !proc.name.to_lowercase().contains(&text_filter)
                        && !proc.user.to_lowercase().contains(&text_filter)
                        && !proc.pid.to_string().contains(&text_filter)
                    {
                        return false;
                    }
                    matches_class_filter(class_filter, proc.class)
                })
                .cloned()
                .collect();
            sort_process_rows(&mut rows, sort_col, sort_dir);
            rows
        };
        let mut shown = 0usize;
        for proc in &processes {
            list.append(&process_row(proc));
            shown += 1;
            if shown >= 300 {
                break;
            }
        }
        if shown == 0 {
            let empty = gtk::Label::new(Some("No matching processes"));
            empty.add_css_class("metis-dash-muted");
            empty.set_margin_top(12);
            empty.set_margin_start(16);
            empty.set_halign(gtk::Align::Start);
            let row = gtk::ListBoxRow::new();
            row.set_child(Some(&empty));
            list.append(&row);
        }
    }
}

fn set_sort_label(btn: &gtk::Button, title: &str, active: bool, dir: SortDirection) {
    let suffix = if active {
        match dir {
            SortDirection::Asc => " ↑",
            SortDirection::Desc => " ↓",
        }
    } else {
        ""
    };
    btn.set_label(&format!("{title}{suffix}"));
    btn.set_sensitive(true);
    if active {
        btn.add_css_class("metis-dash-sort-active");
    } else {
        btn.remove_css_class("metis-dash-sort-active");
    }
}

fn default_sort_direction(col: ProcessSortColumn) -> SortDirection {
    match col {
        ProcessSortColumn::Name | ProcessSortColumn::User | ProcessSortColumn::Kind => {
            SortDirection::Asc
        }
        ProcessSortColumn::Pid | ProcessSortColumn::Cpu | ProcessSortColumn::Memory => {
            SortDirection::Desc
        }
    }
}

fn sort_process_rows(rows: &mut [ProcessRow], col: ProcessSortColumn, dir: SortDirection) {
    rows.sort_by(|a, b| {
        let ord = match col {
            ProcessSortColumn::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            ProcessSortColumn::Pid => a.pid.cmp(&b.pid),
            ProcessSortColumn::User => a.user.to_lowercase().cmp(&b.user.to_lowercase()),
            ProcessSortColumn::Kind => class_order(a.class).cmp(&class_order(b.class)),
            ProcessSortColumn::Cpu => a
                .cpu_percent
                .partial_cmp(&b.cpu_percent)
                .unwrap_or(std::cmp::Ordering::Equal),
            ProcessSortColumn::Memory => a.memory_bytes.cmp(&b.memory_bytes),
        };
        match dir {
            SortDirection::Asc => ord,
            SortDirection::Desc => ord.reverse(),
        }
    });
}

fn class_order(class: ProcessClass) -> u8 {
    match class {
        ProcessClass::Metis => 0,
        ProcessClass::UserApp => 1,
        ProcessClass::System => 2,
    }
}

fn wire_process_sort(headers: &views::ProcessHeader, list: &gtk::ListBox) {
    wire_sort_btn(&headers.name, ProcessSortColumn::Name, list);
    wire_sort_btn(&headers.pid, ProcessSortColumn::Pid, list);
    wire_sort_btn(&headers.user, ProcessSortColumn::User, list);
    wire_sort_btn(&headers.kind, ProcessSortColumn::Kind, list);
    wire_sort_btn(&headers.cpu, ProcessSortColumn::Cpu, list);
    wire_sort_btn(&headers.memory, ProcessSortColumn::Memory, list);
}

fn wire_sort_btn(btn: &gtk::Button, col: ProcessSortColumn, list: &gtk::ListBox) {
    let list = list.clone();
    btn.connect_clicked(move |_| {
        DASHBOARD.with(|d| {
            let holder = d.borrow();
            let Some(dash) = holder.as_ref() else {
                return;
            };
            {
                let mut column = dash.sort_column.borrow_mut();
                let mut dir = dash.sort_direction.borrow_mut();
                if *column == col {
                    *dir = match *dir {
                        SortDirection::Asc => SortDirection::Desc,
                        SortDirection::Desc => SortDirection::Asc,
                    };
                } else {
                    *column = col;
                    *dir = default_sort_direction(col);
                }
            }
            dash.rebuild_process_list(&list);
            dash.refresh_sort_headers();
        });
    });
}

fn matches_class_filter(filter: ProcessClassFilter, class: ProcessClass) -> bool {
    match filter {
        ProcessClassFilter::All => true,
        ProcessClassFilter::UserApps => matches!(class, ProcessClass::UserApp | ProcessClass::Metis),
        ProcessClassFilter::System => class == ProcessClass::System,
    }
}

fn process_row(proc: &ProcessRow) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.add_css_class("metis-dash-table-row");
    let grid = gtk::Grid::builder()
        .column_spacing(8)
        .margin_top(5)
        .margin_bottom(5)
        .build();
    grid.add_css_class("metis-dash-proc-cols");

    let name = gtk::Label::new(Some(&proc.name));
    name.set_halign(gtk::Align::Start);
    name.set_xalign(0.0);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    match proc.class {
        ProcessClass::Metis => name.add_css_class("metis-dash-process-metis"),
        ProcessClass::System => name.add_css_class("metis-dash-muted"),
        ProcessClass::UserApp => {}
    }

    let pid = gtk::Label::new(Some(&proc.pid.to_string()));
    pid.add_css_class("metis-dash-muted");
    pid.set_halign(gtk::Align::Start);

    let user = gtk::Label::new(Some(&proc.user));
    user.add_css_class("metis-dash-muted");
    user.set_halign(gtk::Align::Start);
    user.set_ellipsize(gtk::pango::EllipsizeMode::End);

    let kind = gtk::Label::new(Some(class_label(proc.class)));
    kind.add_css_class("metis-dash-muted");
    kind.set_halign(gtk::Align::Start);

    let cpu = gtk::Label::new(Some(&format!("{:.1}%", proc.cpu_percent)));
    cpu.add_css_class("metis-dash-muted");
    cpu.set_halign(gtk::Align::End);
    cpu.set_xalign(1.0);

    let mem = gtk::Label::new(Some(&format_bytes(proc.memory_bytes)));
    mem.add_css_class("metis-dash-muted");
    mem.set_halign(gtk::Align::End);
    mem.set_xalign(1.0);

    name.set_hexpand(true);
    pid.set_width_request(64);
    user.set_width_request(88);
    kind.set_width_request(64);
    cpu.set_width_request(64);
    mem.set_width_request(80);
    grid.attach(&name, 0, 0, 1, 1);
    grid.attach(&pid, 1, 0, 1, 1);
    grid.attach(&user, 2, 0, 1, 1);
    grid.attach(&kind, 3, 0, 1, 1);
    grid.attach(&cpu, 4, 0, 1, 1);
    grid.attach(&mem, 5, 0, 1, 1);

    let kill = gtk::Button::from_icon_name("process-stop-symbolic");
    kill.add_css_class("flat");
    kill.set_sensitive(proc.killable);
    kill.set_tooltip_text(if proc.killable {
        Some("End task")
    } else {
        Some("Cannot end processes owned by another user")
    });
    let pid_val = proc.pid;
    kill.connect_clicked(move |btn| {
        let cfg = load_dashboard_config();
        let btn = btn.clone();
        if cfg.confirm_before_kill {
            let Some(win) = btn
                .root()
                .and_then(|r| r.downcast::<gtk::Window>().ok())
            else {
                tracing::warn!(pid = pid_val, "end task dialog: no parent window");
                return;
            };
            let dialog = gtk::MessageDialog::builder()
                .transient_for(&win)
                .modal(true)
                .message_type(gtk::MessageType::Warning)
                .text(format!("End process {pid_val}?"))
                .secondary_text("This sends SIGTERM to the process.")
                .build();
            dialog.add_button("Cancel", gtk::ResponseType::Cancel);
            dialog.add_button("End task", gtk::ResponseType::Accept);
            dialog.connect_response(move |d, resp| {
                if resp == gtk::ResponseType::Accept {
                    if let Err(err) = kill_process(pid_val, false) {
                        tracing::warn!(%err, pid = pid_val, "end task failed");
                    }
                }
                d.close();
            });
            dialog.present();
        } else if let Err(err) = kill_process(pid_val, false) {
            tracing::warn!(%err, pid = pid_val, "end task failed");
        }
    });

    grid.attach(&kill, 6, 0, 1, 1);
    row.set_child(Some(&grid));
    row
}

fn class_label(class: ProcessClass) -> &'static str {
    match class {
        ProcessClass::Metis => "Metis",
        ProcessClass::UserApp => "App",
        ProcessClass::System => "System",
    }
}

fn set_temp_label(label: &gtk::Label, temp: Option<f32>) {
    match temp {
        Some(c) => label.set_text(&format!("{c:.0}°C")),
        None => label.set_text("N/A"),
    }
}

fn pct(used: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        (used as f64 / total as f64) * 100.0
    }
}

fn compute_max_extent(position: BarPosition, monitor: Option<&gtk::gdk::Monitor>) -> i32 {
    let (mon_w, mon_h) = monitor_size(monitor);
    let cfg = load_bar_config();
    let dash_cfg = load_dashboard_config();
    let pct = (dash_cfg.max_height_percent.clamp(20, 100) as f64) / 100.0;
    let cross = cfg.height as i32;
    let edge = cfg.margin_top as i32;
    match position {
        BarPosition::Top | BarPosition::Bottom => {
            let available = (mon_h - edge - cross).max(0);
            ((available as f64) * pct).round() as i32
        }
        BarPosition::Left | BarPosition::Right => {
            let available = (mon_w - edge - cross).max(0);
            ((available as f64) * pct).round() as i32
        }
    }
    .max(320)
}

fn monitor_size(monitor: Option<&gtk::gdk::Monitor>) -> (i32, i32) {
    if let Some(monitor) = monitor {
        let g = monitor.geometry();
        if g.width() > 0 && g.height() > 0 {
            return (g.width(), g.height());
        }
    }
    if let Some(display) = gtk::gdk::Display::default() {
        if let Some(obj) = display.monitors().item(0) {
            if let Ok(monitor) = obj.downcast::<gtk::gdk::Monitor>() {
                let g = monitor.geometry();
                if g.width() > 0 && g.height() > 0 {
                    return (g.width(), g.height());
                }
            }
        }
    }
    (1280, 720)
}

mod dropdown {
    pub fn request_close_all() {
        crate::ui::bar::close_popovers();
    }
}
