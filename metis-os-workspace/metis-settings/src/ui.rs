//! Small shared widget helpers so the settings pages share a consistent layout.

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
    let mut header = PageHeader::new(meta.title);
    if let Some(icon) = meta.icon {
        header = header.with_icon(icon);
    }
    if let Some(sub) = meta.subtitle {
        header = header.with_subtitle(sub);
    }
    if let Some(hue) = meta.hue {
        header = header.with_hue(hue);
    }
    page(header)
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
        .child(&content)
        .build();
    scroller.set_kinetic_scrolling(false);
    wire_vertical_scroll(&scroller);
    scroller.add_css_class("metis-settings-scroller");
    wire_click_to_defocus(&content);
    (scroller, content)
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
