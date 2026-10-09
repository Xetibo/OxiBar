use oxibar_plugin_api::{ApplyMode, SettingDescriptor, SettingKind};
use toml::{Table, Value};

use super::store::{get_path, set_path};
use crate::layout;

pub fn host_schema() -> Vec<SettingDescriptor> {
    use ApplyMode::{Live, Restart, Surface};
    use SettingKind::{Boolean, Integer, Text, TextList};
    let dimension = Integer {
        min: 1,
        max: layout::MAX_HOST_SURFACE_DIMENSION,
    };
    let count = Integer { min: 1, max: 50 };
    vec![
        SettingDescriptor::new(
            "bar.enabled",
            "Enable bars",
            Boolean,
            Some(Value::Boolean(true)),
            Surface,
        ),
        SettingDescriptor::new(
            "bar.outputs",
            "Bar outputs",
            TextList,
            Some(Value::Array(Vec::new())),
            Surface,
        )
        .description(
            "[\"*\"] shows a bar on every output; [] retains one compositor-selected output.",
        ),
        SettingDescriptor::new(
            "bar.position",
            "Bar edge",
            SettingKind::Choice(vec!["top".to_owned(), "bottom".to_owned()]),
            Some(Value::String("top".to_owned())),
            Surface,
        ),
        SettingDescriptor::new(
            "bar.output_fallback",
            "Fallback when selected outputs disappear",
            Boolean,
            Some(Value::Boolean(true)),
            Surface,
        ),
        SettingDescriptor::new(
            "monitor.refresh_seconds",
            "Monitor inventory interval",
            Integer { min: 1, max: 3600 },
            Some(Value::Integer(2)),
            Live,
        ),
        SettingDescriptor::new(
            "bar.height",
            "Bar height",
            dimension.clone(),
            Some(Value::Integer(layout::DEFAULT_BAR_HEIGHT as i64)),
            Surface,
        ),
        SettingDescriptor::new("bar.width", "Bar width", dimension.clone(), None, Surface)
            .description("Unset follows the output width."),
        SettingDescriptor::new(
            "bar.transparent",
            "Transparent bar",
            Boolean,
            Some(Value::Boolean(false)),
            Live,
        ),
        SettingDescriptor::new(
            "bar.font",
            "Font family",
            Text,
            Some(Value::String("Adwaita Sans".to_owned())),
            Restart,
        ),
        SettingDescriptor::new("bar.font_file", "Font file", Text, None, Restart),
        SettingDescriptor::new(
            "bar.start",
            "Start widgets",
            TextList,
            Some(Value::Array(Vec::new())),
            Live,
        ),
        SettingDescriptor::new(
            "bar.center",
            "Center widgets",
            TextList,
            Some(Value::Array(Vec::new())),
            Live,
        ),
        SettingDescriptor::new(
            "bar.end",
            "End widgets",
            TextList,
            Some(Value::Array(Vec::new())),
            Live,
        ),
        SettingDescriptor::new(
            "launcher.enabled",
            "Enable launcher",
            Boolean,
            Some(Value::Boolean(true)),
            Live,
        ),
        SettingDescriptor::new(
            "launcher.width",
            "Launcher width",
            dimension.clone(),
            Some(Value::Integer(layout::DEFAULT_LAUNCHER_SIZE.0 as i64)),
            Surface,
        ),
        SettingDescriptor::new(
            "launcher.height",
            "Launcher height",
            dimension.clone(),
            Some(Value::Integer(layout::DEFAULT_LAUNCHER_SIZE.1 as i64)),
            Surface,
        ),
        SettingDescriptor::new(
            "launcher.max_entries",
            "Result limit",
            count,
            Some(Value::Integer(7)),
            Live,
        ),
        SettingDescriptor::new(
            "settings.width",
            "Settings width",
            dimension.clone(),
            Some(Value::Integer(layout::DEFAULT_SETTINGS_SIZE.0 as i64)),
            Surface,
        ),
        SettingDescriptor::new(
            "control_center.enabled",
            "Enable control center",
            Boolean,
            Some(Value::Boolean(true)),
            Live,
        ),
        SettingDescriptor::new(
            "control_center.width",
            "Control center width",
            dimension.clone(),
            Some(Value::Integer(layout::DEFAULT_CONTROL_CENTER_SIZE.0 as i64)),
            Surface,
        ),
        SettingDescriptor::new(
            "control_center.height",
            "Control center height",
            dimension.clone(),
            Some(Value::Integer(layout::DEFAULT_CONTROL_CENTER_SIZE.1 as i64)),
            Surface,
        ),
        SettingDescriptor::new(
            "control_center.widgets",
            "Control center widgets",
            TextList,
            Some(Value::Array(
                [
                    "Audio",
                    "Network",
                    "Bluetooth",
                    "Battery",
                    "Notifications",
                    "Clock",
                ]
                .into_iter()
                .map(|name| Value::String(name.to_owned()))
                .collect(),
            )),
            Live,
        )
        .description(
            "Ordered plugin names. Available control views share the running plugin models.",
        ),
        SettingDescriptor::new(
            "settings.height",
            "Settings height",
            dimension,
            Some(Value::Integer(layout::DEFAULT_SETTINGS_SIZE.1 as i64)),
            Surface,
        ),
        SettingDescriptor::new(
            "plugins",
            "Enabled plugin libraries",
            TextList,
            Some(Value::Array(Vec::new())),
            Restart,
        )
        .description("Select installed libraries. Plugin lifecycle changes require a restart."),
        SettingDescriptor::new(
            "instance.allow_multiple_instances",
            "Allow multiple instances",
            Boolean,
            Some(Value::Boolean(false)),
            Restart,
        ),
        SettingDescriptor::new(
            "instance.lock_path",
            "Instance lock path",
            Text,
            None,
            Restart,
        ),
        SettingDescriptor::new("ipc.socket_path", "Control socket", Text, None, Restart),
        SettingDescriptor::new(
            "startup.enabled",
            "Startup retry",
            Boolean,
            Some(Value::Boolean(true)),
            Restart,
        ),
        SettingDescriptor::new(
            "startup.initial_delay_ms",
            "Initial retry delay",
            Integer {
                min: 0,
                max: i64::MAX,
            },
            Some(Value::Integer(500)),
            Restart,
        ),
        SettingDescriptor::new(
            "startup.max_delay_ms",
            "Maximum retry delay",
            Integer {
                min: 0,
                max: i64::MAX,
            },
            Some(Value::Integer(8000)),
            Restart,
        ),
        SettingDescriptor::new(
            "startup.max_attempts",
            "Startup attempts",
            Integer {
                min: 1,
                max: u32::MAX as i64,
            },
            None,
            Restart,
        ),
        SettingDescriptor::new(
            "startup.catch_panics",
            "Retry startup panics",
            Boolean,
            Some(Value::Boolean(true)),
            Restart,
        ),
    ]
}

