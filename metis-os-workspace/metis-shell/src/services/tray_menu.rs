//! Parsed com.canonical.dbusmenu layouts for tray context menus.

use zbus::zvariant::{Dict, OwnedValue, Structure, Value};

use super::tray_dbus_types::MenuLayout;

#[derive(Debug, Clone)]
pub struct TrayMenu {
    pub id: u32,
    pub submenus: Vec<MenuItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuType {
    Separator,
    Standard,
}

#[derive(Debug, Clone)]
pub struct MenuItem {
    pub id: i32,
    pub label: String,
    pub enabled: bool,
    pub visible: bool,
    pub menu_type: MenuType,
    /// `submenu` when this row is a submenu container (show children only).
    pub children_display: Option<String>,
    pub submenu: Vec<MenuItem>,
}

impl Default for MenuItem {
    fn default() -> Self {
        Self {
            id: 0,
            label: String::new(),
            enabled: true,
            visible: true,
            menu_type: MenuType::Standard,
            children_display: None,
            submenu: Vec::new(),
        }
    }
}

pub fn parse_menu_layout(layout: MenuLayout) -> TrayMenu {
    let submenus = layout
        .fields
        .submenus
        .iter()
        .filter_map(parse_menu_item)
        .collect();
    TrayMenu {
        id: layout.id,
        submenus,
    }
}

fn parse_menu_item(value: &OwnedValue) -> Option<MenuItem> {
    parse_menu_value(&**value)
}

fn parse_menu_value(value: &Value<'_>) -> Option<MenuItem> {
    let structure = match value.downcast_ref::<Structure>() {
        Ok(s) => s,
        Err(_) => return None,
    };
    let mut fields = structure.fields().iter();
    let mut item = MenuItem::default();

    if let Some(Value::I32(id)) = fields.next() {
        item.id = *id;
    }

    let Value::Dict(dict) = fields.next()? else {
        return Some(item);
    };

    if let Some(label) = dict_str(dict, "label") {
        item.label = label;
    }
    if let Some(enabled) = dict_bool(dict, "enabled") {
        item.enabled = enabled;
    }
    if let Some(visible) = dict_bool(dict, "visible") {
        item.visible = visible;
    }
    if let Some(display) = dict_str(dict, "children-display") {
        item.children_display = Some(display);
    }
    if let Some(kind) = dict_str(dict, "type") {
        item.menu_type = match kind.as_str() {
            "separator" => MenuType::Separator,
            _ => MenuType::Standard,
        };
    }

    if let Some(Value::Array(arr)) = fields.next() {
        for child in arr.iter() {
            if let Some(sub) = parse_menu_value(child) {
                item.submenu.push(sub);
            }
        }
    }

    Some(item)
}

fn dict_str(dict: &Dict<'_, '_>, key: &str) -> Option<String> {
    dict.get::<&str, &str>(&key)
        .ok()
        .flatten()
        .map(|label| label.replace('_', ""))
}

fn dict_bool(dict: &Dict<'_, '_>, key: &str) -> Option<bool> {
    dict.get::<&str, bool>(&key).ok().flatten()
}
