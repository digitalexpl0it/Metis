//! Network: a pill-tabbed page splitting Wireless (Wi-Fi scan/connect/known
//! networks + DNS override), Wired (per-NIC IPv4 DHCP/static + DNS override), and
//! Proxy (system proxy via GNOME gsettings). All `nmcli`/`gsettings` work runs off
//! the GTK main thread; results arrive over an mpsc channel drained on a timeout.

use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use gtk::prelude::*;

use crate::net::{self, NetSnapshot};
use crate::ui;

struct Sections {
    radio: gtk::Switch,
    wifi: gtk::Box,
    saved: gtk::Box,
    wifi_dns: gtk::Box,
    eth: gtk::Box,
    proxy: gtk::Box,
}

pub fn build() -> gtk::Widget {
    let (scroller, content) = ui::page_for("network");

    let stack = gtk::Stack::new();
    stack.set_vexpand(true);
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    stack.set_transition_duration(120);

    content.append(&pill_tabs(
        &stack,
        &[
            ("wireless", "Wireless"),
            ("wired", "Wired"),
            ("proxy", "Proxy"),
        ],
    ));
    content.append(&stack);

    // ---- Wireless page ----
    let wireless = page_box();
    let (wifi_card, wifi_body) = ui::section("Wi-Fi");
    let radio = gtk::Switch::new();
    radio.set_halign(gtk::Align::End);
    radio.set_valign(gtk::Align::Center);
    let radio_row = ui::row("Wi-Fi radio", &radio);
    let rescan = gtk::Button::with_label("Rescan");
    radio_row.append(&rescan);
    wifi_body.append(&radio_row);
    let wifi_list = gtk::Box::new(gtk::Orientation::Vertical, 4);
    wifi_list.add_css_class("metis-settings-list");
    wifi_body.append(&wifi_list);
    wireless.append(&wifi_card);

    let (saved_card, saved_body) = ui::section("Known Wi-Fi networks");
    wireless.append(&saved_card);

    let (wdns_card, wdns_body) = ui::section("DNS");
    wireless.append(&wdns_card);
    stack.add_named(&wireless, Some("wireless"));

    // ---- Wired page ----
    let wired = page_box();
    let (eth_card, eth_body) = ui::section("Ethernet");
    wired.append(&eth_card);
    stack.add_named(&wired, Some("wired"));

    // ---- Proxy page ----
    let proxy_page = page_box();
    let (proxy_card, proxy_body) = ui::section("System proxy");
    proxy_page.append(&proxy_card);
    stack.add_named(&proxy_page, Some("proxy"));

    let sections = Rc::new(Sections {
        radio: radio.clone(),
        wifi: wifi_list,
        saved: saved_body,
        wifi_dns: wdns_body,
        eth: eth_body,
        proxy: proxy_body,
    });

    // Snapshot delivery: worker thread -> mpsc -> glib poll -> render.
    let (tx, rx) = mpsc::channel::<NetSnapshot>();
    let refresh = {
        let tx = tx.clone();
        Rc::new(move || {
            let tx = tx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(net::load_snapshot());
            });
        })
    };

    {
        let sections = sections.clone();
        let refresh = refresh.clone();
        glib::timeout_add_local(Duration::from_millis(150), move || {
            if let Ok(snap) = rx.try_recv() {
                render(&sections, &snap, &refresh);
            }
            glib::ControlFlow::Continue
        });
    }

    {
        let refresh = refresh.clone();
        radio.connect_active_notify(move |s| {
            net::set_radio(s.is_active());
            schedule_refresh(&refresh, 1500);
        });
    }
    {
        let refresh = refresh.clone();
        rescan.connect_clicked(move |_| {
            net::set_radio(true);
            schedule_refresh(&refresh, 2500);
        });
    }

    // Initial load.
    refresh();

    scroller.upcast()
}

/// A vertical content box for a stack page (matches the page's own spacing).
fn page_box() -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 16);
    b.set_margin_top(8);
    b
}

/// A segmented pill-tab bar that switches `stack` between named children.
fn pill_tabs(stack: &gtk::Stack, tabs: &[(&str, &str)]) -> gtk::Box {
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    bar.add_css_class("metis-settings-tabs");
    bar.set_halign(gtk::Align::Center);

    let mut group: Option<gtk::ToggleButton> = None;
    for (i, (name, label)) in tabs.iter().enumerate() {
        let btn = gtk::ToggleButton::with_label(label);
        btn.add_css_class("metis-settings-tab");
        match &group {
            Some(g) => btn.set_group(Some(g)),
            None => group = Some(btn.clone()),
        }
        if i == 0 {
            btn.set_active(true);
        }
        let stack = stack.clone();
        let name = name.to_string();
        btn.connect_toggled(move |b| {
            if b.is_active() {
                stack.set_visible_child_name(&name);
            }
        });
        bar.append(&btn);
    }
    bar
}

