use gtk::prelude::*;

use crate::ui::icons::{self, names};

pub struct BatteryWidget {
    root: gtk::Box,
    icon: gtk::Image,
    last_percent: std::cell::Cell<Option<u8>>,
    last_charging: std::cell::Cell<bool>,
}

impl BatteryWidget {
    pub fn new() -> Self {
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(0)
            .build();
        root.add_css_class("metis-bar-widget");
        root.add_css_class("metis-bar-battery");
        root.add_css_class("metis-bar-sys-icon");

        let icon = icons::image(names::battery(100, false));
        root.append(&icon);

        Self {
            root,
            icon,
            last_percent: std::cell::Cell::new(None),
            last_charging: std::cell::Cell::new(false),
        }
    }

    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    pub fn update(&self, percent: Option<u8>, charging: bool) {
        let pct = percent.unwrap_or(0);
        if self.last_percent.get() == percent && self.last_charging.get() == charging {
            return;
        }
        self.last_percent.set(percent);
        self.last_charging.set(charging);
        self.root
            .set_tooltip_text(Some(&format!("Battery {pct}%")));
        icons::set_icon(&self.icon, names::battery(pct, charging));
    }
}

use crate::services::{EthernetStatus, WifiNetwork};

/// Shared state + interactive widgets for the network popover so row/button
/// closures can re-render the Wi-Fi list and drive the connect flow.
struct NetInner {
    list: gtk::Box,
    status_label: gtk::Label,
    connect_box: gtk::Box,
    connect_title: gtk::Label,
    password_entry: gtk::Entry,
    selected_ssid: RefCell<Option<String>>,
    /// SSID we just asked to connect to, with the time of the request (for the
    /// in-row spinner). Cleared once that network reports active or it times out.
    pending: RefCell<Option<(String, Instant)>>,
    last_sig: RefCell<String>,
    wifi: RefCell<Vec<WifiNetwork>>,
    wifi_enabled: Cell<bool>,
}

impl NetInner {
    fn rebuild_list(self: &Rc<Self>) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        if !self.wifi_enabled.get() {
            self.status_label.set_text("Wi-Fi is off");
            self.status_label.set_visible(true);
            return;
        }
        let wifi = self.wifi.borrow();
        if wifi.is_empty() {
            self.status_label.set_text("No networks found");
            self.status_label.set_visible(true);
            return;
        }
        self.status_label.set_visible(false);
        let pending = self.pending.borrow().as_ref().map(|(s, _)| s.clone());
        for net in wifi.iter() {
            let row = self.build_row(net, pending.as_deref());
            self.list.append(&row);
        }
    }

    fn build_row(self: &Rc<Self>, net: &WifiNetwork, pending: Option<&str>) -> gtk::Button {
        let row = gtk::Button::builder().has_frame(false).build();
        row.add_css_class("metis-net-row");

        let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 10);

        let signal = icons::image(wifi_signal_icon(net.signal));
        hbox.append(&signal);

        let ssid = gtk::Label::new(Some(&net.ssid));
        ssid.set_halign(gtk::Align::Start);
        ssid.set_hexpand(true);
        ssid.set_ellipsize(gtk::pango::EllipsizeMode::End);
        ssid.set_max_width_chars(22);
        hbox.append(&ssid);

        if net.secured {
            let lock = icons::image("network-wireless-encrypted-symbolic");
            lock.add_css_class("metis-net-lock");
            hbox.append(&lock);
        }

        if pending == Some(net.ssid.as_str()) && !net.active {
            let spinner = gtk::Spinner::new();
            spinner.start();
            hbox.append(&spinner);
        } else if net.active {
            let check = icons::image("object-select-symbolic");
            check.add_css_class("metis-net-active");
            hbox.append(&check);
        }

        row.set_child(Some(&hbox));

        let inner = self.clone();
        let net = net.clone();
        row.connect_clicked(move |_| inner.on_row_clicked(&net));
        row
    }

    fn on_row_clicked(self: &Rc<Self>, net: &WifiNetwork) {
        if net.active {
            return;
        }
        if net.secured {
            self.selected_ssid.replace(Some(net.ssid.clone()));
            self.connect_title
                .set_text(&format!("Connect to {}", net.ssid));
            self.password_entry.set_text("");
            self.connect_box.set_visible(true);
            self.password_entry.grab_focus();
        } else {
            crate::services::wifi_connect(net.ssid.clone(), None);
            self.pending
                .replace(Some((net.ssid.clone(), Instant::now())));
            self.connect_box.set_visible(false);
            self.rebuild_list();
        }
    }

    fn submit_connect(self: &Rc<Self>) {
        let Some(ssid) = self.selected_ssid.borrow().clone() else {
            return;
        };
        let password = self.password_entry.text().to_string();
        crate::services::wifi_connect(ssid.clone(), Some(password));
        self.pending.replace(Some((ssid, Instant::now())));
        self.connect_box.set_visible(false);
        self.password_entry.set_text("");
        self.rebuild_list();
    }
}

