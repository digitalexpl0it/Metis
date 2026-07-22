//! Network: a pill-tabbed page splitting Wireless (Wi-Fi scan/connect/known
//! networks + DNS override), Wired (per-NIC IPv4 DHCP/static + DNS override),
//! VPN (NetworkManager OpenVPN / WireGuard), and Proxy (system proxy via GNOME
//! gsettings). All `nmcli`/`gsettings` work runs off the GTK main thread;
//! results arrive over an mpsc channel drained on a timeout.

use std::path::Path;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use gtk::prelude::*;

use crate::net::{self, NetSnapshot, VpnConn, VpnKind, WireGuardCreate, WireGuardProfile};
use crate::ui;

struct Sections {
    radio: gtk::Switch,
    wifi: gtk::Box,
    saved: gtk::Box,
    wifi_dns: gtk::Box,
    eth: gtk::Box,
    vpn: gtk::Box,
    vpn_status: gtk::Label,
    proxy: gtk::Box,
}

/// Build the Network page. `initial_tab` selects Wireless / Wired / VPN / Proxy
/// (`Some("vpn")` from `--page network/vpn`).
pub fn build(initial_tab: Option<&str>) -> gtk::Widget {
    let (scroller, content) = ui::page_for("network");

    let stack = gtk::Stack::new();
    stack.set_vexpand(true);
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    stack.set_transition_duration(120);

    let tabs = [
        ("wireless", "Wireless"),
        ("wired", "Wired"),
        ("vpn", "VPN"),
        ("proxy", "Proxy"),
    ];
    let initial = resolve_initial_tab(&tabs, initial_tab.unwrap_or("wireless"));

    // Pill buttons can be marked active before stack children exist; the
    // visible child is applied after all `add_named` calls below.
    content.append(&pill_tabs(&stack, &tabs, initial));
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

    // ---- VPN page ----
    let vpn_page = page_box();
    let (vpn_card, vpn_body) = ui::section("VPN connections");
    if !net::openvpn_plugin_present() {
        let plugin_hint = gtk::Label::new(Some(
            "OpenVPN import needs the NetworkManager plugin. Install with:\n\
             sudo apt install network-manager-openvpn",
        ));
        plugin_hint.set_xalign(0.0);
        plugin_hint.set_wrap(true);
        plugin_hint.add_css_class("metis-settings-error");
        plugin_hint.set_margin_bottom(8);
        vpn_body.append(&plugin_hint);
    }
    let vpn_actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    vpn_actions.add_css_class("metis-settings-actions");
    vpn_actions.set_halign(gtk::Align::Start);
    let import_btn = gtk::Button::with_label("Import…");
    import_btn.add_css_class("suggested-action");
    let add_wg_btn = gtk::Button::with_label("Add WireGuard…");
    vpn_actions.append(&import_btn);
    vpn_actions.append(&add_wg_btn);
    vpn_body.append(&vpn_actions);
    let vpn_status = gtk::Label::new(None);
    vpn_status.set_xalign(0.0);
    vpn_status.set_wrap(true);
    vpn_status.add_css_class("metis-settings-hint");
    vpn_status.set_visible(false);
    vpn_body.append(&vpn_status);
    let vpn_list = gtk::Box::new(gtk::Orientation::Vertical, 4);
    vpn_list.add_css_class("metis-settings-list");
    vpn_body.append(&vpn_list);
    vpn_page.append(&vpn_card);
    vpn_page.append(&hint(
        "Import provider .ovpn / WireGuard .conf files, or add a WireGuard peer. Saved NetworkManager VPN profiles appear here automatically.",
    ));
    stack.add_named(&vpn_page, Some("vpn"));

    // ---- Proxy page ----
    let proxy_page = page_box();
    let (proxy_card, proxy_body) = ui::section("System proxy");
    proxy_page.append(&proxy_card);
    stack.add_named(&proxy_page, Some("proxy"));
    stack.set_visible_child_name(initial);

    let sections = Rc::new(Sections {
        radio: radio.clone(),
        wifi: wifi_list,
        saved: saved_body,
        wifi_dns: wdns_body,
        eth: eth_body,
        vpn: vpn_list,
        vpn_status: vpn_status.clone(),
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
    {
        let refresh = refresh.clone();
        let status = vpn_status.clone();
        import_btn.connect_clicked(move |btn| {
            let parent = btn.root().and_downcast::<gtk::Window>();
            pick_vpn_import(parent.as_ref(), status.clone(), refresh.clone());
        });
    }
    {
        let refresh = refresh.clone();
        let status = vpn_status.clone();
        add_wg_btn.connect_clicked(move |btn| {
            let parent = btn.root().and_downcast::<gtk::Window>();
            show_wireguard_dialog(parent.as_ref(), status.clone(), refresh.clone());
        });
    }

    // Initial load + keep in sync when the edge-bar (or nmcli) toggles VPN.
    refresh();
    {
        let refresh = refresh.clone();
        glib::timeout_add_local(Duration::from_secs(2), move || {
            refresh();
            glib::ControlFlow::Continue
        });
    }

    scroller.upcast()
}

/// A vertical content box for a stack page (matches the page's own spacing).
fn page_box() -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 16);
    b.set_margin_top(8);
    b
}

