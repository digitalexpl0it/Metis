//! Control-center tab layouts for the system dashboard.

use gtk::prelude::*;

use super::charts;

pub struct TempGaugeCard {
    pub card: gtk::Box,
    pub gauge: gtk::DrawingArea,
    pub value: gtk::Label,
    pub title: gtk::Label,
    pub temp: std::rc::Rc<std::cell::RefCell<Option<f32>>>,
}

pub struct OverviewPage {
    pub widget: gtk::Widget,
    pub session_card: gtk::Box,
    pub cpu_value: gtk::Label,
    pub cpu_chart: gtk::DrawingArea,
    pub cpu_legend: gtk::FlowBox,
    pub mem_value: gtk::Label,
    pub mem_chart: gtk::DrawingArea,
    pub mem_legend: gtk::Box,
    pub eth_down: gtk::Label,
    pub eth_up: gtk::Label,
    pub wifi_down: gtk::Label,
    pub wifi_up: gtk::Label,
    pub net_chart: gtk::DrawingArea,
    pub net_legend: gtk::Box,
    pub firewall_status: gtk::Label,
    pub disk_io_value: gtk::Label,
    pub disk_io_chart: gtk::DrawingArea,
    pub load_label: gtk::Label,
    pub uptime_label: gtk::Label,
    pub disk_box: gtk::FlowBox,
    pub hostname: gtk::Label,
    pub cpu_model: gtk::Label,
    pub cpu_cores: gtk::Label,
    pub system_memory: gtk::Label,
    pub kernel: gtk::Label,
    pub temp_gauges: gtk::Box,
    pub cpu_temp: TempGaugeCard,
}

pub struct ProcessHeader {
    pub name: gtk::Button,
    pub pid: gtk::Button,
    pub user: gtk::Button,
    pub kind: gtk::Button,
    pub cpu: gtk::Button,
    pub memory: gtk::Button,
}

pub struct ProcessPage {
    pub widget: gtk::Widget,
    pub search: gtk::SearchEntry,
    pub filter: gtk::DropDown,
    pub monitor_btn: gtk::Button,
    pub list: gtk::ListBox,
    pub headers: ProcessHeader,
}

