//! Heuristics for when Metis draws server-side window chrome (SSD) vs leaving
//! decorations to the client (CSD).
//!
//! **Default: Metis decorates.** Strip chrome only when we are confident the
//! client draws its own (known `app_id`, or `xdg-decoration` ClientSide).
//!
//! While a likely-GTK window is still starting up (`app_id` empty but
//! `xdg-decoration` already bound), defer *painting* Metis chrome so we do not
//! flash a second titlebar — but keep `uses_ssd` true so borderless clients
//! still get controls once classified.

use smithay::reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode as DecorationMode;

/// No native window chrome — Metis draws titlebar + traffic lights.
const SSD_APP_IDS: &[&str] = &[
    "org.alacritty",
    "com.mitchellh.ghostty",
    "org.wezfwez.foot",
    "org.kitty",
    "net.kovidgoyal.kitty",
    "com.metis.Settings",
];

/// Built-in headerbar — never draw Metis SSD on top (non-GNOME entries only;
/// shipped GNOME apps are covered by [`id_looks_csd`] prefix rules).
const CSD_APP_IDS: &[&str] = &[
    "com.google.Chrome",
    "org.chromium.Chromium",
    "com.brave.Browser",
    "com.microsoft.Edge",
    "com.microsoft.EdgeDev",
    "org.mozilla.firefox",
    "org.mozilla.Firefox",
    "dev.zed.Zed",
    "com.obsproject.Studio",
    "com.slack.Slack",
    "com.discordapp.Discord",
    "com.spotify.Client",
    "com.visualstudio.code",
    "code-oss",
    "Cursor",
    "cursor",
    "com.cursor.Cursor",
    "dev.cursor.Cursor",
    "com.github.PintaProject.Pinta",
];

fn norm_app_id(app_id: &str) -> String {
    app_id.trim().to_lowercase()
}

fn id_matches_list(app_id: &str, list: &[&str]) -> bool {
    let id = norm_app_id(app_id);
    list.iter().any(|entry| {
        let entry = entry.trim().to_lowercase();
        id == entry || id.ends_with(&format!(".{entry}"))
    })
}

/// True when the app ships its own titlebar (GNOME/libadwaita, browsers, …).
pub fn id_looks_csd(app_id: &str) -> bool {
    let id = norm_app_id(app_id);
    if id_matches_list(app_id, SSD_APP_IDS) {
        return false;
    }
    if id_matches_list(app_id, CSD_APP_IDS) {
        return true;
    }
    if id.starts_with("org.gnome.") {
        return true;
    }
    if id.starts_with("io.github.")
        || id.starts_with("io.gitlab.")
        || id.starts_with("io.sourceforge.")
    {
        return true;
    }
    id.contains("chromium")
        || id.contains("chrome")
        || id.contains("firefox")
        || id.contains("electron")
        || id.contains("cursor")
        || id.ends_with(".code")
        || id == "code"
}

/// True when the app has no native titlebar and needs Metis chrome.
pub fn id_looks_ssd(app_id: &str) -> bool {
    id_matches_list(app_id, SSD_APP_IDS)
}

/// Whether Metis should own window chrome for this client.
pub fn resolve_uses_ssd(app_id: Option<&str>, negotiated_mode: Option<DecorationMode>) -> bool {
    if let Some(app_id) = app_id.filter(|id| !id.is_empty()) {
        if id_looks_ssd(app_id) {
            return true;
        }
        if id_looks_csd(app_id) {
            return false;
        }
    }

    match negotiated_mode {
        Some(DecorationMode::ClientSide) => false,
        Some(DecorationMode::ServerSide) => !app_id.is_some_and(id_looks_csd),
        Some(_) => !app_id.is_some_and(id_looks_csd),
        None => true,
    }
}

/// Defer drawing Metis SSD while a headerbar app is still reporting its `app_id`.
/// Does not change `uses_ssd` — borderless clients keep default SSD once un-deferred.
pub fn defer_ssd_paint(
    app_id: Option<&str>,
    negotiated_mode: Option<DecorationMode>,
    decoration_bound: bool,
) -> bool {
    if app_id.is_some_and(|id| !id.is_empty()) {
        return false;
    }
    if negotiated_mode.is_some() {
        return false;
    }
    decoration_bound
}

/// Mode to grant over `xdg-decoration` for a window we manage.
pub fn grant_decoration_mode(uses_ssd: bool) -> DecorationMode {
    if uses_ssd {
        DecorationMode::ServerSide
    } else {
        DecorationMode::ClientSide
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gnome_apps_keep_client_headerbar() {
        assert!(!resolve_uses_ssd(Some("org.gnome.Cheese"), None));
        assert!(!resolve_uses_ssd(Some("org.gnome.Calculator"), None));
    }

    #[test]
    fn metis_and_terminals_use_ssd() {
        assert!(resolve_uses_ssd(Some("org.kitty"), None));
        assert!(resolve_uses_ssd(Some("com.metis.Settings"), None));
    }

    #[test]
    fn unknown_defaults_to_ssd() {
        assert!(resolve_uses_ssd(None, None));
    }

    #[test]
    fn client_side_protocol_disables_ssd() {
        assert!(!resolve_uses_ssd(None, Some(DecorationMode::ClientSide)));
    }

    #[test]
    fn defer_paint_for_unclassified_gtk() {
        assert!(defer_ssd_paint(None, None, true));
        assert!(!defer_ssd_paint(None, None, false));
        assert!(!defer_ssd_paint(Some("org.kitty"), None, true));
        assert!(!defer_ssd_paint(None, Some(DecorationMode::ClientSide), true));
    }

    #[test]
    fn decoration_bound_does_not_disable_ssd_in_resolve() {
        assert!(resolve_uses_ssd(None, None));
    }
}
