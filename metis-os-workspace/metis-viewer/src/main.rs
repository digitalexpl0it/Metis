//! Metis Viewer — GTK4 RDP connect UI over FreeRDP (host remains GRD).

mod freerdp;

use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;
use metis_config::{remember_host, ViewerHost};
use metis_i18n::tr;

#[derive(Debug, Clone, Default)]
struct CliPrefill {
    host: Option<String>,
    port: Option<u16>,
    user: Option<String>,
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "metis_viewer=info,warn".into()),
        )
        .init();

    metis_i18n::init();

    let prefill = parse_cli();

    let app = gtk::Application::builder()
        .application_id("com.metis.Viewer")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(move |app| {
        build_ui(app, prefill.clone());
    });

    // Hand GApplication only argv0 so custom flags are not rejected.
    let argv0: Vec<String> = std::env::args().take(1).collect();
    app.run_with_args(&argv0);
}

fn parse_cli() -> CliPrefill {
    let mut out = CliPrefill::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--host" => out.host = args.next(),
            "--port" => {
                if let Some(p) = args.next() {
                    out.port = p.parse().ok();
                }
            }
            "--user" | "--username" => out.user = args.next(),
            "-h" | "--help" => {
                eprintln!(
                    "Usage: metis-viewer [--host HOST] [--port PORT] [--user USER]\n\
                     Connect to an RDP host via FreeRDP (wlfreerdp3 / xfreerdp…)."
                );
                std::process::exit(0);
            }
            other => {
                eprintln!("metis-viewer: unknown argument {other} (try --help)");
                std::process::exit(2);
            }
        }
    }
    out
}

fn build_ui(app: &gtk::Application, prefill: CliPrefill) {
    if let Some(win) = app.active_window() {
        win.present();
        return;
    }

    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title(&tr("Metis Viewer"))
        .default_width(420)
        .default_height(480)
        .resizable(true)
        .build();

    let outer = gtk::Box::new(gtk::Orientation::Vertical, 12);
    outer.set_margin_top(16);
    outer.set_margin_bottom(16);
    outer.set_margin_start(16);
    outer.set_margin_end(16);

    let intro = gtk::Label::new(Some(&tr(
        "Connect to a Metis (or any) RDP host. Sharing on the host is \
         Settings → Remote access; this app is the client.",
    )));
    intro.set_xalign(0.0);
    intro.set_wrap(true);
    intro.add_css_class("dim-label");
    outer.append(&intro);

    let host_entry = gtk::Entry::new();
    host_entry.set_placeholder_text(Some(&tr("Host or IP")));
    if let Some(h) = &prefill.host {
        host_entry.set_text(h);
    }

    let port_entry = gtk::Entry::new();
    port_entry.set_placeholder_text(Some("3389"));
    port_entry.set_input_purpose(gtk::InputPurpose::Digits);
    port_entry.set_max_length(5);
    port_entry.set_width_chars(6);
    port_entry.set_text(
        &prefill
            .port
            .unwrap_or(3389)
            .to_string(),
    );

    let user_entry = gtk::Entry::new();
    user_entry.set_placeholder_text(Some(&tr("Username")));
    if let Some(u) = &prefill.user {
        user_entry.set_text(u);
    } else if let Ok(u) = std::env::var("USER") {
        if !u.is_empty() {
            user_entry.set_text(&u);
        }
    }

    let pass_entry = gtk::PasswordEntry::new();
    pass_entry.set_show_peek_icon(true);
    pass_entry.set_placeholder_text(Some(&tr("Password (optional — FreeRDP may prompt)")));

    outer.append(&labeled_field(&tr("Host"), &host_entry));
    outer.append(&labeled_field(&tr("Port"), &port_entry));
    outer.append(&labeled_field(&tr("Username"), &user_entry));
    outer.append(&labeled_field(&tr("Password"), pass_entry.upcast_ref::<gtk::Widget>()));

    let status = gtk::Label::new(None);
    status.set_xalign(0.0);
    status.set_wrap(true);
    status.add_css_class("dim-label");

    match freerdp::resolve_freerdp() {
        Some(bin) => {
            status.set_text(&format!("{} {}", tr("Using"), bin.display()));
        }
        None => {
            status.set_text(&freerdp::freerdp_install_hint());
            status.remove_css_class("dim-label");
            status.add_css_class("error");
        }
    }
    outer.append(&status);

    let connect_btn = gtk::Button::with_label(&tr("Connect"));
    connect_btn.add_css_class("suggested-action");
    connect_btn.set_halign(gtk::Align::Start);
    outer.append(&connect_btn);

    let recent_label = gtk::Label::new(Some(&tr("Recent")));
    recent_label.set_xalign(0.0);
    recent_label.set_margin_top(8);
    outer.append(&recent_label);

    let recent_list = gtk::ListBox::new();
    recent_list.set_selection_mode(gtk::SelectionMode::None);
    recent_list.add_css_class("boxed-list");
    let recent_scroll = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .hexpand(true)
        .min_content_height(120)
        .child(&recent_list)
        .build();
    outer.append(&recent_scroll);

    let host_for_btn = host_entry.clone();
    let port_for_btn = port_entry.clone();
    let user_for_btn = user_entry.clone();
    let pass_for_btn = pass_entry.clone();
    let status_for_btn = status.clone();
    let recent_for_btn = recent_list.clone();
    let connect_busy = Rc::new(RefCell::new(false));

    connect_btn.connect_clicked(move |_| {
        if *connect_busy.borrow() {
            return;
        }
        *connect_busy.borrow_mut() = true;

        let host = host_for_btn.text().to_string();
        let port_text = port_for_btn.text().to_string();
        let username = user_for_btn.text().to_string();
        let password = pass_for_btn.text().to_string();

        let port: u16 = match port_text.trim().parse() {
            Ok(0) | Err(_) => {
                status_for_btn.set_text(&tr("Enter a valid port (1–65535)."));
                *connect_busy.borrow_mut() = false;
                return;
            }
            Ok(p) => p,
        };

        let req = freerdp::ConnectRequest {
            host: host.clone(),
            port,
            username: username.clone(),
            password: if password.is_empty() {
                None
            } else {
                Some(password)
            },
        };

        match freerdp::spawn_freerdp(&req) {
            Ok(bin) => {
                status_for_btn.set_text(&format!(
                    "{} {} — {}",
                    tr("Started"),
                    bin.display(),
                    tr("password is never saved")
                ));
                let entry = ViewerHost {
                    host: host.trim().to_string(),
                    port,
                    username: username.trim().to_string(),
                };
                if let Err(e) = remember_host(entry) {
                    tracing::warn!("viewer.json save failed: {e}");
                }
                refill_recent(&recent_for_btn, &host_for_btn, &port_for_btn, &user_for_btn);
            }
            Err(e) => {
                status_for_btn.set_text(&e);
            }
        }
        *connect_busy.borrow_mut() = false;
    });

    refill_recent(&recent_list, &host_entry, &port_entry, &user_entry);

    window.set_child(Some(&outer));
    window.present();
}

