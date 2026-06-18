use super::tokens::ThemeTokens;

pub fn build_stylesheet(theme: &ThemeTokens) -> String {
    let accent = theme.accent_primary();
    let surface = theme.surface_rgba();
    let raised = theme.surface_raised.clone();
    let rs = theme.radius_sm;
    let rm = theme.radius_md;

    format!(
        r#"
    window {{
        background-color: transparent;
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
        padding: 4px 14px;
        color: {text};
    }}

    .metis-bar-full {{
        border-radius: 999px;
        padding: 4px 20px;
        box-shadow: 0 6px 24px rgba(0, 0, 0, 0.42), 0 2px 6px rgba(0, 0, 0, 0.28);
    }}

    .metis-bar-floating {{
        border-radius: 999px;
        padding: 4px 14px;
        box-shadow: 0 4px 24px rgba(0, 0, 0, 0.45), 0 1px 0 rgba(255, 255, 255, 0.06) inset;
    }}

    button.metis-bar-widget,
    button.metis-bar-widget:hover {{
        background-image: none;
        background-color: transparent;
        box-shadow: none;
        min-height: 0;
        padding: 0 8px;
    }}

    menubutton.metis-bar-widget {{
        background-image: none;
        background-color: transparent;
        box-shadow: none;
        border: none;
        min-height: 0;
        padding: 0 8px;
    }}

    menubutton.metis-bar-widget > button {{
        background-image: none;
        background-color: transparent;
        box-shadow: none;
        border: none;
        min-height: 0;
        padding: 0;
    }}

    menubutton.metis-bar-widget > button,
    button.metis-bar-widget {{
        background-color: transparent;
        border: none;
        min-height: 0;
        padding: 0;
        box-shadow: none;
    }}

    menubutton.metis-bar-widget:hover > button,
    button.metis-bar-widget:hover {{
        background-color: rgba(255, 255, 255, 0.06);
    }}

    button.metis-bar-dropdown-active {{
        background-color: rgba(255, 255, 255, 0.1);
    }}

    menubutton.metis-bar-notifications:hover > button,
    button.metis-bar-notifications:hover {{
        background-color: transparent;
    }}

    .metis-bar-widget {{
        padding: 0 8px;
        border-radius: {rs}px;
        background-color: transparent;
        border: none;
        color: {text};
        min-height: 0;
    }}

    .metis-bar-widget:hover {{
        background-color: rgba(255, 255, 255, 0.06);
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
        color: #0a0e14;
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
        border-radius: {rm}px;
        box-shadow: 0 12px 32px rgba(0, 0, 0, 0.45);
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
        border: 1.5px solid rgba(255, 255, 255, 0.45);
    }}

    .metis-bar-icon {{
        -gtk-icon-style: regular;
        background-color: transparent;
    }}

    .metis-bar-ws-dot-idle {{
        opacity: 0.5;
    }}

    .metis-bar-ws-dot:hover {{
        background-color: rgba(255, 255, 255, 0.35);
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

    popover.metis-notif-popover {{
        padding: 0;
    }}

    .metis-notif-scrolled {{
        min-width: 332px;
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
        box-shadow: 0 0 18px rgba(239, 68, 68, 0.22);
        border-color: rgba(239, 68, 68, 0.35);
        color: #ef4444;
    }}

    .metis-notif-card-error .metis-notif-accent {{
        background-color: #ef4444;
    }}

    .metis-notif-card-error .metis-notif-title {{
        color: #ef4444;
    }}

    .metis-notif-card-notify {{
        box-shadow: 0 0 18px rgba(245, 158, 11, 0.22);
        border-color: rgba(245, 158, 11, 0.35);
        color: #f59e0b;
    }}

    .metis-notif-card-notify .metis-notif-accent {{
        background-color: #f59e0b;
    }}

    .metis-notif-card-notify .metis-notif-title {{
        color: #f59e0b;
    }}

    .metis-notif-card-success {{
        box-shadow: 0 0 18px rgba(16, 185, 129, 0.22);
        border-color: rgba(16, 185, 129, 0.35);
        color: #10b981;
    }}

    .metis-notif-card-success .metis-notif-accent {{
        background-color: #10b981;
    }}

    .metis-notif-card-success .metis-notif-title {{
        color: #10b981;
    }}

    .metis-notif-card-info {{
        box-shadow: 0 0 18px rgba(59, 130, 246, 0.22);
        border-color: rgba(59, 130, 246, 0.35);
        color: #3b82f6;
    }}

    .metis-notif-card-info .metis-notif-accent {{
        background-color: #3b82f6;
    }}

    .metis-notif-card-info .metis-notif-title {{
        color: #3b82f6;
    }}

    .metis-notif-card-payment {{
        box-shadow: 0 0 18px rgba(132, 204, 22, 0.22);
        border-color: rgba(132, 204, 22, 0.35);
        color: #84cc16;
    }}

    .metis-notif-card-payment .metis-notif-accent {{
        background-color: #84cc16;
    }}

    .metis-notif-card-payment .metis-notif-title {{
        color: #84cc16;
    }}

    .metis-bar-volume-scale {{
        min-width: 180px;
    }}

    .metis-bar-popover-panel {{
        background-color: {surface};
        border: 1px solid {border};
        border-radius: {rm}px;
        box-shadow: 0 12px 32px rgba(0, 0, 0, 0.45);
    }}

    .metis-bar-calendar {{
        margin: 0;
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
"#,
        surface = surface,
        border = theme.border,
        rm = rm,
        rs = rs,
        raised = raised,
        accent = accent,
        text = theme.text,
        muted = theme.text_muted,
    )
}
