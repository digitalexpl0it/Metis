//! Windows: server-side titlebar look and window-frame decoration — animations,
//! titlebar opacity, the focused title-pill border, and the window-frame border.
//! All fields live in `bar.json`, which the shell/compositor watch.

use std::rc::Rc;

use gtk::prelude::*;

use crate::pages::appearance_common::{
    color_dialog_button, hex_to_rgba, rgba_to_hex, set_stops, update_bar,
};
use crate::ui;

pub fn build() -> gtk::Widget {
    let (scroller, content) = ui::page_for("windows");

    let bar = metis_config::load_bar_config();
    let (win_card, win_body) = ui::section_with_icon("Windows", "window-new-symbolic");

    let window_animations = gtk::Switch::new();
    window_animations.set_active(bar.window_animations);
    window_animations.set_halign(gtk::Align::End);
    window_animations.set_valign(gtk::Align::Center);
    win_body.append(&ui::row_with_icon(
        "preferences-desktop-screensaver-symbolic",
        "Window animations",
        &window_animations,
    ));

    let anim_hint = gtk::Label::new(Some(
        "Minimize genie, maximize wobble, and titlebar slide effects. \
         Turn off for instant window transitions. The Compatibility graphics \
         profile on Display also disables animations in VMs.",
    ));
    anim_hint.set_xalign(0.0);
    anim_hint.set_wrap(true);
    anim_hint.add_css_class("metis-settings-hint");
    win_body.append(&anim_hint);

    let window_gap = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 10.0, 1.0);
    window_gap.set_value(bar.window_gap_px.min(10) as f64);
    window_gap.set_digits(0);
    window_gap.set_size_request(200, -1);
    window_gap.set_draw_value(true);
    ui::forward_wheel_to_page_scroller(&window_gap);
    win_body.append(&ui::row_with_icon(
        "view-fullscreen-symbolic",
        "Maximized window padding",
        &window_gap,
    ));

    let gap_hint = gtk::Label::new(Some(
        "Space around maximized and edge-snapped windows. 0 is flush to the \
         screen and bar edges; 10 is the maximum inset. Applies within ~1s.",
    ));
    gap_hint.set_xalign(0.0);
    gap_hint.set_wrap(true);
    gap_hint.add_css_class("metis-settings-hint");
    win_body.append(&gap_hint);

    let titlebar_opacity = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.3, 1.0, 0.01);
    titlebar_opacity.set_value(bar.titlebar_opacity as f64);
    titlebar_opacity.set_size_request(200, -1);
    titlebar_opacity.set_draw_value(true);
    ui::forward_wheel_to_page_scroller(&titlebar_opacity);
    win_body.append(&ui::row_with_icon(
        "display-brightness-symbolic",
        "Titlebar opacity",
        &titlebar_opacity,
    ));

    let win_hint = gtk::Label::new(Some(
        "Dims only the window titlebar background so the wallpaper shows through; \
         the title text and window buttons stay solid. Changes apply within ~1s.",
    ));
    win_hint.set_xalign(0.0);
    win_hint.set_wrap(true);
    win_hint.add_css_class("metis-settings-hint");
    win_body.append(&win_hint);

    // -- Title pill border (mode / width / colors) --
    let pill = bar.titlebar_pill_border.clone();

    let pill_mode = gtk::DropDown::from_strings(&["Theme accent", "Solid color", "Custom gradient"]);
    pill_mode.set_selected(match pill.mode {
        metis_config::BorderMode::Accent => 0,
        metis_config::BorderMode::Solid => 1,
        metis_config::BorderMode::Gradient => 2,
    });
    win_body.append(&ui::row_with_icon(
        "view-paged-symbolic",
        "Title pill border",
        &pill_mode,
    ));

    let pill_width = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 4.0, 0.25);
    pill_width.set_value(pill.width_px as f64);
    pill_width.set_size_request(200, -1);
    pill_width.set_draw_value(true);
    ui::forward_wheel_to_page_scroller(&pill_width);
    win_body.append(&ui::row_with_icon(
        "display-brightness-symbolic",
        "Pill border width",
        &pill_width,
    ));

    // Solid-color control (shown only in Solid mode).
    let pill_solid_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let pill_solid = color_dialog_button();
    pill_solid.set_rgba(&hex_to_rgba(&pill.color));
    pill_solid_box.append(&ui::row_with_icon(
        "applications-graphics-symbolic",
        "Border color",
        &pill_solid,
    ));
    win_body.append(&pill_solid_box);

    // Custom-gradient controls (shown only in Gradient mode): a fixed 3-stop ramp.
    let pill_grad_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let pill_stop = |idx: usize, fallback: &str| {
        let b = color_dialog_button();
        let hex = pill
            .gradient
            .get(idx)
            .map(String::as_str)
            .unwrap_or(fallback);
        b.set_rgba(&hex_to_rgba(hex));
        b
    };
    let pill_g1 = pill_stop(0, "#00F2FE");
    let pill_g2 = pill_stop(1, "#4FACFE");
    let pill_g3 = pill_stop(2, "#A24BFF");
    pill_grad_box.append(&ui::row_with_icon("starred-symbolic", "Gradient start", &pill_g1));
    pill_grad_box.append(&ui::row_with_icon("starred-symbolic", "Gradient middle", &pill_g2));
    pill_grad_box.append(&ui::row_with_icon("starred-symbolic", "Gradient end", &pill_g3));
    win_body.append(&pill_grad_box);

    let pill_hint = gtk::Label::new(Some(
        "The thin accent border around the focused window's title pill. \"Theme \
         accent\" tracks your accent colors; or set a solid color / custom gradient. \
         Unfocused windows use a muted border.",
    ));
    pill_hint.set_xalign(0.0);
    pill_hint.set_wrap(true);
    pill_hint.add_css_class("metis-settings-hint");
    win_body.append(&pill_hint);

    // -- Window frame border (independent of the pill: mode / thickness / colors) --
    let wb = bar.window_border.clone();

    let wb_mode = gtk::DropDown::from_strings(&["Theme accent", "Solid color", "Custom gradient"]);
    wb_mode.set_selected(match wb.mode {
        metis_config::BorderMode::Accent => 0,
        metis_config::BorderMode::Solid => 1,
        metis_config::BorderMode::Gradient => 2,
    });
    win_body.append(&ui::row_with_icon(
        "window-new-symbolic",
        "Window border",
        &wb_mode,
    ));

    let wb_width = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 12.0, 1.0);
    wb_width.set_value(wb.width_px as f64);
    wb_width.set_size_request(200, -1);
    wb_width.set_draw_value(true);
    ui::forward_wheel_to_page_scroller(&wb_width);
    win_body.append(&ui::row_with_icon(
        "view-fullscreen-symbolic",
        "Window border thickness",
        &wb_width,
    ));

    let wb_solid_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let wb_solid = color_dialog_button();
    wb_solid.set_rgba(&hex_to_rgba(&wb.color));
    wb_solid_box.append(&ui::row_with_icon(
        "applications-graphics-symbolic",
        "Border color",
        &wb_solid,
    ));
    win_body.append(&wb_solid_box);

    let wb_grad_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let wb_stop = |idx: usize, fallback: &str| {
        let b = color_dialog_button();
        let hex = wb.gradient.get(idx).map(String::as_str).unwrap_or(fallback);
        b.set_rgba(&hex_to_rgba(hex));
        b
    };
    let wb_g1 = wb_stop(0, "#00F2FE");
    let wb_g2 = wb_stop(1, "#4FACFE");
    let wb_g3 = wb_stop(2, "#A24BFF");
    wb_grad_box.append(&ui::row_with_icon("starred-symbolic", "Gradient top", &wb_g1));
    wb_grad_box.append(&ui::row_with_icon("starred-symbolic", "Gradient middle", &wb_g2));
    wb_grad_box.append(&ui::row_with_icon("starred-symbolic", "Gradient bottom", &wb_g3));
    win_body.append(&wb_grad_box);

    let wb_hint = gtk::Label::new(Some(
        "The border around the whole window frame, independent of the title pill. The \
         gradient flows top→bottom; thickness also insets the window contents. \
         Changes apply within ~1s.",
    ));
    wb_hint.set_xalign(0.0);
    wb_hint.set_wrap(true);
    wb_hint.add_css_class("metis-settings-hint");
    win_body.append(&wb_hint);

    content.append(&win_card);

    // Show only the color controls relevant to the active pill-border mode.
    let pill_update_vis = {
        let pill_solid_box = pill_solid_box.clone();
        let pill_grad_box = pill_grad_box.clone();
        Rc::new(move |mode: metis_config::BorderMode| {
            pill_solid_box.set_visible(mode == metis_config::BorderMode::Solid);
            pill_grad_box.set_visible(mode == metis_config::BorderMode::Gradient);
        })
    };
    pill_update_vis(pill.mode);

    let wb_update_vis = {
        let wb_solid_box = wb_solid_box.clone();
        let wb_grad_box = wb_grad_box.clone();
        Rc::new(move |mode: metis_config::BorderMode| {
            wb_solid_box.set_visible(mode == metis_config::BorderMode::Solid);
            wb_grad_box.set_visible(mode == metis_config::BorderMode::Gradient);
        })
    };
    wb_update_vis(wb.mode);

    // ---- Wiring -----------------------------------------------------------
    ui::defer_switch_active_notify(&window_animations, move |active| {
        update_bar(move |c| c.window_animations = active);
    });
    window_gap.connect_value_changed(move |s| {
        let v = s.value().round().clamp(0.0, 10.0) as u32;
        update_bar(move |c| c.window_gap_px = v);
    });
    titlebar_opacity.connect_value_changed(move |s| {
        let v = s.value() as f32;
        update_bar(move |c| c.titlebar_opacity = v);
    });
    {
        let pill_update_vis = pill_update_vis.clone();
        pill_mode.connect_selected_notify(move |dd| {
            let mode = match dd.selected() {
                1 => metis_config::BorderMode::Solid,
                2 => metis_config::BorderMode::Gradient,
                _ => metis_config::BorderMode::Accent,
            };
            pill_update_vis(mode);
            update_bar(move |c| c.titlebar_pill_border.mode = mode);
        });
    }
    pill_width.connect_value_changed(move |s| {
        let v = s.value() as f32;
        update_bar(move |c| c.titlebar_pill_border.width_px = v);
    });
    pill_solid.connect_rgba_notify(move |b| {
        let hex = rgba_to_hex(&b.rgba());
        update_bar(move |c| c.titlebar_pill_border.color = hex);
    });
    for (idx, btn) in [(0usize, &pill_g1), (1, &pill_g2), (2, &pill_g3)] {
        btn.connect_rgba_notify(move |b| {
            let hex = rgba_to_hex(&b.rgba());
            update_bar(move |c| set_stops(&mut c.titlebar_pill_border.gradient, idx, hex));
        });
    }
    {
        let wb_update_vis = wb_update_vis.clone();
        wb_mode.connect_selected_notify(move |dd| {
            let mode = match dd.selected() {
                1 => metis_config::BorderMode::Solid,
                2 => metis_config::BorderMode::Gradient,
                _ => metis_config::BorderMode::Accent,
            };
            wb_update_vis(mode);
            update_bar(move |c| c.window_border.mode = mode);
        });
    }
    wb_width.connect_value_changed(move |s| {
        let v = s.value() as f32;
        update_bar(move |c| c.window_border.width_px = v);
    });
    wb_solid.connect_rgba_notify(move |b| {
        let hex = rgba_to_hex(&b.rgba());
        update_bar(move |c| c.window_border.color = hex);
    });
    for (idx, btn) in [(0usize, &wb_g1), (1, &wb_g2), (2, &wb_g3)] {
        btn.connect_rgba_notify(move |b| {
            let hex = rgba_to_hex(&b.rgba());
            update_bar(move |c| set_stops(&mut c.window_border.gradient, idx, hex));
        });
    }

    scroller.upcast()
}