pub struct NetworkWidget {
    root: gtk::Button,
    icon: gtk::Image,
    eth_row: gtk::Box,
    eth_icon: gtk::Image,
    eth_label: gtk::Label,
    wifi_switch: gtk::Switch,
    updating_switch: Rc<Cell<bool>>,
    inner: Rc<NetInner>,
}

impl NetworkWidget {
    pub fn new() -> Self {
        let root = gtk::Button::builder().build();
        root.add_css_class("metis-bar-widget");
        root.add_css_class("metis-bar-network");
        root.add_css_class("metis-bar-sys-icon");

        let icon = icons::image(names::network(true));
        root.set_child(Some(&icon));

        let panel = super::super::dropdown::build_panel();
        panel.set_spacing(10);
        panel.set_width_request(300);

        // ---- Header: title, Wi-Fi radio toggle, refresh ----
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let title = gtk::Label::builder()
            .label("Network")
            .halign(gtk::Align::Start)
            .hexpand(true)
            .build();
        title.add_css_class("metis-bar-section-title");
        header.append(&title);

        let updating_switch = Rc::new(Cell::new(false));
        let wifi_switch = gtk::Switch::new();
        wifi_switch.set_valign(gtk::Align::Center);
        wifi_switch.add_css_class("metis-net-switch");
        {
            let updating_switch = updating_switch.clone();
            wifi_switch.connect_state_set(move |_, state| {
                if !updating_switch.get() {
                    crate::services::wifi_set_radio(state);
                }
                glib::Propagation::Proceed
            });
        }
        header.append(&wifi_switch);

        let refresh = gtk::Button::from_icon_name("view-refresh-symbolic");
        refresh.set_valign(gtk::Align::Center);
        refresh.add_css_class("metis-net-refresh");
        refresh.connect_clicked(|_| crate::services::wifi_scan());
        header.append(&refresh);
        panel.append(&header);

        // ---- Ethernet status row (read-only) ----
        let eth_row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        eth_row.add_css_class("metis-net-eth-row");
        let eth_icon = icons::image("network-wired-symbolic");
        eth_row.append(&eth_icon);
        let eth_label = gtk::Label::new(Some("Ethernet"));
        eth_label.set_halign(gtk::Align::Start);
        eth_label.set_hexpand(true);
        eth_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        eth_row.append(&eth_label);
        eth_row.set_visible(false);
        panel.append(&eth_row);

        // ---- Wi-Fi list (scrollable) ----
        let list = gtk::Box::new(gtk::Orientation::Vertical, 2);
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .max_content_height(240)
            .propagate_natural_height(true)
            .child(&list)
            .build();
        scroll.add_css_class("metis-net-scroll");
        panel.append(&scroll);

        let status_label = gtk::Label::new(Some("Scanning…"));
        status_label.add_css_class("metis-net-status");
        status_label.set_halign(gtk::Align::Start);
        panel.append(&status_label);

        // ---- Inline connect area (shared password entry for secured nets) ----
        // A plain, visibility-toggled Box (not a Revealer): the entry stays
        // realized so a synchronous grab_focus() lands and the OnDemand layer
        // surface actually receives keyboard input — same proven pattern as the
        // clock world-clock picker. A Revealer's child isn't focusable mid-
        // animation, so focus (and typing) silently failed.
        let connect_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
        connect_box.add_css_class("metis-net-connect");
        connect_box.set_visible(false);
        let connect_title = gtk::Label::new(Some(""));
        connect_title.set_halign(gtk::Align::Start);
        connect_title.add_css_class("metis-net-connect-title");
        connect_box.append(&connect_title);
        let password_entry = gtk::Entry::builder()
            .visibility(false)
            .placeholder_text("Password")
            .build();
        password_entry.add_css_class("metis-net-password");
        connect_box.append(&password_entry);
        let btn_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        btn_row.set_halign(gtk::Align::End);
        let cancel_btn = gtk::Button::with_label("Cancel");
        cancel_btn.add_css_class("metis-net-cancel");
        let connect_btn = gtk::Button::with_label("Connect");
        connect_btn.add_css_class("metis-net-connect-btn");
        btn_row.append(&cancel_btn);
        btn_row.append(&connect_btn);
        connect_box.append(&btn_row);
        panel.append(&connect_box);

        let inner = Rc::new(NetInner {
            list,
            status_label,
            connect_box: connect_box.clone(),
            connect_title,
            password_entry: password_entry.clone(),
            selected_ssid: RefCell::new(None),
            pending: RefCell::new(None),
            last_sig: RefCell::new(String::new()),
            wifi: RefCell::new(Vec::new()),
            wifi_enabled: Cell::new(true),
        });

        {
            let inner = inner.clone();
            connect_btn.connect_clicked(move |_| inner.submit_connect());
        }
        {
            let inner = inner.clone();
            password_entry.connect_activate(move |_| inner.submit_connect());
        }
        {
            let inner = inner.clone();
            cancel_btn.connect_clicked(move |_| {
                inner.selected_ssid.replace(None);
                inner.connect_box.set_visible(false);
            });
        }

        // Trigger a scan whenever the popover opens.
        super::super::dropdown::wire_toggle_prepare(&root, &panel, || {
            crate::services::wifi_scan();
        });

        Self {
            root,
            icon,
            eth_row,
            eth_icon,
            eth_label,
            wifi_switch,
            updating_switch,
            inner,
        }
    }