pub fn defaults(schema: &[SettingDescriptor]) -> Result<Table, String> {
    let mut table = Table::new();
    for field in schema {
        if let Some(value) = field.default.as_ref() {
            validate_value(field, value)?;
            set_path(&mut table, &field.path, value.clone())?;
        }
    }
    Ok(table)
}

pub fn validate(config: &Table, schema: &[SettingDescriptor]) -> Result<(), String> {
    for section in [
        "bar",
        "launcher",
        "settings",
        "control_center",
        "startup",
        "instance",
        "ipc",
        "monitor",
    ] {
        if config.get(section).is_some_and(|value| !value.is_table()) {
            return Err(format!("{section}: expected a table"));
        }
    }
    for field in schema {
        if let Some(value) = get_path(config, &field.path) {
            validate_value(field, value)?;
        }
    }
    if let Some(sizes) = config.get("bar").and_then(|value| value.get("popup_sizes")) {
        let table = sizes
            .as_table()
            .ok_or("bar.popup_sizes: expected a table")?;
        for (name, value) in table {
            let valid = value.as_array().is_some_and(|values| {
                values.len() == 2
                    && values.iter().all(|value| {
                        value.as_integer().is_some_and(|value| {
                            value > 0
                                && value <= i32::MAX as i64 - layout::POPUP_CONNECTOR_PADDING as i64
                        })
                    })
            });
            if !valid {
                return Err(format!(
                    "bar.popup_sizes.{name}: expected [positive width, positive height]"
                ));
            }
        }
    }
    super::bars::validate_profiles(config, schema)
}