pub fn build_overview() -> OverviewPage {
    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .margin_start(12)
        .margin_end(12)
        .margin_top(4)
        .margin_bottom(10)
        .build();
    page.add_css_class("metis-dash-overview");

    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .vexpand(true)
        .hexpand(true)
        .build();
    body.add_css_class("metis-dash-overview-body");

    // Row 1: CPU | Memory
    let cpu_mem = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .homogeneous(true)
        .build();

    let cpu_card = card_with_icon(
        "Processor",
        &["cpu-symbolic", "utilities-system-monitor-symbolic"],
    );
    let cpu_header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    let cpu_value = gtk::Label::new(Some("—"));
    cpu_value.add_css_class("metis-dash-value-inline");
    cpu_value.set_halign(gtk::Align::End);
    cpu_value.set_hexpand(true);
    cpu_header.append(&cpu_value);
    cpu_card.append(&cpu_header);
    let cpu_chart = gtk::DrawingArea::new();
    cpu_chart.add_css_class("metis-dash-chart");
    cpu_chart.add_css_class("metis-dash-chart-cpu");
    cpu_chart.set_content_height(110);
    cpu_chart.set_vexpand(true);
    cpu_chart.set_hexpand(true);
    cpu_card.append(&cpu_chart);
    let cpu_legend = gtk::FlowBox::new();
    cpu_legend.set_selection_mode(gtk::SelectionMode::None);
    cpu_legend.set_max_children_per_line(10);
    cpu_legend.set_column_spacing(8);
    cpu_legend.set_row_spacing(2);
    cpu_legend.add_css_class("metis-dash-legend");
    cpu_card.append(&cpu_legend);
    cpu_card.set_vexpand(true);

    let mem_card = card_with_icon(
        "Memory",
        &["media-flash-symbolic", "drive-harddisk-solidstate-symbolic"],
    );
    let mem_value = gtk::Label::new(Some("—"));
    mem_value.add_css_class("metis-dash-value");
    mem_value.set_halign(gtk::Align::Start);
    mem_card.append(&mem_value);
    let mem_chart = gtk::DrawingArea::new();
    mem_chart.add_css_class("metis-dash-chart");
    mem_chart.set_content_height(110);
    mem_chart.set_vexpand(true);
    mem_chart.set_hexpand(true);
    mem_card.append(&mem_chart);
    let mem_legend = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .build();
    mem_legend.add_css_class("metis-dash-legend");
    mem_legend.append(&legend_chip(0, "RAM"));
    mem_legend.append(&legend_chip(1, "Swap"));
    mem_card.append(&mem_legend);
    mem_card.set_vexpand(true);

    cpu_mem.append(&cpu_card);
    cpu_mem.append(&mem_card);
    body.append(&cpu_mem);

    // Row 2: Network (Ethernet / Wi‑Fi) | Disk I/O
    let net_disk = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .homogeneous(true)
        .build();

    let net_card = card_with_icon(
        "Network",
        &[
            "network-transmit-receive-symbolic",
            "network-wireless-symbolic",
        ],
    );
    let iface_grid = gtk::Grid::builder()
        .column_spacing(12)
        .row_spacing(4)
        .margin_bottom(4)
        .build();
    let eth_down = iface_rate_label("Ethernet ↓");
    let eth_up = iface_rate_label("Ethernet ↑");
    let wifi_down = iface_rate_label("Wi‑Fi ↓");
    let wifi_up = iface_rate_label("Wi‑Fi ↑");
    iface_grid.attach(&eth_down, 0, 0, 1, 1);
    iface_grid.attach(&eth_up, 1, 0, 1, 1);
    iface_grid.attach(&wifi_down, 0, 1, 1, 1);
    iface_grid.attach(&wifi_up, 1, 1, 1, 1);
    net_card.append(&iface_grid);
    let net_chart = gtk::DrawingArea::new();
    net_chart.add_css_class("metis-dash-chart");
    net_chart.set_content_height(88);
    net_chart.set_vexpand(true);
    net_chart.set_hexpand(true);
    net_card.append(&net_chart);
    let net_legend = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .build();
    net_legend.add_css_class("metis-dash-legend");
    net_legend.append(&rate_legend_chip(true, "Download"));
    net_legend.append(&rate_legend_chip(false, "Upload"));
    net_card.append(&net_legend);
    let firewall_status = gtk::Label::new(Some("—"));
    firewall_status.add_css_class("metis-dash-muted");
    firewall_status.set_halign(gtk::Align::Start);
    firewall_status.set_xalign(0.0);
    net_card.append(&firewall_status);
    net_card.set_vexpand(true);

    let disk_io_card = card_with_icon(
        "Disk I/O",
        &["drive-harddisk-symbolic", "media-flash-symbolic"],
    );
    let disk_io_value = gtk::Label::new(Some("—"));
    disk_io_value.add_css_class("metis-dash-value");
    disk_io_value.set_halign(gtk::Align::Start);
    disk_io_card.append(&disk_io_value);
    let disk_io_chart = gtk::DrawingArea::new();
    disk_io_chart.add_css_class("metis-dash-chart");
    disk_io_chart.set_content_height(88);
    disk_io_chart.set_vexpand(true);
    disk_io_chart.set_hexpand(true);
    disk_io_card.append(&disk_io_chart);
    let disk_io_legend = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .build();
    disk_io_legend.add_css_class("metis-dash-legend");
    disk_io_legend.append(&rate_legend_chip(true, "Read"));
    disk_io_legend.append(&rate_legend_chip(false, "Write"));
    disk_io_card.append(&disk_io_legend);
    disk_io_card.set_vexpand(true);

    net_disk.append(&net_card);
    net_disk.append(&disk_io_card);
    body.append(&net_disk);

    // Row 3: Session | Storage
    let session_card = card_with_icon(
        "Session",
        &["clock-symbolic", "appointment-soon-symbolic"],
    );
    let session_grid = gtk::Grid::builder()
        .column_spacing(12)
        .row_spacing(10)
        .margin_top(4)
        .build();
    session_grid.add_css_class("metis-dash-session-grid");
    let load_label = session_value_label();
    let uptime_label = session_value_label();
    session_stat_row(&session_grid, 0, "Load average", &load_label);
    session_stat_row(&session_grid, 1, "Uptime", &uptime_label);
    session_card.append(&session_grid);

    let disk_card = card_with_icon(
        "Storage",
        &["drive-harddisk-symbolic", "folder-symbolic"],
    );
    let disk_box = gtk::FlowBox::new();
    disk_box.set_selection_mode(gtk::SelectionMode::None);
    disk_box.set_max_children_per_line(2);
    disk_box.set_column_spacing(8);
    disk_box.set_row_spacing(6);
    disk_box.set_homogeneous(true);
    disk_box.add_css_class("metis-dash-disk-grid");
    disk_card.append(&disk_box);

    let mid = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    session_card.set_hexpand(false);
    session_card.set_size_request(240, -1);
    disk_card.set_hexpand(true);
    mid.append(&session_card);
    mid.append(&disk_card);
    body.append(&mid);

    // Row 4: System
    let system_card = card_with_icon(
        "System",
        &["computer-symbolic", "system-run-symbolic"],
    );
    let sys_grid = gtk::Grid::builder()
        .column_spacing(16)
        .row_spacing(6)
        .margin_top(2)
        .build();
    sys_grid.add_css_class("metis-dash-kv-grid");
    let hostname = kv_row(&sys_grid, 0, "Hostname");
    let cpu_model = kv_row(&sys_grid, 1, "Processor");
    let cpu_cores = kv_row(&sys_grid, 2, "Cores");
    let system_memory = kv_row(&sys_grid, 3, "Memory");
    let kernel = kv_row(&sys_grid, 4, "Kernel");
    kernel.set_ellipsize(gtk::pango::EllipsizeMode::End);
    cpu_model.set_ellipsize(gtk::pango::EllipsizeMode::End);
    system_card.append(&sys_grid);
    system_card.set_hexpand(false);
    system_card.set_size_request(300, -1);
    kernel.set_max_width_chars(32);
    cpu_model.set_max_width_chars(32);

    let temp_gauges = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    temp_gauges.add_css_class("metis-dash-temp-gauges");

    let (cpu_temp_card, cpu_temp) = build_temp_gauge_card(
        "CPU",
        &[
            "sensor-temperature-symbolic",
            "temperature-high-symbolic",
            "preferences-system-power-symbolic",
            "utilities-system-monitor-symbolic",
        ],
    );
    temp_gauges.append(&cpu_temp_card);

    let system_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    system_row.add_css_class("metis-dash-system-row");
    system_row.append(&temp_gauges);
    system_row.append(&system_card);
    let system_row_spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    system_row_spacer.set_hexpand(true);
    system_row.append(&system_row_spacer);
    body.append(&system_row);

    page.append(&body);

    OverviewPage {
        widget: page.upcast(),
        session_card,
        cpu_value,
        cpu_chart,
        cpu_legend,
        mem_value,
        mem_chart,
        mem_legend,
        eth_down,
        eth_up,
        wifi_down,
        wifi_up,
        net_chart,
        net_legend,
        firewall_status,
        disk_io_value,
        disk_io_chart,
        load_label,
        uptime_label,
        disk_box,
        hostname,
        cpu_model,
        cpu_cores,
        system_memory,
        kernel,
        temp_gauges,
        cpu_temp,
    }
}

