//! Appearance: theme mode, accent + semantic colours (written to `themes/*.json`),
//! and bar opacity/blur (written to `bar.json`). The shell's file watchers apply
//! colour/bar changes live; a `reload-theme` runtime command re-themes on a mode
//! switch (since the mode lives in `config.json`, which isn't watched).

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk::gdk;
use gtk::gio;
use gtk::prelude::*;

use metis_config::ThemeMode;

use crate::{runtime, ui};

const WALLPAPER_EXTS: &[&str] = &["jpg", "jpeg", "png", "webp"];

struct State {
    name: String,
    tokens: metis_config::ThemeTokens,
}

#[derive(Clone)]
struct ColorButtons {
    accent: gtk::ColorDialogButton,
    accent2: gtk::ColorDialogButton,
    error: gtk::ColorDialogButton,
    warning: gtk::ColorDialogButton,
    success: gtk::ColorDialogButton,
    info: gtk::ColorDialogButton,
    payment: gtk::ColorDialogButton,
    text: gtk::ColorDialogButton,
}

pub fn build() -> gtk::Widget {
    let (scroller, content) = ui::page("Appearance");

    let mode = metis_config::load_theme_preference().unwrap_or(ThemeMode::Dark);
    let name = effective_name(&mode);
    let state = Rc::new(RefCell::new(State {
        tokens: metis_config::load_theme_tokens(&name),
        name,
    }));
    // Guards programmatic `set_rgba` (during refresh) from triggering a save.
    let suppress = Rc::new(Cell::new(false));
    // Style buttons fire `toggled` when we set the initial active state below.
    let suppress_mode = Rc::new(Cell::new(true));

    // Current wallpaper drives the live preview thumbnails in the style buttons.
    let current_wp = current_wallpaper();

    // ---- Style (light / dark) --------------------------------------------
    let (mode_card, mode_body) = ui::section_with_icon("Style", "applications-graphics-symbolic");
    let chooser = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    chooser.set_halign(gtk::Align::Center);
    chooser.set_margin_top(4);
    chooser.set_margin_bottom(4);
    let light_btn = style_button("Light", false, current_wp.as_deref());
    let dark_btn = style_button("Dark", true, current_wp.as_deref());
    dark_btn.set_group(Some(&light_btn));
    match mode {
        ThemeMode::Light => light_btn.set_active(true),
        // System falls back to Dark in the picker (auto-detect was unreliable).
        _ => dark_btn.set_active(true),
    }
    suppress_mode.set(false);
    chooser.append(&light_btn);
    chooser.append(&dark_btn);
    mode_body.append(&chooser);
    content.append(&mode_card);

    // ---- Colors -----------------------------------------------------------
    let (color_card, color_body) = ui::section_with_icon("Colors", "applications-graphics-symbolic");
    let buttons = ColorButtons {
        accent: color_dialog_button(),
        accent2: color_dialog_button(),
        error: color_dialog_button(),
        warning: color_dialog_button(),
        success: color_dialog_button(),
        info: color_dialog_button(),
        payment: color_dialog_button(),
        text: color_dialog_button(),
    };
    color_body.append(&ui::row_with_icon("starred-symbolic", "Accent", &buttons.accent));
    color_body.append(&ui::row_with_icon(
        "starred-symbolic",
        "Accent (secondary)",
        &buttons.accent2,
    ));
    color_body.append(&ui::row_with_icon("dialog-error-symbolic", "Error", &buttons.error));
    color_body.append(&ui::row_with_icon(
        "dialog-warning-symbolic",
        "Warning",
        &buttons.warning,
    ));
    color_body.append(&ui::row_with_icon("emblem-ok-symbolic", "Success", &buttons.success));
    color_body.append(&ui::row_with_icon(
        "dialog-information-symbolic",
        "Info",
        &buttons.info,
    ));
    color_body.append(&ui::row_with_icon(
        "emblem-system-symbolic",
        "Payment",
        &buttons.payment,
    ));
    content.append(&color_card);

    refresh_buttons(&buttons, &state.borrow().tokens, &suppress);

    // Wire each color button to its token field. Changing the primary accent also
    // re-derives the on-accent text color so labels stay readable on it (e.g. a
    // black accent flips on-accent text to white).
    wire_color(&buttons.accent, &state, &suppress, |t, hex| {
        t.text_on_accent = contrast_on(&hex);
        set_accent(t, 0, hex);
    });
    wire_color(&buttons.accent2, &state, &suppress, |t, hex| set_accent(t, 1, hex));
    wire_color(&buttons.error, &state, &suppress, |t, hex| t.semantic.error = hex);
    wire_color(&buttons.warning, &state, &suppress, |t, hex| t.semantic.warning = hex);
    wire_color(&buttons.success, &state, &suppress, |t, hex| t.semantic.success = hex);
    wire_color(&buttons.info, &state, &suppress, |t, hex| t.semantic.info = hex);
    wire_color(&buttons.payment, &state, &suppress, |t, hex| t.semantic.payment = hex);

    // ---- Font -------------------------------------------------------------
    // Text color edits the theme's `text` token (recolors body text DE-wide).
    wire_color(&buttons.text, &state, &suppress, |t, hex| t.text = hex);

    let (font_card, font_body) = ui::section_with_icon("Font", "font-x-generic-symbolic");
    let font_btn = gtk::FontDialogButton::new(Some(gtk::FontDialog::new()));
    {
        let st = state.borrow();
        let fam = st.tokens.font_family.trim();
        let pt = st.tokens.font_size_pt;
        if !fam.is_empty() || pt > 0 {
            let mut desc = gtk::pango::FontDescription::new();
            if !fam.is_empty() {
                desc.set_family(fam);
            }
            let size_pt = if pt > 0 { pt } else { 11 };
            desc.set_size(size_pt as i32 * gtk::pango::SCALE);
            font_btn.set_font_desc(&desc);
        }
    }
    font_body.append(&ui::row_with_icon(
        "font-x-generic-symbolic",
        "Family & size",
        &font_btn,
    ));
    font_body.append(&ui::row_with_icon(
        "applications-graphics-symbolic",
        "Text color",
        &buttons.text,
    ));
    content.append(&font_card);
    {
        let state = state.clone();
        let suppress_font = Rc::new(Cell::new(true));
        let suppress_font_flag = suppress_font.clone();
        font_btn.connect_font_desc_notify(move |btn| {
            if suppress_font_flag.get() {
                return;
            }
            let desc = btn.font_desc();
            let family = desc
                .as_ref()
                .and_then(|d| d.family())
                .map(|s| s.to_string())
                .unwrap_or_default();
            let size = desc.as_ref().map(|d| d.size()).unwrap_or(0);
            let pt = if size > 0 {
                (size / gtk::pango::SCALE) as u32
            } else {
                0
            };
            let name = {
                let mut s = state.borrow_mut();
                s.tokens.font_family = family;
                s.tokens.font_size_pt = pt;
                s.name.clone()
            };
            let tokens = state.borrow().tokens.clone();
            if let Err(err) = metis_config::save_theme_tokens(&name, &tokens) {
                tracing::warn!(%err, "failed to save theme tokens");
            }
            crate::theme::reapply();
            runtime::send("reload-theme");
        });
        suppress_font.set(false);
    }

    // Style buttons: persist preference, re-target the colour editor, re-theme.
    let apply_mode: Rc<dyn Fn(ThemeMode)> = {
        let state = state.clone();
        let buttons = buttons.clone();
        let suppress = suppress.clone();
        let suppress_mode = suppress_mode.clone();
        Rc::new(move |mode: ThemeMode| {
            if suppress_mode.get() {
                return;
            }
            if let Err(err) = metis_config::save_theme_preference(mode.clone()) {
                tracing::warn!(%err, "failed to save theme preference");
            }
            let name = effective_name(&mode);
            let tokens = metis_config::load_theme_tokens(&name);
            refresh_buttons(&buttons, &tokens, &suppress);
            {
                let mut s = state.borrow_mut();
                s.name = name;
                s.tokens = tokens;
            }
            // Re-theme this settings window (incl. titlebar) live, then nudge the
            // shell/compositor to reload too.
            crate::theme::reapply();
            runtime::send("reload-theme");
        })
    };
    {
        let apply_mode = apply_mode.clone();
        light_btn.connect_toggled(move |b| {
            if b.is_active() {
                apply_mode(ThemeMode::Light);
            }
        });
    }
    {
        let apply_mode = apply_mode.clone();
        dark_btn.connect_toggled(move |b| {
            if b.is_active() {
                apply_mode(ThemeMode::Dark);
            }
        });
    }

    // ---- Background (picture / solid / gradient) -------------------------
    let (bg_card, bg_body) = ui::section_with_icon("Background", "preferences-desktop-wallpaper-symbolic");
    let bgcfg = Rc::new(RefCell::new(metis_config::load_wallpaper_config()));

    let type_dd = gtk::DropDown::from_strings(&["Picture", "Solid color", "Gradient"]);
    type_dd.set_selected(match bgcfg.borrow().kind {
        metis_config::BackgroundKind::Image => 0,
        metis_config::BackgroundKind::Solid => 1,
        metis_config::BackgroundKind::Gradient => 2,
    });
    bg_body.append(&ui::row_with_icon("view-paged-symbolic", "Type", &type_dd));

    // -- Picture controls --
    let picture_box = gtk::Box::new(gtk::Orientation::Vertical, 12);
    let add_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    add_row.set_halign(gtk::Align::End);
    let add_btn = gtk::Button::new();
    add_btn.add_css_class("flat");
    let add_content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    add_content.append(&gtk::Image::from_icon_name("list-add-symbolic"));
    add_content.append(&gtk::Label::new(Some("Add Picture…")));
    add_btn.set_child(Some(&add_content));
    add_row.append(&add_btn);
    picture_box.append(&add_row);

    let flow = gtk::FlowBox::new();
    flow.set_selection_mode(gtk::SelectionMode::None);
    flow.set_max_children_per_line(3);
    flow.set_min_children_per_line(2);
    flow.set_column_spacing(12);
    flow.set_row_spacing(12);
    flow.set_homogeneous(true);
    flow.add_css_class("metis-wallpaper-grid");
    picture_box.append(&flow);
    bg_body.append(&picture_box);
    populate_wallpapers(&flow, current_wp.as_deref(), &bgcfg);

    // -- Solid colour controls --
    let solid_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let solid_btn = color_dialog_button();
    solid_btn.set_rgba(&hex_to_rgba(&bgcfg.borrow().color));
    solid_box.append(&ui::row_with_icon(
        "applications-graphics-symbolic",
        "Color",
        &solid_btn,
    ));
    bg_body.append(&solid_box);

    // -- Gradient controls --
    let gradient_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let grad_start = color_dialog_button();
    grad_start.set_rgba(&hex_to_rgba(&bgcfg.borrow().gradient_start));
    let grad_end = color_dialog_button();
    grad_end.set_rgba(&hex_to_rgba(&bgcfg.borrow().gradient_end));
    let dir_dd = gtk::DropDown::from_strings(&[
        "Top → Bottom",
        "Bottom → Top",
        "Left → Right",
        "Right → Left",
        "Diagonal ↘",
        "Diagonal ↗",
    ]);
    dir_dd.set_selected(direction_to_index(bgcfg.borrow().gradient_direction));
    gradient_box.append(&ui::row_with_icon("starred-symbolic", "Start color", &grad_start));
    gradient_box.append(&ui::row_with_icon("starred-symbolic", "End color", &grad_end));
    gradient_box.append(&ui::row_with_icon("object-rotate-right-symbolic", "Direction", &dir_dd));
    bg_body.append(&gradient_box);

    content.append(&bg_card);

    // Show only the controls for the active background kind.
    let update_visibility = {
        let picture_box = picture_box.clone();
        let solid_box = solid_box.clone();
        let gradient_box = gradient_box.clone();
        Rc::new(move |kind: metis_config::BackgroundKind| {
            picture_box.set_visible(kind == metis_config::BackgroundKind::Image);
            solid_box.set_visible(kind == metis_config::BackgroundKind::Solid);
            gradient_box.set_visible(kind == metis_config::BackgroundKind::Gradient);
        })
    };
    update_visibility(bgcfg.borrow().kind);

    // Type chooser.
    {
        let bgcfg = bgcfg.clone();
        let update_visibility = update_visibility.clone();
        type_dd.connect_selected_notify(move |dd| {
            let kind = match dd.selected() {
                1 => metis_config::BackgroundKind::Solid,
                2 => metis_config::BackgroundKind::Gradient,
                _ => metis_config::BackgroundKind::Image,
            };
            bgcfg.borrow_mut().kind = kind;
            update_visibility(kind);
            save_and_apply(&bgcfg.borrow());
        });
    }
    // Solid colour.
    {
        let bgcfg = bgcfg.clone();
        solid_btn.connect_rgba_notify(move |b| {
            bgcfg.borrow_mut().color = rgba_to_hex(&b.rgba());
            save_and_apply(&bgcfg.borrow());
        });
    }
    // Gradient stops + direction.
    {
        let bgcfg = bgcfg.clone();
        grad_start.connect_rgba_notify(move |b| {
            bgcfg.borrow_mut().gradient_start = rgba_to_hex(&b.rgba());
            save_and_apply(&bgcfg.borrow());
        });
    }
    {
        let bgcfg = bgcfg.clone();
        grad_end.connect_rgba_notify(move |b| {
            bgcfg.borrow_mut().gradient_end = rgba_to_hex(&b.rgba());
            save_and_apply(&bgcfg.borrow());
        });
    }
    {
        let bgcfg = bgcfg.clone();
        dir_dd.connect_selected_notify(move |dd| {
            bgcfg.borrow_mut().gradient_direction = index_to_direction(dd.selected());
            save_and_apply(&bgcfg.borrow());
        });
    }
    // Add Picture… → import + select.
    {
        let flow = flow.clone();
        let bgcfg = bgcfg.clone();
        add_btn.connect_clicked(move |btn| {
            let flow = flow.clone();
            let bgcfg = bgcfg.clone();
            let root = btn.root().and_then(|r| r.downcast::<gtk::Window>().ok());
            pick_picture(root.as_ref(), move |path| {
                select_picture(&bgcfg, &path);
                populate_wallpapers(&flow, Some(&path), &bgcfg);
            });
        });
    }

    // ---- Bar (position / distance / opacity / blur / border) -------------
    let bar = Rc::new(RefCell::new(metis_config::load_bar_config()));
    let (bar_card, bar_body) = ui::section_with_icon("Edge bar", "preferences-system-symbolic");

    let position_dd = gtk::DropDown::from_strings(&["Top", "Bottom", "Left", "Right"]);
    position_dd.set_selected(bar_position_to_index(bar.borrow().position));
    bar_body.append(&ui::row_with_icon(
        "view-paged-symbolic",
        "Position",
        &position_dd,
    ));

    let edge_margin = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 64.0, 1.0);
    edge_margin.set_value(bar.borrow().margin_top as f64);
    edge_margin.set_size_request(200, -1);
    edge_margin.set_draw_value(true);
    bar_body.append(&ui::row_with_icon(
        "view-fullscreen-symbolic",
        "Distance from edge",
        &edge_margin,
    ));

    let opacity = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.3, 1.0, 0.01);
    opacity.set_value(bar.borrow().opacity as f64);
    opacity.set_size_request(200, -1);
    opacity.set_draw_value(true);
    bar_body.append(&ui::row_with_icon("display-brightness-symbolic", "Opacity", &opacity));

    let menu_opacity = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.3, 1.0, 0.01);
    menu_opacity.set_value(bar.borrow().menu_opacity as f64);
    menu_opacity.set_size_request(200, -1);
    menu_opacity.set_draw_value(true);
    bar_body.append(&ui::row_with_icon(
        "view-app-grid-symbolic",
        "Start menu opacity",
        &menu_opacity,
    ));

    let blur = gtk::Switch::new();
    blur.set_active(bar.borrow().blur);
    blur.set_halign(gtk::Align::End);
    blur.set_valign(gtk::Align::Center);
    bar_body.append(&ui::row_with_icon("weather-fog-symbolic", "Backdrop blur", &blur));

    let blur_radius = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 40.0, 1.0);
    blur_radius.set_value(bar.borrow().blur_radius as f64);
    blur_radius.set_size_request(200, -1);
    blur_radius.set_draw_value(true);
    bar_body.append(&ui::row_with_icon("weather-fog-symbolic", "Blur radius", &blur_radius));

    let blur_hint = gtk::Label::new(Some(
        "Blur frosts the wallpaper behind the bar — it needs a wallpaper set and \
         the bar opacity below 1.0 to show through. Changes apply within ~1s.",
    ));
    blur_hint.set_xalign(0.0);
    blur_hint.set_wrap(true);
    blur_hint.add_css_class("metis-settings-hint");
    bar_body.append(&blur_hint);

    // -- Edge-bar border (mode / width / colors) --
    let bb = bar.borrow().bar_border.clone();

    let bb_mode = gtk::DropDown::from_strings(&["Theme accent", "Solid color", "Custom gradient"]);
    bb_mode.set_selected(match bb.mode {
        metis_config::BorderMode::Accent => 0,
        metis_config::BorderMode::Solid => 1,
        metis_config::BorderMode::Gradient => 2,
    });
    bar_body.append(&ui::row_with_icon("view-paged-symbolic", "Bar border", &bb_mode));

    let bb_width = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 8.0, 1.0);
    bb_width.set_value(bb.width_px as f64);
    bb_width.set_size_request(200, -1);
    bb_width.set_draw_value(true);
    bar_body.append(&ui::row_with_icon(
        "view-fullscreen-symbolic",
        "Bar border thickness",
        &bb_width,
    ));

    let bb_solid_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let bb_solid = color_dialog_button();
    bb_solid.set_rgba(&hex_to_rgba(&bb.color));
    bb_solid_box.append(&ui::row_with_icon(
        "applications-graphics-symbolic",
        "Border color",
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
    bb_grad_box.append(&ui::row_with_icon("starred-symbolic", "Gradient start", &bb_g1));
    bb_grad_box.append(&ui::row_with_icon("starred-symbolic", "Gradient middle", &bb_g2));
    bb_grad_box.append(&ui::row_with_icon("starred-symbolic", "Gradient end", &bb_g3));
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

    // ---- Windows (titlebar) ----------------------------------------------
    let (win_card, win_body) = ui::section_with_icon("Windows", "window-new-symbolic");

    let titlebar_opacity = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.3, 1.0, 0.01);
    titlebar_opacity.set_value(bar.borrow().titlebar_opacity as f64);
    titlebar_opacity.set_size_request(200, -1);
    titlebar_opacity.set_draw_value(true);
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
    let pill = bar.borrow().titlebar_pill_border.clone();

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
    let wb = bar.borrow().window_border.clone();

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

    {
        let bar = bar.clone();
        position_dd.connect_selected_notify(move |dd| {
            bar.borrow_mut().position = index_to_bar_position(dd.selected());
            save_bar(&bar.borrow());
        });
    }
    {
        let bar = bar.clone();
        edge_margin.connect_value_changed(move |s| {
            bar.borrow_mut().margin_top = s.value().round() as u32;
            save_bar(&bar.borrow());
        });
    }
    {
        let bar = bar.clone();
        opacity.connect_value_changed(move |s| {
            bar.borrow_mut().opacity = s.value() as f32;
            save_bar(&bar.borrow());
        });
    }
    {
        let bar = bar.clone();
        menu_opacity.connect_value_changed(move |s| {
            bar.borrow_mut().menu_opacity = s.value() as f32;
            save_bar(&bar.borrow());
        });
    }
    {
        let bar = bar.clone();
        titlebar_opacity.connect_value_changed(move |s| {
            bar.borrow_mut().titlebar_opacity = s.value() as f32;
            save_bar(&bar.borrow());
        });
    }
    {
        let bar = bar.clone();
        blur.connect_active_notify(move |s| {
            bar.borrow_mut().blur = s.is_active();
            save_bar(&bar.borrow());
        });
    }
    {
        let bar = bar.clone();
        blur_radius.connect_value_changed(move |s| {
            bar.borrow_mut().blur_radius = s.value() as f32;
            save_bar(&bar.borrow());
        });
    }
    {
        let bar = bar.clone();
        let pill_update_vis = pill_update_vis.clone();
        pill_mode.connect_selected_notify(move |dd| {
            let mode = match dd.selected() {
                1 => metis_config::BorderMode::Solid,
                2 => metis_config::BorderMode::Gradient,
                _ => metis_config::BorderMode::Accent,
            };
            bar.borrow_mut().titlebar_pill_border.mode = mode;
            pill_update_vis(mode);
            save_bar(&bar.borrow());
        });
    }
    {
        let bar = bar.clone();
        pill_width.connect_value_changed(move |s| {
            bar.borrow_mut().titlebar_pill_border.width_px = s.value() as f32;
            save_bar(&bar.borrow());
        });
    }
    {
        let bar = bar.clone();
        pill_solid.connect_rgba_notify(move |b| {
            bar.borrow_mut().titlebar_pill_border.color = rgba_to_hex(&b.rgba());
            save_bar(&bar.borrow());
        });
    }
    for (idx, btn) in [(0usize, &pill_g1), (1, &pill_g2), (2, &pill_g3)] {
        let bar = bar.clone();
        btn.connect_rgba_notify(move |b| {
            let hex = rgba_to_hex(&b.rgba());
            set_stops(&mut bar.borrow_mut().titlebar_pill_border.gradient, idx, hex);
            save_bar(&bar.borrow());
        });
    }
    {
        let bar = bar.clone();
        let wb_update_vis = wb_update_vis.clone();
        wb_mode.connect_selected_notify(move |dd| {
            let mode = match dd.selected() {
                1 => metis_config::BorderMode::Solid,
                2 => metis_config::BorderMode::Gradient,
                _ => metis_config::BorderMode::Accent,
            };
            bar.borrow_mut().window_border.mode = mode;
            wb_update_vis(mode);
            save_bar(&bar.borrow());
        });
    }
    {
        let bar = bar.clone();
        wb_width.connect_value_changed(move |s| {
            bar.borrow_mut().window_border.width_px = s.value() as f32;
            save_bar(&bar.borrow());
        });
    }
    {
        let bar = bar.clone();
        wb_solid.connect_rgba_notify(move |b| {
            bar.borrow_mut().window_border.color = rgba_to_hex(&b.rgba());
            save_bar(&bar.borrow());
        });
    }
    for (idx, btn) in [(0usize, &wb_g1), (1, &wb_g2), (2, &wb_g3)] {
        let bar = bar.clone();
        btn.connect_rgba_notify(move |b| {
            let hex = rgba_to_hex(&b.rgba());
            set_stops(&mut bar.borrow_mut().window_border.gradient, idx, hex);
            save_bar(&bar.borrow());
        });
    }
    {
        let bar = bar.clone();
        let bb_update_vis = bb_update_vis.clone();
        bb_mode.connect_selected_notify(move |dd| {
            let mode = match dd.selected() {
                1 => metis_config::BorderMode::Solid,
                2 => metis_config::BorderMode::Gradient,
                _ => metis_config::BorderMode::Accent,
            };
            bar.borrow_mut().bar_border.mode = mode;
            bb_update_vis(mode);
            save_bar(&bar.borrow());
        });
    }
    {
        let bar = bar.clone();
        bb_width.connect_value_changed(move |s| {
            bar.borrow_mut().bar_border.width_px = s.value() as f32;
            save_bar(&bar.borrow());
        });
    }
    {
        let bar = bar.clone();
        bb_solid.connect_rgba_notify(move |b| {
            bar.borrow_mut().bar_border.color = rgba_to_hex(&b.rgba());
            save_bar(&bar.borrow());
        });
    }
    for (idx, btn) in [(0usize, &bb_g1), (1, &bb_g2), (2, &bb_g3)] {
        let bar = bar.clone();
        btn.connect_rgba_notify(move |b| {
            let hex = rgba_to_hex(&b.rgba());
            set_stops(&mut bar.borrow_mut().bar_border.gradient, idx, hex);
            save_bar(&bar.borrow());
        });
    }

    scroller.upcast()
}

