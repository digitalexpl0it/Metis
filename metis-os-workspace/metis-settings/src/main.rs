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

/// Sidebar pages, in display order: `(id, title, symbolic icon)`. The
/// `--page <name>` flag preselects one.
const PAGES: &[(&str, &str, &str)] = &[
    ("appearance", "Appearance", "preferences-desktop-appearance-symbolic"),
    ("startmenu", "Start Menu", "view-app-grid-symbolic"),
    ("weather", "Weather", "weather-few-clouds-symbolic"),
    ("network", "Network", "network-wireless-symbolic"),
    ("calendars", "Calendars", "x-office-calendar-symbolic"),
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
        .find(|(id, _, _)| *id == name)
        .map(|(id, _, _)| id.to_string())
}

fn build_ui(app: &Application, page: Option<String>) {
    theme::install();

    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    stack.set_hexpand(true);
    stack.set_vexpand(true);

    stack.add_titled(&pages::appearance::build(), Some("appearance"), "Appearance");
    stack.add_titled(&pages::startmenu::build(), Some("startmenu"), "Start Menu");
    stack.add_titled(&pages::weather::build(), Some("weather"), "Weather");
    stack.add_titled(&pages::network::build(), Some("network"), "Network");
    stack.add_titled(&pages::calendars::build(), Some("calendars"), "Calendars");

    // Custom icon + label sidebar (GtkStackSidebar is title-only). A GtkListBox
    // drives the stack; row index maps 1:1 to `PAGES`.
    let nav = gtk::ListBox::new();
    nav.set_selection_mode(gtk::SelectionMode::Single);
    for (_, title, icon) in PAGES {
        let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        let img = gtk::Image::from_icon_name(icon);
        let label = gtk::Label::new(Some(title));
        label.set_xalign(0.0);
        row_box.append(&img);
        row_box.append(&label);
        let row = gtk::ListBoxRow::new();
        row.set_child(Some(&row_box));
        nav.append(&row);
    }
    {
        let stack = stack.clone();
        nav.connect_row_selected(move |_, row| {
            if let Some(row) = row {
                if let Some((id, _, _)) = PAGES.get(row.index() as usize) {
                    stack.set_visible_child_name(id);
                }
            }
        });
    }

    let sidebar = gtk::Box::new(gtk::Orientation::Vertical, 0);
    sidebar.add_css_class("metis-settings-sidebar");
    sidebar.set_width_request(180);
    sidebar.append(&nav);

    let layout = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    layout.append(&sidebar);
    layout.append(&gtk::Separator::new(gtk::Orientation::Vertical));
    layout.append(&stack);
    layout.add_css_class("metis-settings-root");

    // Preselect the requested (or first) page; selecting the row switches the stack.
    let initial = page
        .as_deref()
        .and_then(|p| PAGES.iter().position(|(id, _, _)| *id == p))
        .unwrap_or(0);
    if let Some(row) = nav.row_at_index(initial as i32) {
        nav.select_row(Some(&row));
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