pub fn build_processes() -> ProcessPage {
    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .margin_start(12)
        .margin_end(12)
        .margin_top(8)
        .margin_bottom(12)
        .build();
    page.add_css_class("metis-dash-process-page");

    let panel = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .vexpand(true)
        .hexpand(true)
        .build();
    panel.add_css_class("metis-dash-card");
    panel.add_css_class("metis-dash-proc-panel");

    let toolbar = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .margin_start(10)
        .margin_end(10)
        .margin_top(10)
        .margin_bottom(6)
        .build();
    let search = gtk::SearchEntry::builder()
        .placeholder_text("Search name, user, or PID…")
        .hexpand(true)
        .build();
    search.add_css_class("metis-dash-search");
    let filter = gtk::DropDown::from_strings(&["All processes", "User apps", "System"]);
    filter.add_css_class("metis-dash-filter");
    let monitor_btn = gtk::Button::builder()
        .label("Open monitor")
        .tooltip_text("Open btop or htop in a terminal")
        .build();
    monitor_btn.add_css_class("metis-dash-monitor-btn");
    toolbar.append(&search);
    toolbar.append(&filter);
    toolbar.append(&monitor_btn);
    panel.append(&toolbar);

    let table = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .margin_start(10)
        .margin_end(10)
        .vexpand(true)
        .build();

    let header = gtk::Grid::builder()
        .column_spacing(8)
        .margin_bottom(4)
        .build();
    header.add_css_class("metis-dash-table-head");
    header.add_css_class("metis-dash-proc-cols");

    let name = sort_header("Name", -1, gtk::Align::Start);
    name.set_hexpand(true);
    let pid = sort_header("PID", 64, gtk::Align::Start);
    let user = sort_header("User", 88, gtk::Align::Start);
    let kind = sort_header("Type", 64, gtk::Align::Start);
    let cpu = sort_header("CPU", 64, gtk::Align::End);
    let memory = sort_header("Memory", 80, gtk::Align::End);
    let end_spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    end_spacer.set_width_request(36);
    header.attach(&name, 0, 0, 1, 1);
    header.attach(&pid, 1, 0, 1, 1);
    header.attach(&user, 2, 0, 1, 1);
    header.attach(&kind, 3, 0, 1, 1);
    header.attach(&cpu, 4, 0, 1, 1);
    header.attach(&memory, 5, 0, 1, 1);
    header.attach(&end_spacer, 6, 0, 1, 1);
    table.append(&header);

    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("metis-dash-table");
    let scroll = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .child(&list)
        .build();
    scroll.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
    scroll.add_css_class("metis-dashboard-scroll");
    scroll.set_margin_bottom(8);
    table.append(&scroll);
    panel.append(&table);
    page.append(&panel);

    ProcessPage {
        widget: page.upcast(),
        search,
        filter,
        monitor_btn,
        list,
        headers: ProcessHeader {
            name,
            pid,
            user,
            kind,
            cpu,
            memory,
        },
    }
}