fn schedule_refresh(refresh: &Rc<impl Fn() + 'static>, delay_ms: u32) {
    let refresh = refresh.clone();
    glib::timeout_add_local_once(Duration::from_millis(delay_ms as u64), move || refresh());
}

fn render<F: Fn() + 'static>(sections: &Rc<Sections>, snap: &NetSnapshot, refresh: &Rc<F>) {
    sections.radio.set_active(snap.wifi_enabled);

    // ---- Wi-Fi list ----
    clear(&sections.wifi);
    if !snap.wifi_enabled {
        sections.wifi.append(&hint("Wi-Fi is off."));
    } else if snap.wifi.is_empty() {
        sections.wifi.append(&hint("No networks found."));
    } else {
        for n in &snap.wifi {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            let lock = if n.secured { "🔒 " } else { "" };
            let label = gtk::Label::new(Some(&format!("{lock}{}  ·  {}%", n.ssid, n.signal)));
            label.set_xalign(0.0);
            label.set_hexpand(true);
            row.append(&label);
            if n.active {
                let tag = gtk::Label::new(Some("Connected"));
                tag.add_css_class("metis-settings-hint");
                row.append(&tag);
            } else {
                let connect = gtk::Button::with_label("Connect");
                {
                    let refresh = refresh.clone();
                    let net_box = sections.wifi.clone();
                    let row_ref = row.clone();
                    let n = n.clone();
                    connect.connect_clicked(move |btn| {
                        if n.secured {
                            prompt_password(&net_box, &row_ref, &n.ssid, &refresh);
                            btn.set_sensitive(false);
                        } else {
                            net::connect_wifi(n.ssid.clone(), None);
                            schedule_refresh(&refresh, 3000);
                        }
                    });
                }
                row.append(&connect);
            }
            sections.wifi.append(&row);
        }
    }

    // ---- Known networks ----
    clear(&sections.saved);
    let saved_list = gtk::Box::new(gtk::Orientation::Vertical, 4);
    saved_list.add_css_class("metis-settings-list");
    if snap.saved.is_empty() {
        sections.saved.append(&hint("No saved Wi-Fi networks."));
    } else {
        for c in &snap.saved {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            let label = gtk::Label::new(Some(&c.name));
            label.set_xalign(0.0);
            label.set_hexpand(true);
            let forget = gtk::Button::with_label("Forget");
            forget.add_css_class("destructive-action");
            {
                let refresh = refresh.clone();
                let uuid = c.uuid.clone();
                forget.connect_clicked(move |_| {
                    net::forget(&uuid);
                    schedule_refresh(&refresh, 1200);
                });
            }
            row.append(&label);
            row.append(&forget);
            saved_list.append(&row);
        }
        sections.saved.append(&saved_list);
    }

    // ---- Wi-Fi DNS override ----
    clear(&sections.wifi_dns);
    match &snap.active_wifi {
        Some(conn) => sections
            .wifi_dns
            .append(&dns_override_editor(&conn.name, &conn.ipv4, refresh)),
        None => sections
            .wifi_dns
            .append(&hint("Connect to a Wi-Fi network to override its DNS.")),
    }

    // ---- Ethernet ----
    clear(&sections.eth);
    if snap.eth.is_empty() {
        sections.eth.append(&hint("No Ethernet devices."));
    } else {
        for dev in &snap.eth {
            sections.eth.append(&ethernet_editor(dev, refresh));
        }
    }

    // ---- Proxy ----
    clear(&sections.proxy);
    sections.proxy.append(&proxy_editor(&snap.proxy, refresh));
}

