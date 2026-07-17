use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Persistent app-menu state, stored at `~/.config/metis/menu.json`.
///
/// `pinned` is an ordered list of `.desktop` ids shown in the menu's pinned grid.
/// `launch_counts` tracks how often each app id has been launched from the menu,
/// driving the "Frequent Apps" ordering.
///
/// `terminal` / `file_manager` are the user's chosen quick-launch programs for the
/// menu rail. Each is a binary name on `$PATH` *or* an absolute path. `None` or an
/// empty string means "auto-detect" — fall back to the first installed entry in
/// [`KNOWN_TERMINALS`] / [`KNOWN_FILE_MANAGERS`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MenuConfig {
    #[serde(default)]
    pub pinned: Vec<String>,
    #[serde(default)]
    pub launch_counts: HashMap<String, u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_manager: Option<String>,
}

/// Known terminal emulators in auto-detect preference order: `(binary, label)`.
/// Kitty is first — Metis ships it as a package dependency and prefers it when
/// Settings → Menu terminal is left on auto-detect.
pub const KNOWN_TERMINALS: &[(&str, &str)] = &[
    ("kitty", "kitty"),
    ("kgx", "GNOME Console"),
    ("gnome-terminal", "GNOME Terminal"),
    ("konsole", "Konsole"),
    ("foot", "foot"),
    ("alacritty", "Alacritty"),
    ("wezterm", "WezTerm"),
    ("xterm", "xterm"),
];

/// Known file managers in auto-detect preference order: `(binary, label)`.
pub const KNOWN_FILE_MANAGERS: &[(&str, &str)] = &[
    ("nautilus", "Files (Nautilus)"),
    ("dolphin", "Dolphin"),
    ("nemo", "Nemo"),
    ("thunar", "Thunar"),
    ("pcmanfm", "PCManFM"),
    ("pcmanfm-qt", "PCManFM-Qt"),
    ("caja", "Caja"),
];

/// True when `bin` is launchable: an executable on `$PATH`, or — when it contains a
/// `/` — an absolute/relative path pointing at an executable file.
pub fn binary_in_path(bin: &str) -> bool {
    let bin = bin.trim();
    if bin.is_empty() {
        return false;
    }
    if bin.contains('/') {
        return is_executable_file(std::path::Path::new(bin));
    }
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| is_executable_file(&dir.join(bin)))
}

#[cfg(unix)]
fn is_executable_file(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(path) {
        Ok(meta) => meta.is_file() && meta.permissions().mode() & 0o111 != 0,
        Err(_) => false,
    }
}

#[cfg(not(unix))]
fn is_executable_file(path: &std::path::Path) -> bool {
    path.is_file()
}

pub fn menu_config_path() -> std::path::PathBuf {
    super::config_dir().join("menu.json")
}

pub fn load_menu_config() -> MenuConfig {
    let path = menu_config_path();
    if path.exists() {
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(cfg) = serde_json::from_str::<MenuConfig>(&text) {
                return cfg;
            }
            tracing::warn!("menu.json parse failed — using defaults");
        }
    }
    MenuConfig::default()
}

pub fn save_menu_config(config: &MenuConfig) -> std::io::Result<()> {
    super::ensure_config_dirs()?;
    let json = serde_json::to_string_pretty(config).map_err(std::io::Error::other)?;
    std::fs::write(menu_config_path(), json)
}