pub fn rate_legend_chip(primary: bool, label: &str) -> gtk::Box {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(5)
        .build();
    row.add_css_class("metis-dash-legend-item");
    let swatch = gtk::DrawingArea::new();
    swatch.set_content_width(10);
    swatch.set_content_height(10);
    swatch.add_css_class("metis-dash-legend-swatch");
    swatch.set_draw_func(move |_area, cr, w, h| {
        let (r, g, b) = charts::rate_series_color(primary);
        let pad = 1.0;
        cr.arc(
            w as f64 / 2.0,
            h as f64 / 2.0,
            (w.min(h) as f64 / 2.0 - pad).max(1.0),
            0.0,
            std::f64::consts::TAU,
        );
        cr.set_source_rgb(r, g, b);
        let _ = cr.fill();
    });
    let text = gtk::Label::new(Some(label));
    text.add_css_class("metis-dash-legend-label");
    row.append(&swatch);
    row.append(&text);
    row
}

pub fn aggregate_legend_chip(label: &str) -> gtk::Box {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(5)
        .build();
    row.add_css_class("metis-dash-legend-item");
    let swatch = gtk::DrawingArea::new();
    swatch.set_content_width(10);
    swatch.set_content_height(10);
    swatch.add_css_class("metis-dash-legend-swatch");
    swatch.set_draw_func(move |_area, cr, w, h| {
        let tokens = crate::ui::theme::active_tokens();
        let (r, g, b) = charts::accent_series_color(&tokens);
        let pad = 1.0;
        cr.arc(
            w as f64 / 2.0,
            h as f64 / 2.0,
            (w.min(h) as f64 / 2.0 - pad).max(1.0),
            0.0,
            std::f64::consts::TAU,
        );
        cr.set_source_rgb(r, g, b);
        let _ = cr.fill();
    });
    let text = gtk::Label::new(Some(label));
    text.add_css_class("metis-dash-legend-label");
    row.append(&swatch);
    row.append(&text);
    row
}