pub fn validate_value(field: &SettingDescriptor, value: &Value) -> Result<(), String> {
    let valid = match &field.kind {
        SettingKind::Boolean => value.is_bool(),
        SettingKind::Text => value.is_str(),
        SettingKind::Arguments => value.as_str().is_some_and(|value| {
            shell_words::split(value).is_ok_and(|arguments| !arguments.is_empty())
        }),
        SettingKind::TextList => value
            .as_array()
            .is_some_and(|values| values.iter().all(Value::is_str)),
        SettingKind::Integer { min, max } => value
            .as_integer()
            .is_some_and(|value| (*min..=*max).contains(&value)),
        SettingKind::Number { min, max } => value
            .as_float()
            .or_else(|| value.as_integer().map(|value| value as f64))
            .is_some_and(|value| value.is_finite() && (*min..=*max).contains(&value)),
        SettingKind::Choice(options) => value
            .as_str()
            .is_some_and(|value| options.iter().any(|option| option == value)),
    };
    if !valid {
        return Err(format!(
            "{}: invalid value for {:?}",
            field.path, field.kind
        ));
    }
    if field.path == "plugins"
        && value.as_array().is_some_and(|values| {
            values.iter().any(|value| {
                let name = value.as_str().unwrap_or_default();
                name.is_empty()
                    || name.contains('/')
                    || name.contains('\\')
                    || !name.ends_with(".so")
            })
        })
    {
        return Err("plugins: expected library filenames ending in .so".to_owned());
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub struct HostConfig {
    pub bar_width: Option<u32>,
    pub bar_height: u32,
    pub transparent: bool,
    pub start: Vec<String>,
    pub center: Vec<String>,
    pub end: Vec<String>,
    pub launcher_enabled: bool,
    pub launcher_size: (u32, u32),
    pub launcher_limit: usize,
    pub settings_size: (u32, u32),
    pub control_center_enabled: bool,
    pub control_center_size: (u32, u32),
    pub control_center_widgets: Vec<String>,
}

pub(crate) fn fallback_layout(host: &HostConfig, overrides: &Table) -> bool {
    host.start.is_empty()
        && host.center.is_empty()
        && host.end.is_empty()
        && !["bar.start", "bar.center", "bar.end"]
            .iter()
            .any(|path| get_path(overrides, path).is_some())
}

impl HostConfig {
    pub(crate) fn from_validated(table: &Table) -> Self {
        let integer = |path| {
            get_path(table, path)
                .and_then(Value::as_integer)
                .expect("validated integer") as u32
        };
        let boolean = |path| {
            get_path(table, path)
                .and_then(Value::as_bool)
                .expect("validated boolean")
        };
        let list = |path| {
            get_path(table, path)
                .and_then(Value::as_array)
                .expect("validated array")
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        };
        Self {
            bar_width: get_path(table, "bar.width")
                .and_then(Value::as_integer)
                .map(|value| value as u32),
            bar_height: integer("bar.height"),
            transparent: boolean("bar.transparent"),
            start: list("bar.start"),
            center: list("bar.center"),
            end: list("bar.end"),
            launcher_enabled: boolean("launcher.enabled"),
            launcher_size: (integer("launcher.width"), integer("launcher.height")),
            launcher_limit: integer("launcher.max_entries") as usize,
            settings_size: (integer("settings.width"), integer("settings.height")),
            control_center_enabled: boolean("control_center.enabled"),
            control_center_size: (
                integer("control_center.width"),
                integer("control_center.height"),
            ),
            control_center_widgets: list("control_center.widgets"),
        }
    }
}

pub(crate) fn fallback_widgets(host: &HostConfig, order: &[String]) -> Vec<String> {
    host.launcher_enabled
        .then_some("launcher".to_owned())
        .into_iter()
        .chain(
            host.control_center_enabled
                .then_some("control-center".to_owned()),
        )
        .chain(std::iter::once("settings".to_owned()))
        .chain(order.iter().cloned())
        .collect()
}
