//! Small shared widget helpers so the settings pages share a consistent layout.

use std::rc::Rc;

use gtk::gio;
use gtk::prelude::*;

/// Metadata for a settings content page (macOS-style header).
pub struct PageHeader<'a> {
    pub title: &'a str,
    pub icon: Option<&'a str>,
    pub subtitle: Option<&'a str>,
    pub hue: Option<crate::nav::NavHue>,
}

impl<'a> PageHeader<'a> {
    pub fn new(title: &'a str) -> Self {
        Self {
            title,
            icon: None,
            subtitle: None,
            hue: None,
        }
    }

    pub fn with_hue(mut self, hue: crate::nav::NavHue) -> Self {
        self.hue = Some(hue);
        self
    }

    pub fn with_icon(mut self, icon: &'a str) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn with_subtitle(mut self, subtitle: &'a str) -> Self {
        self.subtitle = Some(subtitle);
        self
    }
}

/// Build a page using sidebar metadata from [`crate::nav`].
pub fn page_for(id: &'static str) -> (gtk::ScrolledWindow, gtk::Box) {
    let meta = crate::nav::meta_for(id).unwrap_or_else(|| panic!("unknown page id: {id}"));
    let title = metis_i18n::tr(meta.title);
    let subtitle = meta.subtitle.map(metis_i18n::tr);
    let mut header = PageHeader::new(title.as_str());
    if let Some(icon) = meta.icon {
        header = header.with_icon(icon);
    }
    if let Some(ref sub) = subtitle {
        header = header.with_subtitle(sub.as_str());
    }
    if let Some(hue) = meta.hue {
        header = header.with_hue(hue);
    }
    // Keep owned strings alive for the duration of page() by leaking into static…
    // Better: change PageHeader to take Cow/String. For now extend lifetime via
    // storing on the content after build — PageHeader only borrows during page().
    let (scroller, content) = page(header);
    // Pin translations on the widget so DropDown-free pages stay valid.
    let _ = (title, subtitle);
    (scroller, content)
}

/// Build a scrollable page with a heading. Returns the outer scroller (add to the
/// stack) and the inner content box (append rows/sections to it).
pub fn page(header: PageHeader<'_>) -> (gtk::ScrolledWindow, gtk::Box) {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 20);
    content.set_margin_top(20);
    content.set_margin_bottom(28);
    content.set_margin_start(32);
    content.set_margin_end(32);
    content.add_css_class("metis-settings-page");

    let header_box = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    header_box.add_css_class("metis-settings-page-header");
    header_box.set_margin_bottom(4);

    if let Some(icon) = header.icon {
        let wrap = gtk::Box::builder()
            .width_request(52)
            .height_request(52)
            .halign(gtk::Align::Start)
            .valign(gtk::Align::Center)
            .build();
        wrap.add_css_class("metis-settings-page-icon-wrap");
        if let Some(hue) = header.hue {
            wrap.add_css_class(hue.css_class());
        }
        let img = gtk::Image::from_icon_name(icon);
        img.set_pixel_size(28);
        img.add_css_class("metis-settings-page-icon");
        wrap.append(&img);
        header_box.append(&wrap);
    }

    let titles = gtk::Box::new(gtk::Orientation::Vertical, 2);
    titles.set_valign(gtk::Align::Center);
    titles.set_hexpand(true);

    let heading = gtk::Label::new(Some(header.title));
    heading.set_xalign(0.0);
    heading.add_css_class("metis-settings-title");
    titles.append(&heading);

    if let Some(sub) = header.subtitle {
        let sublabel = gtk::Label::new(Some(sub));
        sublabel.set_xalign(0.0);
        sublabel.set_wrap(true);
        sublabel.add_css_class("metis-settings-subtitle");
        titles.append(&sublabel);
    }

    header_box.append(&titles);
    content.append(&header_box);

    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .hexpand(true)
        .vexpand(true)
        .overlay_scrolling(false)
        .propagate_natural_width(false)
        .propagate_natural_height(false)
        .child(&content)
        .build();
    scroller.set_kinetic_scrolling(false);
    wire_vertical_scroll(&scroller);
    scroller.add_css_class("metis-settings-scroller");
    wire_click_to_defocus(&content);
    (scroller, content)
}

