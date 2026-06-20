//! Appearance: theme mode, accent + semantic colours (written to `themes/*.json`),
//! and bar opacity/blur (written to `bar.json`). The shell's file watchers apply
//! colour/bar changes live; a `reload-theme` runtime command re-themes on a mode
//! switch (since the mode lives in `config.json`, which isn't watched).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::gdk;
use gtk::prelude::*;

use metis_config::ThemeMode;

use crate::{runtime, ui};

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

    // ---- Theme mode -------------------------------------------------------
    let (mode_card, mode_body) = ui::section("Theme");
    let mode_dd = gtk::DropDown::from_strings(&["Dark", "Light", "System"]);
    mode_dd.set_selected(match mode {
        ThemeMode::Dark => 0,
        ThemeMode::Light => 1,
        ThemeMode::System => 2,
    });
    mode_body.append(&ui::row("Mode", &mode_dd));
    let hint = gtk::Label::new(Some(
        "Colours below edit the active theme file (dark or light).",
    ));
    hint.set_xalign(0.0);
    hint.add_css_class("metis-settings-hint");
    mode_body.append(&hint);
    content.append(&mode_card);

    // ---- Colours ----------------------------------------------------------
    let (color_card, color_body) = ui::section("Colours");
    let buttons = ColorButtons {
        accent: color_dialog_button(),
        accent2: color_dialog_button(),
        error: color_dialog_button(),
        warning: color_dialog_button(),
        success: color_dialog_button(),
        info: color_dialog_button(),
        payment: color_dialog_button(),
    };
    color_body.append(&ui::row("Accent", &buttons.accent));
    color_body.append(&ui::row("Accent (secondary)", &buttons.accent2));
    color_body.append(&ui::row("Error", &buttons.error));
    color_body.append(&ui::row("Warning", &buttons.warning));
    color_body.append(&ui::row("Success", &buttons.success));
    color_body.append(&ui::row("Info", &buttons.info));
    color_body.append(&ui::row("Payment", &buttons.payment));
    content.append(&color_card);

    refresh_buttons(&buttons, &state.borrow().tokens, &suppress);

    // Wire each colour button to its token field.
    wire_color(&buttons.accent, &state, &suppress, |t, hex| set_accent(t, 0, hex));
    wire_color(&buttons.accent2, &state, &suppress, |t, hex| set_accent(t, 1, hex));
    wire_color(&buttons.error, &state, &suppress, |t, hex| t.semantic.error = hex);
    wire_color(&buttons.warning, &state, &suppress, |t, hex| t.semantic.warning = hex);
    wire_color(&buttons.success, &state, &suppress, |t, hex| t.semantic.success = hex);
    wire_color(&buttons.info, &state, &suppress, |t, hex| t.semantic.info = hex);
    wire_color(&buttons.payment, &state, &suppress, |t, hex| t.semantic.payment = hex);

    // Mode dropdown: persist preference, re-target the colour editor, re-theme.
    {
        let state = state.clone();
        let buttons = buttons.clone();
        let suppress = suppress.clone();
        mode_dd.connect_selected_notify(move |dd| {
            let mode = match dd.selected() {
                1 => ThemeMode::Light,
                2 => ThemeMode::System,
                _ => ThemeMode::Dark,
            };
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
            runtime::send("reload-theme");
        });
    }

    // ---- Bar (opacity / blur) --------------------------------------------
    let bar = Rc::new(RefCell::new(metis_config::load_bar_config()));
    let (bar_card, bar_body) = ui::section("Edge bar");

    let opacity = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.3, 1.0, 0.01);
    opacity.set_value(bar.borrow().opacity as f64);
    opacity.set_size_request(200, -1);
    opacity.set_draw_value(true);
    bar_body.append(&ui::row("Opacity", &opacity));

    let blur = gtk::Switch::new();
    blur.set_active(bar.borrow().blur);
    blur.set_halign(gtk::Align::End);
    bar_body.append(&ui::row("Backdrop blur", &blur));

    let blur_radius = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 40.0, 1.0);
    blur_radius.set_value(bar.borrow().blur_radius as f64);
    blur_radius.set_size_request(200, -1);
    blur_radius.set_draw_value(true);
    bar_body.append(&ui::row("Blur radius", &blur_radius));
    content.append(&bar_card);

    {
        let bar = bar.clone();
        opacity.connect_value_changed(move |s| {
            bar.borrow_mut().opacity = s.value() as f32;
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

    scroller.upcast()
}

fn save_bar(cfg: &metis_config::BarConfig) {
    if let Err(err) = metis_config::save_bar_config(cfg) {
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

fn rgba_to_hex(rgba: &gdk::RGBA) -> String {
    let to_u8 = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}",
        to_u8(rgba.red()),
        to_u8(rgba.green()),
        to_u8(rgba.blue())
    )
}