/// A standalone DNS-override editor for a connection (used on the Wireless tab):
/// a comma-separated DNS list applied with `ignore-auto-dns` so it overrides DHCP.
fn dns_override_editor<F: Fn() + 'static>(
    conn: &str,
    ipv4: &net::Ipv4,
    refresh: &Rc<F>,
) -> gtk::Widget {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 8);

    let title = gtk::Label::new(Some(&format!("Connected: {conn}")));
    title.set_xalign(0.0);
    card.append(&title);

    let dns = entry("1.1.1.1, 8.8.8.8", &ipv4.dns);
    card.append(&ui::row("DNS servers", &dns));
    card.append(&hint(
        "Comma-separated. Leave empty to use the DHCP-provided DNS.",
    ));

    let apply = gtk::Button::with_label("Apply DNS");
    apply.add_css_class("suggested-action");
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    actions.add_css_class("metis-settings-actions");
    actions.set_halign(gtk::Align::End);
    actions.append(&apply);
    card.append(&actions);
    {
        let refresh = refresh.clone();
        let conn = conn.to_string();
        apply.connect_clicked(move |_| {
            net::set_dns_override(&conn, &dns.text());
            schedule_refresh(&refresh, 2500);
        });
    }

    card.upcast()
}

fn ethernet_editor<F: Fn() + 'static>(dev: &net::EthDev, refresh: &Rc<F>) -> gtk::Widget {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 8);
    card.set_margin_top(4);

    let status = if dev.connected { "Connected" } else { "Disconnected" };
    let title = match &dev.connection {
        Some(conn) if !conn.is_empty() => format!("{}  ·  {status}  ·  {conn}", dev.device),
        _ => format!("{}  ·  {status}", dev.device),
    };
    let title = gtk::Label::new(Some(&title));
    title.set_xalign(0.0);
    title.add_css_class("metis-settings-value");
    card.append(&title);

    let Some(conn) = dev.connection.clone() else {
        card.append(&hint("No active profile for this device."));
        return card.upcast();
    };

    let method = gtk::DropDown::from_strings(&["Automatic (DHCP)", "Manual (static)"]);
    let is_manual = dev.ipv4.method == "manual";
    method.set_selected(if is_manual { 1 } else { 0 });
    card.append(&ui::row("IPv4 method", &method));

    // Address + gateway only apply to a static config.
    let manual_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let addr = entry("192.168.1.50/24", &dev.ipv4.addresses);
    let gw = entry("192.168.1.1", &dev.ipv4.gateway);
    manual_box.append(&ui::row("Address (CIDR)", &addr));
    manual_box.append(&ui::row("Gateway", &gw));
    manual_box.set_visible(is_manual);
    card.append(&manual_box);

    // DNS applies to both methods: on DHCP it overrides the provided servers.
    let dns = entry("1.1.1.1, 8.8.8.8", &dev.ipv4.dns);
    card.append(&ui::row("DNS (override)", &dns));

    {
        let manual_box = manual_box.clone();
        method.connect_selected_notify(move |dd| {
            manual_box.set_visible(dd.selected() == 1);
        });
    }

    let apply = gtk::Button::with_label("Apply");
    apply.add_css_class("suggested-action");
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    actions.add_css_class("metis-settings-actions");
    actions.set_halign(gtk::Align::End);
    actions.append(&apply);
    card.append(&actions);
    {
        let refresh = refresh.clone();
        let conn = conn.clone();
        let method = method.clone();
        apply.connect_clicked(move |_| {
            if method.selected() == 1 {
                net::set_ipv4_static(&conn, &addr.text(), &gw.text(), &dns.text());
            } else {
                net::set_ipv4_dhcp(&conn, &dns.text());
            }
            schedule_refresh(&refresh, 2500);
        });
    }

    card.upcast()
}