    pub fn root(&self) -> &gtk::Button {
        &self.root
    }

    pub fn update(&self, eth: &EthernetStatus, wifi: &[WifiNetwork], wifi_enabled: bool) {
        icons::set_icon(&self.icon, bar_icon(eth, wifi, wifi_enabled));
        self.root
            .set_tooltip_text(Some(&network_tooltip(eth, wifi, wifi_enabled)));

        self.eth_row.set_visible(eth.present);
        if eth.present {
            icons::set_icon(
                &self.eth_icon,
                if eth.connected {
                    "network-wired-symbolic"
                } else {
                    "network-wired-disconnected-symbolic"
                },
            );
            self.eth_label.set_text(&eth.label);
        }

        self.updating_switch.set(true);
        self.wifi_switch.set_active(wifi_enabled);
        self.updating_switch.set(false);

        // Clear a stale "connecting" spinner once the target is active (or it
        // has been pending too long).
        {
            let mut pending = self.inner.pending.borrow_mut();
            if let Some((ssid, started)) = pending.clone() {
                let connected_now = wifi.iter().any(|n| n.ssid == ssid && n.active);
                if connected_now || started.elapsed() > Duration::from_secs(30) {
                    *pending = None;
                }
            }
        }

        *self.inner.wifi.borrow_mut() = wifi.to_vec();
        self.inner.wifi_enabled.set(wifi_enabled);

        let sig = network_signature(wifi, wifi_enabled, &self.inner.pending.borrow());
        if *self.inner.last_sig.borrow() != sig {
            *self.inner.last_sig.borrow_mut() = sig;
            self.inner.rebuild_list();
        }
    }
}

fn wifi_signal_icon(signal: u8) -> &'static str {
    match signal {
        80..=u8::MAX => "network-wireless-signal-excellent-symbolic",
        55..=79 => "network-wireless-signal-good-symbolic",
        30..=54 => "network-wireless-signal-ok-symbolic",
        10..=29 => "network-wireless-signal-weak-symbolic",
        _ => "network-wireless-signal-none-symbolic",
    }
}

