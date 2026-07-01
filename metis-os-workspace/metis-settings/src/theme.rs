//! Apply the same Metis theme tokens the shell uses, so the settings window looks
//! native to the desktop. Builds CSS from the shared `metis_config` stylesheet.

use std::cell::RefCell;

use gtk::CssProvider;
use gtk::STYLE_PROVIDER_PRIORITY_APPLICATION;

use metis_config::{ThemeMode, ThemeTokens};

thread_local! {
    /// The two display providers (shared bar stylesheet + settings chrome), kept
    /// so the theme can be re-applied live when the mode/colours change.
    static PROVIDERS: RefCell<Option<(CssProvider, CssProvider)>> = const { RefCell::new(None) };
}

/// Resolve the currently active theme tokens (honouring the saved mode, with a
/// GTK fallback for `system`).
pub fn active_tokens() -> ThemeTokens {
    let mode = metis_config::load_theme_preference().unwrap_or(ThemeMode::Dark);
    let name = match mode {
        ThemeMode::Light => "light",
        ThemeMode::Dark => "dark",
        ThemeMode::System => {
            if prefers_dark() {
                "dark"
            } else {
                "light"
            }
        }
    };
    metis_config::load_theme_tokens(name)
}

fn prefers_dark() -> bool {
    gtk::Settings::default()
        .map(|s| s.is_gtk_application_prefer_dark_theme())
        .unwrap_or(true)
}

pub fn install() {
    let Some(display) = gtk::gdk::Display::default() else {
        return;
    };

    let base = CssProvider::new();
    gtk::style_context_add_provider_for_display(&display, &base, STYLE_PROVIDER_PRIORITY_APPLICATION);
    let extra = CssProvider::new();
    gtk::style_context_add_provider_for_display(
        &display,
        &extra,
        STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
    );
    PROVIDERS.with(|p| *p.borrow_mut() = Some((base, extra)));
    reapply();
}

/// Re-read the active theme and reload both providers. Call after the user changes
/// the theme mode or any colour so the settings window (and its titlebar) update
/// live — mirroring the shell's own live theme reload.
pub fn reapply() {
    // Flip GTK's built-in Adwaita variant so default widget chrome (dropdowns,
    // popovers, scales, switches, scrollbars) switches light/dark too — our CSS
    // only restyles our own classes, not GTK's internal widget nodes.
    if let Some(settings) = gtk::Settings::default() {
        settings.set_gtk_application_prefer_dark_theme(active_mode_is_dark());
    }
    let tokens = active_tokens();
    PROVIDERS.with(|p| {
        if let Some((base, extra)) = p.borrow().as_ref() {
            // The shared stylesheet sets `window { background-color: transparent }`
            // so the shell's layer-shell overlays (bar, popovers) can show through.
            // The settings app is a real opaque toplevel, though — without forcing it
            // solid, a transparent window behind it (e.g. a terminal) bleeds through
            // the body. Append the opaque override LAST in the *same* provider so it
            // always wins by source order, regardless of cross-provider cascade.
            let mut css = metis_config::build_stylesheet(&tokens);
            css.push_str(&format!(
                "\nwindow, window.background {{ background-color: {}; }}\n",
                tokens.bg
            ));
            base.load_from_data(&css);
            extra.load_from_data(&settings_css(&tokens));
        }
    });
}

/// Whether the active theme resolves to a dark variant.
fn active_mode_is_dark() -> bool {
    match metis_config::load_theme_preference().unwrap_or(ThemeMode::Dark) {
        ThemeMode::Dark => true,
        ThemeMode::Light => false,
        ThemeMode::System => prefers_dark(),
    }
}

