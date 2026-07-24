//! Edge bar: position, per-display placement, workspace + layout behaviour,
//! tray mode, distance from the screen edge, opacity, backdrop blur, and the
//! bar's pill border. All fields live in `bar.json`, which the shell watches.

use std::rc::Rc;

use gtk::prelude::*;

use crate::pages::appearance_common::{
    color_dialog_button, hex_to_rgba, rgba_to_hex, set_stops, update_bar,
};
use crate::{runtime, ui};
use metis_i18n::tr;

pub fn build() -> gtk::Widget {
    let (scroller, content) = ui::page_for("edgebar");

    let bar = metis_config::load_bar_config();
    let (bar_card, bar_body) = ui::section_with_icon(&tr("Edge bar"), "preferences-system-symbolic");

    let position_dd = {
        let __dd_labels = [tr("Top"), tr("Bottom"), tr("Left"), tr("Right")];
        let __dd_refs: Vec<&str> = __dd_labels.iter().map(|s| s.as_str()).collect();
        gtk::DropDown::from_strings(&__dd_refs)
    };
    position_dd.set_selected(bar_position_to_index(bar.position));
    bar_body.append(&ui::row_with_icon(
        "view-paged-symbolic",
        &tr("Position"),
        &position_dd,
    ));

    let displays_dd = {
        let __dd_labels = [tr("All displays"), tr("Primary display only")];
        let __dd_refs: Vec<&str> = __dd_labels.iter().map(|s| s.as_str()).collect();
        gtk::DropDown::from_strings(&__dd_refs)
    };
    displays_dd.set_selected(bar_displays_to_index(bar.displays));
    bar_body.append(&ui::row_with_icon(
        "video-display-symbolic",
        &tr("Show bar on"),
        &displays_dd,
    ));

    let workspace_mode_dd =
        {
        let __dd_labels = [tr("Independent per display"), tr("Linked across displays")];
        let __dd_refs: Vec<&str> = __dd_labels.iter().map(|s| s.as_str()).collect();
        gtk::DropDown::from_strings(&__dd_refs)
    };
    workspace_mode_dd.set_selected(workspace_mode_to_index(bar.workspace_mode));
    workspace_mode_dd.set_tooltip_text(Some(&tr(
        "Independent: each monitor keeps its own workspaces.\n\
         Linked: switching a workspace moves every monitor at once."
        )));
    bar_body.append(&ui::row_with_icon(
        "view-grid-symbolic",
        &tr("Workspaces"),
        &workspace_mode_dd,
    ));

    let default_layout_dd =
        {
        let __dd_labels = [tr("Free desktop"), tr("Grid tiling"), tr("Scrolling")];
        let __dd_refs: Vec<&str> = __dd_labels.iter().map(|s| s.as_str()).collect();
        gtk::DropDown::from_strings(&__dd_refs)
    };
    default_layout_dd.set_selected(default_layout_to_index(bar.default_layout));
    default_layout_dd.set_tooltip_text(Some(&tr(
        "Applies to every workspace right away.\n\
         Toggle the active workspace with Super + \\ (cycles free → grid → scroll)."
        )));
    bar_body.append(&ui::row_with_icon(
        "view-dual-symbolic",
        &tr("New workspace layout"),
        &default_layout_dd,
    ));

    let tray_mode_dd =
        {
        let __dd_labels = [tr("Collapsed (popup list)"), tr("Pinned on bar")];
        let __dd_refs: Vec<&str> = __dd_labels.iter().map(|s| s.as_str()).collect();
        gtk::DropDown::from_strings(&__dd_refs)
    };
    tray_mode_dd.set_selected(tray_icon_mode_to_index(bar.tray_icon_mode));
    tray_mode_dd.set_tooltip_text(Some(&tr(
        "Collapsed: one tray button opens a popover with all background app icons.\n\
         Pinned: tray icons stay visible on the bar, left of the tray button."
        )));
    bar_body.append(&ui::row_with_icon(
        "view-more-horizontal-symbolic",
        &tr("System tray icons"),
        &tray_mode_dd,
    ));

    let edge_margin = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 64.0, 1.0);
    edge_margin.set_value(bar.margin_top as f64);
    edge_margin.set_size_request(200, -1);
    edge_margin.set_draw_value(true);
    ui::forward_wheel_to_page_scroller(&edge_margin);
    bar_body.append(&ui::row_with_icon(
        "view-fullscreen-symbolic",
        &tr("Distance from edge"),
        &edge_margin,
    ));

    let opacity = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.3, 1.0, 0.01);
    opacity.set_value(bar.opacity as f64);
    opacity.set_size_request(200, -1);
    opacity.set_draw_value(true);
    ui::forward_wheel_to_page_scroller(&opacity);
    bar_body.append(&ui::row_with_icon("display-brightness-symbolic", &tr("Opacity"), &opacity));

    let blur = gtk::Switch::new();
    blur.set_active(bar.blur);
    blur.set_halign(gtk::Align::End);
    blur.set_valign(gtk::Align::Center);
    bar_body.append(&ui::row_with_icon("weather-fog-symbolic", &tr("Backdrop blur"), &blur));

    let blur_radius = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 40.0, 1.0);
    blur_radius.set_value(bar.blur_radius as f64);
    blur_radius.set_size_request(200, -1);
    blur_radius.set_draw_value(true);
    ui::forward_wheel_to_page_scroller(&blur_radius);
    bar_body.append(&ui::row_with_icon("weather-fog-symbolic", &tr("Blur radius"), &blur_radius));

    let blur_hint = gtk::Label::new(Some(&tr(
        "Blur frosts the wallpaper behind the bar — it needs a wallpaper set and \
         the bar opacity below 1.0 to show through. Changes apply within ~1s."
        )));
    blur_hint.set_xalign(0.0);
    blur_hint.set_wrap(true);
    blur_hint.add_css_class("metis-settings-hint");
    bar_body.append(&blur_hint);

    // -- Edge-bar border (mode / width / colors) --
    let bb = bar.bar_border.clone();

    let bb_mode = {
        let __dd_labels = [tr("Theme accent"), tr("Solid color"), tr("Custom gradient")];
        let __dd_refs: Vec<&str> = __dd_labels.iter().map(|s| s.as_str()).collect();
        gtk::DropDown::from_strings(&__dd_refs)
    };
    bb_mode.set_selected(match bb.mode {
        metis_config::BorderMode::Accent => 0,
        metis_config::BorderMode::Solid => 1,
        metis_config::BorderMode::Gradient => 2,
    });
    bar_body.append(&ui::row_with_icon("view-paged-symbolic", &tr("Bar border"), &bb_mode));

    let bb_width = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 8.0, 1.0);
    bb_width.set_value(bb.width_px as f64);
    bb_width.set_size_request(200, -1);
    bb_width.set_draw_value(true);
    ui::forward_wheel_to_page_scroller(&bb_width);
    bar_body.append(&ui::row_with_icon(
        "view-fullscreen-symbolic",
        &tr("Bar border thickness"),
        &bb_width,
    ));

    let bb_solid_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let bb_solid = color_dialog_button();
    bb_solid.set_rgba(&hex_to_rgba(&bb.color));
    bb_solid_box.append(&ui::row_with_icon(
        "applications-graphics-symbolic",
        &tr("Border color"),
        &bb_solid,
    ));
    bar_body.append(&bb_solid_box);

    let bb_grad_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let bb_stop = |idx: usize, fallback: &str| {
        let b = color_dialog_button();
        let hex = bb.gradient.get(idx).map(String::as_str).unwrap_or(fallback);
        b.set_rgba(&hex_to_rgba(hex));
        b
    };
    let bb_g1 = bb_stop(0, "#00F2FE");
    let bb_g2 = bb_stop(1, "#4FACFE");
    let bb_g3 = bb_stop(2, "#A24BFF");
    bb_grad_box.append(&ui::row_with_icon("starred-symbolic", &tr("Gradient start"), &bb_g1));
    bb_grad_box.append(&ui::row_with_icon("starred-symbolic", &tr("Gradient middle"), &bb_g2));
    bb_grad_box.append(&ui::row_with_icon("starred-symbolic", &tr("Gradient end"), &bb_g3));
    bar_body.append(&bb_grad_box);

    let bb_hint = gtk::Label::new(Some(
        "The border around the edge bar's pill. \"Theme accent\" tracks your accent \
         colors; or pick a solid color / custom gradient. Set thickness to 0 to \
         disable it. Changes apply within ~1s.",
    ));
    bb_hint.set_xalign(0.0);
    bb_hint.set_wrap(true);
    bb_hint.add_css_class("metis-settings-hint");
    bar_body.append(&bb_hint);

    let bb_update_vis = {
        let bb_solid_box = bb_solid_box.clone();
        let bb_grad_box = bb_grad_box.clone();
        Rc::new(move |mode: metis_config::BorderMode| {
            bb_solid_box.set_visible(mode == metis_config::BorderMode::Solid);
            bb_grad_box.set_visible(mode == metis_config::BorderMode::Gradient);
        })
    };
    bb_update_vis(bb.mode);

    content.append(&bar_card);

    // ---- Wiring -----------------------------------------------------------
    position_dd.connect_selected_notify(move |dd| {
        let v = index_to_bar_position(dd.selected());
        update_bar(move |c| c.position = v);
    });
    displays_dd.connect_selected_notify(move |dd| {
        let v = index_to_bar_displays(dd.selected());
        update_bar(move |c| c.displays = v);
    });
    workspace_mode_dd.connect_selected_notify(move |dd| {
        let v = index_to_workspace_mode(dd.selected());
        update_bar(move |c| c.workspace_mode = v);
    });
    default_layout_dd.connect_selected_notify(move |dd| {
        let layout = index_to_default_layout(dd.selected());
        update_bar(move |c| c.default_layout = layout);
        // Apply live to every workspace so the dropdown is a real on/off switch,
        // not just a seed for future workspaces.
        runtime::apply_default_layout(match layout {
            metis_config::DefaultLayout::Free => metis_protocol::LayoutKind::Free,
            metis_config::DefaultLayout::Grid => metis_protocol::LayoutKind::Grid,
            metis_config::DefaultLayout::Scroll => metis_protocol::LayoutKind::Scroll,
        });
    });
    tray_mode_dd.connect_selected_notify(move |dd| {
        let v = index_to_tray_icon_mode(dd.selected());
        update_bar(move |c| c.tray_icon_mode = v);
    });
    edge_margin.connect_value_changed(move |s| {
        let v = s.value().round() as u32;
        update_bar(move |c| c.margin_top = v);
    });
    opacity.connect_value_changed(move |s| {
        let v = s.value() as f32;
        update_bar(move |c| c.opacity = v);
    });
    ui::defer_switch_active_notify(&blur, move |active| {
        update_bar(move |c| c.blur = active);
    });
    blur_radius.connect_value_changed(move |s| {
        let v = s.value() as f32;
        update_bar(move |c| c.blur_radius = v);
    });
    {
        let bb_update_vis = bb_update_vis.clone();
        bb_mode.connect_selected_notify(move |dd| {
            let mode = match dd.selected() {
                1 => metis_config::BorderMode::Solid,
                2 => metis_config::BorderMode::Gradient,
                _ => metis_config::BorderMode::Accent,
            };
            bb_update_vis(mode);
            update_bar(move |c| c.bar_border.mode = mode);
        });
    }
    bb_width.connect_value_changed(move |s| {
        let v = s.value() as f32;
        update_bar(move |c| c.bar_border.width_px = v);
    });
    bb_solid.connect_rgba_notify(move |b| {
        let hex = rgba_to_hex(&b.rgba());
        update_bar(move |c| c.bar_border.color = hex);
    });
    for (idx, btn) in [(0usize, &bb_g1), (1, &bb_g2), (2, &bb_g3)] {
        btn.connect_rgba_notify(move |b| {
            let hex = rgba_to_hex(&b.rgba());
            update_bar(move |c| set_stops(&mut c.bar_border.gradient, idx, hex));
        });
    }

    scroller.upcast()
}