/// Set the `idx`-th gradient stop in a stop list, growing it if needed so a sparse
/// config still accepts edits to later stops.
fn set_stops(stops: &mut Vec<String>, idx: usize, hex: String) {
    while stops.len() <= idx {
        stops.push(hex.clone());
    }
    stops[idx] = hex;
}

fn save_bar(cfg: &metis_config::BarConfig) {
    // This page holds an in-memory copy of bar.json taken when it opened. Other
    // components (e.g. the dock's pin/unpin, which writes `taskbar_pinned`
    // directly) may have changed the file since. Re-read the on-disk config and
    // overwrite only the fields this page manages so we don't clobber theirs.
    let mut on_disk = metis_config::load_bar_config();
    on_disk.position = cfg.position;
    on_disk.margin_top = cfg.margin_top;
    on_disk.opacity = cfg.opacity;
    on_disk.menu_opacity = cfg.menu_opacity;
    on_disk.titlebar_opacity = cfg.titlebar_opacity;
    on_disk.titlebar_pill_border = cfg.titlebar_pill_border.clone();
    on_disk.window_border = cfg.window_border.clone();
    on_disk.bar_border = cfg.bar_border.clone();
    on_disk.blur = cfg.blur;
    on_disk.blur_radius = cfg.blur_radius;
    if let Err(err) = metis_config::save_bar_config(&on_disk) {
        tracing::warn!(%err, "failed to save bar.json");
    }
    // bar.json is watched by the shell (and re-read by the compositor for blur),
    // but nudge a reload so the change is instant.
    runtime::send("reload-bar");
}