fn labeled_field(title: &str, widget: &impl IsA<gtk::Widget>) -> gtk::Box {
    let col = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let label = gtk::Label::new(Some(title));
    label.set_xalign(0.0);
    label.add_css_class("caption-heading");
    col.append(&label);
    col.append(widget);
    col
}

fn refill_recent(
    list: &gtk::ListBox,
    host_entry: &gtk::Entry,
    port_entry: &gtk::Entry,
    user_entry: &gtk::Entry,
) {
    while let Some(row) = list.row_at_index(0) {
        list.remove(&row);
    }
    let cfg = metis_config::load_viewer_config();
    if cfg.recent.is_empty() {
        let empty = gtk::Label::new(Some(&tr("No recent hosts yet.")));
        empty.set_xalign(0.0);
        empty.add_css_class("dim-label");
        empty.set_margin_top(8);
        empty.set_margin_bottom(8);
        empty.set_margin_start(8);
        empty.set_margin_end(8);
        list.append(&empty);
        return;
    }
    for entry in cfg.recent {
        let row = gtk::ListBoxRow::new();
        let btn = gtk::Button::new();
        btn.set_has_frame(false);
        let label = gtk::Label::new(Some(&format!(
            "{}:{}  {}",
            entry.host, entry.port, entry.username
        )));
        label.set_xalign(0.0);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        btn.set_child(Some(&label));
        let h = host_entry.clone();
        let p = port_entry.clone();
        let u = user_entry.clone();
        let e = entry.clone();
        btn.connect_clicked(move |_| {
            h.set_text(&e.host);
            p.set_text(&e.port.to_string());
            u.set_text(&e.username);
        });
        row.set_child(Some(&btn));
        list.append(&row);
    }
}