pub fn legend_chip(index: usize, label: &str) -> gtk::Box {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(5)
        .build();
    row.add_css_class("metis-dash-legend-item");
    let swatch = gtk::DrawingArea::new();
    swatch.set_content_width(10);
    swatch.set_content_height(10);
    swatch.add_css_class("metis-dash-legend-swatch");
    let idx = index;
    swatch.set_draw_func(move |_area, cr, w, h| {
        let tokens = crate::ui::theme::active_tokens();
        let (r, g, b) = charts::core_color(idx, idx + 1, &tokens);
        let pad = 1.0;
        cr.arc(
            w as f64 / 2.0,
            h as f64 / 2.0,
            (w.min(h) as f64 / 2.0 - pad).max(1.0),
            0.0,
            std::f64::consts::TAU,
        );
        cr.set_source_rgb(r, g, b);
        let _ = cr.fill();
    });
    let text = gtk::Label::new(Some(label));
    text.add_css_class("metis-dash-legend-label");
    row.append(&swatch);
    row.append(&text);
    row
}

pub fn disk_mount_card(mount: &str, pct: f64, used: &str, total: &str) -> gtk::Box {
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .margin_top(4)
        .margin_bottom(4)
        .margin_start(4)
        .margin_end(4)
        .build();
    card.add_css_class("metis-dash-disk-tile");
    let title = gtk::Label::new(Some(mount));
    title.add_css_class("metis-dash-kv");
    title.set_halign(gtk::Align::Start);
    let pct_lbl = gtk::Label::new(Some(&format!("{pct:.0}%")));
    pct_lbl.add_css_class("metis-dash-value-inline");
    pct_lbl.set_halign(gtk::Align::Start);
    let detail = gtk::Label::new(Some(&format!("{used} / {total}")));
    detail.add_css_class("metis-dash-sub");
    detail.set_halign(gtk::Align::Start);
    let bar = gtk::LevelBar::new();
    bar.add_css_class("metis-dash-meter");
    bar.set_value(pct / 100.0);
    card.append(&title);
    card.append(&pct_lbl);
    card.append(&detail);
    card.append(&bar);
    card
}

fn iface_rate_label(prefix: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(&format!("{prefix} —")));
    label.add_css_class("metis-dash-sub");
    label.set_halign(gtk::Align::Start);
    label.set_xalign(0.0);
    label
}

fn session_value_label() -> gtk::Label {
    let label = gtk::Label::new(Some("—"));
    label.add_css_class("metis-dash-session-value");
    label.set_halign(gtk::Align::End);
    label.set_xalign(1.0);
    label.set_hexpand(true);
    label
}