fn resolve_initial_tab<'a>(tabs: &[(&'a str, &str)], requested: &'a str) -> &'a str {
    if tabs.iter().any(|(name, _)| *name == requested) {
        requested
    } else {
        tabs.first().map(|(n, _)| *n).unwrap_or("wireless")
    }
}

/// A segmented pill-tab bar that switches `stack` between named children.
/// Caller must call `stack.set_visible_child_name(initial)` after children exist.
fn pill_tabs(stack: &gtk::Stack, tabs: &[(&str, &str)], initial: &str) -> gtk::Box {
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    bar.add_css_class("metis-settings-tabs");
    bar.set_halign(gtk::Align::Center);

    let mut group: Option<gtk::ToggleButton> = None;
    for (name, label) in tabs {
        let btn = gtk::ToggleButton::with_label(label);
        btn.add_css_class("metis-settings-tab");
        match &group {
            Some(g) => btn.set_group(Some(g)),
            None => group = Some(btn.clone()),
        }
        if *name == initial {
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

    // ---- VPN ----
    clear(&sections.vpn);
    if snap.vpn.is_empty() {
        sections.vpn.append(&hint(
            "No VPN profiles yet. Import an .ovpn or WireGuard .conf, or add WireGuard manually.",
        ));
    } else {
        for vpn in &snap.vpn {
            sections
                .vpn
                .append(&vpn_row(vpn, refresh, &sections.vpn_status));
        }
    }

    // ---- Proxy ----
    clear(&sections.proxy);
    sections.proxy.append(&proxy_editor(&snap.proxy, refresh));
}

fn vpn_row<F: Fn() + 'static>(
    vpn: &VpnConn,
    refresh: &Rc<F>,
    status: &gtk::Label,
) -> gtk::Widget {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.set_valign(gtk::Align::Center);
    if vpn.active {
        row.add_css_class("metis-settings-row-active");
    }

    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    text.set_valign(gtk::Align::Center);
    let name = gtk::Label::new(Some(&vpn.name));
    name.set_xalign(0.0);
    text.append(&name);
    let mut meta = vpn.kind.label().to_string();
    if vpn.active {
        meta.push_str(" · Connected");
    }
    let kind = gtk::Label::new(Some(&meta));
    kind.set_xalign(0.0);
    kind.add_css_class("metis-settings-hint");
    text.append(&kind);
    row.append(&text);

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    actions.set_valign(gtk::Align::Center);
    actions.set_halign(gtk::Align::End);

    let auto_lbl = gtk::Label::new(Some("Auto-connect"));
    auto_lbl.add_css_class("metis-settings-hint");
    auto_lbl.set_valign(gtk::Align::Center);
    auto_lbl.set_tooltip_text(Some(
        "Connect this VPN after login when the network is ready. Only one profile can auto-connect.",
    ));
    actions.append(&auto_lbl);
    let auto = gtk::Switch::new();
    auto.set_active(vpn.autoconnect);
    auto.set_valign(gtk::Align::Center);
    auto.set_tooltip_text(Some(
        "Connect this VPN after login when the network is ready. Only one profile can auto-connect.",
    ));
    {
        let refresh = refresh.clone();
        let status = status.clone();
        let uuid = vpn.uuid.clone();
        let was_active = vpn.active;
        auto.connect_state_set(move |sw, on| {
            let uuid_for_set = uuid.clone();
            let uuid = uuid.clone();
            let status = status.clone();
            let refresh = refresh.clone();
            let sw = sw.clone();
            let (tx, rx) = mpsc::channel::<Result<(), String>>();
            std::thread::spawn(move || {
                let _ = tx.send(net::vpn_set_autoconnect(&uuid_for_set, on));
            });
            glib::timeout_add_local(Duration::from_millis(50), move || {
                match rx.try_recv() {
                    Ok(Ok(())) => {
                        set_vpn_status(
                            &status,
                            if on {
                                "Autoconnect enabled (only this profile)."
                            } else {
                                "Autoconnect disabled."
                            },
                            false,
                        );
                        if on && !was_active {
                            // Connect now so the setting takes effect without
                            // waiting for the next login.
                            let uuid = uuid.clone();
                            let status = status.clone();
                            let refresh = refresh.clone();
                            set_vpn_status(&status, "Connecting…", false);
                            let (tx2, rx2) = mpsc::channel::<Result<(), String>>();
                            std::thread::spawn(move || {
                                let _ = tx2.send(net::vpn_up(&uuid));
                            });
                            glib::timeout_add_local(Duration::from_millis(50), move || {
                                match rx2.try_recv() {
                                    Ok(Ok(())) => {
                                        set_vpn_status(&status, "Connected.", false);
                                        refresh();
                                        glib::ControlFlow::Break
                                    }
                                    Ok(Err(e)) => {
                                        set_vpn_status(&status, &e, true);
                                        refresh();
                                        glib::ControlFlow::Break
                                    }
                                    Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                                    Err(mpsc::TryRecvError::Disconnected) => {
                                        refresh();
                                        glib::ControlFlow::Break
                                    }
                                }
                            });
                        } else {
                            refresh();
                        }
                        glib::ControlFlow::Break
                    }
                    Ok(Err(e)) => {
                        sw.set_active(!on);
                        set_vpn_status(&status, &e, true);
                        glib::ControlFlow::Break
                    }
                    Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        sw.set_active(!on);
                        glib::ControlFlow::Break
                    }
                }
            });
            glib::Propagation::Proceed
        });
    }
    actions.append(&auto);

    if vpn.kind == VpnKind::WireGuard {
        let edit = gtk::Button::with_label("Edit");
        edit.set_valign(gtk::Align::Center);
        {
            let refresh = refresh.clone();
            let status = status.clone();
            let uuid = vpn.uuid.clone();
            edit.connect_clicked(move |btn| {
                let parent = btn.root().and_downcast::<gtk::Window>();
                show_wireguard_edit_dialog(
                    parent.as_ref(),
                    &uuid,
                    status.clone(),
                    refresh.clone(),
                );
            });
        }
        actions.append(&edit);
    }

    if vpn.active {
        let disconnect = gtk::Button::with_label("Disconnect");
        disconnect.set_valign(gtk::Align::Center);
        {
            let refresh = refresh.clone();
            let status = status.clone();
            let uuid = vpn.uuid.clone();
            disconnect.connect_clicked(move |btn| {
                run_vpn_toggle(false, &uuid, btn, &status, &refresh);
            });
        }
        actions.append(&disconnect);
    } else {
        let connect = gtk::Button::with_label("Connect");
        connect.add_css_class("suggested-action");
        connect.set_valign(gtk::Align::Center);
        {
            let refresh = refresh.clone();
            let status = status.clone();
            let uuid = vpn.uuid.clone();
            connect.connect_clicked(move |btn| {
                run_vpn_toggle(true, &uuid, btn, &status, &refresh);
            });
        }
        actions.append(&connect);
    }

    let delete = gtk::Button::with_label("Delete");
    delete.add_css_class("destructive-action");
    delete.set_valign(gtk::Align::Center);
    {
        let refresh = refresh.clone();
        let uuid = vpn.uuid.clone();
        delete.connect_clicked(move |_| {
            net::vpn_delete(&uuid);
            schedule_refresh(&refresh, 1200);
        });
    }
    actions.append(&delete);
    row.append(&actions);
    row.upcast()
}