fn set_accent(tokens: &mut metis_config::ThemeTokens, idx: usize, hex: String) {
    while tokens.accent.len() <= idx {
        tokens.accent.push(hex.clone());
    }
    tokens.accent[idx] = hex;
}

fn color_dialog_button() -> gtk::ColorDialogButton {
    let dialog = gtk::ColorDialog::new();
    dialog.set_with_alpha(false);
    gtk::ColorDialogButton::new(Some(dialog))
}

fn wire_color<F>(
    button: &gtk::ColorDialogButton,
    state: &Rc<RefCell<State>>,
    suppress: &Rc<Cell<bool>>,
    apply: F,
) where
    F: Fn(&mut metis_config::ThemeTokens, String) + 'static,
{
    let state = state.clone();
    let suppress = suppress.clone();
    button.connect_rgba_notify(move |btn| {
        if suppress.get() {
            return;
        }
        let hex = rgba_to_hex(&btn.rgba());
        let name = {
            let mut s = state.borrow_mut();
            apply(&mut s.tokens, hex);
            s.name.clone()
        };
        let tokens = state.borrow().tokens.clone();
        if let Err(err) = metis_config::save_theme_tokens(&name, &tokens) {
            tracing::warn!(%err, "failed to save theme tokens");
        }
        crate::theme::reapply();
        runtime::send("reload-theme");
    });
}

