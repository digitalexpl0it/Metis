//! Network: Wi-Fi (scan / connect / radio), known connections (forget), and a
//! per-NIC Ethernet IPv4 editor (DHCP/static). All `nmcli` work runs off the GTK
//! main thread; results arrive over an mpsc channel drained on a glib timeout.

use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use gtk::prelude::*;

use crate::net::{self, NetSnapshot};
use crate::ui;

struct Sections {
    wifi: gtk::Box,
    radio: gtk::Switch,
    saved: gtk::Box,
    eth: gtk::Box,
}

pub fn build() -> gtk::Widget {
    let (scroller, content) = ui::page("Network");

    let (wifi_card, wifi_body) = ui::section("Wi-Fi");
    let radio = gtk::Switch::new();
    radio.set_halign(gtk::Align::End);
    let radio_row = ui::row("Wi-Fi radio", &radio);
    let rescan = gtk::Button::with_label("Rescan");
    radio_row.append(&rescan);
    wifi_body.append(&radio_row);
    let wifi_list = gtk::Box::new(gtk::Orientation::Vertical, 4);
    wifi_list.add_css_class("metis-settings-list");
    wifi_body.append(&wifi_list);
    content.append(&wifi_card);

    let (saved_card, saved_body) = ui::section("Known networks");
    content.append(&saved_card);

    let (eth_card, eth_body) = ui::section("Ethernet");
    content.append(&eth_card);

    let sections = Rc::new(Sections {
        wifi: wifi_list,
        radio: radio.clone(),
        saved: saved_body,
        eth: eth_body,
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
        sections.saved.append(&hint("No saved connections."));
    } else {
        for c in &snap.saved {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            let label = gtk::Label::new(Some(&format!("{}  ·  {}", c.name, c.ctype)));
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

    // ---- Ethernet ----
    clear(&sections.eth);
    if snap.eth.is_empty() {
        sections.eth.append(&hint("No Ethernet devices."));
    } else {
        for dev in &snap.eth {
            sections.eth.append(&ethernet_editor(dev, refresh));
        }
    }
}

fn ethernet_editor<F: Fn() + 'static>(dev: &net::EthDev, refresh: &Rc<F>) -> gtk::Widget {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 8);
    card.add_css_class("metis-settings-list");
    card.set_margin_top(4);

    let title = gtk::Label::new(Some(&format!(
        "{}  ·  {}",
        dev.device,
        if dev.connected { "connected" } else { "down" }
    )));
    title.set_xalign(0.0);
    card.append(&title);

    let Some(conn) = dev.connection.clone() else {
        card.append(&hint("No active profile for this device."));
        return card.upcast();
    };

    let method = gtk::DropDown::from_strings(&["Automatic (DHCP)", "Manual (static)"]);
    let is_manual = dev.ipv4.method == "manual";
    method.set_selected(if is_manual { 1 } else { 0 });
    card.append(&ui::row("IPv4 method", &method));

    let manual_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let addr = entry("192.168.1.50/24", &dev.ipv4.addresses);
    let gw = entry("192.168.1.1", &dev.ipv4.gateway);
    let dns = entry("1.1.1.1,8.8.8.8", &dev.ipv4.dns);
    manual_box.append(&ui::row("Address (CIDR)", &addr));
    manual_box.append(&ui::row("Gateway", &gw));
    manual_box.append(&ui::row("DNS", &dns));
    manual_box.set_visible(is_manual);
    card.append(&manual_box);

    {
        let manual_box = manual_box.clone();
        method.connect_selected_notify(move |dd| {
            manual_box.set_visible(dd.selected() == 1);
        });
    }

    let apply = gtk::Button::with_label("Apply");
    apply.set_halign(gtk::Align::End);
    card.append(&apply);
    {
        let refresh = refresh.clone();
        let conn = conn.clone();
        let method = method.clone();
        apply.connect_clicked(move |_| {
            if method.selected() == 1 {
                net::set_ipv4_static(&conn, &addr.text(), &gw.text(), &dns.text());
            } else {
                net::set_ipv4_dhcp(&conn);
            }
            schedule_refresh(&refresh, 2500);
        });
    }

    card.upcast()
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