/// Opaque rounded card for modal Settings sheets. Pair with a transparent
/// `metis-settings-password-dialog` / `metis-settings-widget-dialog` window so
/// pixels outside the radius stay true alpha instead of a solid grey fill.
pub fn dialog_sheet(content: &impl IsA<gtk::Widget>) -> gtk::Box {
    let sheet = gtk::Box::new(gtk::Orientation::Vertical, 0);
    sheet.add_css_class("metis-settings-dialog-sheet");
    sheet.set_hexpand(true);
    sheet.set_vexpand(true);
    sheet.append(content);
    sheet
}

/// Drop keyboard focus (committing any editable entry) when the user clicks an
/// empty part of the page. Clicks that land on a focusable control — entries,
/// spin buttons, dropdowns, switches, buttons — are left alone so those widgets
/// keep working normally.
fn wire_click_to_defocus(content: &gtk::Box) {
    let click = gtk::GestureClick::new();
    // Respond to any mouse button, and run after child widgets so an interactive
    // control that claims the press keeps its focus.
    click.set_button(0);
    let root_ref: gtk::Widget = content.clone().upcast();
    click.connect_pressed(move |_gesture, _n_press, x, y| {
        let mut node = root_ref.pick(x, y, gtk::PickFlags::DEFAULT);
        let mut hit_focusable = false;
        while let Some(widget) = node {
            if widget.is_focusable() {
                hit_focusable = true;
                break;
            }
            if widget == root_ref {
                break;
            }
            node = widget.parent();
        }
        if !hit_focusable {
            if let Some(root) = root_ref.root() {
                root.set_focus(None::<&gtk::Widget>);
            }
        }
    });
    content.add_controller(click);
}