fn session_stat_row(grid: &gtk::Grid, row: i32, key: &str, value: &gtk::Label) {
    let k = gtk::Label::new(Some(key));
    k.add_css_class("metis-dash-session-key");
    k.set_halign(gtk::Align::Start);
    k.set_xalign(0.0);
    grid.attach(&k, 0, row, 1, 1);
    grid.attach(value, 1, row, 1, 1);
}

const ICON_LAST_RESORT: &str = "utilities-system-monitor-symbolic";

fn resolve_icon_name(candidates: &[&str]) -> String {
    gtk::gdk::Display::default()
        .and_then(|display| {
            let theme = gtk::IconTheme::for_display(&display);
            candidates
                .iter()
                .find(|name| theme.has_icon(name))
                .map(|name| (*name).to_string())
        })
        .unwrap_or_else(|| ICON_LAST_RESORT.to_string())
}

fn dashboard_icon(candidates: &[&str]) -> gtk::Image {
    let img = gtk::Image::new();
    img.add_css_class("metis-dash-card-icon");
    img.set_pixel_size(16);
    img.set_icon_name(Some(&resolve_icon_name(candidates)));
    img
}

fn card_with_icon(title: &str, icons: &[&str]) -> gtk::Box {
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .build();
    card.add_css_class("metis-dash-card");
    let head = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    let img = dashboard_icon(icons);
    let heading = gtk::Label::new(Some(title));
    heading.add_css_class("metis-dash-card-title");
    heading.set_halign(gtk::Align::Start);
    heading.set_hexpand(true);
    head.append(&img);
    head.append(&heading);
    card.append(&head);
    card
}

pub fn build_temp_gauge_card(title: &str, icons: &[&str]) -> (gtk::Box, TempGaugeCard) {
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .build();
    card.add_css_class("metis-dash-card");
    card.add_css_class("metis-dash-gauge-card");

    let head = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .build();
    let img = dashboard_icon(icons);
    let heading = gtk::Label::new(Some(title));
    heading.add_css_class("metis-dash-card-title");
    heading.set_halign(gtk::Align::Start);
    head.append(&img);
    head.append(&heading);
    card.append(&head);

    let gauge = gtk::DrawingArea::new();
    gauge.add_css_class("metis-dash-gauge");
    gauge.set_content_width(96);
    gauge.set_content_height(72);
    gauge.set_halign(gtk::Align::Center);
    card.append(&gauge);

    let value = gtk::Label::new(Some("—"));
    value.add_css_class("metis-dash-gauge-value");
    value.set_halign(gtk::Align::Center);
    value.set_xalign(0.5);
    card.append(&value);

    let temp = std::rc::Rc::new(std::cell::RefCell::new(None));
    charts::wire_temp_gauge(&gauge, temp.clone());

    let gauge_card = TempGaugeCard {
        card: card.clone(),
        gauge,
        value,
        title: heading,
        temp,
    };

    (card, gauge_card)
}

fn sort_header(text: &str, width: i32, align: gtk::Align) -> gtk::Button {
    let btn = gtk::Button::builder()
        .label(text)
        .halign(align)
        .build();
    if width > 0 {
        btn.set_width_request(width);
    }
    if align == gtk::Align::End {
        btn.add_css_class("metis-dash-sort-end");
    }
    btn.add_css_class("metis-dash-sort");
    btn
}

fn kv_row(grid: &gtk::Grid, row: i32, key: &str) -> gtk::Label {
    let k = gtk::Label::new(Some(key));
    k.add_css_class("metis-dash-kv-key");
    k.set_halign(gtk::Align::Start);
    let v = gtk::Label::new(Some("—"));
    v.add_css_class("metis-dash-kv");
    v.set_halign(gtk::Align::Start);
    v.set_xalign(0.0);
    v.set_hexpand(true);
    v.set_wrap(false);
    grid.attach(&k, 0, row, 1, 1);
    grid.attach(&v, 1, row, 1, 1);
    v
}
