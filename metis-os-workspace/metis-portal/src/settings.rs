use std::collections::HashMap;

use ashpd::{
    PortalError,
    backend::settings::{SettingsImpl, SettingsSignalEmitter},
    desktop::settings::{
        APPEARANCE_NAMESPACE, COLOR_SCHEME_KEY, ColorScheme, Namespace,
    },
    zbus::zvariant::{OwnedValue, Value},
};
use async_trait::async_trait;
use metis_config::{ThemeMode, load_theme_preference};

const NS_WM: &str = "org.gnome.desktop.wm.preferences";
const KEY_BUTTON_LAYOUT: &str = "button-layout";
const NS_INTERFACE: &str = "org.gnome.desktop.interface";
const KEY_DECO_LAYOUT: &str = "gtk-decoration-layout";
const KEY_GTK_THEME: &str = "gtk-theme";
/// Standard Ubuntu/GNOME headerbar layout for client-side decorations.
///
/// Metis only draws server-side chrome for classified terminal/SSD apps; GTK
/// headerbar clients (Cheese, Calculator, …) and browsers keep native CSD.
/// Serving `":"` here hid minimize/maximize on Firefox/Chromium and could
/// leave a partial headerbar visible alongside Metis SSD on misclassified apps.
const GNOME_CSD_BUTTON_LAYOUT: &str = "icon:minimize,maximize,close";

struct Snapshot {
    color_scheme: ColorScheme,
    gtk_theme: String,
}

impl Snapshot {
    fn from_config() -> Self {
        let mode = load_theme_preference().unwrap_or(ThemeMode::Dark);
        Self {
            color_scheme: color_scheme_for(&mode),
            gtk_theme: gtk_theme_for(&mode),
        }
    }
}

fn color_scheme_for(mode: &ThemeMode) -> ColorScheme {
    match mode {
        ThemeMode::Light => ColorScheme::PreferLight,
        ThemeMode::Dark => ColorScheme::PreferDark,
        ThemeMode::System => ColorScheme::NoPreference,
    }
}

fn gtk_theme_for(mode: &ThemeMode) -> String {
    match mode {
        ThemeMode::Light => "Adwaita".into(),
        ThemeMode::Dark | ThemeMode::System => "Adwaita-dark".into(),
    }
}

fn owned_string(value: &str) -> OwnedValue {
    Value::from(value).try_into().unwrap_or_else(|_| {
        OwnedValue::try_from(Value::from(String::from(value))).expect("portal string value")
    })
}

fn namespace_allowed(namespaces: &[String], ns: &str) -> bool {
    namespaces.is_empty() || namespaces.iter().any(|n| n == ns)
}

fn appearance_namespace(snapshot: &Snapshot) -> Namespace {
    let mut map = HashMap::new();
    map.insert(
        COLOR_SCHEME_KEY.to_owned(),
        OwnedValue::from(snapshot.color_scheme),
    );
    map
}

fn interface_namespace(snapshot: &Snapshot) -> Namespace {
    let mut map = HashMap::new();
    map.insert(
        KEY_DECO_LAYOUT.to_owned(),
        owned_string(GNOME_CSD_BUTTON_LAYOUT),
    );
    map.insert(
        KEY_GTK_THEME.to_owned(),
        owned_string(&snapshot.gtk_theme),
    );
    map
}

fn wm_namespace() -> Namespace {
    let mut map = HashMap::new();
    map.insert(
        KEY_BUTTON_LAYOUT.to_owned(),
        owned_string(GNOME_CSD_BUTTON_LAYOUT),
    );
    map
}

fn read_key(snapshot: &Snapshot, namespace: &str, key: &str) -> Result<OwnedValue, PortalError> {
    match (namespace, key) {
        (APPEARANCE_NAMESPACE, COLOR_SCHEME_KEY) => Ok(OwnedValue::from(snapshot.color_scheme)),
        (NS_WM, KEY_BUTTON_LAYOUT) => Ok(owned_string(GNOME_CSD_BUTTON_LAYOUT)),
        (NS_INTERFACE, KEY_DECO_LAYOUT) => Ok(owned_string(GNOME_CSD_BUTTON_LAYOUT)),
        (NS_INTERFACE, KEY_GTK_THEME) => Ok(owned_string(&snapshot.gtk_theme)),
        _ => Err(PortalError::NotFound(format!(
            "unknown namespace/key: {namespace}/{key}"
        ))),
    }
}

pub struct MetisSettings;

#[async_trait]
impl SettingsImpl for MetisSettings {
    async fn read_all(
        &self,
        namespaces: Vec<String>,
    ) -> Result<HashMap<String, Namespace>, PortalError> {
        let snapshot = Snapshot::from_config();
        let mut out = HashMap::new();
        if namespace_allowed(&namespaces, APPEARANCE_NAMESPACE) {
            out.insert(
                APPEARANCE_NAMESPACE.to_owned(),
                appearance_namespace(&snapshot),
            );
        }
        if namespace_allowed(&namespaces, NS_INTERFACE) {
            out.insert(
                NS_INTERFACE.to_owned(),
                interface_namespace(&snapshot),
            );
        }
        if namespace_allowed(&namespaces, NS_WM) {
            out.insert(NS_WM.to_owned(), wm_namespace());
        }
        Ok(out)
    }

    async fn read(&self, namespace: &str, key: &str) -> Result<OwnedValue, PortalError> {
        let snapshot = Snapshot::from_config();
        read_key(&snapshot, namespace, key)
    }

    fn set_signal_emitter(&mut self, _signal_emitter: std::sync::Arc<dyn SettingsSignalEmitter>) {}
}