fn proxy_editor<F: Fn() + 'static>(cfg: &net::ProxyConfig, refresh: &Rc<F>) -> gtk::Widget {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 8);

    if !cfg.available {
        card.append(&hint(
            "System proxy settings are unavailable (the GNOME proxy schema isn't installed).",
        ));
        return card.upcast();
    }

    let mode = gtk::DropDown::from_strings(&["None", "Manual", "Automatic (PAC)"]);
    let mode_idx = match cfg.mode.as_str() {
        "manual" => 1,
        "auto" => 2,
        _ => 0,
    };
    mode.set_selected(mode_idx);
    card.append(&ui::row("Proxy mode", &mode));

    // Manual: per-protocol host:port + ignore list.
    let manual_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let (http_row, http_host, http_port) =
        host_port_row("HTTP", &cfg.http_host, cfg.http_port);
    let (https_row, https_host, https_port) =
        host_port_row("HTTPS", &cfg.https_host, cfg.https_port);
    let (socks_row, socks_host, socks_port) =
        host_port_row("SOCKS", &cfg.socks_host, cfg.socks_port);
    let ignore = entry("localhost, 127.0.0.0/8, ::1", &cfg.ignore_hosts);
    manual_box.append(&http_row);
    manual_box.append(&https_row);
    manual_box.append(&socks_row);
    manual_box.append(&ui::row("Ignore hosts", &ignore));
    manual_box.set_visible(mode_idx == 1);
    card.append(&manual_box);

    // Automatic: PAC URL.
    let auto_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let pac = entry("http://example.com/proxy.pac", &cfg.auto_url);
    auto_box.append(&ui::row("PAC URL", &pac));
    auto_box.set_visible(mode_idx == 2);
    card.append(&auto_box);

    {
        let manual_box = manual_box.clone();
        let auto_box = auto_box.clone();
        mode.connect_selected_notify(move |dd| {
            manual_box.set_visible(dd.selected() == 1);
            auto_box.set_visible(dd.selected() == 2);
        });
    }

    card.append(&hint(
        "Applies to GLib/GTK apps via the system proxy resolver.",
    ));

    let apply = gtk::Button::with_label("Apply");
    apply.add_css_class("suggested-action");
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    actions.add_css_class("metis-settings-actions");
    actions.set_halign(gtk::Align::End);
    actions.append(&apply);
    card.append(&actions);
    {
        let refresh = refresh.clone();
        let mode = mode.clone();
        apply.connect_clicked(move |_| {
            let mode_str = match mode.selected() {
                1 => "manual",
                2 => "auto",
                _ => "none",
            };
            let new = net::ProxyConfig {
                mode: mode_str.to_string(),
                auto_url: pac.text().to_string(),
                http_host: http_host.text().to_string(),
                http_port: parse_port(&http_port.text()),
                https_host: https_host.text().to_string(),
                https_port: parse_port(&https_port.text()),
                socks_host: socks_host.text().to_string(),
                socks_port: parse_port(&socks_port.text()),
                ignore_hosts: ignore.text().to_string(),
                available: true,
            };
            net::set_proxy(new);
            schedule_refresh(&refresh, 1200);
        });
    }

    card.upcast()
}

/// A "<proto>  [host............] [port]" row for the proxy editor.
fn host_port_row(label: &str, host: &str, port: u32) -> (gtk::Box, gtk::Entry, gtk::Entry) {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.add_css_class("metis-settings-row");
    let lbl = gtk::Label::new(Some(label));
    lbl.set_xalign(0.0);
    lbl.set_width_chars(6);
    row.append(&lbl);

    let host_e = gtk::Entry::builder()
        .placeholder_text("proxy.example.com")
        .hexpand(true)
        .build();
    host_e.set_text(host);
    row.append(&host_e);

    let port_e = gtk::Entry::builder()
        .placeholder_text("8080")
        .max_width_chars(6)
        .build();
    if port != 0 {
        port_e.set_text(&port.to_string());
    }
    row.append(&port_e);

    (row, host_e, port_e)
}

fn parse_port(s: &str) -> u32 {
    s.trim().parse().unwrap_or(0)
}

fn prompt_password<F: Fn() + 'static>(
    container: &gtk::Box,
    after: &gtk::Box,
    ssid: &str,
    refresh: &Rc<F>,
) {
    let prompt = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    prompt.add_css_class("metis-settings-row");
    let entry = gtk::PasswordEntry::builder()
        .show_peek_icon(true)
        .hexpand(true)
        .build();
    let join = gtk::Button::with_label("Join");
    prompt.append(&entry);
    prompt.append(&join);

    // Insert right after the network row.
    container.insert_child_after(&prompt, Some(after));
    entry.grab_focus();

    {
        let refresh = refresh.clone();
        let ssid = ssid.to_string();
        let entry2 = entry.clone();
        join.connect_clicked(move |_| {
            net::connect_wifi(ssid.clone(), Some(entry2.text().to_string()));
            schedule_refresh(&refresh, 3000);
        });
    }
    {
        let refresh = refresh.clone();
        let ssid = ssid.to_string();
        entry.connect_activate(move |e| {
            net::connect_wifi(ssid.clone(), Some(e.text().to_string()));
            schedule_refresh(&refresh, 3000);
        });
    }
}

fn entry(placeholder: &str, value: &str) -> gtk::Entry {
    let e = gtk::Entry::builder()
        .placeholder_text(placeholder)
        .hexpand(true)
        .build();
    e.set_text(value);
    e
}

fn hint(text: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    l.add_css_class("metis-settings-hint");
    l
}

fn clear(b: &gtk::Box) {
    while let Some(child) = b.first_child() {
        b.remove(&child);
    }
}
