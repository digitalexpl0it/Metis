//! Declarative JSON extension widgets (Phase 14 §E).
//!
//! Action fields are **not** settings-interpolated (labels/copy text may be).
//! URI / launch targets are re-validated at click time.

use gtk::prelude::*;
use metis_config::{
    find_widget_extension, interpolate_settings, is_safe_launch_exec, is_safe_launch_id,
    is_safe_open_uri, load_widget_layout, validate_action, DesktopWidgetInstance, WidgetExtAction,
    WidgetExtLabelStyle, WidgetExtNode, WIDGET_EXT_MAX_COPY,
};

use crate::services::applications;

pub fn build(inst: &DesktopWidgetInstance) -> gtk::Widget {
    let Some(ext) = find_widget_extension(&inst.extension_id) else {
        return missing_card(&inst.extension_id);
    };
    let layout = match load_widget_layout(&ext.root) {
        Ok(n) => n,
        Err(err) => {
            tracing::warn!(
                id = %inst.extension_id,
                %err,
                "extension widget.json failed to load"
            );
            return error_card(&format!("Invalid widget.json: {err}"));
        }
    };
    let settings = inst.extension_settings.clone();
    build_node(&layout, &settings)
}

fn missing_card(id: &str) -> gtk::Widget {
    let col = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let title = gtk::Label::new(Some(&metis_i18n::tr("Extension not found")));
    title.set_xalign(0.0);
    title.add_css_class("metis-dw-title");
    let hint = gtk::Label::new(Some(&format!(
        "{} — install under ~/.local/share/metis/widgets/{id}/",
        if id.is_empty() { "(no id)" } else { id }
    )));
    hint.set_wrap(true);
    hint.set_xalign(0.0);
    hint.add_css_class("metis-dw-hint");
    col.append(&title);
    col.append(&hint);
    col.upcast()
}

fn error_card(msg: &str) -> gtk::Widget {
    let col = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let hint = gtk::Label::new(Some(msg));
    hint.set_wrap(true);
    hint.set_xalign(0.0);
    hint.add_css_class("metis-dw-hint");
    col.append(&hint);
    col.upcast()
}

fn build_node(
    node: &WidgetExtNode,
    settings: &serde_json::Map<String, serde_json::Value>,
) -> gtk::Widget {
    match node {
        WidgetExtNode::Column { spacing, children } => {
            let col = gtk::Box::new(gtk::Orientation::Vertical, (*spacing).max(0));
            for child in children {
                col.append(&build_node(child, settings));
            }
            col.upcast()
        }
        WidgetExtNode::Row { spacing, children } => {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, (*spacing).max(0));
            for child in children {
                row.append(&build_node(child, settings));
            }
            row.upcast()
        }
        WidgetExtNode::List { spacing, children } => {
            let col = gtk::Box::new(gtk::Orientation::Vertical, (*spacing).max(0));
            col.add_css_class("metis-dw-ext-list");
            for child in children {
                col.append(&build_node(child, settings));
            }
            col.upcast()
        }
        WidgetExtNode::Label { text, style } => {
            let rendered = interpolate_settings(text, settings);
            let label = gtk::Label::new(Some(&rendered));
            label.set_xalign(0.0);
            label.set_wrap(true);
            label.set_use_markup(false);
            match style {
                WidgetExtLabelStyle::Title => label.add_css_class("metis-dw-title"),
                WidgetExtLabelStyle::Muted => label.add_css_class("metis-dw-hint"),
                WidgetExtLabelStyle::Body => label.add_css_class("metis-dw-body"),
            }
            label.upcast()
        }
        WidgetExtNode::Icon { name, pixel_size } => {
            let icon = gtk::Image::from_icon_name(name);
            icon.set_pixel_size((*pixel_size).clamp(8, 128));
            icon.upcast()
        }
        WidgetExtNode::Separator => {
            let sep = gtk::Separator::new(gtk::Orientation::Horizontal);
            sep.upcast()
        }
        WidgetExtNode::Button { label, on_click } => {
            let rendered = interpolate_settings(label, settings);
            let btn = gtk::Button::with_label(&rendered);
            btn.set_halign(gtk::Align::Fill);
            btn.add_css_class("metis-dw-ext-button");
            let action = on_click.clone();
            let settings = settings.clone();
            btn.connect_clicked(move |_| {
                run_action(&action, &settings);
            });
            btn.upcast()
        }
    }
}

fn run_action(
    action: &WidgetExtAction,
    settings: &serde_json::Map<String, serde_json::Value>,
) {
    // Re-check at click time (defense in depth; layout already validated on load).
    if let Err(err) = validate_action(action) {
        tracing::warn!(%err, "extension action blocked");
        return;
    }
    match action {
        WidgetExtAction::OpenUri { uri } => {
            // No settings interpolation into URIs.
            let uri = uri.trim();
            if !is_safe_open_uri(uri) {
                tracing::warn!(uri = %uri, "extension open_uri blocked");
                return;
            }
            open_https_uri(uri);
        }
        WidgetExtAction::Launch { id, exec } => {
            // No settings interpolation into launch targets.
            let id = id.trim();
            let exec = exec.trim();
            if !id.is_empty() {
                if !is_safe_launch_id(id) {
                    tracing::warn!(id = %id, "extension launch id blocked");
                    return;
                }
                applications::launch_id(id);
            } else if !exec.is_empty() {
                if !is_safe_launch_exec(exec) {
                    tracing::warn!(exec = %exec, "extension launch exec blocked");
                    return;
                }
                // Single argv token only (validators already forbid whitespace/args).
                if let Err(err) = crate::compositor::launch_argv([exec]) {
                    tracing::warn!(%err, exec = %exec, "extension launch failed");
                }
            } else {
                tracing::warn!("extension launch action missing id and exec");
            }
        }
        WidgetExtAction::CopyText { text } => {
            let text = interpolate_settings(text, settings);
            let clipped = if text.len() > WIDGET_EXT_MAX_COPY {
                let mut end = WIDGET_EXT_MAX_COPY;
                while end > 0 && !text.is_char_boundary(end) {
                    end -= 1;
                }
                tracing::warn!("extension copy_text truncated");
                &text[..end]
            } else {
                text.as_str()
            };
            if let Some(display) = gtk::gdk::Display::default() {
                display.clipboard().set_text(clipped);
            }
        }
    }
}

fn open_https_uri(uri: &str) {
    let launcher = gtk::UriLauncher::new(uri);
    let uri_owned = uri.to_string();
    launcher.launch(None::<&gtk::Window>, None::<&gio::Cancellable>, move |res| {
        if let Err(err) = res {
            tracing::warn!(%err, uri = %uri_owned, "extension UriLauncher failed");
        }
    });
}
