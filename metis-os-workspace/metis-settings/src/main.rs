//! Metis Settings — a standalone GTK4 app for configuring appearance, weather,
//! network, and calendars. Reads/writes the shared `~/.config/metis/*.json` via
//! the `metis-config` crate; the running shell picks up changes through its file
//! watchers (or an explicit `reload-*` runtime command).

mod msauth;
mod net;
mod pages;
mod runtime;
mod theme;
mod ui;

use gtk::prelude::*;
use gtk::{Application, ApplicationWindow};

const APP_ID: &str = "com.metis.Settings";

/// Sidebar pages, in display order. The `--page <name>` flag preselects one.
const PAGES: &[(&str, &str)] = &[
    ("appearance", "Appearance"),
    ("weather", "Weather"),
    ("network", "Network"),
    ("calendars", "Calendars"),
];

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "metis_settings=info,warn".into()),
        )
        .init();

    let page = parse_page_arg();

    let app = Application::builder().application_id(APP_ID).build();
    app.connect_activate(move |app| build_ui(app, page.clone()));
    // Run with an empty arg list so GTK doesn't try to parse our `--page` flag.
    let empty: [&str; 0] = [];
    app.run_with_args(&empty);
}

/// Parse `--page <name>` / `--page=<name>` from the process args.
fn parse_page_arg() -> Option<String> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if let Some(name) = arg.strip_prefix("--page=") {
            return normalize_page(name);
        }
        if arg == "--page" {
            if let Some(name) = args.next() {
                return normalize_page(&name);
            }
        }
    }
    None
}

fn normalize_page(name: &str) -> Option<String> {
    let name = name.trim().to_lowercase();
    PAGES
        .iter()
        .find(|(id, _)| *id == name)
        .map(|(id, _)| id.to_string())
}

fn build_ui(app: &Application, page: Option<String>) {
    theme::install();

    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    stack.set_hexpand(true);
    stack.set_vexpand(true);

    stack.add_titled(&pages::appearance::build(), Some("appearance"), "Appearance");
    stack.add_titled(&pages::weather::build(), Some("weather"), "Weather");
    stack.add_titled(&pages::network::build(), Some("network"), "Network");
    stack.add_titled(&pages::calendars::build(), Some("calendars"), "Calendars");

    let sidebar = gtk::StackSidebar::new();
    sidebar.set_stack(&stack);
    sidebar.set_width_request(180);
    sidebar.add_css_class("metis-settings-sidebar");

    let layout = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    layout.append(&sidebar);
    layout.append(&gtk::Separator::new(gtk::Orientation::Vertical));
    layout.append(&stack);
    layout.add_css_class("metis-settings-root");

    if let Some(page) = page {
        stack.set_visible_child_name(&page);
    }

    // Inside a Metis session the compositor draws the server-side titlebar +
    // window controls, so suppress GTK's own client-side titlebar to avoid a
    // doubled-up frame. On the host (no Metis), keep GTK's titlebar so the window
    // stays movable/closable.
    let under_metis = std::env::var_os("METIS_SESSION").is_some();
    let window = ApplicationWindow::builder()
        .application(app)
        .title("Metis Settings")
        .default_width(880)
        .default_height(640)
        .decorated(!under_metis)
        .build();
    window.add_css_class("metis-settings-window");
    window.set_child(Some(&layout));
    window.present();
}