fn refresh_buttons(b: &ColorButtons, t: &metis_config::ThemeTokens, suppress: &Rc<Cell<bool>>) {
    suppress.set(true);
    b.accent.set_rgba(&hex_to_rgba(t.accent_primary()));
    b.accent2.set_rgba(&hex_to_rgba(t.accent_secondary()));
    b.error.set_rgba(&hex_to_rgba(&t.semantic.error));
    b.warning.set_rgba(&hex_to_rgba(&t.semantic.warning));
    b.success.set_rgba(&hex_to_rgba(&t.semantic.success));
    b.info.set_rgba(&hex_to_rgba(&t.semantic.info));
    b.payment.set_rgba(&hex_to_rgba(&t.semantic.payment));
    b.text.set_rgba(&hex_to_rgba(&t.text));
    suppress.set(false);
}

fn effective_name(mode: &ThemeMode) -> String {
    match mode {
        ThemeMode::Light => "light".to_string(),
        ThemeMode::Dark => "dark".to_string(),
        ThemeMode::System => {
            let dark = gtk::Settings::default()
                .map(|s| s.is_gtk_application_prefer_dark_theme())
                .unwrap_or(true);
            if dark { "dark" } else { "light" }.to_string()
        }
    }
}

fn hex_to_rgba(hex: &str) -> gdk::RGBA {
    gdk::RGBA::parse(hex).unwrap_or_else(|_| gdk::RGBA::new(0.0, 0.95, 1.0, 1.0))
}

