//! Free-floating desktop widgets (`~/.config/metis/desktop-widgets.json`).
//!
//! Phase 14: optional wallpaper-layer panels (Folders, Apps, Clock, …). Master
//! switch defaults to **off** so fresh installs stay wallpaper-clean. Geometry is
//! per-instance and per-output; `desk.json` remains app-grid only.

use serde::{Deserialize, Serialize};

/// Built-in desktop widget kinds (v1 + platform placeholder).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopWidgetKind {
    Folders,
    Apps,
    Clock,
    System,
    Weather,
    /// Temporary card for platform bring-up (move / resize / lock).
    Placeholder,
}

impl DesktopWidgetKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Folders => "Folders",
            Self::Apps => "Apps",
            Self::Clock => "Clock",
            Self::System => "System",
            Self::Weather => "Weather",
            Self::Placeholder => "Placeholder",
        }
    }

    /// Kinds the Settings UI can add today (builtins that have UI).
    pub fn addable() -> &'static [DesktopWidgetKind] {
        &[
            DesktopWidgetKind::Placeholder,
            DesktopWidgetKind::Folders,
            DesktopWidgetKind::Apps,
            DesktopWidgetKind::Clock,
            DesktopWidgetKind::System,
            DesktopWidgetKind::Weather,
        ]
    }
}

fn default_desktop_path() -> String {
    "~/Desktop".into()
}

fn default_w() -> u32 {
    320
}

fn default_h() -> u32 {
    240
}

/// One placed widget instance on a monitor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesktopWidgetInstance {
    pub id: String,
    pub kind: DesktopWidgetKind,
    /// Output connector name (`DP-1`, …). Empty / missing = primary monitor.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub output: String,
    #[serde(default)]
    pub x: i32,
    #[serde(default)]
    pub y: i32,
    #[serde(default = "default_w")]
    pub w: u32,
    #[serde(default = "default_h")]
    pub h: u32,
    #[serde(default)]
    pub locked: bool,
    /// Folders widget: directory to list (`~/Desktop` by default).
    #[serde(default = "default_desktop_path", skip_serializing_if = "is_default_path")]
    pub path: String,
    /// Apps widget: desktop ids to show (separate from start-menu pins).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pins: Vec<String>,
}

fn is_default_path(path: &str) -> bool {
    path.is_empty() || path == "~/Desktop"
}

impl DesktopWidgetInstance {
    pub fn new(kind: DesktopWidgetKind) -> Self {
        Self {
            id: new_instance_id(),
            kind,
            output: String::new(),
            x: 80,
            y: 80,
            w: default_w(),
            h: default_h(),
            locked: false,
            path: default_desktop_path(),
            pins: Vec::new(),
        }
    }
}

fn new_instance_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("dw-{nanos:x}")
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesktopWidgetsConfig {
    /// Master switch. Default off — wallpaper stays clean until the user opts in.
    #[serde(default)]
    pub enabled: bool,
    /// When true, unlocked instances can be moved / resized on the desktop.
    #[serde(default)]
    pub edit_mode: bool,
    #[serde(default)]
    pub instances: Vec<DesktopWidgetInstance>,
}

impl Default for DesktopWidgetsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            edit_mode: false,
            instances: Vec::new(),
        }
    }
}

pub fn desktop_widgets_config_path() -> std::path::PathBuf {
    super::config_dir().join("desktop-widgets.json")
}

pub fn load_desktop_widgets_config() -> DesktopWidgetsConfig {
    let path = desktop_widgets_config_path();
    if path.exists() {
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(cfg) = serde_json::from_str(&text) {
                return sanitize(cfg);
            }
        }
    }
    DesktopWidgetsConfig::default()
}

pub fn save_desktop_widgets_config(cfg: &DesktopWidgetsConfig) -> std::io::Result<()> {
    super::ensure_config_dirs()?;
    let cfg = sanitize(cfg.clone());
    let json = serde_json::to_string_pretty(&cfg).map_err(std::io::Error::other)?;
    let path = desktop_widgets_config_path();
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(tmp, path)
}

fn sanitize(mut cfg: DesktopWidgetsConfig) -> DesktopWidgetsConfig {
    let mut seen = std::collections::HashSet::new();
    cfg.instances.retain(|inst| {
        if inst.id.is_empty() || !seen.insert(inst.id.clone()) {
            return false;
        }
        true
    });
    for inst in &mut cfg.instances {
        inst.w = inst.w.clamp(160, 2400);
        inst.h = inst.h.clamp(120, 1800);
        if inst.path.trim().is_empty() {
            inst.path = default_desktop_path();
        }
        if inst.kind != DesktopWidgetKind::Folders {
            // Keep path for round-trip but don't require it for other kinds.
        }
        if inst.kind != DesktopWidgetKind::Apps {
            // pins only meaningful for Apps; leave stored for kind switches.
        }
    }
    cfg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_disabled() {
        let cfg = DesktopWidgetsConfig::default();
        assert!(!cfg.enabled);
        assert!(cfg.instances.is_empty());
    }

    #[test]
    fn round_trip_placeholder() {
        let mut cfg = DesktopWidgetsConfig {
            enabled: true,
            edit_mode: true,
            instances: vec![DesktopWidgetInstance::new(DesktopWidgetKind::Placeholder)],
        };
        cfg = sanitize(cfg);
        let json = serde_json::to_string(&cfg).unwrap();
        let back: DesktopWidgetsConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.instances.len(), 1);
        assert_eq!(back.instances[0].kind, DesktopWidgetKind::Placeholder);
    }
}