fn run_vpn_toggle<F: Fn() + 'static>(
    connect: bool,
    uuid: &str,
    btn: &gtk::Button,
    status: &gtk::Label,
    refresh: &Rc<F>,
) {
    let busy = if connect { "Connecting…" } else { "Disconnecting…" };
    let done_label = if connect { "Connect" } else { "Disconnect" };
    btn.set_sensitive(false);
    btn.set_label(busy);
    set_vpn_status(status, busy, false);

    let uuid = uuid.to_string();
    let (tx, rx) = mpsc::channel::<Result<(), String>>();
    std::thread::spawn(move || {
        let result = if connect {
            net::vpn_up(&uuid)
        } else {
            net::vpn_down(&uuid)
        };
        let _ = tx.send(result);
    });

    let btn = btn.clone();
    let status = status.clone();
    let refresh = refresh.clone();
    glib::timeout_add_local(Duration::from_millis(50), move || {
        match rx.try_recv() {
            Ok(Ok(())) => {
                set_vpn_status(
                    &status,
                    if connect {
                        "Connected."
                    } else {
                        "Disconnected."
                    },
                    false,
                );
                refresh();
                glib::ControlFlow::Break
            }
            Ok(Err(e)) => {
                set_vpn_status(&status, &e, true);
                btn.set_sensitive(true);
                btn.set_label(done_label);
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => {
                btn.set_sensitive(true);
                btn.set_label(done_label);
                glib::ControlFlow::Break
            }
        }
    });
}

fn set_vpn_status(label: &gtk::Label, msg: &str, error: bool) {
    label.set_text(msg);
    label.set_visible(!msg.is_empty());
    if error {
        label.add_css_class("metis-settings-error");
        label.remove_css_class("metis-settings-hint");
    } else {
        label.remove_css_class("metis-settings-error");
        label.add_css_class("metis-settings-hint");
    }
}

fn pick_vpn_import(parent: Option<&gtk::Window>, status: gtk::Label, refresh: Rc<impl Fn() + 'static>) {
    let dialog = gtk::FileDialog::new();
    dialog.set_title("Import VPN profile");
    let filter = gtk::FileFilter::new();
    filter.set_name(Some("VPN configs (*.ovpn, *.conf)"));
    filter.add_pattern("*.ovpn");
    filter.add_pattern("*.OVPN");
    filter.add_pattern("*.conf");
    filter.add_pattern("*.CONF");
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    dialog.set_filters(Some(&filters));

    dialog.open(parent, gio::Cancellable::NONE, move |res| {
        let Ok(file) = res else { return };
        let Some(path) = file.path() else { return };
        let path_s = path.to_string_lossy().to_string();
        set_vpn_status(&status, "Importing…", false);
        // Brief main-thread wait — import is a single nmcli call.
        match import_vpn_file(&path_s) {
            Ok(name) => {
                set_vpn_status(&status, &format!("Imported “{name}”."), false);
                refresh();
            }
            Err(err) => set_vpn_status(&status, &err, true),
        }
    });
}

fn import_vpn_file(path: &str) -> Result<String, String> {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "ovpn" => net::vpn_import_openvpn(path),
        "conf" => net::vpn_import_wireguard(path),
        _ => {
            // Try OpenVPN first, then WireGuard.
            net::vpn_import_openvpn(path).or_else(|_| net::vpn_import_wireguard(path))
        }
    }
}

