use crate::theme::ThemeTokens;

pub fn build_stylesheet(theme: &ThemeTokens) -> String {
    let accent = theme.accent_primary();
    // Bare `r, g, b` triplets let the stylesheet inline accent-tinted rgba() with
    // per-rule opacities, so hover/selection states track the theme accent.
    let accent_rgb = theme.accent_rgb();
    let accent2 = theme.accent_secondary();
    let accent2_rgb = theme.accent_secondary_rgb();
    let on_accent = theme.text_on_accent.clone();
    let text_rgb = theme.text_rgb();
    let surface = theme.surface_rgba();
    let raised = theme.surface_raised.clone();
    let shadow = theme.shadow_ambient.clone();
    // The gradient brand icon washes out against the pale light-mode bar, so add a
    // soft drop shadow under it to lift it off the surface. A dark shadow is
    // effectively invisible on the dark theme, so it's only emitted for light.
    let launcher_icon_shadow = if theme.mode.eq_ignore_ascii_case("light") {
        "-gtk-icon-shadow: 0 1px 3px rgba(0, 0, 0, 0.55);"
    } else {
        ""
    };
    let rs = theme.radius_sm;
    let rm = theme.radius_md;
    let rl = theme.radius_lg;
    // Semantic status palette (notifications + state highlights).
    let c_error = theme.semantic.error.clone();
    let c_warning = theme.semantic.warning.clone();
    let c_success = theme.semantic.success.clone();
    let c_info = theme.semantic.info.clone();
    let c_payment = theme.semantic.payment.clone();
    let c_error_rgb = crate::theme::rgb_triplet_from_hex(&theme.semantic.error);
    let c_warning_rgb = crate::theme::rgb_triplet_from_hex(&theme.semantic.warning);
    let c_success_rgb = crate::theme::rgb_triplet_from_hex(&theme.semantic.success);
    let c_info_rgb = crate::theme::rgb_triplet_from_hex(&theme.semantic.info);
    let c_payment_rgb = crate::theme::rgb_triplet_from_hex(&theme.semantic.payment);

    // Optional DE-wide font family/size; empty unless the user customized them.
    let font_decls = theme.font_declarations();

    format!(
        r#"
    window {{
        background-color: transparent;
        {font_decls}
    }}

    .metis-bar-window {{
        background-color: transparent;
    }}

    .metis-bar-outer {{
        background-color: transparent;
    }}

    .metis-bar-pill {{
        background-color: {surface};
        border: 1px solid {border};
        padding: 0 14px;
        color: {text};
    }}

    .metis-bar-full {{
        border-radius: 999px;
        padding: 0 20px;
        box-shadow: 0 3px 10px rgba(0, 0, 0, 0.42), 0 1px 3px rgba(0, 0, 0, 0.30);
    }}

    .metis-bar-floating {{
        border-radius: 999px;
        padding: 0 14px;
        box-shadow: 0 3px 10px rgba(0, 0, 0, 0.45), 0 1px 0 rgba(255, 255, 255, 0.06) inset;
    }}

    /* Bar widget buttons share one geometry across every interaction state so
       the icon never shifts on hover or press; only decoration changes. */
    .metis-bar-widget,
    button.metis-bar-widget,
    button.metis-bar-widget:hover,
    button.metis-bar-widget:active,
    button.metis-bar-widget:checked,
    button.metis-bar-widget:focus,
    menubutton.metis-bar-widget,
    menubutton.metis-bar-widget > button,
    menubutton.metis-bar-widget:hover > button {{
        padding: 0 8px;
        margin: 0;
        min-height: 0;
        border: none;
        outline: none;
        border-radius: {rs}px;
    }}

    .metis-bar-widget,
    button.metis-bar-widget,
    menubutton.metis-bar-widget,
    menubutton.metis-bar-widget > button {{
        background-image: none;
        background-color: transparent;
        box-shadow: none;
        color: {text};
    }}

    /* Hover: cyan gradient rising from the bottom into the grey highlight, with
       a thin 1px cyan line under the icon box (inset shadow adds no layout). */
    button.metis-bar-widget:hover,
    menubutton.metis-bar-widget:hover > button {{
        background-image: linear-gradient(to top,
            rgba({accent_rgb}, 0.34) 0%,
            rgba({accent2_rgb}, 0.12) 45%,
            rgba(255, 255, 255, 0.06) 100%);
        box-shadow: inset 0 -1px 0 0 rgba({accent_rgb}, 0.95);
        border-radius: {rs}px {rs}px 0 0;
    }}

    /* Match the hover style exactly so the open icon looks identical whether or
       not the pointer is over it. Specificity is raised to beat `:hover`. */
    button.metis-bar-dropdown-active,
    button.metis-bar-widget.metis-bar-dropdown-active,
    button.metis-bar-widget.metis-bar-dropdown-active:hover,
    menubutton.metis-bar-widget.metis-bar-dropdown-active > button,
    menubutton.metis-bar-widget.metis-bar-dropdown-active:hover > button {{
        background-image: linear-gradient(to top,
            rgba({accent_rgb}, 0.34) 0%,
            rgba({accent2_rgb}, 0.12) 45%,
            rgba(255, 255, 255, 0.06) 100%);
        box-shadow: inset 0 -1px 0 0 rgba({accent_rgb}, 0.95);
        border-radius: {rs}px {rs}px 0 0;
    }}

    /* The clock MenuButton uses a custom time/date child; never reserve space for
       the default dropdown arrow. */
    menubutton.metis-bar-widget > button > .arrow {{
        min-width: 0;
        min-height: 0;
        -gtk-icon-size: 0;
        margin: 0;
        padding: 0;
    }}

    .metis-bar-launcher {{
        padding: 0 6px;
        margin-right: 2px;
    }}

    .metis-bar-launcher-icon {{
        -gtk-icon-style: regular;
        {launcher_icon_shadow}
    }}

    .metis-bar-sys-icon {{
        padding: 0 5px;
        border: none;
        border-radius: {rs}px;
        min-height: 0;
        background-color: transparent;
    }}

    .metis-bar-notifications {{
        padding: 0 4px;
        background-color: transparent;
    }}

    .metis-bar-notifications:hover {{
        background-color: transparent;
    }}

    .metis-bar-notif-overlay {{
        background-color: transparent;
        min-width: 18px;
        min-height: 18px;
    }}

    .metis-bar-notif-icon {{
        font-size: 14px;
        padding: 0 2px;
        background-color: transparent;
    }}

    .metis-bar-notif-badge {{
        font-size: 8px;
        font-weight: 700;
        color: {on_accent};
        background-color: {accent};
        border-radius: 999px;
        min-width: 12px;
        min-height: 12px;
        padding: 0 3px;
        border: 1px solid rgba(0, 0, 0, 0.35);
        box-shadow: none;
    }}

    .metis-bar-dropdown-revealer {{
        background-color: transparent;
    }}

    .metis-bar-dropdown-shell {{
        background-color: transparent;
    }}

    .metis-bar-dropdown-panel {{
        background-color: {raised};
        border: 1px solid {border};
        border-radius: {rl}px;
        padding: 14px 16px;
        box-shadow: {shadow},
                    inset 0 1px 0 rgba(255, 255, 255, 0.05);
    }}

    .metis-bar-clock {{
        margin-left: 4px;
        padding: 0 8px 0 4px;
        min-height: 0;
    }}

    .metis-bar-clock-time {{
        font-size: 13px;
        font-weight: 600;
        letter-spacing: 0.02em;
        color: {text};
    }}

    .metis-bar-clock-date {{
        font-size: 11px;
        color: {muted};
    }}

    .metis-bar-pill-vertical {{
        border-radius: {rm}px;
        padding: 10px 6px;
    }}

    .metis-bar-outer-vertical {{
        min-width: 0;
    }}

    .metis-bar-workspaces {{
        padding: 2px 6px;
    }}

    .metis-bar-ws-dot {{
        min-width: 7px;
        min-height: 7px;
        padding: 0;
        margin: 0;
        border-radius: 999px;
        background-color: transparent;
        border: 1.5px solid rgba({text_rgb}, 0.55);
    }}

    .metis-bar-icon {{
        -gtk-icon-style: regular;
        background-color: transparent;
    }}

    .metis-bar-ws-dot-idle {{
        opacity: 0.5;
    }}

    .metis-bar-ws-dot:hover {{
        background-color: rgba({text_rgb}, 0.30);
    }}

    .metis-bar-ws-dot-active {{
        background-color: {text};
        border-color: {text};
        box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.25);
    }}

    .metis-notif-dnd-label {{
        font-size: 11px;
        color: {muted};
    }}

    popover.metis-bar-popover {{
        background-color: transparent;
        padding: 0;
        border: none;
        box-shadow: none;
    }}

    popover.metis-bar-popover contents {{
        padding: 0;
        border: none;
        background-color: transparent;
    }}

    popover.metis-bar-popover > arrow {{
        background-color: {raised};
        border: 1px solid {border};
        min-width: 16px;
        min-height: 8px;
    }}

    popover.metis-notif-popover {{
        padding: 0;
    }}

    .metis-notif-scrolled {{
        min-width: 372px;
    }}

    .metis-notif-scrolled scrollbar.vertical {{
        min-width: 8px;
        margin: 4px 2px;
    }}

    .metis-notif-scrolled scrollbar.vertical slider {{
        min-width: 6px;
        border-radius: 999px;
        background-color: rgba(255, 255, 255, 0.18);
    }}

    .metis-notif-scrolled scrollbar.vertical slider:hover {{
        background-color: rgba(255, 255, 255, 0.28);
    }}

    .metis-notif-empty {{
        font-size: 12px;
        color: {muted};
        padding: 24px 8px;
    }}

    .metis-notif-card {{
        background-color: rgba(12, 16, 22, 0.92);
        border-radius: 10px;
        border: 1px solid rgba(255, 255, 255, 0.08);
        padding: 12px 14px;
    }}

    .metis-notif-icon {{
        -gtk-icon-size: 20px;
        margin-top: 1px;
    }}

    .metis-notif-count {{
        min-width: 18px;
        padding: 1px 7px;
        border-radius: 999px;
        font-size: 11px;
        font-weight: 700;
        color: currentColor;
        background-color: rgba(255, 255, 255, 0.12);
    }}

    .metis-notif-clear {{
        padding: 5px 14px;
        border-radius: 8px;
        font-size: 12px;
        font-weight: 600;
        color: {muted};
        background-color: rgba(255, 255, 255, 0.05);
        background-image: none;
        border: 1px solid {border};
        box-shadow: none;
    }}
    .metis-notif-clear:hover {{
        color: {text};
        background-color: rgba(255, 255, 255, 0.10);
    }}
    .metis-notif-clear:disabled {{
        opacity: 0.45;
    }}

    .metis-notif-accent {{
        border-radius: 10px 0 0 10px;
        min-width: 5px;
    }}

    .metis-notif-diamond {{
        min-width: 40px;
        min-height: 40px;
        border-radius: 6px;
        border: 1.5px solid currentColor;
        transform: rotate(45deg);
    }}

    .metis-notif-diamond-icon {{
        font-size: 15px;
        font-weight: 700;
        transform: rotate(-45deg);
    }}

    .metis-notif-title {{
        font-size: 14px;
        font-weight: 700;
    }}

    .metis-notif-message {{
        font-size: 12px;
        color: {muted};
        line-height: 1.35;
    }}

    .metis-notif-card-error {{
        box-shadow: 0 0 18px rgba({c_error_rgb}, 0.22);
        border-color: rgba({c_error_rgb}, 0.35);
        color: {c_error};
    }}

    .metis-notif-card-error .metis-notif-accent {{
        background-color: {c_error};
    }}

    .metis-notif-card-error .metis-notif-title {{
        color: {c_error};
    }}

    .metis-notif-card-notify {{
        box-shadow: 0 0 18px rgba({c_warning_rgb}, 0.22);
        border-color: rgba({c_warning_rgb}, 0.35);
        color: {c_warning};
    }}

    .metis-notif-card-notify .metis-notif-accent {{
        background-color: {c_warning};
    }}

    .metis-notif-card-notify .metis-notif-title {{
        color: {c_warning};
    }}

    .metis-notif-card-success {{
        box-shadow: 0 0 18px rgba({c_success_rgb}, 0.22);
        border-color: rgba({c_success_rgb}, 0.35);
        color: {c_success};
    }}

    .metis-notif-card-success .metis-notif-accent {{
        background-color: {c_success};
    }}

    .metis-notif-card-success .metis-notif-title {{
        color: {c_success};
    }}

    .metis-notif-card-info {{
        box-shadow: 0 0 18px rgba({c_info_rgb}, 0.22);
        border-color: rgba({c_info_rgb}, 0.35);
        color: {c_info};
    }}

    .metis-notif-card-info .metis-notif-accent {{
        background-color: {c_info};
    }}

    .metis-notif-card-info .metis-notif-title {{
        color: {c_info};
    }}

    .metis-notif-card-payment {{
        box-shadow: 0 0 18px rgba({c_payment_rgb}, 0.22);
        border-color: rgba({c_payment_rgb}, 0.35);
        color: {c_payment};
    }}

    .metis-notif-card-payment .metis-notif-accent {{
        background-color: {c_payment};
    }}

    .metis-notif-card-payment .metis-notif-title {{
        color: {c_payment};
    }}

    .metis-bar-volume-scale {{
        min-width: 180px;
        padding: 2px 0;
    }}

    .metis-bar-volume-scale trough {{
        background-color: rgba(255, 255, 255, 0.12);
        border: none;
        border-radius: 999px;
        min-height: 5px;
    }}

    .metis-bar-volume-scale highlight {{
        background-color: {accent};
        border-radius: 999px;
        min-height: 5px;
    }}

    .metis-bar-volume-scale slider {{
        background-color: #ffffff;
        border: none;
        border-radius: 999px;
        min-width: 15px;
        min-height: 15px;
        margin: -6px;
        box-shadow: 0 1px 4px rgba(0, 0, 0, 0.5);
    }}

    .metis-bar-volume-scale value {{
        color: {muted};
        font-size: 12px;
        margin-left: 8px;
    }}

    .metis-bar-audio-mute {{
        padding: 6px;
        margin: 0;
        min-width: 0;
        min-height: 0;
        border: none;
        outline: none;
        background-image: none;
        background-color: transparent;
        box-shadow: none;
        color: {muted};
        border-radius: {rs}px;
    }}

    .metis-bar-audio-mute:hover {{
        color: {accent};
        background-color: rgba({accent_rgb}, 0.12);
    }}

    .metis-bar-audio-mute:active {{
        background-color: rgba({accent_rgb}, 0.20);
    }}

    .metis-net-eth-row {{
        padding: 6px 4px;
        border-bottom: 1px solid {border};
        color: {text};
    }}

    .metis-net-row {{
        padding: 6px 6px;
        border: none;
        outline: none;
        background-image: none;
        background-color: transparent;
        box-shadow: none;
        color: {text};
        border-radius: {rs}px;
    }}

    .metis-net-row:hover {{
        background-color: rgba({accent_rgb}, 0.12);
    }}

    .metis-net-lock {{
        color: {muted};
    }}

    .metis-net-active {{
        color: {accent};
    }}

    .metis-net-status {{
        padding: 6px 4px;
        color: {muted};
        font-size: 12px;
    }}

    .metis-net-refresh {{
        padding: 4px;
        min-width: 0;
        min-height: 0;
        border: none;
        outline: none;
        background-image: none;
        background-color: transparent;
        box-shadow: none;
        color: {muted};
        border-radius: {rs}px;
    }}

    .metis-net-refresh:hover {{
        color: {accent};
        background-color: rgba({accent_rgb}, 0.12);
    }}

    .metis-net-connect {{
        padding: 8px 4px 2px 4px;
        border-top: 1px solid {border};
    }}

    .metis-net-connect-title {{
        color: {text};
        font-weight: 600;
    }}

    .metis-net-password {{
        border-radius: {rs}px;
    }}

    .metis-net-connect-btn {{
        background-color: {accent};
        background-image: none;
        color: {on_accent};
        font-weight: 600;
        border-radius: {rs}px;
        padding: 4px 12px;
    }}

    .metis-net-cancel {{
        border-radius: {rs}px;
        padding: 4px 12px;
    }}

    .metis-bar-weather {{
        padding: 0 8px;
    }}

    .metis-weather-bar-label {{
        font-size: 13px;
        font-weight: 600;
        color: {text};
    }}

    .metis-weather-primary {{
        padding: 2px 2px 6px 2px;
    }}

    .metis-weather-loc {{
        font-size: 13px;
        font-weight: 600;
        color: {text};
    }}

    .metis-weather-temp {{
        font-size: 34px;
        font-weight: 300;
        color: {text};
    }}

    .metis-weather-cond {{
        font-size: 13px;
        color: {text};
    }}

    .metis-weather-hl {{
        font-size: 12px;
        color: {muted};
    }}

    .metis-weather-hourly {{
        padding: 6px 0;
        border-top: 1px solid {border};
        border-bottom: 1px solid {border};
    }}

    .metis-weather-hour {{
        padding: 2px 0;
    }}

    .metis-weather-hour-label {{
        font-size: 11px;
        color: {muted};
    }}

    .metis-weather-hour-temp {{
        font-size: 12px;
        font-weight: 600;
        color: {text};
    }}

    .metis-weather-sep {{
        background-color: {border};
        min-height: 1px;
    }}

    .metis-weather-other {{
        padding: 4px 2px;
    }}

    .metis-weather-other-temp {{
        font-size: 13px;
        font-weight: 600;
        color: {text};
    }}

    .metis-weather-status {{
        font-size: 12px;
        color: {muted};
    }}

    .metis-weather-attrib {{
        font-size: 10px;
        color: {muted};
        opacity: 0.7;
    }}

    .metis-bar-dropdown-panel switch {{
        background-color: rgba(255, 255, 255, 0.14);
        border: none;
        border-radius: 999px;
        min-width: 40px;
        min-height: 22px;
    }}

    .metis-bar-dropdown-panel switch:checked {{
        background-color: {accent};
        background-image: linear-gradient(135deg, {accent}, {accent2});
    }}

    .metis-bar-dropdown-panel switch > slider {{
        background-color: #ffffff;
        border-radius: 999px;
        min-width: 18px;
        min-height: 18px;
        box-shadow: 0 1px 3px rgba(0, 0, 0, 0.45);
    }}

    .metis-bar-dropdown-panel separator {{
        background-color: {border};
        min-height: 1px;
    }}

    .metis-bar-popover-panel {{
        background-color: {surface};
        border: 1px solid {border};
        border-radius: {rm}px;
        box-shadow: {shadow};
    }}

    .metis-bar-calendar {{
        margin: 0;
    }}

    .metis-cal-today-legacy {{
        background-color: rgba({accent_rgb}, 0.85);
        color: {on_accent};
        font-weight: 700;
    }}

    .metis-bar-section-title {{
        font-size: 11px;
        font-weight: 700;
        color: {accent};
        letter-spacing: 0.04em;
        text-transform: uppercase;
    }}

    .metis-bar-tz-name {{
        font-size: 12px;
        color: {muted};
    }}

    .metis-bar-tz-time {{
        font-size: 13px;
        font-weight: 600;
        color: {text};
    }}

    /* ---- Clock / calendar popover: pill tabs ---- */
    .metis-clock-tabs {{
        padding: 2px;
    }}
    .metis-clock-tab {{
        padding: 5px 14px;
        min-height: 0;
        border-radius: 999px;
        color: {muted};
        background-image: none;
        background-color: rgba(255, 255, 255, 0.04);
        box-shadow: none;
        border: 1px solid transparent;
    }}
    .metis-clock-tab image {{
        -gtk-icon-size: 15px;
    }}
    .metis-clock-tab:hover {{
        color: {text};
        background-color: rgba(255, 255, 255, 0.08);
    }}
    .metis-clock-tab:checked {{
        color: {text};
        background-color: rgba({accent_rgb}, 0.18);
        border-color: rgba({accent_rgb}, 0.55);
        box-shadow: none;
    }}
    .metis-clock-tab:checked image {{
        color: {accent};
    }}

    /* ---- Stopwatch page ---- */
    .metis-sw-digits {{
        font-size: 46px;
        font-weight: 700;
        color: {text};
        font-feature-settings: "tnum";
        letter-spacing: 0.01em;
    }}
    .metis-sw-btn {{
        padding: 10px 28px;
        min-height: 0;
        border: none;
        border-radius: 999px;
        font-weight: 700;
        color: #ffffff;
        box-shadow: none;
        background-image: none;
    }}
    .metis-sw-btn-go {{
        background-color: {accent};
        color: {on_accent};
    }}
    .metis-sw-btn-go:hover {{
        background-color: {accent2};
    }}
    .metis-sw-btn-stop {{
        background-color: rgba(255, 255, 255, 0.10);
        color: {text};
    }}
    .metis-sw-btn-stop:hover {{
        background-color: rgba(255, 255, 255, 0.16);
    }}
    .metis-sw-btn:disabled {{
        opacity: 0.45;
    }}
    .metis-sw-lap {{
        padding: 8px 12px;
        border-radius: {rs}px;
        background-color: rgba(255, 255, 255, 0.04);
    }}
    .metis-sw-lap-total {{
        font-feature-settings: "tnum";
        color: {text};
        font-weight: 600;
    }}
    .metis-sw-lap-delta {{
        font-feature-settings: "tnum";
        color: {accent};
        font-size: 12px;
    }}
    .metis-sw-lap-name {{
        color: {muted};
        font-size: 12px;
    }}

    /* ---- Timer page ---- */
    .metis-timer-digits {{
        font-size: 44px;
        font-weight: 700;
        color: {text};
        font-feature-settings: "tnum";
    }}
    .metis-timer-section {{
        font-size: 12px;
        font-weight: 700;
        color: {muted};
        letter-spacing: 0.06em;
    }}
    .metis-timer-preset {{
        padding: 8px 0;
        min-height: 0;
        border: 1px solid {border};
        border-radius: {rs}px;
        background-color: rgba(255, 255, 255, 0.04);
        background-image: none;
        color: {text};
        box-shadow: none;
        font-weight: 600;
    }}
    .metis-timer-preset:hover {{
        background-color: rgba({accent_rgb}, 0.16);
        border-color: rgba({accent_rgb}, 0.45);
    }}
    .metis-timer-stepper {{
        padding: 6px;
        border-radius: {rm}px;
        background-color: rgba(255, 255, 255, 0.04);
    }}
    .metis-timer-step-btn {{
        min-width: 56px;
        min-height: 28px;
        padding: 0;
        border: none;
        border-radius: {rs}px;
        background-color: rgba(255, 255, 255, 0.06);
        background-image: none;
        box-shadow: none;
        color: {muted};
    }}
    .metis-timer-step-btn:hover {{
        background-color: rgba({accent_rgb}, 0.18);
        color: {text};
    }}
    .metis-timer-step-value {{
        font-size: 38px;
        font-weight: 700;
        color: {text};
        font-feature-settings: "tnum";
        padding: 2px 0;
    }}
    .metis-timer-colon {{
        font-size: 34px;
        font-weight: 700;
        color: {muted};
        padding: 0 2px;
    }}

    /* ---- Alarm page ---- */
    .metis-alarm-form {{
        padding: 14px;
        border-radius: {rm}px;
        background-color: rgba(255, 255, 255, 0.04);
        border: 1px solid {border};
    }}
    .metis-alarm-ampm {{
        padding: 8px 16px;
        min-height: 0;
        border: 1px solid {border};
        border-radius: {rs}px;
        background-color: rgba(255, 255, 255, 0.06);
        background-image: none;
        box-shadow: none;
        color: {text};
        font-weight: 700;
        margin-left: 6px;
    }}
    .metis-alarm-ampm:hover {{
        background-color: rgba({accent_rgb}, 0.18);
    }}
    .metis-alarm-caption {{
        font-size: 12px;
        font-weight: 700;
        color: {muted};
        letter-spacing: 0.04em;
    }}
    .metis-alarm-day {{
        min-width: 34px;
        min-height: 34px;
        padding: 0;
        border-radius: 999px;
        border: 1px solid {border};
        background-color: rgba(255, 255, 255, 0.04);
        background-image: none;
        box-shadow: none;
        color: {muted};
        font-weight: 700;
    }}
    .metis-alarm-day:hover {{
        color: {text};
        background-color: rgba(255, 255, 255, 0.08);
    }}
    .metis-alarm-day:checked {{
        color: {on_accent};
        background-color: {accent};
        border-color: {accent};
    }}
    .metis-clock-card-main {{
        background-color: rgba({accent_rgb}, 0.08);
        border-color: rgba({accent_rgb}, 0.35);
    }}
    .metis-clock-card-time-main {{
        font-size: 24px;
    }}

    .metis-alarm-sound-seg button {{
        padding: 6px 10px;
        min-height: 0;
        border: 1px solid {border};
        background-color: rgba(255, 255, 255, 0.04);
        background-image: none;
        box-shadow: none;
        color: {muted};
        font-weight: 600;
    }}
    .metis-alarm-sound-seg button:hover {{
        color: {text};
        background-color: rgba(255, 255, 255, 0.08);
    }}
    .metis-alarm-sound-seg button:checked {{
        color: {on_accent};
        background-color: {accent};
        border-color: {accent};
    }}

    /* ---- Inline timezone picker ---- */
    .metis-tz-picker {{
        padding: 10px;
        border-radius: {rm}px;
        background-color: rgba(255, 255, 255, 0.04);
        border: 1px solid {border};
    }}
    .metis-tz-scroll {{
        border-radius: {rs}px;
        background-color: rgba(0, 0, 0, 0.22);
    }}
    .metis-tz-list {{
        background-color: transparent;
    }}
    .metis-tz-list row {{
        padding: 0;
        background-color: transparent;
    }}
    .metis-tz-list row:hover {{
        background-color: rgba({accent_rgb}, 0.16);
    }}
    .metis-tz-row {{
        padding: 7px 12px;
        color: {text};
    }}

    /* ---- Stopwatch laps / picker scrollbars (always visible) ---- */
    .metis-sw-laps-scroll scrollbar,
    .metis-tz-scroll scrollbar {{
        background-color: transparent;
    }}
    .metis-sw-laps-scroll scrollbar slider,
    .metis-tz-scroll scrollbar slider {{
        min-width: 7px;
        border-radius: 999px;
        background-color: rgba(255, 255, 255, 0.22);
    }}
    .metis-sw-laps-scroll scrollbar slider:hover,
    .metis-tz-scroll scrollbar slider:hover {{
        background-color: rgba(255, 255, 255, 0.34);
    }}

    /* ---- Running-timer HUD (layer-shell overlay under the bar) ---- */
    window.metis-timer-hud-window {{
        background-color: transparent;
    }}
    .metis-timer-hud {{
        padding: 8px 12px;
        border-radius: 999px;
        background-color: {raised};
        border: 1px solid rgba({accent_rgb}, 0.45);
        box-shadow: {shadow};
        color: {text};
    }}
    .metis-timer-hud-grip {{
        color: {muted};
        opacity: 0.7;
    }}
    .metis-timer-hud-icon {{
        color: {accent};
    }}
    .metis-timer-hud-time {{
        font-size: 18px;
        font-weight: 700;
        font-feature-settings: "tnum";
        color: {text};
        padding: 0 4px;
    }}
    .metis-timer-hud-btn {{
        min-width: 28px;
        min-height: 28px;
        padding: 2px;
        border: none;
        border-radius: 999px;
        background-color: rgba(255, 255, 255, 0.08);
        background-image: none;
        box-shadow: none;
        color: {text};
    }}
    .metis-timer-hud-btn:hover {{
        background-color: rgba({accent_rgb}, 0.22);
    }}

    /* ---- Startup splash (centered overlay layer) ---- */
    window.metis-splash-window {{
        background-color: transparent;
    }}
    .metis-splash-card {{
        padding: 40px 56px 34px 56px;
        border-radius: 28px;
        background-color: rgba(12, 16, 24, 0.82);
        border: 1px solid rgba(255, 255, 255, 0.08);
        box-shadow: {shadow},
                    inset 0 1px 0 rgba(255, 255, 255, 0.05);
    }}
    .metis-splash-label {{
        font-size: 12px;
        letter-spacing: 0.4px;
        color: {muted};
    }}
    .metis-splash-progress {{
        min-height: 6px;
    }}
    .metis-splash-progress trough {{
        min-height: 6px;
        border-radius: 999px;
        background-color: rgba(255, 255, 255, 0.10);
        border: none;
    }}
    .metis-splash-progress progress {{
        min-height: 6px;
        border-radius: 999px;
        background-image: linear-gradient(to right,
            {accent} 0%, {accent2} 100%);
        border: none;
    }}

    .metis-cal-head-weekday {{
        font-size: 13px;
        color: {muted};
    }}
    .metis-cal-head-date {{
        font-size: 22px;
        font-weight: 700;
        color: {text};
    }}
    .metis-cal-title {{
        font-size: 13px;
        font-weight: 700;
        color: {text};
    }}
    .metis-cal-nav, .metis-cal-today-btn {{
        padding: 2px 8px;
        min-height: 0;
        border: none;
        outline: none;
        background-image: none;
        background-color: transparent;
        box-shadow: none;
        color: {muted};
        border-radius: {rs}px;
    }}
    .metis-cal-today-btn {{
        font-size: 11px;
        font-weight: 700;
        color: {accent};
    }}
    .metis-cal-nav:hover, .metis-cal-today-btn:hover {{
        color: {text};
        background-color: rgba({accent_rgb}, 0.14);
    }}

    .metis-cal-weekday {{
        font-size: 10px;
        font-weight: 700;
        color: {muted};
        padding: 2px 0;
    }}
    button.metis-cal-day {{
        padding: 2px 0;
        min-width: 36px;
        min-height: 34px;
        border: none;
        outline: none;
        background-image: none;
        background-color: transparent;
        box-shadow: none;
        color: {text};
        border-radius: {rs}px;
    }}
    button.metis-cal-day:hover {{
        background-color: rgba(255, 255, 255, 0.06);
    }}
    button.metis-cal-adjacent {{
        opacity: 0.32;
    }}
    button.metis-cal-today .metis-cal-daynum {{
        color: {accent};
        font-weight: 700;
    }}
    button.metis-cal-selected {{
        background-color: rgba({accent_rgb}, 0.10);
        box-shadow: inset 0 0 0 1px {accent};
    }}
    .metis-cal-daynum {{
        font-size: 12px;
    }}
    .metis-cal-dot {{
        min-width: 5px;
        min-height: 5px;
        background-color: {accent};
        border-radius: 999px;
        margin-top: 1px;
    }}

    .metis-cal-empty {{
        font-size: 12px;
        color: {muted};
        font-style: italic;
    }}
    .metis-cal-add-btn {{
        padding: 4px 10px;
        min-height: 0;
        border: none;
        background-color: rgba({accent_rgb}, 0.14);
        color: {text};
        border-radius: {rs}px;
        box-shadow: none;
    }}
    .metis-cal-add-btn:hover {{
        background-color: rgba({accent_rgb}, 0.24);
    }}

    .metis-cal-event {{
        padding: 6px 4px;
        border-radius: {rs}px;
    }}
    .metis-cal-event:hover {{
        background-color: rgba(255, 255, 255, 0.04);
    }}
    .metis-cal-event-color {{
        background-color: {accent};
        border-radius: 999px;
    }}
    .metis-cal-event-title {{
        font-size: 13px;
        color: {text};
    }}
    .metis-cal-event-sub {{
        font-size: 11px;
        color: {muted};
    }}
    .metis-cal-event-action {{
        padding: 2px;
        min-height: 0;
        min-width: 0;
        border: none;
        background-color: transparent;
        box-shadow: none;
        color: {muted};
        border-radius: {rs}px;
    }}
    .metis-cal-event-action:hover {{
        color: {text};
        background-color: rgba(255, 255, 255, 0.08);
    }}

    .metis-clock-cards {{
        margin-top: 2px;
    }}
    .metis-clock-card {{
        padding: 10px 12px;
        background-color: rgba(255, 255, 255, 0.04);
        border: 1px solid {border};
        border-radius: {rm}px;
    }}
    .metis-clock-card-name {{
        font-size: 14px;
        font-weight: 600;
        color: {text};
    }}
    .metis-clock-card-offset {{
        font-size: 11px;
        color: {muted};
    }}
    .metis-clock-card-time {{
        font-size: 18px;
        font-weight: 700;
        color: {accent};
    }}
    .metis-entry-error {{
        box-shadow: inset 0 0 0 1px #ff5c5c;
    }}

    .metis-clock-digits {{
        font-size: 30px;
        font-weight: 700;
        color: {text};
        font-feature-settings: "tnum";
    }}
    .metis-clock-btn {{
        padding: 4px 12px;
        min-height: 0;
        border: none;
        border-radius: {rs}px;
        color: {text};
        background-color: rgba(255, 255, 255, 0.06);
        box-shadow: none;
    }}
    .metis-clock-btn:hover {{
        background-color: rgba({accent_rgb}, 0.20);
    }}
    .metis-clock-lap {{
        font-size: 12px;
        color: {muted};
    }}
    .metis-clock-alarm {{
        padding: 6px 8px;
        background-color: rgba(255, 255, 255, 0.04);
        border-radius: {rs}px;
    }}
    .metis-clock-alarm-time {{
        font-size: 16px;
        font-weight: 700;
        color: {text};
    }}

    /* ---- Add-event form + Calendars account management ---- */
    .metis-cal-form {{
        padding: 8px;
        background-color: rgba(255, 255, 255, 0.04);
        border: 1px solid {border};
        border-radius: {rm}px;
    }}
    .metis-acct-form {{
        padding: 12px;
        background-color: rgba(255, 255, 255, 0.04);
        border: 1px solid {border};
        border-radius: {rm}px;
    }}
    .metis-acct-row {{
        padding: 10px 12px;
        background-color: rgba(255, 255, 255, 0.04);
        border: 1px solid {border};
        border-radius: {rm}px;
    }}
    .metis-acct-name {{
        font-size: 14px;
        font-weight: 600;
        color: {text};
    }}
    .metis-acct-status {{
        font-size: 11px;
        color: {muted};
    }}

    /* ---- App menu popover ---- */
    .metis-menu-panel {{
        padding: 14px;
    }}

    /* Tooltip for the icon-only rail: a label inside the menu's GtkOverlay (drawn
       on the menu's own surface, so it can't stack behind the translucent panel
       like a separate popup would). */
    .metis-menu-tooltip-label {{
        padding: 4px 9px;
        border-radius: {rs}px;
        border: 1px solid {border};
        background-color: {raised};
        color: {text};
        font-size: 12px;
    }}

    .metis-menu-rail {{
        padding: 2px 10px 2px 0;
        margin-right: 4px;
        border-right: 1px solid {border};
    }}

    .metis-menu-rail-btn {{
        padding: 8px;
        min-width: 0;
        min-height: 0;
        border: none;
        outline: none;
        background-image: none;
        background-color: transparent;
        box-shadow: none;
        color: {muted};
        border-radius: {rs}px;
    }}
    .metis-menu-rail-btn:hover {{
        color: {accent};
        background-color: rgba({accent_rgb}, 0.14);
    }}
    .metis-menu-rail-btn:active {{
        background-color: rgba({accent_rgb}, 0.22);
    }}

    .metis-menu-center {{
        min-width: 320px;
    }}

    .metis-menu-pinned {{
        min-width: 240px;
        margin-left: 4px;
    }}

    .metis-menu-divider {{
        background-color: {border};
        min-width: 1px;
        margin: 4px 8px;
    }}

    .metis-menu-scroll {{
        min-height: 420px;
    }}
    .metis-menu-scroll scrollbar {{
        background-color: transparent;
    }}
    .metis-menu-scroll scrollbar slider {{
        min-width: 7px;
        border-radius: 999px;
        background-color: rgba({text_rgb}, 0.22);
    }}
    .metis-menu-scroll scrollbar slider:hover {{
        background-color: rgba({text_rgb}, 0.34);
    }}

    .metis-menu-row {{
        padding: 7px 8px;
        border: none;
        outline: none;
        background-image: none;
        background-color: transparent;
        box-shadow: none;
        color: {text};
        border-radius: {rs}px;
    }}
    .metis-menu-row:hover {{
        background-color: rgba({accent_rgb}, 0.14);
    }}
    .metis-menu-row:active {{
        background-color: rgba({accent_rgb}, 0.22);
    }}

    .metis-menu-letter {{
        font-size: 11px;
        font-weight: 700;
        color: {muted};
        padding: 8px 8px 2px 8px;
    }}

    .metis-menu-empty {{
        font-size: 12px;
        color: {muted};
        padding: 10px 8px;
    }}

    .metis-menu-search {{
        margin-top: 4px;
        border-radius: {rs}px;
        background-color: {raised};
        border: 1px solid {border};
        color: {text};
        caret-color: {text};
        box-shadow: none;
    }}
    .metis-menu-search > text {{
        background-color: transparent;
        color: {text};
    }}
    .metis-menu-search > text > placeholder {{
        color: {muted};
    }}
    .metis-menu-search image {{
        color: {muted};
    }}
    .metis-menu-search:focus-within {{
        border-color: {accent};
    }}

    .metis-menu-pinned-flow {{
        /* Extra right pad keeps the tiles clear of the overlay scrollbar. */
        padding: 2px 12px 2px 2px;
    }}

    .metis-menu-tile {{
        padding: 10px 6px;
        border: none;
        outline: none;
        background-image: none;
        background-color: transparent;
        box-shadow: none;
        color: {text};
        border-radius: {rm}px;
    }}
    .metis-menu-tile:hover {{
        background-color: rgba({accent_rgb}, 0.14);
    }}
    .metis-menu-tile:active {{
        background-color: rgba({accent_rgb}, 0.22);
    }}

    .metis-menu-tile-label {{
        font-size: 11px;
        color: {text};
        margin-top: 2px;
    }}
"#,
        surface = surface,
        border = theme.border,
        rm = rm,
        rs = rs,
        rl = rl,
        raised = raised,
        accent = accent,
        text = theme.text,
        muted = theme.text_muted,
    )
}
