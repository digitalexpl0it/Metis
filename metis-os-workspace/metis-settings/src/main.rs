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

use gtk::glib;
use gtk::prelude::*;

/// One sidebar row: either a section header or a navigable page.
struct NavItem {
    page_id: Option<&'static str>,
    title: &'static str,
    icon: Option<&'static str>,
}

const NAV: &[NavItem] = &[
    NavItem {
        page_id: None,
        title: "Personalization",
        icon: None,
    },
    NavItem {
        page_id: Some("appearance"),
        title: "Appearance",
        icon: Some("preferences-desktop-appearance-symbolic"),
    },
    NavItem {
        page_id: Some("menu"),
        title: "Metis Menu",
        icon: Some("view-app-grid-symbolic"),
    },
    NavItem {
        page_id: Some("weather"),
        title: "Weather",
        icon: Some("weather-few-clouds-symbolic"),
    },
    NavItem {
        page_id: Some("network"),
        title: "Network",
        icon: Some("network-wireless-symbolic"),
    },
    NavItem {
        page_id: Some("calendars"),
        title: "Calendars",
        icon: Some("x-office-calendar-symbolic"),
    },
    NavItem {
        page_id: None,
        title: "Input",
        icon: None,
    },
    NavItem {
        page_id: Some("mouse"),
        title: "Mouse",
        icon: Some("input-mouse-symbolic"),
    },
    NavItem {
        page_id: Some("touchpad"),
        title: "Touchpad",
        icon: Some("input-touchpad-symbolic"),
    },
    NavItem {
        page_id: Some("keyboard"),
        title: "Keyboard",
        icon: Some("input-keyboard-symbolic"),
    },
];

fn page_ids() -> Vec<&'static str> {
    NAV.iter()
        .filter_map(|item| item.page_id)
        .collect()
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "metis_settings=info,warn".into()),
        )
        .init();

    let page = parse_page_arg();

    // Same rationale as metis-shell: GtkApplication startup does a sync portal
    // proxy that blocks ~25s when xdg-desktop-portal cold-starts in a bare session.
    if let Err(err) = gtk::init() {
        tracing::error!(?err, "gtk::init() failed");
        std::process::exit(1);
    }
    build_ui(page);
    glib::MainLoop::new(None, false).run();
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
    page_ids()
        .into_iter()
        .find(|id| *id == name)
        .map(str::to_string)
}

fn build_ui(page: Option<String>) {
    theme::install();

    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    stack.set_hexpand(true);
    stack.set_vexpand(true);

    stack.add_titled(&pages::appearance::build(), Some("appearance"), "Appearance");
    stack.add_titled(&pages::menu::build(), Some("menu"), "Metis Menu");
    stack.add_titled(&pages::weather::build(), Some("weather"), "Weather");
    stack.add_titled(&pages::network::build(), Some("network"), "Network");
    stack.add_titled(&pages::calendars::build(), Some("calendars"), "Calendars");
    stack.add_titled(&pages::mouse::build(), Some("mouse"), "Mouse");
    stack.add_titled(&pages::touchpad::build(), Some("touchpad"), "Touchpad");
    stack.add_titled(&pages::keyboard::build(), Some("keyboard"), "Keyboard");

    let nav = gtk::ListBox::new();
    nav.set_selection_mode(gtk::SelectionMode::Single);
    for item in NAV {
        let row = if let Some(icon) = item.icon {
            let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 10);
            let img = gtk::Image::from_icon_name(icon);
            let label = gtk::Label::new(Some(item.title));
            label.set_xalign(0.0);
            row_box.append(&img);
            row_box.append(&label);
            let row = gtk::ListBoxRow::new();
            row.set_child(Some(&row_box));
            row
        } else {
            let label = gtk::Label::new(Some(item.title));
            label.set_xalign(0.0);
            label.add_css_class("metis-settings-nav-section");
            let row = gtk::ListBoxRow::new();
            row.set_selectable(false);
            row.set_activatable(false);
            row.set_child(Some(&label));
            row
        };
        nav.append(&row);
    }
    {
        let stack = stack.clone();
        nav.connect_row_selected(move |list, row| {
            let Some(row) = row else {
                return;
            };
            let index = row.index() as usize;
            let Some(item) = NAV.get(index) else {
                return;
            };
            if let Some(id) = item.page_id {
                stack.set_visible_child_name(id);
            } else if let Some(next) = list.row_at_index(row.index() + 1) {
                list.select_row(Some(&next));
            }
        });
    }

    let nav_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .vexpand(true)
        .child(&nav)
        .build();

    let sidebar = gtk::Box::new(gtk::Orientation::Vertical, 0);
    sidebar.add_css_class("metis-settings-sidebar");
    sidebar.set_width_request(200);
    sidebar.set_vexpand(true);
    sidebar.append(&nav_scroll);

    let layout = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    layout.append(&sidebar);
    layout.append(&gtk::Separator::new(gtk::Orientation::Vertical));
    layout.append(&stack);
    layout.add_css_class("metis-settings-root");

    let initial_row = page
        .as_deref()
        .and_then(|p| NAV.iter().position(|item| item.page_id == Some(p)))
        .unwrap_or(1);
    if let Some(row) = nav.row_at_index(initial_row as i32) {
        nav.select_row(Some(&row));
    }

    let under_metis = std::env::var_os("METIS_SESSION").is_some();
    let window = gtk::Window::builder()
        .title("Metis Settings")
        .default_width(880)
        .default_height(640)
        .decorated(!under_metis)
        .build();
    window.add_css_class("metis-settings-window");
    window.set_child(Some(&layout));
    window.present();
}