fn show_wireguard_dialog(
    parent: Option<&gtk::Window>,
    status: gtk::Label,
    refresh: Rc<impl Fn() + 'static>,
) {
    let Some(parent) = parent else {
        set_vpn_status(&status, "Could not open WireGuard dialog (no parent window).", true);
        return;
    };

    // Undecorated so Metis does not paint a second compositor titlebar over the
    // in-dialog close control (same pattern as Remote / Gaming / Desktop widgets).
    let dialog = gtk::Window::builder()
        .title("Add WireGuard")
        .modal(true)
        .transient_for(parent)
        .decorated(false)
        .resizable(false)
        .default_width(480)
        .build();
    dialog.add_css_class("metis-settings-window");
    dialog.add_css_class("metis-settings-password-dialog");

    let outer = gtk::Box::new(gtk::Orientation::Vertical, 10);
    outer.set_margin_top(16);
    outer.set_margin_bottom(16);
    outer.set_margin_start(20);
    outer.set_margin_end(20);

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    header.set_margin_bottom(4);
    let heading = gtk::Label::new(Some("Add WireGuard connection"));
    heading.set_xalign(0.0);
    heading.set_hexpand(true);
    heading.add_css_class("metis-settings-section-title");
    header.append(&heading);
    outer.append(&header);

    // Vertical fields (not `ui::row`): keeps focus inside the entry, avoids the
    // card-row hexpand fight that made CIDR typing feel sticky, and matches the
    // Remote password dialog pattern.
    let name = wg_entry("Home VPN", "");
    let private_key = wg_entry("Interface private key", "");
    let address = wg_entry("10.0.0.2/32", "");
    let peer_pub = wg_entry("Peer public key", "");
    let endpoint = wg_entry("vpn.example.com:51820", "");
    let allowed = wg_entry("0.0.0.0/0, ::/0", "0.0.0.0/0, ::/0");
    let dns = wg_entry("1.1.1.1", "");

    outer.append(&wg_field("Name", &name));
    outer.append(&wg_field("Private key", &private_key));
    outer.append(&wg_field("Address (CIDR)", &address));
    outer.append(&wg_field("Peer public key", &peer_pub));
    outer.append(&wg_field("Endpoint", &endpoint));
    outer.append(&wg_field("Allowed IPs", &allowed));
    outer.append(&wg_field("DNS (optional)", &dns));

    let err = gtk::Label::new(None);
    err.set_xalign(0.0);
    err.set_wrap(true);
    err.add_css_class("metis-settings-error");
    err.set_visible(false);
    outer.append(&err);

    let btn_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    btn_row.set_halign(gtk::Align::End);
    btn_row.set_margin_top(4);
    let cancel = gtk::Button::with_label("Cancel");
    let create = gtk::Button::with_label("Create");
    create.add_css_class("suggested-action");
    btn_row.append(&cancel);
    btn_row.append(&create);
    outer.append(&btn_row);

    dialog.set_child(Some(&outer));

    {
        let dialog = dialog.clone();
        cancel.connect_clicked(move |_| dialog.close());
    }
    {
        let dialog = dialog.clone();
        let status = status.clone();
        let create_btn = create.clone();
        let name = name.clone();
        let private_key = private_key.clone();
        let address = address.clone();
        let peer_pub = peer_pub.clone();
        let endpoint = endpoint.clone();
        let allowed = allowed.clone();
        let dns = dns.clone();
        let err = err.clone();
        let refresh = refresh.clone();
        create.connect_clicked(move |_| {
            create_btn.set_sensitive(false);
            create_btn.set_label("Creating…");
            err.set_visible(false);

            let cfg = WireGuardCreate {
                name: name.text().to_string(),
                private_key: private_key.text().to_string(),
                address: address.text().to_string(),
                peer_public_key: peer_pub.text().to_string(),
                endpoint: endpoint.text().to_string(),
                allowed_ips: allowed.text().to_string(),
                dns: dns.text().to_string(),
            };

            // nmcli can take several seconds — never block the GTK thread (that
            // froze Settings and made the edge bar look dead).
            let (tx, rx) = mpsc::channel::<Result<(), String>>();
            std::thread::spawn(move || {
                let _ = tx.send(net::vpn_create_wireguard(cfg));
            });

            let dialog = dialog.clone();
            let status = status.clone();
            let refresh = refresh.clone();
            let err = err.clone();
            let create_btn = create_btn.clone();
            glib::timeout_add_local(Duration::from_millis(50), move || {
                match rx.try_recv() {
                    Ok(Ok(())) => {
                        set_vpn_status(&status, "WireGuard connection created.", false);
                        refresh();
                        dialog.close();
                        glib::ControlFlow::Break
                    }
                    Ok(Err(e)) => {
                        err.set_text(&e);
                        err.set_visible(true);
                        create_btn.set_sensitive(true);
                        create_btn.set_label("Create");
                        glib::ControlFlow::Break
                    }
                    Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        create_btn.set_sensitive(true);
                        create_btn.set_label("Create");
                        glib::ControlFlow::Break
                    }
                }
            });
        });
    }

    dialog.present();
    // Wayland often needs a tick after present before grab_focus sticks; without
    // it, keystrokes land in the Settings sidebar search and thrash the nav filter.
    let focus_entry = name.clone();
    glib::idle_add_local_once(move || {
        focus_entry.grab_focus();
    });
}