/// Pick a readable text color (near-black or white) for content drawn on top of
/// `hex`, using perceived luminance so dark accents get light text and vice versa.
fn contrast_on(hex: &str) -> String {
    let h = hex.trim_start_matches('#');
    let (r, g, b) = if h.len() == 6 {
        (
            u8::from_str_radix(&h[0..2], 16).unwrap_or(0) as f32,
            u8::from_str_radix(&h[2..4], 16).unwrap_or(0) as f32,
            u8::from_str_radix(&h[4..6], 16).unwrap_or(0) as f32,
        )
    } else {
        (0.0, 0.0, 0.0)
    };
    let luminance = 0.299 * r + 0.587 * g + 0.114 * b;
    if luminance > 150.0 {
        "#0a0e14".to_string()
    } else {
        "#ffffff".to_string()
    }
}

fn rgba_to_hex(rgba: &gdk::RGBA) -> String {
    let to_u8 = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}",
        to_u8(rgba.red()),
        to_u8(rgba.green()),
        to_u8(rgba.blue())
    )
}

// ---- Style preview buttons ------------------------------------------------

/// A large toggle showing the current wallpaper with a mock window in the given
/// (light/dark) tone, plus a caption — mirrors GNOME's Style chooser.
fn style_button(label: &str, dark: bool, wallpaper: Option<&Path>) -> gtk::ToggleButton {
    let btn = gtk::ToggleButton::new();
    btn.add_css_class("metis-style-button");

    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 8);

    let overlay = gtk::Overlay::new();
    overlay.add_css_class("metis-style-preview");
    overlay.set_size_request(150, 94);

    let pic = gtk::Picture::new();
    pic.set_content_fit(gtk::ContentFit::Cover);
    pic.set_size_request(150, 94);
    if let Some(path) = wallpaper {
        pic.set_filename(Some(path));
    } else {
        pic.add_css_class(if dark { "metis-style-fallback-dark" } else { "metis-style-fallback-light" });
    }
    overlay.set_child(Some(&pic));

    // Mock window floated over the wallpaper to convey the light/dark surface.
    let mock = gtk::Box::new(gtk::Orientation::Vertical, 0);
    mock.add_css_class(if dark { "metis-style-mock-dark" } else { "metis-style-mock-light" });
    mock.set_halign(gtk::Align::Center);
    mock.set_valign(gtk::Align::Center);
    mock.set_size_request(82, 52);
    overlay.add_overlay(&mock);

    vbox.append(&overlay);

    let caption = gtk::Label::new(Some(label));
    caption.add_css_class("metis-style-caption");
    vbox.append(&caption);

    btn.set_child(Some(&vbox));
    btn
}

