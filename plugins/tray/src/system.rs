use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use zbus::{
    blocking::Connection,
    zvariant::{OwnedObjectPath, OwnedValue, Structure, Type, Value},
};

const ITEM_IFACE: &str = "org.kde.StatusNotifierItem";
const DBUSMENU_IFACE: &str = "com.canonical.dbusmenu";

#[derive(Clone, Debug)]
pub struct TrayItem {
    pub(crate) key: String,
    pub(crate) service: String,
    pub(crate) path: String,
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) icon_path: Option<String>,
    pub(crate) menu_path: Option<String>,
}

impl TrayItem {
    pub(crate) fn new_fallback(service: String, path: String, title: String) -> Self {
        Self {
            key: format!("{service}{path}"),
            service,
            path,
            id: String::new(),
            title,
            icon_path: None,
            menu_path: None,
        }
    }

    pub(crate) fn label(&self) -> String {
        if !self.title.is_empty() {
            self.title.clone()
        } else {
            let label = if !self.id.is_empty() {
                &self.id
            } else {
                &self.service
            };
            label
                .rsplit('.')
                .next()
                .unwrap_or(label)
                .trim_matches(':')
                .to_owned()
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum TrayAction {
    Activate,
    SecondaryActivate,
    ContextMenu,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrayMenuAction {
    DbusMenu(i32),
    Activate,
    SecondaryActivate,
    NativeContextMenu,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayMenuItem {
    pub(crate) label: String,
    pub(crate) enabled: bool,
    pub(crate) depth: usize,
    pub(crate) action: Option<TrayMenuAction>,
    pub(crate) separator: bool,
}

#[derive(Debug, Deserialize, Type)]
struct MenuLayoutItem {
    id: i32,
    properties: HashMap<String, OwnedValue>,
    children: Vec<OwnedValue>,
}

pub(crate) fn normalize_item_address(sender: &str, service_or_path: &str) -> (String, String) {
    if service_or_path.starts_with('/') {
        (sender.to_owned(), service_or_path.to_owned())
    } else {
        (service_or_path.to_owned(), "/StatusNotifierItem".to_owned())
    }
}

pub(crate) fn query_item(service: &str, path: &str) -> Option<TrayItem> {
    let connection = Connection::session().ok()?;
    let id = get_item_string_property(&connection, service, path, "Id").unwrap_or_default();
    let title = get_item_string_property(&connection, service, path, "Title").unwrap_or_default();
    let icon_name =
        get_item_string_property(&connection, service, path, "IconName").unwrap_or_default();
    let icon_theme_path =
        get_item_string_property(&connection, service, path, "IconThemePath").unwrap_or_default();
    let menu_path = get_item_object_path_property(&connection, service, path, "Menu");
    let icon_path = resolve_icon_path(&icon_name, &icon_theme_path);
    Some(TrayItem {
        key: format!("{service}{path}"),
        service: service.to_owned(),
        path: path.to_owned(),
        id,
        title,
        icon_path,
        menu_path,
    })
}

fn resolve_icon_path(icon_name: &str, icon_theme_path: &str) -> Option<String> {
    if icon_name.is_empty() {
        return None;
    }
    let direct = Path::new(icon_name);
    if direct.is_absolute() && direct.exists() {
        return Some(icon_name.to_owned());
    }

    let mut roots = Vec::new();
    if !icon_theme_path.is_empty() {
        roots.push(PathBuf::from(icon_theme_path));
    }
    if let Ok(home) = std::env::var("HOME") {
        roots.push(PathBuf::from(format!("{home}/.local/share/icons")));
        roots.push(PathBuf::from(format!("{home}/.icons")));
        roots.push(PathBuf::from(format!("{home}/.nix-profile/share/icons")));
    }
    if let Ok(data_dirs) = std::env::var("XDG_DATA_DIRS") {
        for dir in data_dirs.split(':').filter(|dir| !dir.is_empty()) {
            roots.push(PathBuf::from(dir).join("icons"));
            roots.push(PathBuf::from(dir).join("pixmaps"));
        }
    } else {
        roots.push(PathBuf::from("/usr/local/share/icons"));
        roots.push(PathBuf::from("/usr/share/icons"));
        roots.push(PathBuf::from("/usr/share/pixmaps"));
    }
    roots.push(PathBuf::from("/run/current-system/sw/share/icons"));
    roots.push(PathBuf::from("/run/current-system/sw/share/pixmaps"));

    roots
        .into_iter()
        .find_map(|root| find_icon_in_dir(&root, icon_name, 0))
        .map(|path| path.to_string_lossy().into_owned())
}

fn find_icon_in_dir(root: &Path, icon_name: &str, depth: u8) -> Option<PathBuf> {
    if depth > 8 || !root.is_dir() {
        return None;
    }
    let candidates = [
        root.join(format!("{icon_name}.svg")),
        root.join(format!("{icon_name}.svgz")),
        root.join(format!("{icon_name}.png")),
        root.join(format!("{icon_name}.xpm")),
    ];
    if let Some(path) = candidates.into_iter().find(|path| path.is_file()) {
        return Some(path);
    }

    let entries = std::fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir()
            && let Some(found) = find_icon_in_dir(&path, icon_name, depth + 1)
        {
            return Some(found);
        }
    }
    None
}

fn get_item_string_property(
    connection: &Connection,
    service: &str,
    path: &str,
    property: &str,
) -> Option<String> {
    let reply = connection
        .call_method(
            Some(service),
            path,
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &(ITEM_IFACE, property),
        )
        .ok()?;
    let value = reply.body().deserialize::<OwnedValue>().ok()?;
    value.try_into().ok()
}

fn get_item_object_path_property(
    connection: &Connection,
    service: &str,
    path: &str,
    property: &str,
) -> Option<String> {
    let reply = connection
        .call_method(
            Some(service),
            path,
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &(ITEM_IFACE, property),
        )
        .ok()?;
    let value = reply.body().deserialize::<OwnedValue>().ok()?;
    let path = OwnedObjectPath::try_from(value).ok()?;
    Some(path.to_string())
}

pub(crate) fn call_item_action(item: &TrayItem, action: TrayAction) -> zbus::Result<()> {
    let connection = Connection::session()?;
    let method = match action {
        TrayAction::Activate => "Activate",
        TrayAction::SecondaryActivate => "SecondaryActivate",
        TrayAction::ContextMenu => "ContextMenu",
    };
    connection.call_method(
        Some(item.service.as_str()),
        item.path.as_str(),
        Some(ITEM_IFACE),
        method,
        &(0i32, 0i32),
    )?;
    Ok(())
}

pub(crate) fn query_item_menu(item: &TrayItem) -> zbus::Result<Vec<TrayMenuItem>> {
    let Some(menu_path) = item.menu_path.as_deref() else {
        return Ok(Vec::new());
    };
    let connection = Connection::session()?;
    let _ = connection.call_method(
        Some(item.service.as_str()),
        menu_path,
        Some(DBUSMENU_IFACE),
        "AboutToShow",
        &(0i32,),
    );
    let reply = connection.call_method(
        Some(item.service.as_str()),
        menu_path,
        Some(DBUSMENU_IFACE),
        "GetLayout",
        &(0i32, -1i32, Vec::<String>::new()),
    )?;
    let (_revision, layout) = reply.body().deserialize::<(u32, MenuLayoutItem)>()?;
    let mut items = Vec::new();
    flatten_menu(layout.children, 0, &mut items);
    Ok(items)
}

pub(crate) fn call_menu_item(item: &TrayItem, id: i32) -> zbus::Result<()> {
    let Some(menu_path) = item.menu_path.as_deref() else {
        return Ok(());
    };
    let connection = Connection::session()?;
    connection.call_method(
        Some(item.service.as_str()),
        menu_path,
        Some(DBUSMENU_IFACE),
        "Event",
        &(id, "clicked", Value::from(0i32), 0u32),
    )?;
    Ok(())
}

fn flatten_menu(layout: Vec<OwnedValue>, depth: usize, out: &mut Vec<TrayMenuItem>) {
    for item in layout {
        let Some(item) = menu_layout_from_owned(item) else {
            continue;
        };
        let kind = property_string(&item.properties, "type");
        let visible = property_bool(&item.properties, "visible").unwrap_or(true);
        if !visible {
            continue;
        }

        let separator = kind.as_deref() == Some("separator");
        let label = property_string(&item.properties, "label")
            .map(clean_menu_label)
            .unwrap_or_default();
        let enabled = property_bool(&item.properties, "enabled").unwrap_or(true);
        if separator || !label.is_empty() {
            out.push(TrayMenuItem {
                label,
                enabled: enabled && !separator,
                depth,
                action: (!separator).then_some(TrayMenuAction::DbusMenu(item.id)),
                separator,
            });
        }

        flatten_menu(item.children, depth + 1, out);
    }
}

fn menu_layout_from_owned(value: OwnedValue) -> Option<MenuLayoutItem> {
    let structure = Structure::try_from(value).ok()?;
    let mut fields = structure.into_fields().into_iter();
    let id = i32::try_from(fields.next()?).ok()?;
    let properties = HashMap::<String, OwnedValue>::try_from(fields.next()?).ok()?;
    let children = Vec::<OwnedValue>::try_from(fields.next()?).ok()?;
    Some(MenuLayoutItem {
        id,
        properties,
        children,
    })
}

fn property_string(properties: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    properties
        .get(key)
        .cloned()
        .and_then(|value| String::try_from(value).ok())
}

fn property_bool(properties: &HashMap<String, OwnedValue>, key: &str) -> Option<bool> {
    properties
        .get(key)
        .cloned()
        .and_then(|value| bool::try_from(value).ok())
}

fn clean_menu_label(label: String) -> String {
    label
        .replace("__", "\0")
        .replace('_', "")
        .replace('\0', "_")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tray_item(key: &str, title: &str) -> TrayItem {
        TrayItem {
            key: key.to_owned(),
            service: "org.example.App".to_owned(),
            path: "/StatusNotifierItem".to_owned(),
            id: "example-id".to_owned(),
            title: title.to_owned(),
            icon_path: None,
            menu_path: None,
        }
    }

    #[test]
    fn normalizes_status_notifier_addresses() {
        assert_eq!(
            normalize_item_address(":1.42", "/custom/path"),
            (":1.42".to_owned(), "/custom/path".to_owned())
        );
        assert_eq!(
            normalize_item_address(":1.42", "org.example.App"),
            (
                "org.example.App".to_owned(),
                "/StatusNotifierItem".to_owned()
            )
        );
    }

    #[test]
    fn tray_item_label_uses_title_then_id_then_service_tail() {
        assert_eq!(tray_item("k", "Title").label(), "Title");

        let mut item = tray_item("k", "");
        assert_eq!(item.label(), "example-id");

        item.id.clear();
        assert_eq!(item.label(), "App");
    }

    #[test]
    fn clean_menu_label_removes_mnemonic_markers() {
        assert_eq!(clean_menu_label("_Open".to_owned()), "Open");
        assert_eq!(clean_menu_label("Save __As".to_owned()), "Save _As");
    }
}