fn show_wireguard_edit_dialog(
    parent: Option<&gtk::Window>,
    uuid: &str,
    status: gtk::Label,
    refresh: Rc<impl Fn() + 'static>,
) {
    let Some(parent) = parent else {
        set_vpn_status(&status, "Could not open edit dialog (no parent window).", true);
        return;
    };
    let Some(profile) = net::vpn_get_wireguard(uuid) else {
        set_vpn_status(
            &status,
            "Could not load WireGuard profile details.",
            true,
        );
        return;
    };

    let dialog = gtk::Window::builder()
        .title("Edit WireGuard")
        .modal(true)
        .transient_for(parent)
        .decorated(false)
        .resizable(false)
        .default_width(480)
        .build();
    dialog.add_css_class("metis-settings-window");
    dialog.add_css_class("metis-settings-password-dialog");

    let outer = gtk::Box::new(gtk::Orientation::Vertical, 10);
    outer.set_margin_top(16);
    outer.set_margin_bottom(16);
    outer.set_margin_start(20);
    outer.set_margin_end(20);

    let heading = gtk::Label::new(Some("Edit WireGuard connection"));
    heading.set_xalign(0.0);
    heading.add_css_class("metis-settings-section-title");
    outer.append(&heading);

    let name = wg_entry("Home VPN", &profile.name);
    let address = wg_entry("10.0.0.2/32", &profile.address);
    let peer_pub = wg_entry("Peer public key", &profile.peer_public_key);
    let endpoint = wg_entry("vpn.example.com:51820", &profile.endpoint);
    let allowed = wg_entry(
        "0.0.0.0/0, ::/0",
        if profile.allowed_ips.is_empty() {
            "0.0.0.0/0, ::/0"
        } else {
            &profile.allowed_ips
        },
    );
    let dns = wg_entry("1.1.1.1", &profile.dns);

    outer.append(&wg_field("Name", &name));
    outer.append(&wg_field("Address (CIDR)", &address));
    outer.append(&wg_field("Peer public key", &peer_pub));
    outer.append(&wg_field("Endpoint", &endpoint));
    outer.append(&wg_field("Allowed IPs", &allowed));
    outer.append(&wg_field("DNS (optional)", &dns));

    let hint = gtk::Label::new(Some(
        "Private key is unchanged. Disconnect and reconnect for peer/address edits to take effect.",
    ));
    hint.set_xalign(0.0);
    hint.set_wrap(true);
    hint.add_css_class("metis-settings-hint");
    outer.append(&hint);

    let err = gtk::Label::new(None);
    err.set_xalign(0.0);
    err.set_wrap(true);
    err.add_css_class("metis-settings-error");
    err.set_visible(false);
    outer.append(&err);

    let btn_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    btn_row.set_halign(gtk::Align::End);
    btn_row.set_margin_top(4);
    let cancel = gtk::Button::with_label("Cancel");
    let save = gtk::Button::with_label("Save");
    save.add_css_class("suggested-action");
    btn_row.append(&cancel);
    btn_row.append(&save);
    outer.append(&btn_row);

    dialog.set_child(Some(&outer));

    {
        let dialog = dialog.clone();
        cancel.connect_clicked(move |_| dialog.close());
    }
    {
        let dialog = dialog.clone();
        let status = status.clone();
        let save_btn = save.clone();
        let uuid = uuid.to_string();
        let name = name.clone();
        let address = address.clone();
        let peer_pub = peer_pub.clone();
        let endpoint = endpoint.clone();
        let allowed = allowed.clone();
        let dns = dns.clone();
        let err = err.clone();
        let refresh = refresh.clone();
        save.connect_clicked(move |_| {
            save_btn.set_sensitive(false);
            save_btn.set_label("Saving…");
            err.set_visible(false);

            let cfg = WireGuardProfile {
                name: name.text().to_string(),
                address: address.text().to_string(),
                peer_public_key: peer_pub.text().to_string(),
                endpoint: endpoint.text().to_string(),
                allowed_ips: allowed.text().to_string(),
                dns: dns.text().to_string(),
            };
            let uuid = uuid.clone();
            let (tx, rx) = mpsc::channel::<Result<(), String>>();
            std::thread::spawn(move || {
                let _ = tx.send(net::vpn_update_wireguard(&uuid, cfg));
            });

            let dialog = dialog.clone();
            let status = status.clone();
            let refresh = refresh.clone();
            let err = err.clone();
            let save_btn = save_btn.clone();
            glib::timeout_add_local(Duration::from_millis(50), move || {
                match rx.try_recv() {
                    Ok(Ok(())) => {
                        set_vpn_status(&status, "WireGuard profile updated.", false);
                        refresh();
                        dialog.close();
                        glib::ControlFlow::Break
                    }
                    Ok(Err(e)) => {
                        err.set_text(&e);
                        err.set_visible(true);
                        save_btn.set_sensitive(true);
                        save_btn.set_label("Save");
                        glib::ControlFlow::Break
                    }
                    Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        save_btn.set_sensitive(true);
                        save_btn.set_label("Save");
                        glib::ControlFlow::Break
                    }
                }
            });
        });
    }

    dialog.present();
    let focus_entry = name.clone();
    glib::idle_add_local_once(move || {
        focus_entry.grab_focus();
    });
}

fn wg_entry(placeholder: &str, value: &str) -> gtk::Entry {
    let e = gtk::Entry::builder()
        .placeholder_text(placeholder)
        .hexpand(true)
        .build();
    e.set_text(value);
    // Held Backspace on an empty field must not bubble to the sidebar search.
    ui::swallow_empty_backspace(&e);
    e
}

fn wg_field(label: &str, entry: &gtk::Entry) -> gtk::Box {
    let box_ = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let lbl = gtk::Label::new(Some(label));
    lbl.set_xalign(0.0);
    lbl.add_css_class("metis-settings-hint");
    box_.append(&lbl);
    box_.append(entry);
    box_
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