fn bar_icon(eth: &EthernetStatus, wifi: &[WifiNetwork], wifi_enabled: bool) -> &'static str {
    if let Some(active) = wifi.iter().find(|n| n.active) {
        return wifi_signal_icon(active.signal);
    }
    if eth.connected {
        return "network-wired-symbolic";
    }
    if !wifi_enabled {
        return "network-wireless-disabled-symbolic";
    }
    "network-wireless-offline-symbolic"
}

fn network_tooltip(eth: &EthernetStatus, wifi: &[WifiNetwork], wifi_enabled: bool) -> String {
    if let Some(active) = wifi.iter().find(|n| n.active) {
        return active.ssid.clone();
    }
    if eth.connected {
        return eth.label.clone();
    }
    if !wifi_enabled {
        return "Wi-Fi off".into();
    }
    "Offline".into()
}

fn network_signature(
    wifi: &[WifiNetwork],
    enabled: bool,
    pending: &Option<(String, Instant)>,
) -> String {
    let mut s = format!("e{}|", enabled as u8);
    for n in wifi {
        // Bucket the signal so minor RSSI jitter doesn't trigger a rebuild.
        s.push_str(&format!(
            "{}:{}:{}:{};",
            n.ssid,
            n.active as u8,
            n.secured as u8,
            n.signal / 25
        ));
    }
    if let Some((ssid, _)) = pending {
        s.push_str("p:");
        s.push_str(ssid);
    }
    s
}

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

/// One labelled row: a mute icon-button on the left, a slider filling the rest.
struct AudioRow {
    scale: gtk::Scale,
    mute_icon: gtk::Image,
    percent: Rc<Cell<u8>>,
    muted: Rc<Cell<bool>>,
}

pub struct VolumeWidget {
    root: gtk::Button,
    icon: gtk::Image,
    output: AudioRow,
    input: AudioRow,
    updating: Rc<Cell<bool>>,
    suppress_until: Rc<Cell<Instant>>,
    last_out: Cell<(u8, bool)>,
    last_in: Cell<(u8, bool)>,
}

/// Hold off poller-driven updates briefly after a user action so optimistic UI
/// state isn't reverted by the lagging pactl read-back (fixes the mute flicker).
fn bump_suppress(cell: &Rc<Cell<Instant>>) {
    cell.set(Instant::now() + Duration::from_millis(700));
}

impl VolumeWidget {
    pub fn new() -> Self {
        let root = gtk::Button::builder().build();
        root.add_css_class("metis-bar-widget");
        root.add_css_class("metis-bar-volume");
        root.add_css_class("metis-bar-sys-icon");

        let icon = icons::image(names::volume(50, false));
        root.set_child(Some(&icon));

        let panel = super::super::dropdown::build_panel();
        panel.set_spacing(12);
        panel.set_width_request(260);

        let title = gtk::Label::builder()
            .label("Audio")
            .halign(gtk::Align::Start)
            .build();
        title.add_css_class("metis-bar-section-title");
        panel.append(&title);

        let updating = Rc::new(Cell::new(false));
        let suppress_until = Rc::new(Cell::new(Instant::now()));

        let output = build_audio_row(
            &panel,
            AudioKind::Output,
            &updating,
            &suppress_until,
        );
        let input = build_audio_row(
            &panel,
            AudioKind::Input,
            &updating,
            &suppress_until,
        );

        super::super::dropdown::wire_toggle(&root, &panel, "volume");

        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
        {
            let suppress_until = suppress_until.clone();
            scroll.connect_scroll(move |_, _, dy| {
                let delta = if dy < 0.0 { 5i8 } else { -5i8 };
                bump_suppress(&suppress_until);
                crate::services::set_volume_relative(delta);
                glib::Propagation::Stop
            });
        }
        root.add_controller(scroll);

        Self {
            root,
            icon,
            output,
            input,
            updating,
            suppress_until,
            last_out: Cell::new((255, false)),
            last_in: Cell::new((255, false)),
        }
    }

    pub fn root(&self) -> &gtk::Button {
        &self.root
    }

