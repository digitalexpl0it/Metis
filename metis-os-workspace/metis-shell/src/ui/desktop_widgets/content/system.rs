//! System glance — CPU / RAM / disk (lightweight local sampling).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;
use metis_config::DesktopWidgetInstance;
use sysinfo::{Disks, System};

use crate::services::format_bytes;

use super::font::apply_font;

pub fn build(inst: &DesktopWidgetInstance) -> gtk::Widget {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
    root.set_hexpand(true);
    root.set_vexpand(true);

    let cpu_row = metric_row(&metis_i18n::tr("CPU"), &inst.font);
    let mem_row = metric_row(&metis_i18n::tr("Memory"), &inst.font);
    let disk_row = metric_row(&metis_i18n::tr("Disk"), &inst.font);
    root.append(&cpu_row.root);
    root.append(&mem_row.root);
    root.append(&disk_row.root);

    let sys = Rc::new(RefCell::new(System::new()));
    let tick = {
        let sys = sys.clone();
        let cpu = cpu_row.clone();
        let mem = mem_row.clone();
        let disk = disk_row.clone();
        move || {
            let mut sys = sys.borrow_mut();
            sys.refresh_cpu_usage();
            sys.refresh_memory();
            let cpu_pct = sys.global_cpu_usage().clamp(0.0, 100.0);
            cpu.set(cpu_pct / 100.0, &format!("{cpu_pct:.0}%"));

            let total = sys.total_memory().max(1);
            let used = sys.used_memory();
            let mem_pct = (used as f64 / total as f64) as f32;
            mem.set(
                mem_pct.clamp(0.0, 1.0),
                &format!("{} / {}", format_bytes(used), format_bytes(total)),
            );

            let disks = Disks::new_with_refreshed_list();
            let (du, dt) = disks
                .list()
                .iter()
                .find(|d| d.mount_point() == std::path::Path::new("/"))
                .map(|d| {
                    let total = d.total_space();
                    let avail = d.available_space();
                    (total.saturating_sub(avail), total.max(1))
                })
                .unwrap_or((0, 1));
            let disk_pct = (du as f64 / dt as f64) as f32;
            disk.set(
                disk_pct.clamp(0.0, 1.0),
                &format!("{} / {}", format_bytes(du), format_bytes(dt)),
            );

            glib::ControlFlow::Continue
        }
    };
    // First sample after a short delay so CPU usage isn't always ~0.
    tick();
    let source = Rc::new(Cell::new(Some(glib::timeout_add_seconds_local(2, tick))));
    root.connect_destroy(move |_| {
        if let Some(id) = source.take() {
            id.remove();
        }
    });

    root.upcast()
}

#[derive(Clone)]
struct MetricRow {
    root: gtk::Box,
    bar: gtk::ProgressBar,
    value: gtk::Label,
}

impl MetricRow {
    fn set(&self, fraction: f32, text: &str) {
        self.bar.set_fraction(fraction.clamp(0.0, 1.0) as f64);
        self.value.set_text(text);
    }
}

fn metric_row(title: &str, font: &str) -> MetricRow {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let label = gtk::Label::new(Some(title));
    label.add_css_class("metis-dw-metric-label");
    label.set_xalign(0.0);
    label.set_hexpand(true);
    apply_font(&label, font);
    head.append(&label);
    let value = gtk::Label::new(Some("—"));
    value.add_css_class("metis-dw-hint");
    value.set_xalign(1.0);
    apply_font(&value, font);
    head.append(&value);
    root.append(&head);

    let bar = gtk::ProgressBar::new();
    bar.add_css_class("metis-dw-progress");
    bar.set_show_text(false);
    root.append(&bar);

    MetricRow { root, bar, value }
}