/// Settings-window chrome that isn't part of the shared bar stylesheet.
fn settings_css(t: &ThemeTokens) -> String {
    let bg = &t.bg;
    let surface = &t.surface;
    let raised = &t.surface_raised;
    let border = &t.border;
    let text = &t.text;
    let muted = &t.text_muted;
    let accent = t.accent_primary();
    let on_accent = &t.text_on_accent;
    let error = &t.semantic.error;
    let warning = &t.semantic.warning;
    let rl = t.radius_lg;
    let rs = t.radius_sm;
    let accent2 = t.accent_secondary();
    format!(
        r#"
        /* The shared bar stylesheet makes every `window` transparent for the
           layer-shell overlays; in the settings app we want solid windows so
           spawned dialogs (e.g. the colour picker) aren't see-through. */
        window {{ background-color: {bg}; color: {text}; }}
        window.dialog, window.csd, .colorchooser {{ background-color: {bg}; color: {text}; }}

        /* Window + CSD titlebar so the whole frame tracks the active theme. */
        .metis-settings-window {{ background-color: {bg}; color: {text}; }}
        windowhandle, headerbar, .titlebar {{
            background-color: {surface};
            background-image: none;
            color: {text};
            border-bottom: 1px solid {border};
            box-shadow: none;
        }}
        headerbar label, .titlebar label {{ color: {text}; }}
        headerbar button, windowcontrols button {{
            color: {text};
            background-color: transparent;
            box-shadow: none;
            border: none;
        }}
        headerbar button:hover, windowcontrols button:hover {{ background-color: {raised}; }}
        windowcontrols button image {{ color: {text}; }}

        .metis-settings-root {{ background-color: {bg}; }}

        /* Dividers between sidebar/content + any separators: theme-coloured, flat. */
        separator {{
            background-color: {border};
            background-image: none;
            min-width: 1px;
            min-height: 1px;
            border: none;
            box-shadow: none;
        }}
        .metis-settings-sidebar {{ box-shadow: none; border: none; }}
        /* Kill GTK's dark scroll edge fades (undershoot) and bounce glows
           (overshoot) on every edge — they don't suit the light theme. */
        undershoot.top, undershoot.bottom, undershoot.left, undershoot.right,
        overshoot.top, overshoot.bottom, overshoot.left, overshoot.right {{
            background-color: transparent;
            background-image: none;
            box-shadow: none;
            border: none;
        }}

        .metis-settings-sidebar {{ background-color: {surface}; }}
        .metis-settings-sidebar list {{ background-color: transparent; padding: 8px; }}
        .metis-settings-sidebar row {{ border-radius: {rl}px; padding: 8px 12px; }}
        .metis-settings-sidebar row label {{ color: {text}; }}
        .metis-settings-sidebar row image {{ color: {muted}; -gtk-icon-size: 16px; }}
        .metis-settings-sidebar row:hover {{ background-color: {raised}; }}
        .metis-settings-sidebar row:selected {{ background-color: {accent}; }}
        .metis-settings-sidebar row:selected label {{ color: {on_accent}; font-weight: 700; }}
        .metis-settings-sidebar row:selected image {{ color: {on_accent}; }}
        .metis-settings-nav-section {{
            color: {muted};
            font-size: 11px;
            font-weight: 700;
            letter-spacing: 0.04em;
            text-transform: uppercase;
            padding: 12px 12px 4px;
        }}

        .metis-settings-page {{ background-color: {bg}; }}
        .metis-settings-title {{ font-size: 26px; font-weight: 800; color: {text}; }}
        .metis-settings-section {{
            background-color: {surface};
            border: 1px solid {border};
            border-radius: {rl}px;
            padding: 18px;
        }}
        .metis-settings-section-header {{ margin-bottom: 2px; }}
        .metis-settings-section-title {{
            font-size: 12px;
            font-weight: 800;
            color: {muted};
            letter-spacing: 1px;
            text-transform: uppercase;
        }}
        .metis-settings-section-icon {{ color: {accent}; }}
        .metis-settings-row {{ padding: 6px 0; }}
        .metis-settings-row label {{ color: {text}; }}
        .metis-settings-row-icon {{ color: {muted}; }}
        .metis-settings-hint {{ color: {muted}; font-size: 12px; }}
        .metis-settings-display-chip {{
            padding: 8px 12px;
            border-radius: {rs}px;
            border: 1px solid {border};
            background-color: {raised};
            background-image: none;
            box-shadow: none;
            color: {text};
        }}
        .metis-settings-display-chip:hover {{ background-color: {surface}; }}
        .metis-settings-display-chip image {{ color: {muted}; -gtk-icon-style: symbolic; }}
        .metis-settings-display-chip label {{ color: {text}; font-size: 13px; }}
        .metis-settings-display-chip-active {{
            border-color: {accent};
            background-color: {surface};
        }}
        .metis-settings-display-chip-active image {{ color: {accent}; }}
        .metis-settings-value {{ color: {text}; font-weight: 600; font-feature-settings: "tnum"; }}
        .metis-bt-battery-low {{ color: {warning}; font-weight: 700; }}
        .metis-settings-list {{
            background-color: {raised};
            border: 1px solid {border};
            border-radius: {rl}px;
            padding: 8px 12px;
        }}
        .metis-settings-list row {{ padding: 8px 10px; background-color: transparent; }}
        .metis-settings-list,
        .metis-settings-list row,
        .metis-settings-list label {{ color: {text}; }}
        .metis-settings-list row:hover {{ background-color: {surface}; }}

        /* Dropdowns (e.g. the Mode selector) + their popups. */
        dropdown, dropdown > button {{
            background-color: {raised};
            background-image: none;
            color: {text};
            border: 1px solid {border};
            border-radius: {rs}px;
            box-shadow: none;
        }}
        dropdown > button:hover {{ background-color: {surface}; }}
        dropdown arrow, dropdown button image {{ color: {text}; }}
        popover > contents, popover.background > contents, popover.menu > contents {{
            background-color: {raised};
            color: {text};
            border: 1px solid {border};
            border-radius: {rs}px;
        }}
        popover listview, popover row, popover label {{
            background-color: transparent;
            color: {text};
        }}
        popover row:selected, popover row:hover {{
            background-color: {accent};
            color: {on_accent};
        }}

        /* Sliders + switches readable in both themes. */
        scale trough {{ background-color: {border}; }}
        scale highlight {{ background-color: {accent}; }}
        scale value {{ color: {muted}; }}
        scale slider {{ background-color: {text}; }}

        /* Text inputs (search boxes, CalDAV fields, etc.). */
        entry, entry.flat, spinbutton {{
            background-color: {raised};
            background-image: none;
            color: {text};
            border: 1px solid {border};
            border-radius: {rs}px;
            box-shadow: none;
            caret-color: {text};
        }}
        entry text, spinbutton text {{ color: {text}; background-color: transparent; }}
        entry text placeholder, entry > text > placeholder {{ color: {muted}; opacity: 1; }}
        entry:focus-within {{ border-color: {accent}; }}
        entry image, entry > image {{ color: {muted}; }}

        /* Generic buttons (Search, Rescan, Connect, trash, …). The more specific
           headerbar/dropdown rules above keep their own styling. */
        button {{
            background-color: {raised};
            background-image: none;
            color: {text};
            border: 1px solid {border};
            border-radius: {rs}px;
            box-shadow: none;
        }}
        button:hover {{ background-color: {surface}; }}
        button:active, button:checked {{ background-color: {surface}; }}
        button label {{ color: {text}; }}
        button image {{ color: {text}; }}
        button:disabled {{ color: {muted}; }}
        button:disabled label {{ color: {muted}; }}
        /* Primary action buttons stay accent-coloured. */
        button.suggested-action {{
            background-color: {accent};
            border-color: {accent};
        }}
        button.suggested-action label {{ color: {on_accent}; }}
        button.destructive-action image {{ color: {error}; }}
        /* Flat buttons (Add Picture…) have no chrome until hovered. */
        button.flat {{ background-color: transparent; border-color: transparent; }}
        button.flat:hover {{ background-color: {raised}; }}

        /* Appearance · Style preview buttons */
        .metis-style-button {{
            background-color: transparent;
            background-image: none;
            border: 2px solid transparent;
            border-radius: {rl}px;
            padding: 6px;
            box-shadow: none;
        }}
        .metis-style-button:hover {{ background-color: {raised}; }}
        .metis-style-button:checked, .metis-style-button:active {{
            background-color: transparent;
            border-color: {accent};
        }}
        .metis-style-preview {{ border-radius: 10px; background-color: {surface}; }}
        .metis-style-preview picture {{ border-radius: 10px; }}
        .metis-style-fallback-light {{ background-color: #f2f2f4; }}
        .metis-style-fallback-dark {{ background-color: #1c1c20; }}
        .metis-style-mock-light {{
            background-color: #ffffff;
            border-radius: 7px;
            border-top: 9px solid #e6e6e9;
            box-shadow: 0 3px 8px rgba(0,0,0,0.35);
        }}
        .metis-style-mock-dark {{
            background-color: #2b2b30;
            border-radius: 7px;
            border-top: 9px solid #3a3a40;
            box-shadow: 0 3px 8px rgba(0,0,0,0.45);
        }}
        .metis-style-caption {{ color: {text}; font-weight: 600; }}

        /* Appearance · Wallpaper grid */
        .metis-wallpaper-grid {{ padding: 4px; }}
        .metis-wallpaper-grid flowboxchild {{
            padding: 0;
            background-color: transparent;
            border-radius: 10px;
        }}
        .metis-wallpaper-thumb {{
            background-color: transparent;
            background-image: none;
            border: 2px solid transparent;
            border-radius: 10px;
            padding: 0;
            box-shadow: none;
        }}
        .metis-wallpaper-thumb:hover {{ border-color: {border}; background-color: transparent; }}
        .metis-wallpaper-thumb.selected {{ border-color: {accent}; }}
        .metis-wallpaper-image {{ border-radius: 8px; }}
        .metis-wallpaper-check {{
            color: {on_accent};
            background-color: {accent};
            border-radius: 999px;
            padding: 4px;
        }}

        .metis-settings-row colorswatch {{ border-radius: 6px; }}
        button.metis-accent2-hint {{ color: {accent2}; }}

        /* Segmented pill tabs (e.g. Network: Wireless / Wired / Proxy). */
        .metis-settings-tabs {{
            padding: 3px;
            background-color: {raised};
            border: 1px solid {border};
            border-radius: 999px;
        }}
        button.metis-settings-tab {{
            padding: 6px 18px;
            min-height: 0;
            border-radius: 999px;
            border: 1px solid transparent;
            background-color: transparent;
            background-image: none;
            box-shadow: none;
            color: {muted};
        }}
        button.metis-settings-tab:hover {{
            background-color: {surface};
            color: {text};
        }}
        button.metis-settings-tab:checked,
        button.metis-settings-tab:active {{
            background-color: {accent};
            border-color: {accent};
            color: {on_accent};
        }}
        button.metis-settings-tab:checked label {{ color: {on_accent}; font-weight: 700; }}
        "#
    )
}