    pub fn update(&self, percent: u8, muted: bool, mic_percent: u8, mic_muted: bool) {
        // Don't let the poller stomp optimistic UI right after a user action.
        if Instant::now() < self.suppress_until.get() {
            return;
        }

        if self.last_out.get() != (percent, muted) {
            self.last_out.set((percent, muted));
            self.output.percent.set(percent);
            self.output.muted.set(muted);
            self.root
                .set_tooltip_text(Some(&format!("Volume {percent}%")));
            self.updating.set(true);
            self.output
                .scale
                .set_value(f64::from(if muted { 0 } else { percent }));
            self.updating.set(false);
            icons::set_icon(&self.icon, names::volume(percent, muted));
            icons::set_icon(&self.output.mute_icon, names::volume(percent, muted));
        }

        if self.last_in.get() != (mic_percent, mic_muted) {
            self.last_in.set((mic_percent, mic_muted));
            self.input.percent.set(mic_percent);
            self.input.muted.set(mic_muted);
            self.updating.set(true);
            self.input
                .scale
                .set_value(f64::from(if mic_muted { 0 } else { mic_percent }));
            self.updating.set(false);
            icons::set_icon(&self.input.mute_icon, names::mic(mic_percent, mic_muted));
        }
    }
}

#[derive(Clone, Copy)]
enum AudioKind {
    Output,
    Input,
}

fn build_audio_row(
    panel: &gtk::Box,
    kind: AudioKind,
    updating: &Rc<Cell<bool>>,
    suppress_until: &Rc<Cell<Instant>>,
) -> AudioRow {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .build();

    let percent = Rc::new(Cell::new(0u8));
    let muted = Rc::new(Cell::new(false));

    let initial_icon = match kind {
        AudioKind::Output => names::volume(50, false),
        AudioKind::Input => names::mic(50, false),
    };
    let mute_icon = icons::image(initial_icon);

    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
    scale.set_draw_value(false);
    scale.set_hexpand(true);
    scale.add_css_class("metis-bar-volume-scale");

    let set_mute_icon = {
        let mute_icon = mute_icon.clone();
        move |pct: u8, muted: bool| {
            let name = match kind {
                AudioKind::Output => names::volume(pct, muted),
                AudioKind::Input => names::mic(pct, muted),
            };
            icons::set_icon(&mute_icon, name);
        }
    };

    let mute_btn = gtk::Button::builder().build();
    mute_btn.add_css_class("metis-bar-audio-mute");
    mute_btn.set_child(Some(&mute_icon));
    mute_btn.set_valign(gtk::Align::Center);
    {
        let muted = muted.clone();
        let percent = percent.clone();
        let suppress_until = suppress_until.clone();
        let updating = updating.clone();
        let scale = scale.clone();
        let set_mute_icon = set_mute_icon.clone();
        mute_btn.connect_clicked(move |_| {
            let new_muted = !muted.get();
            muted.set(new_muted);
            bump_suppress(&suppress_until);
            set_mute_icon(percent.get(), new_muted);
            // Reflect mute on the slider immediately (poller is suppressed now).
            updating.set(true);
            scale.set_value(f64::from(if new_muted { 0 } else { percent.get() }));
            updating.set(false);
            match kind {
                AudioKind::Output => crate::services::set_mute(new_muted),
                AudioKind::Input => crate::services::set_mic_mute(new_muted),
            }
        });
    }
    row.append(&mute_btn);

    {
        let updating = updating.clone();
        let suppress_until = suppress_until.clone();
        let percent = percent.clone();
        let muted = muted.clone();
        let set_mute_icon = set_mute_icon.clone();
        scale.connect_value_changed(move |scale| {
            if updating.get() {
                return;
            }
            let pct = scale.value().round() as u8;
            percent.set(pct);
            bump_suppress(&suppress_until);
            // Dragging the slider implies the user wants sound: unmute.
            if muted.get() {
                muted.set(false);
                set_mute_icon(pct, false);
                match kind {
                    AudioKind::Output => crate::services::set_mute(false),
                    AudioKind::Input => crate::services::set_mic_mute(false),
                }
            } else {
                set_mute_icon(pct, false);
            }
            match kind {
                AudioKind::Output => crate::services::set_volume_absolute(pct),
                AudioKind::Input => crate::services::set_mic_volume_absolute(pct),
            }
        });
    }
    row.append(&scale);

    panel.append(&row);

    AudioRow {
        scale,
        mute_icon,
        percent,
        muted,
    }
}
