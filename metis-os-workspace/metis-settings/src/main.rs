//! Metis Settings — a standalone GTK4 app for configuring appearance, weather,
//! network, and calendars. Reads/writes the shared `~/.config/metis/*.json` via
//! the `metis-config` crate; the running shell picks up changes through its file
//! watchers (or an explicit `reload-*` runtime command).

mod bluetooth;
mod gaming;
mod msauth;
mod nav;
mod net;
mod pages;
mod power;
mod printers;
mod remote;
mod runtime;
mod sound;
mod theme;
mod ui;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;

use nav::{NavHue, NAV};

const SIDEBAR_WIDTH: i32 = 248;
/// Embedded settings icon — same asset installed as `metis-settings` in the icon theme.
const APP_ICON_BYTES: &[u8] = include_bytes!("../../assets/metis-settings.png");

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "metis_settings=info,warn".into()),
        )
        .init();

    let page = parse_page_arg();

    if let Err(err) = gtk::init() {
        tracing::error!(?err, "gtk::init() failed");
        std::process::exit(1);
    }
    build_ui(page);
    glib::MainLoop::new(None, false).run();
}

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
    nav::page_ids()
        .into_iter()
        .find(|id| *id == name)
        .map(str::to_string)
}

fn build_ui(page: Option<String>) {
    theme::install();

    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    stack.set_transition_duration(0);
    stack.set_hexpand(true);
    stack.set_vexpand(true);

    let under_metis = std::env::var_os("METIS_SESSION").is_some();
    let window = gtk::Window::builder()
        .title("Settings")
        .default_width(960)
        .default_height(680)
        .decorated(!under_metis)
        .build();
    window.add_css_class("metis-settings-window");
    let window_for_icon = window.clone();
    window.connect_map(move |win| {
        apply_window_icon(win);
    });
    apply_window_icon(&window_for_icon);

    stack.add_titled(
        &pages::appearance::build(),
        Some("appearance"),
        "Appearance",
    );
    stack.add_titled(&pages::background::build(), Some("background"), "Background");
    stack.add_titled(&pages::edgebar::build(), Some("edgebar"), "Edge bar");
    stack.add_titled(&pages::windows::build(), Some("windows"), "Windows");
    stack.add_titled(&pages::menu::build(), Some("menu"), "Metis Menu");
    stack.add_titled(&pages::weather::build(), Some("weather"), "Weather");
    stack.add_titled(&pages::network::build(), Some("network"), "Network");
    stack.add_titled(&pages::calendars::build(), Some("calendars"), "Calendars");
    stack.add_titled(&pages::mouse::build(), Some("mouse"), "Mouse");
    stack.add_titled(&pages::touchpad::build(), Some("touchpad"), "Touchpad");
    stack.add_titled(&pages::keyboard::build(), Some("keyboard"), "Keyboard");
    stack.add_titled(&pages::bluetooth::build(), Some("bluetooth"), "Bluetooth");
    stack.add_titled(&pages::printers::build(), Some("printers"), "Printers");
    stack.add_titled(&pages::sound::build(), Some("sound"), "Sound");
    stack.add_titled(&pages::power::build(), Some("power"), "Power");
    stack.add_titled(&pages::remote::build(&window), Some("remote"), "Remote access");
    stack.add_titled(&pages::gaming::build(), Some("gaming"), "Gaming");
    stack.add_titled(
        &pages::display::build(&window),
        Some("display"),
        "Display",
    );

    let nav = gtk::ListBox::new();
    nav.add_css_class("metis-settings-nav");
    nav.set_selection_mode(gtk::SelectionMode::Single);
    for item in NAV {
        let row = if let Some(icon) = item.icon {
            build_nav_row(item.title, icon, item.hue)
        } else {
            let label = gtk::Label::new(Some(item.title));
            label.set_xalign(0.0);
            label.add_css_class("metis-settings-nav-section");
            let row = gtk::ListBoxRow::new();
            row.add_css_class("metis-settings-nav-section-row");
            row.set_selectable(false);
            row.set_activatable(false);
            row.set_child(Some(&label));
            row
        };
        nav.append(&row);
    }

    let selecting = Rc::new(Cell::new(false));
    let last_query = Rc::new(RefCell::new(String::new()));
    let pending_query = Rc::new(RefCell::new(String::new()));
    let filter_debounce = Rc::new(RefCell::new(None::<glib::SourceId>));

    {
        let stack = stack.clone();
        let selecting = selecting.clone();
        nav.connect_row_selected(move |list, row| {
            if selecting.get() {
                return;
            }
            let Some(row) = row else {
                return;
            };
            let index = row.index() as usize;
            let Some(item) = NAV.get(index) else {
                return;
            };
            if let Some(id) = item.page_id {
                if stack.visible_child_name().as_deref() != Some(id) {
                    stack.set_visible_child_name(id);
                }
            } else if let Some(next) = list.row_at_index(row.index() + 1) {
                selecting.set(true);
                list.select_row(Some(&next));
                selecting.set(false);
            }
        });
    }

    let nav_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .vexpand(true)
        .hexpand(true)
        .overlay_scrolling(false)
        .child(&nav)
        .build();
    nav_scroll.add_css_class("metis-settings-nav-scroll");
    nav_scroll.set_kinetic_scrolling(false);

    let search = gtk::Entry::builder()
        .placeholder_text("Search")
        .hexpand(true)
        .build();
    search.add_css_class("metis-settings-search");
    search.set_margin_start(14);
    search.set_margin_end(14);
    search.set_margin_bottom(8);

    // Held backspace on an empty field still generates key-repeat events that can
    // bubble to the sidebar list and flood the main loop — swallow them here.
    {
        let search_key = search.clone();
        let key = gtk::EventControllerKey::new();
        key.connect_key_pressed(move |_, key, _, _| {
            if key == gtk::gdk::Key::BackSpace && search_key.text().is_empty() {
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        search.add_controller(key);
    }

    {
        let nav = nav.clone();
        let stack = stack.clone();
        let selecting = selecting.clone();
        let last_query = last_query.clone();
        let pending_query = pending_query.clone();
        let filter_debounce = filter_debounce.clone();
        search.connect_changed(move |entry| {
            *pending_query.borrow_mut() = entry.text().trim().to_ascii_lowercase();
            schedule_nav_filter(
                &nav,
                &stack,
                &selecting,
                &last_query,
                &pending_query,
                &filter_debounce,
            );
        });
    }

    {
        let nav = nav.clone();
        let stack = stack.clone();
        let selecting = selecting.clone();
        let last_query = last_query.clone();
        let pending_query = pending_query.clone();
        let filter_debounce = filter_debounce.clone();
        let search_key = search.clone();
        let key = gtk::EventControllerKey::new();
        key.connect_key_released(move |_, key, _, _| {
            if key == gtk::gdk::Key::BackSpace || key == gtk::gdk::Key::Delete {
                *pending_query.borrow_mut() = search_key.text().trim().to_ascii_lowercase();
                flush_nav_filter(
                    &nav,
                    &stack,
                    &selecting,
                    &last_query,
                    &pending_query,
                    &filter_debounce,
                );
            }
        });
        search.add_controller(key);
    }

    let sidebar = gtk::Box::new(gtk::Orientation::Vertical, 0);
    sidebar.add_css_class("metis-settings-sidebar");
    sidebar.set_size_request(SIDEBAR_WIDTH, -1);
    sidebar.set_hexpand(false);
    sidebar.set_halign(gtk::Align::Start);
    sidebar.set_vexpand(true);

    let sidebar_title = gtk::Label::new(Some("Settings"));
    sidebar_title.set_xalign(0.0);
    sidebar_title.add_css_class("metis-settings-sidebar-title");
    sidebar_title.set_hexpand(true);

    let title_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .margin_top(18)
        .margin_bottom(10)
        .margin_start(20)
        .margin_end(16)
        .build();
    if let Some(icon) = load_app_icon() {
        let title_icon = gtk::Image::new();
        title_icon.set_paintable(Some(&icon));
        title_icon.set_pixel_size(28);
        title_icon.add_css_class("metis-settings-sidebar-icon");
        title_row.append(&title_icon);
    }
    title_row.append(&sidebar_title);
    sidebar.append(&title_row);
    sidebar.append(&search);
    sidebar.append(&nav_scroll);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.add_css_class("metis-settings-content");
    content.set_hexpand(true);
    content.set_halign(gtk::Align::Fill);
    content.set_vexpand(true);
    content.append(&stack);

    let layout = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    layout.append(&sidebar);
    layout.append(&gtk::Separator::new(gtk::Orientation::Vertical));
    layout.append(&content);
    layout.add_css_class("metis-settings-root");

    let initial_row = page
        .as_deref()
        .and_then(|p| NAV.iter().position(|item| item.page_id == Some(p)))
        .unwrap_or(1);
    if let Some(row) = nav.row_at_index(initial_row as i32) {
        nav.select_row(Some(&row));
    }

    window.set_child(Some(&layout));
    window.present();
}

fn load_app_icon() -> Option<gtk::gdk::Texture> {
    let bytes = glib::Bytes::from_static(APP_ICON_BYTES);
    match gtk::gdk::Texture::from_bytes(&bytes) {
        Ok(texture) => Some(texture),
        Err(err) => {
            tracing::warn!(%err, "failed to decode embedded settings icon");
            None
        }
    }
}

fn apply_window_icon(window: &gtk::Window) {
    if let Some(texture) = load_app_icon() {
        if let Some(surface) = window.surface() {
            if let Some(toplevel) = surface.downcast_ref::<gtk::gdk::Toplevel>() {
                toplevel.set_icon_list(&[texture]);
                return;
            }
        }
    }
    window.set_icon_name(Some("metis-settings"));
}

const FILTER_DEBOUNCE_MS: u64 = 16;

fn schedule_nav_filter(
    nav: &gtk::ListBox,
    stack: &gtk::Stack,
    selecting: &Rc<Cell<bool>>,
    last_query: &Rc<RefCell<String>>,
    pending_query: &Rc<RefCell<String>>,
    debounce: &Rc<RefCell<Option<glib::SourceId>>>,
) {
    let mut slot = debounce.borrow_mut();
    if let Some(id) = slot.take() {
        id.remove();
    }
    let nav = nav.clone();
    let stack = stack.clone();
    let selecting = selecting.clone();
    let last_query = last_query.clone();
    let pending_query = pending_query.clone();
    let debounce = debounce.clone();
    let id = glib::timeout_add_local(Duration::from_millis(FILTER_DEBOUNCE_MS), move || {
        *debounce.borrow_mut() = None;
        let query = pending_query.borrow().clone();
        if *last_query.borrow() == query {
            return glib::ControlFlow::Break;
        }
        *last_query.borrow_mut() = query.clone();
        apply_nav_filter(&nav, &query, &selecting, &stack);
        glib::ControlFlow::Break
    });
    *slot = Some(id);
}

fn flush_nav_filter(
    nav: &gtk::ListBox,
    stack: &gtk::Stack,
    selecting: &Rc<Cell<bool>>,
    last_query: &Rc<RefCell<String>>,
    pending_query: &Rc<RefCell<String>>,
    debounce: &Rc<RefCell<Option<glib::SourceId>>>,
) {
    if let Some(id) = debounce.borrow_mut().take() {
        id.remove();
    }
    let query = pending_query.borrow().clone();
    if *last_query.borrow() == query {
        return;
    }
    *last_query.borrow_mut() = query.clone();
    apply_nav_filter(nav, &query, selecting, stack);
}

/// Apply sidebar search by toggling row visibility (avoids ListBox filter/selection loops).
fn apply_nav_filter(nav: &gtk::ListBox, query: &str, selecting: &Cell<bool>, stack: &gtk::Stack) {
    selecting.set(true);

    let mut first_visible_page: Option<usize> = None;

    for index in 0..NAV.len() {
        let Some(row) = nav.row_at_index(index as i32) else {
            continue;
        };
        let visible = nav::row_visible_for_search(index, query);
        if row.is_visible() != visible {
            row.set_visible(visible);
        }
        if visible && NAV[index].page_id.is_some() && first_visible_page.is_none() {
            first_visible_page = Some(index);
        }
    }

    if let Some(selected) = nav.selected_row() {
        if !selected.is_visible() {
            if let Some(idx) = first_visible_page {
                if let Some(row) = nav.row_at_index(idx as i32) {
                    nav.select_row(Some(&row));
                    if let Some(id) = NAV[idx].page_id {
                        if stack.visible_child_name().as_deref() != Some(id) {
                            stack.set_visible_child_name(id);
                        }
                    }
                }
            }
        }
    }

    selecting.set(false);
}

fn build_nav_row(title: &str, icon: &str, hue: Option<NavHue>) -> gtk::ListBoxRow {
    let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row_box.add_css_class("metis-settings-nav-row-inner");

    let icon_wrap = gtk::Box::builder()
        .width_request(30)
        .height_request(30)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .build();
    icon_wrap.add_css_class("metis-settings-nav-icon-wrap");
    if let Some(hue) = hue {
        icon_wrap.add_css_class(hue.css_class());
    }
    let img = gtk::Image::from_icon_name(icon);
    img.set_pixel_size(16);
    img.add_css_class("metis-settings-nav-icon");
    icon_wrap.append(&img);
    row_box.append(&icon_wrap);

    let label = gtk::Label::new(Some(title));
    label.set_xalign(0.0);
    label.set_hexpand(true);
    label.add_css_class("metis-settings-nav-label");
    row_box.append(&label);

    let row = gtk::ListBoxRow::new();
    row.add_css_class("metis-settings-nav-row");
    row.set_child(Some(&row_box));
    row
}
