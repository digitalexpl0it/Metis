//! Installed-application enumeration and app-menu state for the launcher popover.
//!
//! Apps are discovered through `gio::AppInfo` (the freedesktop `.desktop` index).
//! Launch frequency and pinned apps persist to `menu.json` via `metis-config`.
//! Launching routes through the compositor (`launch_program`) so children inherit
//! the nested Wayland environment and are tracked for cleanup.

use std::cell::RefCell;
use std::rc::Rc;
use gio::prelude::*;

use metis_config::{load_menu_config, save_menu_config};

/// A single launchable application resolved from a `.desktop` entry.
#[derive(Clone)]
pub struct AppEntry {
    /// Desktop file id, e.g. `firefox.desktop`. Stable key for pinning/frequency.
    pub id: String,
    pub name: String,
    /// Command line with `.desktop` field codes stripped, ready for `launch_program`.
    pub exec: String,
    pub icon: Option<gio::Icon>,
    pub keywords: Vec<String>,
    /// `StartupWMClass` from the desktop entry, used to map a running window's
    /// Wayland `app_id` back to its launcher entry/icon.
    pub wm_class: Option<String>,
    /// Flatpak application id (`X-Flatpak` desktop key), e.g.
    /// `com.valvesoftware.Steam`. Flatpak windows commonly report this as their
    /// Wayland `app_id`, so it is another handle for window→entry matching.
    pub flatpak_id: Option<String>,
}