// ---- Wallpaper discovery + selection --------------------------------------

fn current_wallpaper() -> Option<PathBuf> {
    if let Some(p) = metis_config::load_wallpaper_config().path {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    list_wallpapers().into_iter().next()
}

/// Collect selectable wallpapers: user-imported pictures first, then bundled.
fn list_wallpapers() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    collect_images(&metis_config::wallpaper_store_dir(), &mut out, &mut seen);
    for dir in bundled_wallpaper_dirs() {
        collect_images(&dir, &mut out, &mut seen);
    }
    out
}

fn collect_images(dir: &Path, out: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut found: Vec<PathBuf> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let is_image = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| WALLPAPER_EXTS.contains(&e.to_ascii_lowercase().as_str()))
            .unwrap_or(false);
        if !is_image {
            continue;
        }
        let canon = path.canonicalize().unwrap_or(path.clone());
        if seen.insert(canon) {
            found.push(path);
        }
    }
    found.sort();
    out.extend(found);
}

/// Candidate directories holding the bundled wallpapers (resolved relative to the
/// settings binary, mirroring the compositor's lookup).
fn bundled_wallpaper_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for rel in [
                "assets/wallpapers",
                "../assets/wallpapers",
                "../../assets/wallpapers",
            ] {
                let p = dir.join(rel);
                if p.is_dir() {
                    dirs.push(p);
                }
            }
        }
    }
    dirs
}

