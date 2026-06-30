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
    // GNOME Text Editor ships a libadwaita headerbar; Metis SSD gives consistent
    // tiling controls and avoids double-chrome layout fights in grid mode.
    "org.gnome.TextEditor",
    // Firefox (snap/deb) reports varying app_ids; explicit entries plus
    // [`id_looks_firefox`] below route them to Metis chrome.
    "org.mozilla.firefox",
    "org.mozilla.Firefox",
    "firefox_firefox",
    // Chromium-based browsers on Wayland often ship only a close button in their
    // client chrome while the portal tells them the compositor owns decorations.
    // Metis grants ServerSide over xdg-decoration so they drop CSD entirely.
    "com.google.Chrome",
    "org.chromium.Chromium",
    "com.brave.Browser",
    "com.microsoft.Edge",
    "com.microsoft.EdgeDev",
];

/// Built-in headerbar — never draw Metis SSD on top (non-GNOME entries only;
/// shipped GNOME apps are covered by [`id_looks_csd`] prefix rules).
const CSD_APP_IDS: &[&str] = &[
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

/// True when the app id belongs to a Chromium-based browser that should use
/// Metis SSD (they often report a bare `"chromium"` / `"chrome"` app id).
pub fn id_looks_chromium_family(app_id: &str) -> bool {
    let id = norm_app_id(app_id);
    id.contains("chromium")
        || id == "chrome"
        || id.ends_with(".chrome")
        || id.starts_with("com.google.chrome")
        || id.starts_with("com.brave.")
        || id.starts_with("com.microsoft.edge")
}

/// True for Mozilla Firefox builds (snap `firefox_firefox`, deb `firefox`, …).
pub fn id_looks_firefox(app_id: &str) -> bool {
    norm_app_id(app_id).contains("firefox")
}

/// True when the app has no native titlebar and needs Metis chrome.
pub fn id_looks_ssd(app_id: &str) -> bool {
    id_matches_list(app_id, SSD_APP_IDS)
        || id_looks_chromium_family(app_id)
        || id_looks_firefox(app_id)
}

/// True when the app ships its own titlebar (GNOME/libadwaita, browsers, …).
pub fn id_looks_csd(app_id: &str) -> bool {
    if id_looks_ssd(app_id) {
        return false;
    }
    let id = norm_app_id(app_id);
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
    id.contains("electron")
        || id.contains("cursor")
        || id.ends_with(".code")
        || id == "code"
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
        // Honor client-side preference only for apps we classify as full CSD.
        // Unknown / not-yet-classified clients keep Metis SSD.
        Some(DecorationMode::ClientSide) => !app_id.is_some_and(id_looks_csd),
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

/// True when auto-hide should reveal only a compact control strip (top-right)
/// so the client's tab bar stays interactive (Chromium-family browsers, Firefox).
pub fn id_uses_compact_overlay(app_id: &str) -> bool {
    id_looks_chromium_family(app_id) || id_looks_firefox(app_id)
}

/// Whether an SSD-decorated window should auto-hide its Metis titlebar when
/// maximized / snapped / grid-tiled. All Metis-decorated windows use the
/// hover overlay so the client can fill the footprint; apps with top tabs only
/// see the titlebar while the pointer is in the reveal strip.
pub fn id_auto_hides_titlebar(_app_id: &str) -> bool {
    true
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
    fn firefox_and_text_editor_use_ssd() {
        assert!(resolve_uses_ssd(Some("firefox_firefox"), None));
        assert!(resolve_uses_ssd(Some("org.mozilla.firefox"), None));
        assert!(resolve_uses_ssd(Some("org.gnome.TextEditor"), None));
    }

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
    fn chromium_browsers_use_ssd() {
        assert!(resolve_uses_ssd(Some("org.chromium.Chromium"), None));
        assert!(resolve_uses_ssd(Some("com.google.Chrome"), None));
        assert!(resolve_uses_ssd(Some("chromium"), None));
        assert!(resolve_uses_ssd(
            Some("org.chromium.Chromium"),
            Some(DecorationMode::ClientSide)
        ));
        assert!(resolve_uses_ssd(
            Some("chromium"),
            Some(DecorationMode::ClientSide)
        ));
    }

    #[test]
    fn client_side_request_ignored_until_app_id_known() {
        assert!(resolve_uses_ssd(None, Some(DecorationMode::ClientSide)));
        assert!(!resolve_uses_ssd(
            Some("org.gnome.Cheese"),
            Some(DecorationMode::ClientSide)
        ));
    }

    #[test]
    fn unknown_defaults_to_ssd() {
        assert!(resolve_uses_ssd(None, None));
    }

    #[test]
    fn client_side_protocol_disables_ssd() {
        assert!(!resolve_uses_ssd(
            Some("org.gnome.Cheese"),
            Some(DecorationMode::ClientSide)
        ));
    }

    #[test]
    fn defer_paint_for_unclassified_gtk() {
        assert!(defer_ssd_paint(None, None, true));
        assert!(!defer_ssd_paint(None, None, false));
        assert!(!defer_ssd_paint(Some("org.kitty"), None, true));
        assert!(!defer_ssd_paint(None, Some(DecorationMode::ClientSide), true));
    }

    #[test]
    fn chromium_uses_compact_overlay() {
        assert!(id_uses_compact_overlay("org.chromium.Chromium"));
        assert!(id_uses_compact_overlay("chromium"));
        assert!(id_uses_compact_overlay("firefox_firefox"));
        assert!(!id_uses_compact_overlay("org.kitty"));
    }

    #[test]
    fn all_ssd_apps_auto_hide_titlebar() {
        assert!(id_auto_hides_titlebar("chromium"));
        assert!(id_auto_hides_titlebar("org.kitty"));
        assert!(id_auto_hides_titlebar("com.metis.Settings"));
    }

    #[test]
    fn decoration_bound_does_not_disable_ssd_in_resolve() {
        assert!(resolve_uses_ssd(None, None));
    }
}