thread_local! {
    static APP_CACHE: RefCell<Option<Vec<AppEntry>>> = const { RefCell::new(None) };
    /// Repaint hooks for widgets that enumerate installed apps (menu, taskbar).
    static REFRESH: RefCell<Vec<std::rc::Weak<dyn Fn()>>> = const { RefCell::new(Vec::new()) };
    static APP_MONITOR_ARMED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Drop the in-process app index so the next read rescans `.desktop` entries.
pub fn invalidate_app_cache() {
    APP_CACHE.with(|cache| *cache.borrow_mut() = None);
}

/// Register a callback invoked when the freedesktop app index changes (install,
/// remove, `.desktop` edit). Each consumer registers its own hook; dead hooks from
/// rebuilt bars are pruned on the next register/fire.
pub fn register_refresh(cb: Rc<dyn Fn()>) {
    REFRESH.with(|r| {
        let mut list = r.borrow_mut();
        list.retain(|w| w.strong_count() > 0);
        list.push(Rc::downgrade(&cb));
    });
}

fn fire_refresh() {
    let callbacks: Vec<Rc<dyn Fn()>> = REFRESH.with(|r| {
        let mut list = r.borrow_mut();
        list.retain(|w| w.strong_count() > 0);
        list.iter().filter_map(std::rc::Weak::upgrade).collect()
    });
    for cb in callbacks {
        cb();
    }
}

fn on_app_index_changed() {
    tracing::debug!("desktop app index changed, refreshing app list");
    invalidate_app_cache();
    fire_refresh();
}

/// Watch `GAppInfoMonitor` so newly installed apps appear without restarting the
/// shell. Safe to call more than once; only the first call wires the monitor.
pub fn watch_app_index() {
    APP_MONITOR_ARMED.with(|armed| {
        if armed.get() {
            return;
        }
        armed.set(true);
        let monitor = gio::AppInfoMonitor::get();
        monitor.connect_changed(|_| on_app_index_changed());
    });
}

/// Enumerate all visible installed applications, sorted alphabetically by name.
pub fn list_apps() -> Vec<AppEntry> {
    APP_CACHE.with(|cache| {
        if let Some(entries) = cache.borrow().as_ref() {
            return entries.clone();
        }
        let entries = list_apps_uncached();
        *cache.borrow_mut() = Some(entries.clone());
        entries
    })
}

fn list_apps_uncached() -> Vec<AppEntry> {
    let mut entries: Vec<AppEntry> = gio::AppInfo::all()
        .into_iter()
        .filter(|info| info.should_show())
        .filter_map(entry_from_info)
        .collect();
    entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    entries
}

fn entry_from_info(info: gio::AppInfo) -> Option<AppEntry> {
    let id = info.id()?.to_string();
    let name = info.name().to_string();
    if name.is_empty() {
        return None;
    }
    let exec = info
        .commandline()
        .map(|cmd| clean_exec(&cmd.to_string_lossy()))
        .filter(|s| !s.is_empty())?;

    let desktop = info.downcast_ref::<gio::DesktopAppInfo>();
    let keywords = desktop
        .map(|desktop| {
            desktop
                .keywords()
                .iter()
                .map(|k| k.to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let wm_class = desktop
        .and_then(|desktop| desktop.startup_wm_class())
        .map(|s| s.to_string());
    let flatpak_id = desktop
        .and_then(|desktop| desktop.string("X-Flatpak"))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    Some(AppEntry {
        id,
        name,
        exec,
        icon: info.icon(),
        keywords,
        wm_class,
        flatpak_id,
    })
}

/// Strip freedesktop Exec field codes (`%U`, `%f`, `%i`, ...) so the residual
/// command line can be spawned directly by the compositor.
fn clean_exec(exec: &str) -> String {
    exec.split_whitespace()
        .filter(|tok| !(tok.len() == 2 && tok.starts_with('%')))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Apps the user has launched at least once, ordered by descending launch count
/// (ties broken alphabetically). Drives the "Frequent Apps" section.
pub fn frequent_from(apps: &[AppEntry], limit: usize) -> Vec<AppEntry> {
    let counts = load_menu_config().launch_counts;
    let mut scored: Vec<(u32, AppEntry)> = apps
        .iter()
        .filter_map(|e| counts.get(&e.id).copied().map(|c| (c, e.clone())))
        .filter(|(c, _)| *c > 0)
        .collect();
    scored.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1.name.to_lowercase().cmp(&b.1.name.to_lowercase()))
    });
    scored.into_iter().take(limit).map(|(_, e)| e).collect()
}

/// Apps the user has launched at least once, ordered by descending launch count
/// (ties broken alphabetically). Drives the "Frequent Apps" section.
pub fn frequent(limit: usize) -> Vec<AppEntry> {
    frequent_from(&list_apps(), limit)
}

/// Case-insensitive search over app name and keywords, ranked by launch count.
pub fn search_in(apps: &[AppEntry], query: &str) -> Vec<AppEntry> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    let counts = load_menu_config().launch_counts;
    let mut matches: Vec<AppEntry> = apps
        .iter()
        .filter(|e| {
            e.name.to_lowercase().contains(&needle)
                || e.keywords.iter().any(|k| k.to_lowercase().contains(&needle))
        })
        .cloned()
        .collect();
    matches.sort_by(|a, b| {
        let starts_a = a.name.to_lowercase().starts_with(&needle);
        let starts_b = b.name.to_lowercase().starts_with(&needle);
        starts_b
            .cmp(&starts_a)
            .then_with(|| {
                counts
                    .get(&b.id)
                    .copied()
                    .unwrap_or(0)
                    .cmp(&counts.get(&a.id).copied().unwrap_or(0))
            })
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    matches
}

/// Case-insensitive search over app name and keywords, ranked by launch count.
pub fn search(query: &str) -> Vec<AppEntry> {
    search_in(&list_apps(), query)
}

/// The user's pinned apps, in saved order, resolved to current `AppEntry`s.
pub fn pinned_entries_from(apps: &[AppEntry]) -> Vec<AppEntry> {
    let pinned = load_menu_config().pinned;
    pinned
        .iter()
        .filter_map(|id| apps.iter().find(|e| &e.id == id).cloned())
        .collect()
}

/// The user's pinned apps, in saved order, resolved to current `AppEntry`s.
pub fn pinned_entries() -> Vec<AppEntry> {
    pinned_entries_from(&list_apps())
}

/// Toggle an app's pinned state, persisting the change. Returns the new state.
pub fn toggle_pin(id: &str) -> bool {
    let mut cfg = load_menu_config();
    let now_pinned = if let Some(pos) = cfg.pinned.iter().position(|p| p == id) {
        cfg.pinned.remove(pos);
        false
    } else {
        cfg.pinned.push(id.to_string());
        true
    };
    if let Err(err) = save_menu_config(&cfg) {
        tracing::warn!(%err, "failed to persist menu.json after pin toggle");
    }
    now_pinned
}

/// Fallback icon name used when a window's `app_id` matches no installed app.
pub const FALLBACK_ICON_NAME: &str = "application-x-executable-symbolic";

/// Resolve a running window's Wayland `app_id` to its launcher entry. The
/// Wayland `app_id` is usually reverse-DNS (e.g. `org.gnome.Calculator`), which
/// matches a desktop file basename, but some apps report their `StartupWMClass`
/// instead — both are matched case-insensitively.
pub fn resolve_entry_for_app_id(app_id: &str) -> Option<AppEntry> {
    let needle = app_id.trim().to_lowercase();
    if needle.is_empty() {
        return None;
    }
    list_apps().into_iter().find(|e| {
        let base = e
            .id
            .strip_suffix(".desktop")
            .unwrap_or(&e.id)
            .to_lowercase();
        base == needle
            || e.id.to_lowercase() == needle
            || e
                .wm_class
                .as_deref()
                .is_some_and(|w| w.to_lowercase() == needle)
            || e
                .flatpak_id
                .as_deref()
                .is_some_and(|f| f.to_lowercase() == needle)
    })
}

/// Resolve a window's `app_id` to a displayable icon, falling back to a generic
/// application glyph when no match (or no `app_id`) is found.
pub fn resolve_icon_for_app_id(app_id: Option<&str>) -> gio::Icon {
    app_id
        .and_then(resolve_entry_for_app_id)
        .and_then(|e| e.icon)
        .unwrap_or_else(|| gio::ThemedIcon::new(FALLBACK_ICON_NAME).upcast())
}

/// Record a launch (bumping its frequency) and spawn the app via the compositor.
pub fn launch(entry: &AppEntry) {
    let mut cfg = load_menu_config();
    *cfg.launch_counts.entry(entry.id.clone()).or_insert(0) += 1;
    if let Err(err) = save_menu_config(&cfg) {
        tracing::warn!(%err, "failed to persist menu.json after launch");
    }
    if let Err(err) = crate::compositor::launch_program(&entry.exec) {
        tracing::warn!(%err, exec = %entry.exec, "failed to launch application");
    }
}

/// Return true if `program` is an executable on `$PATH`.
fn on_path(program: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| {
        let candidate = dir.join(program);
        candidate.is_file()
            && std::fs::metadata(&candidate)
                .map(|m| {
                    use std::os::unix::fs::PermissionsExt;
                    m.permissions().mode() & 0o111 != 0
                })
                .unwrap_or(false)
    })
}

/// Resolve the command that opens Steam Big Picture / gamepad UI, or `None` when
/// Steam is not installed. Prefers a native `steam` on `$PATH`; otherwise falls
/// back to a discovered Flatpak Steam (`com.valvesoftware.Steam`). Callers gate
/// UI (e.g. the Big Picture launcher) on this returning `Some`, so non-gaming
/// setups are unaffected.
pub fn steam_big_picture_command() -> Option<String> {
    if on_path("steam") {
        return Some("steam -gamepadui".to_string());
    }
    let has_flatpak_steam = list_apps().iter().any(|e| {
        e.flatpak_id.as_deref() == Some("com.valvesoftware.Steam")
            || e.id == "com.valvesoftware.Steam.desktop"
    });
    if has_flatpak_steam && on_path("flatpak") {
        return Some("flatpak run com.valvesoftware.Steam -gamepadui".to_string());
    }
    None
}