fn populate_wallpapers(
    flow: &gtk::FlowBox,
    selected: Option<&Path>,
    bgcfg: &Rc<RefCell<metis_config::WallpaperConfig>>,
) {
    while let Some(child) = flow.first_child() {
        flow.remove(&child);
    }
    let selected_canon = selected.and_then(|p| p.canonicalize().ok());
    for path in list_wallpapers() {
        let is_selected = path
            .canonicalize()
            .ok()
            .zip(selected_canon.clone())
            .map(|(a, b)| a == b)
            .unwrap_or(false);
        flow.insert(&wallpaper_thumb(&path, is_selected, flow, bgcfg), -1);
    }
}

fn wallpaper_thumb(
    path: &Path,
    selected: bool,
    flow: &gtk::FlowBox,
    bgcfg: &Rc<RefCell<metis_config::WallpaperConfig>>,
) -> gtk::Widget {
    let btn = gtk::Button::new();
    btn.add_css_class("metis-wallpaper-thumb");
    btn.add_css_class("flat");
    if selected {
        btn.add_css_class("selected");
    }

    let overlay = gtk::Overlay::new();
    let pic = gtk::Picture::for_filename(path);
    pic.set_content_fit(gtk::ContentFit::Cover);
    pic.set_size_request(150, 92);
    pic.add_css_class("metis-wallpaper-image");
    overlay.set_child(Some(&pic));

    if selected {
        let check = gtk::Image::from_icon_name("emblem-ok-symbolic");
        check.add_css_class("metis-wallpaper-check");
        check.set_halign(gtk::Align::End);
        check.set_valign(gtk::Align::End);
        check.set_margin_end(6);
        check.set_margin_bottom(6);
        overlay.add_overlay(&check);
    }
    btn.set_child(Some(&overlay));

    {
        let path = path.to_path_buf();
        let flow = flow.clone();
        let bgcfg = bgcfg.clone();
        btn.connect_clicked(move |_| {
            select_picture(&bgcfg, &path);
            populate_wallpapers(&flow, Some(&path), &bgcfg);
        });
    }
    btn.upcast()
}