/// Drive vertical scrolling from wheel events — re-exported for the sidebar scroller.
pub fn wire_vertical_scroll(scroller: &gtk::ScrolledWindow) {
    let ctrl = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
    ctrl.set_propagation_phase(gtk::PropagationPhase::Capture);
    let vadj = scroller.vadjustment();
    ctrl.connect_scroll(move |_, _, dy| {
        let page = vadj.page_size();
        let upper = vadj.upper();
        let lower = vadj.lower();
        if upper - lower <= page {
            return glib::Propagation::Proceed;
        }
        // Discrete wheel notches report ±1; smooth trackpads report pixel deltas.
        let delta = if dy.abs() <= 3.0 {
            dy * vadj.step_increment().max(48.0)
        } else {
            dy
        };
        let max = (upper - page).max(lower);
        let new_val = (vadj.value() + delta).clamp(lower, max);
        if (new_val - vadj.value()).abs() > f64::EPSILON {
            vadj.set_value(new_val);
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    scroller.add_controller(ctrl);
}

/// Keep wheel events on a GtkScale/GtkRange from adjusting the value; scroll the
/// enclosing settings page instead (otherwise scrolling the Display page drags
/// the night-light slider and spams compositor reload IPC).
pub fn forward_wheel_to_page_scroller(widget: &impl IsA<gtk::Widget>) {
    let ctrl = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
    ctrl.set_propagation_phase(gtk::PropagationPhase::Capture);
    ctrl.connect_scroll(move |controller, _, dy| {
        let mut parent = controller.widget().parent();
        while let Some(p) = parent {
            if let Ok(scroller) = p.clone().downcast::<gtk::ScrolledWindow>() {
                let vadj = scroller.vadjustment();
                let delta = if dy.abs() <= 3.0 {
                    dy * vadj.step_increment().max(48.0)
                } else {
                    dy
                };
                let page = vadj.page_size();
                let max = (vadj.upper() - page).max(vadj.lower());
                let new_val = (vadj.value() + delta).clamp(vadj.lower(), max);
                if (new_val - vadj.value()).abs() > f64::EPSILON {
                    vadj.set_value(new_val);
                }
                return glib::Propagation::Stop;
            }
            parent = p.parent();
        }
        glib::Propagation::Stop
    });
    widget.add_controller(ctrl);
}

/// A titled card grouping related controls. Returns the body box to fill.
pub fn section(title: &str) -> (gtk::Box, gtk::Box) {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
    card.add_css_class("metis-settings-section");

    let header = gtk::Label::new(Some(title));
    header.set_xalign(0.0);
    header.add_css_class("metis-settings-section-title");
    card.append(&header);

    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    body.add_css_class("metis-settings-section-body");
    card.append(&body);
    (card, body)
}

/// Like [`section`] but with a leading symbolic icon in the header.
pub fn section_with_icon(title: &str, icon: &str) -> (gtk::Box, gtk::Box) {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
    card.add_css_class("metis-settings-section");

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    header.add_css_class("metis-settings-section-header");
    let img = gtk::Image::from_icon_name(icon);
    img.set_pixel_size(14);
    img.add_css_class("metis-settings-section-icon");
    header.append(&img);
    let label = gtk::Label::new(Some(title));
    label.set_xalign(0.0);
    label.add_css_class("metis-settings-section-title");
    header.append(&label);
    card.append(&header);

    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    body.add_css_class("metis-settings-section-body");
    card.append(&body);
    (card, body)
}

/// A leading-icon + label + trailing control row.
pub fn row_with_icon(
    icon: &str,
    label: &str,
    control: &impl IsA<gtk::Widget>,
) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.add_css_class("metis-settings-row");
    let img = gtk::Image::from_icon_name(icon);
    img.set_pixel_size(16);
    img.add_css_class("metis-settings-row-icon");
    row.append(&img);
    let lbl = gtk::Label::new(Some(label));
    lbl.set_xalign(0.0);
    lbl.set_hexpand(true);
    row.append(&lbl);
    row.append(control);
    row
}

/// A label + trailing control row.
pub fn row(label: &str, control: &impl IsA<gtk::Widget>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.add_css_class("metis-settings-row");
    let lbl = gtk::Label::new(Some(label));
    lbl.set_xalign(0.0);
    lbl.set_hexpand(true);
    row.append(&lbl);
    row.append(control);
    row
}

/// Settings row with a trailing switch. Clicking the label toggles the switch
/// (GNOME-style) so users are not forced to hit the small thumb target.
pub fn switch_row(label: &str) -> (gtk::Box, gtk::Switch) {
    let sw = gtk::Switch::new();
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.add_css_class("metis-settings-row");
    let lbl = gtk::Label::new(Some(label));
    lbl.set_xalign(0.0);
    lbl.set_hexpand(true);
    lbl.add_css_class("metis-settings-switch-label");
    let sw_toggle = sw.clone();
    let gesture = gtk::GestureClick::new();
    gesture.connect_released(move |_, _, _, _| {
        sw_toggle.set_active(!sw_toggle.is_active());
    });
    lbl.add_controller(gesture);
    row.append(&lbl);
    row.append(&sw);
    (row, sw)
}

/// Prevent held Backspace on an empty entry from bubbling to the Settings sidebar
/// search filter (same class of lag/lockup as the main search field).
pub fn swallow_empty_backspace(entry: &gtk::Entry) {
    let entry_key = entry.clone();
    let key = gtk::EventControllerKey::new();
    key.connect_key_pressed(move |_, key, _, _| {
        if key == gtk::gdk::Key::BackSpace && entry_key.text().is_empty() {
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    entry.add_controller(key);
}

/// Run `action` on the next main-loop idle turn so GTK can paint the switch
/// state before any file I/O or IPC in the handler.
pub fn defer_switch_active_notify<F>(sw: &gtk::Switch, action: F) -> glib::SignalHandlerId
where
    F: Fn(bool) + 'static,
{
    let action = std::rc::Rc::new(action);
    sw.connect_active_notify(move |switch| {
        let active = switch.is_active();
        let action = action.clone();
        glib::idle_add_local_once(move || action(active));
    })
}

/// Labelled dropdown of installed candidates (plus Auto-detect / Custom), with a
/// revealed path entry + file chooser when Custom is selected. `on_change` receives
/// the chosen value (`None` = auto-detect) whenever the selection or path changes.
pub fn launcher_picker(
    icon: &str,
    label: &str,
    candidates: &[(&str, &str)],
    current: Option<String>,
    on_change: impl Fn(Option<String>) + 'static,
) -> gtk::Box {
    let installed: Vec<(String, String)> = candidates
        .iter()
        .filter(|(bin, _)| metis_config::binary_in_path(bin))
        .map(|(bin, lbl)| (bin.to_string(), lbl.to_string()))
        .collect();

    let mut labels: Vec<String> = Vec::with_capacity(installed.len() + 2);
    labels.push("Auto-detect".to_string());
    for (_, lbl) in &installed {
        labels.push(lbl.clone());
    }
    labels.push("Custom…".to_string());
    let custom_index = (labels.len() - 1) as u32;
    let label_refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let dd = gtk::DropDown::from_strings(&label_refs);

    let entry = gtk::Entry::builder()
        .placeholder_text("Path to executable, e.g. /usr/bin/btop")
        .hexpand(true)
        .build();
    let browse = gtk::Button::from_icon_name("document-open-symbolic");
    browse.set_tooltip_text(Some("Browse…"));
    let custom_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    custom_box.append(&entry);
    custom_box.append(&browse);
    custom_box.set_visible(false);

    if let Some(cur) = current.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        if let Some(pos) = installed.iter().position(|(bin, _)| bin == cur) {
            dd.set_selected(1 + pos as u32);
        } else {
            dd.set_selected(custom_index);
            entry.set_text(cur);
            custom_box.set_visible(true);
        }
    }

    let on_change = Rc::new(on_change);
    let installed_bins: Vec<String> = installed.iter().map(|(bin, _)| bin.clone()).collect();

    {
        let entry = entry.clone();
        let custom_box = custom_box.clone();
        let on_change = on_change.clone();
        let installed_bins = installed_bins.clone();
        dd.connect_selected_notify(move |dd| {
            let sel = dd.selected();
            if sel == 0 {
                custom_box.set_visible(false);
                on_change(None);
            } else if sel == custom_index {
                custom_box.set_visible(true);
                on_change(non_empty_path(&entry.text()));
            } else {
                custom_box.set_visible(false);
                on_change(installed_bins.get((sel - 1) as usize).cloned());
            }
        });
    }

    {
        let dd = dd.clone();
        let on_change = on_change.clone();
        entry.connect_changed(move |e| {
            if dd.selected() == custom_index {
                on_change(non_empty_path(&e.text()));
            }
        });
    }

    {
        let entry = entry.clone();
        browse.connect_clicked(move |btn| {
            let dialog = gtk::FileDialog::new();
            dialog.set_title("Choose an executable");
            let parent = btn.root().and_downcast::<gtk::Window>();
            let entry = entry.clone();
            dialog.open(parent.as_ref(), gio::Cancellable::NONE, move |res| {
                if let Ok(file) = res {
                    if let Some(path) = file.path() {
                        entry.set_text(&path.to_string_lossy());
                    }
                }
            });
        });
    }

    let container = gtk::Box::new(gtk::Orientation::Vertical, 8);
    container.append(&row_with_icon(icon, label, &dd));
    container.append(&custom_box);
    container
}

fn non_empty_path(text: &str) -> Option<String> {
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}
