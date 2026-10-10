use crate::config::store::get_path;
use oxibar_plugin_api::toml::{Table, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockConfig {
    pub enabled: bool,
    pub outputs: Vec<String>,
    pub fallback: bool,
    pub pinned: Vec<String>,
    pub running: bool,
    pub bottom: bool,
    pub icon_size: u32,
    pub margin: u32,
    pub reserve: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WidgetSpec {
    pub id: String,
    pub plugin: String,
    pub output: String,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub enabled: bool,
    pub interactive: Option<bool>,
}

impl WidgetSpec {
    pub fn value(&self) -> Value {
        let mut table = Table::new();
        for (key, value) in [
            ("id", &self.id),
            ("plugin", &self.plugin),
            ("output", &self.output),
        ] {
            table.insert(key.into(), Value::String(value.clone()));
        }
        for (key, value) in [
            ("x", self.x),
            ("y", self.y),
            ("width", self.width),
            ("height", self.height),
        ] {
            table.insert(key.into(), Value::Integer(i64::from(value)));
        }
        table.insert("enabled".into(), Value::Boolean(self.enabled));
        if let Some(interactive) = self.interactive {
            table.insert("interactive".into(), Value::Boolean(interactive));
        }
        Value::Table(table)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopConfig {
    pub enabled: bool,
    pub fallback: bool,
    pub snap: u32,
    pub widgets: Vec<WidgetSpec>,
}

pub(crate) fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

pub(crate) fn widgets(table: &Table) -> Result<Vec<WidgetSpec>, String> {
    let Some(values) = get_path(table, "desktop.widgets").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    if values.len() > 32 {
        return Err("desktop.widgets supports at most 32 instances".into());
    }
    let mut ids = std::collections::BTreeSet::new();
    values
        .iter()
        .map(|value| {
            let table = value.as_table().ok_or("Desktop widgets must be tables")?;
            let string = |key: &str, default: &str| -> Result<String, String> {
                table.get(key).map_or(Ok(default.to_owned()), |value| {
                    value
                        .as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| format!("Widget {key} must be text"))
                })
            };
            let number = |key: &str, default: u32, min: u32| -> Result<u32, String> {
                let value = table.get(key).map_or(Ok(i64::from(default)), |value| {
                    value
                        .as_integer()
                        .ok_or_else(|| format!("Widget {key} must be an integer"))
                })?;
                if !(i64::from(min)..=16384).contains(&value) {
                    return Err(format!("Widget {key} is out of range"));
                }
                Ok(value as u32)
            };
            let id = string("id", "")?;
            let plugin = string("plugin", "")?;
            if !valid_id(&id) || !ids.insert(id.clone()) {
                return Err("Widget IDs must be unique nonempty identifiers".into());
            }
            if plugin.is_empty() || plugin.len() > 128 {
                return Err("Widget plugin must be a nonempty name".into());
            }
            let enabled = table.get("enabled").map_or(Ok(true), |value| {
                value.as_bool().ok_or("Widget enabled must be boolean")
            })?;
            let interactive = table
                .get("interactive")
                .map(|value| value.as_bool().ok_or("Widget interactive must be boolean"))
                .transpose()?;
            Ok(WidgetSpec {
                id,
                plugin,
                output: string("output", "")?,
                x: number("x", 32, 0)?,
                y: number("y", 64, 0)?,
                width: number("width", 360, 64)?,
                height: number("height", 360, 64)?,
                enabled,
                interactive,
            })
        })
        .collect()
}

pub(crate) fn dock(table: &Table) -> DockConfig {
    let boolean = |path| get_path(table, path).and_then(Value::as_bool).unwrap();
    let integer = |path| get_path(table, path).and_then(Value::as_integer).unwrap() as u32;
    let list = |path| {
        get_path(table, path)
            .and_then(Value::as_array)
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect()
    };
    DockConfig {
        enabled: boolean("dock.enabled"),
        outputs: list("dock.outputs"),
        fallback: boolean("dock.output_fallback"),
        pinned: list("dock.pinned"),
        running: boolean("dock.show_running"),
        bottom: get_path(table, "dock.position").and_then(Value::as_str) == Some("bottom"),
        icon_size: integer("dock.icon_size"),
        margin: integer("dock.margin"),
        reserve: boolean("dock.reserve_space"),
    }
}

pub(crate) fn desktop(table: &Table) -> DesktopConfig {
    DesktopConfig {
        enabled: get_path(table, "desktop.enabled")
            .and_then(Value::as_bool)
            .unwrap(),
        fallback: get_path(table, "desktop.output_fallback")
            .and_then(Value::as_bool)
            .unwrap(),
        snap: get_path(table, "desktop.grid")
            .and_then(Value::as_integer)
            .unwrap() as u32,
        widgets: widgets(table).expect("Validated widget layout"),
    }
}