/// Switch the background to the given picture (preserving solid/gradient fields)
/// and persist + apply it.
fn select_picture(bgcfg: &Rc<RefCell<metis_config::WallpaperConfig>>, path: &Path) {
    {
        let mut cfg = bgcfg.borrow_mut();
        cfg.kind = metis_config::BackgroundKind::Image;
        cfg.path = Some(path.to_string_lossy().to_string());
    }
    save_and_apply(&bgcfg.borrow());
}

/// Persist the background config (live via the compositor, durable via
/// `wallpaper.json` which the compositor also reads on next start).
fn save_and_apply(cfg: &metis_config::WallpaperConfig) {
    if let Err(err) = metis_config::save_wallpaper_config(cfg) {
        tracing::warn!(%err, "failed to save wallpaper.json");
    }
    runtime::apply_background();
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

fn direction_to_index(dir: metis_config::GradientDirection) -> u32 {
    use metis_config::GradientDirection as D;
    match dir {
        D::Vertical => 0,
        D::VerticalReverse => 1,
        D::Horizontal => 2,
        D::HorizontalReverse => 3,
        D::Diagonal => 4,
        D::DiagonalReverse => 5,
    }
}

fn index_to_direction(idx: u32) -> metis_config::GradientDirection {
    use metis_config::GradientDirection as D;
    match idx {
        1 => D::VerticalReverse,
        2 => D::Horizontal,
        3 => D::HorizontalReverse,
        4 => D::Diagonal,
        5 => D::DiagonalReverse,
        _ => D::Vertical,
    }
}

/// Open a file chooser for a custom picture; copies it into the wallpaper store
/// then invokes `on_pick` with the stored copy's path.
fn pick_picture<F>(parent: Option<&gtk::Window>, on_pick: F)
where
    F: Fn(PathBuf) + 'static,
{
    let dialog = gtk::FileDialog::new();
    dialog.set_title("Choose a picture");
    let filter = gtk::FileFilter::new();
    filter.set_name(Some("Images"));
    for ext in WALLPAPER_EXTS {
        filter.add_pattern(&format!("*.{ext}"));
        filter.add_pattern(&format!("*.{}", ext.to_ascii_uppercase()));
    }
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    dialog.set_filters(Some(&filters));

    dialog.open(parent, gio::Cancellable::NONE, move |res| {
        let Ok(file) = res else { return };
        let Some(src) = file.path() else { return };
        match import_picture(&src) {
            Ok(stored) => on_pick(stored),
            Err(err) => tracing::warn!(%err, "failed to import wallpaper"),
        }
    });
}

fn import_picture(src: &Path) -> std::io::Result<PathBuf> {
    let dir = metis_config::wallpaper_store_dir();
    std::fs::create_dir_all(&dir)?;
    let name = src
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "wallpaper".to_string());
    let mut dest = dir.join(&name);
    // Avoid clobbering an existing import with the same name.
    if dest.exists() && std::fs::canonicalize(&dest).ok() != std::fs::canonicalize(src).ok() {
        let stem = src.file_stem().map(|s| s.to_string_lossy().to_string());
        let ext = src.extension().map(|e| e.to_string_lossy().to_string());
        let unique = format!(
            "{}-{}",
            stem.as_deref().unwrap_or("wallpaper"),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        );
        dest = match ext {
            Some(ext) => dir.join(format!("{unique}.{ext}")),
            None => dir.join(unique),
        };
    }
    if std::fs::canonicalize(&dest).ok() != std::fs::canonicalize(src).ok() {
        std::fs::copy(src, &dest)?;
    }
    Ok(dest)
}