fn bar_position_to_index(pos: metis_config::BarPosition) -> u32 {
    use metis_config::BarPosition as P;
    match pos {
        P::Top => 0,
        P::Bottom => 1,
        P::Left => 2,
        P::Right => 3,
    }
}

fn index_to_bar_position(idx: u32) -> metis_config::BarPosition {
    use metis_config::BarPosition as P;
    match idx {
        1 => P::Bottom,
        2 => P::Left,
        3 => P::Right,
        _ => P::Top,
    }
}

fn bar_displays_to_index(displays: metis_config::BarDisplays) -> u32 {
    match displays {
        metis_config::BarDisplays::All => 0,
        metis_config::BarDisplays::Primary => 1,
    }
}

fn index_to_bar_displays(idx: u32) -> metis_config::BarDisplays {
    match idx {
        1 => metis_config::BarDisplays::Primary,
        _ => metis_config::BarDisplays::All,
    }
}

fn workspace_mode_to_index(mode: metis_config::WorkspaceMode) -> u32 {
    match mode {
        metis_config::WorkspaceMode::Separate => 0,
        metis_config::WorkspaceMode::Linked => 1,
    }
}

fn index_to_workspace_mode(idx: u32) -> metis_config::WorkspaceMode {
    match idx {
        1 => metis_config::WorkspaceMode::Linked,
        _ => metis_config::WorkspaceMode::Separate,
    }
}

fn tray_icon_mode_to_index(mode: metis_config::TrayIconMode) -> u32 {
    match mode {
        metis_config::TrayIconMode::Collapsed => 0,
        metis_config::TrayIconMode::Pinned => 1,
    }
}

fn index_to_tray_icon_mode(idx: u32) -> metis_config::TrayIconMode {
    match idx {
        1 => metis_config::TrayIconMode::Pinned,
        _ => metis_config::TrayIconMode::Collapsed,
    }
}

fn default_layout_to_index(layout: metis_config::DefaultLayout) -> u32 {
    match layout {
        metis_config::DefaultLayout::Free => 0,
        metis_config::DefaultLayout::Grid => 1,
        metis_config::DefaultLayout::Scroll => 2,
    }
}

fn index_to_default_layout(idx: u32) -> metis_config::DefaultLayout {
    match idx {
        1 => metis_config::DefaultLayout::Grid,
        2 => metis_config::DefaultLayout::Scroll,
        _ => metis_config::DefaultLayout::Free,
    }
}
