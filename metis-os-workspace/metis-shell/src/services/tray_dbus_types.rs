//! Shared D-Bus parsing helpers for the system tray.

use std::collections::HashMap;

use zbus::zvariant::{OwnedValue, Value};

#[derive(Debug, zbus::zvariant::Type, serde::Deserialize)]
pub struct MenuLayout {
    pub id: u32,
    pub fields: SubMenuLayout,
}

#[derive(Debug, zbus::zvariant::Type, serde::Deserialize)]
pub struct SubMenuLayout {
    pub id: i32,
    pub fields: HashMap<String, OwnedValue>,
    pub submenus: Vec<OwnedValue>,
}

#[derive(Debug, Clone)]
pub struct IconPixmap {
    pub width: i32,
    pub height: i32,
    pub pixels: Vec<u8>,
}

pub struct ParsedItemProps {
    pub id: String,
    pub title: String,
    pub icon_name: Option<String>,
    pub icon_theme_path: Option<String>,
    pub icon_pixmap: Option<IconPixmap>,
    pub menu: Option<String>,
    pub item_is_menu: bool,
}

#[derive(Clone, Debug)]
pub struct ServiceParts {
    pub bus_name: String,
    pub object_path: String,
}

const DEFAULT_SNI_PATH: &str = "/StatusNotifierItem";

pub fn service_parts(item_id: &str) -> Option<ServiceParts> {
    let rest = item_id.strip_prefix(':')?;
    let bus_len = rest
        .char_indices()
        .take_while(|(_, c)| c.is_ascii_digit() || *c == '.')
        .map(|(i, c)| i + c.len_utf8())
        .last()
        .unwrap_or(0);
    if bus_len == 0 {
        return None;
    }
    let unique_name = format!(":{}", &rest[..bus_len]);
    let service = item_id[unique_name.len()..].trim();

    if service.is_empty() {
        return Some(ServiceParts {
            bus_name: unique_name,
            object_path: DEFAULT_SNI_PATH.into(),
        });
    }

    if service.starts_with('/') {
        // Registered with an object path on the sender's unique connection.
        Some(ServiceParts {
            bus_name: unique_name,
            object_path: normalize_object_path(service),
        })
    } else if service.starts_with(':') {
        Some(ServiceParts {
            bus_name: service.to_string(),
            object_path: DEFAULT_SNI_PATH.into(),
        })
    } else {
        // Registered with a well-known bus name (Flameshot, most Qt/KDE apps).
        Some(ServiceParts {
            bus_name: service.to_string(),
            object_path: DEFAULT_SNI_PATH.into(),
        })
    }
}

fn normalize_object_path(service: &str) -> String {
    if service.is_empty() {
        DEFAULT_SNI_PATH.into()
    } else if service.starts_with('/') {
        service.to_string()
    } else {
        format!("/{service}")
    }
}

#[cfg(test)]
mod tests {
    use super::service_parts;

    #[test]
    fn parses_ayatana_item_id() {
        let parts = service_parts(":1.45/org/ayatana/NotificationItem/123").unwrap();
        assert_eq!(parts.bus_name, ":1.45");
        assert_eq!(parts.object_path, "/org/ayatana/NotificationItem/123");
    }

    #[test]
    fn parses_kde_well_known_service_name() {
        let parts = service_parts(":1.282org.kde.StatusNotifierItem-229658-1").unwrap();
        assert_eq!(parts.bus_name, "org.kde.StatusNotifierItem-229658-1");
        assert_eq!(parts.object_path, "/StatusNotifierItem");
    }

    #[test]
    fn parses_kde_item_id_without_leading_slash() {
        let parts = service_parts(":1.250org.kde.StatusNotifierItem-205764-1").unwrap();
        assert_eq!(parts.bus_name, "org.kde.StatusNotifierItem-205764-1");
        assert_eq!(parts.object_path, "/StatusNotifierItem");
    }

    #[test]
    fn parses_unique_name_only_fallback() {
        let parts = service_parts(":1.253").unwrap();
        assert_eq!(parts.bus_name, ":1.253");
        assert_eq!(parts.object_path, "/StatusNotifierItem");
    }

    #[test]
    fn parses_duplicate_unique_service() {
        let parts = service_parts(":1.285:1.285").unwrap();
        assert_eq!(parts.bus_name, ":1.285");
        assert_eq!(parts.object_path, "/StatusNotifierItem");
    }
}

pub fn parse_item_props(props: &HashMap<String, OwnedValue>) -> ParsedItemProps {
    let id = get_string(props, "Id")
        .filter(|s| !s.is_empty())
        .or_else(|| get_string(props, "Title"))
        .unwrap_or_else(|| "tray-item".into());
    let title = get_string(props, "Title")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| id.clone());
    ParsedItemProps {
        id: id.clone(),
        title,
        icon_name: get_string(props, "IconName"),
        icon_theme_path: get_string(props, "IconThemePath"),
        icon_pixmap: get_icon_pixmap(props),
        menu: get_object_path(props, "Menu"),
        item_is_menu: get_bool(props, "ItemIsMenu").unwrap_or(false),
    }
}

fn get_string(props: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    props.get(key).and_then(|v| match &**v {
        Value::Str(s) => Some(s.as_str().to_string()),
        _ => None,
    })
}

fn get_object_path(props: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    props.get(key).and_then(|v| match &**v {
        Value::ObjectPath(p) => Some(p.as_str().to_string()),
        Value::Str(s) => Some(s.as_str().to_string()),
        _ => None,
    })
}

fn get_bool(props: &HashMap<String, OwnedValue>, key: &str) -> Option<bool> {
    props.get(key).and_then(|v| match &**v {
        Value::Bool(b) => Some(*b),
        _ => None,
    })
}

fn get_icon_pixmap(props: &HashMap<String, OwnedValue>) -> Option<IconPixmap> {
    let Value::Array(arr) = &**props.get("IconPixmap")? else {
        return None;
    };
    let mut best: Option<(i32, IconPixmap)> = None;
    for entry in arr.iter() {
        let Value::Structure(s) = entry else {
            continue;
        };
        let mut fields = s.fields().iter();
        let Value::I32(width) = fields.next()? else {
            continue;
        };
        let Value::I32(height) = fields.next()? else {
            continue;
        };
        let pixels_val = fields.next()?;
        let Value::Array(pixel_arr) = pixels_val else {
            continue;
        };
        let mut pixels = Vec::new();
        for p in pixel_arr.iter() {
            let Value::U8(byte) = p else {
                continue;
            };
            pixels.push(*byte);
        }
        let pixmap = IconPixmap {
            width: *width,
            height: *height,
            pixels,
        };
        let dist = (height - 22).abs();
        if best.as_ref().is_none_or(|(h, _)| (h - 22).abs() > dist) {
            best = Some((*height, pixmap));
        }
    }
    best.map(|(_, pm)| pm)
}
