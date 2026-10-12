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
            "bar.auto_hide",
            "Reveal bar near its edge",
            Boolean,
            Some(Value::Boolean(false)),
            Surface,
        )
        .description("Hide the bar until the pointer reaches its screen edge. Open dropdowns keep it visible."),
        SettingDescriptor::new(
            "bar.reveal_zone",
            "Pointer reveal distance",
            Integer { min: 1, max: 128 },
            Some(Value::Integer(8)),
            Surface,
        )
        .description("Size in pixels of the edge strip that reveals an auto-hidden bar."),
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
        SettingDescriptor::new("monitor.profiles", "Saved display layout", SettingKind::TableList, Some(Value::Array(Vec::new())), Live).description("Confirmed connector modes, scale, rotation and placement. Use the display editor to test changes before saving."),
        SettingDescriptor::new("monitor.apply_on_start", "Restore saved display layout on startup", Boolean, Some(Value::Boolean(true)), Restart),
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
        SettingDescriptor::new("bar.show_labels", "Show bar labels", Boolean, Some(Value::Boolean(false)), Live)
            .description("Show names and status text beside icons. Clock text and workspace numbers always remain visible."),
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
        SettingDescriptor::new("launcher.default_plugins", "Default launcher plugin libraries", TextList, Some(Value::Array(Vec::new())), Live)
            .description("Installed runner libraries to include alongside the ordinary plugin list."),
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
        SettingDescriptor::new("control_center.presentation", "Control center presentation", SettingKind::Choice(vec!["dropdown".into(), "window".into()]), Some(Value::String("dropdown".into())), Surface)
            .description("Open inside the originating bar. A separate window remains available when no bar exists."),
        SettingDescriptor::new("control_center.attach_to_clock", "Open controls from the clock", Boolean, Some(Value::Boolean(true)), Live)
            .description("The clock opens its calendar inside the configurable control-center dropdown."),
        SettingDescriptor::new("menus.notifications_dropdown", "Notifications inside the bar", Boolean, Some(Value::Boolean(true)), Live),
        SettingDescriptor::new("menus.notification_width", "Notification dropdown width", dimension.clone(), Some(Value::Integer(600)), Surface),
        SettingDescriptor::new("menus.notification_height", "Notification dropdown height", dimension.clone(), Some(Value::Integer(640)), Surface),
        SettingDescriptor::new("menus.session_enabled", "Enable session menu", Boolean, Some(Value::Boolean(true)), Live),
        SettingDescriptor::new("menus.session_actions", "Session menu actions", TextList, Some(Value::Array(["lock", "logout", "suspend", "hibernate", "reboot", "power-off"].into_iter().map(|value| Value::String(value.into())).collect())), Live)
            .description("Ordered native System actions. Power and logout actions keep their confirmation dialog."),
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
                    "System",
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
            "osd.enabled",
            "Enable OSDs",
            Boolean,
            Some(Value::Boolean(true)),
            Live,
        ),
        SettingDescriptor::new(
            "osd.width",
            "OSD width",
            dimension.clone(),
            Some(Value::Integer(layout::DEFAULT_OSD_SIZE.0 as i64)),
            Surface,
        ),
        SettingDescriptor::new(
            "osd.height",
            "OSD height",
            dimension.clone(),
            Some(Value::Integer(layout::DEFAULT_OSD_SIZE.1 as i64)),
            Surface,
        ),
        SettingDescriptor::new(
            "osd.position",
            "OSD edge",
            SettingKind::Choice(vec!["top".to_owned(), "bottom".to_owned()]),
            Some(Value::String("bottom".to_owned())),
            Surface,
        ),
        SettingDescriptor::new(
            "osd.margin",
            "OSD edge margin",
            Integer {
                min: 0,
                max: layout::MAX_HOST_SURFACE_DIMENSION,
            },
            Some(Value::Integer(64)),
            Surface,
        ),
        SettingDescriptor::new(
            "osd.timeout_ms",
            "OSD duration (ms)",
            Integer {
                min: 100,
                max: 10_000,
            },
            Some(Value::Integer(1800)),
            Live,
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
            Live,
        )
        .description("Select installed libraries. Providers become available immediately; disabled models stay resident until exit."),
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
            "dock.enabled",
            "Enable dock",
            Boolean,
            Some(Value::Boolean(true)),
            Surface,
        ),
        SettingDescriptor::new(
            "dock.outputs",
            "Dock outputs",
            TextList,
            Some(Value::Array(Vec::new())),
            Surface,
        )
        .description("Empty selects one output; * selects every connected output."),
        SettingDescriptor::new(
            "dock.output_fallback",
            "Dock output fallback",
            Boolean,
            Some(Value::Boolean(true)),
            Surface,
        ),
        SettingDescriptor::new(
            "dock.pinned",
            "Pinned desktop IDs",
            TextList,
            Some(Value::Array(Vec::new())),
            Live,
        ),
        SettingDescriptor::new(
            "dock.show_running",
            "Show running applications",
            Boolean,
            Some(Value::Boolean(true)),
            Live,
        ),
        SettingDescriptor::new(
            "dock.position",
            "Dock edge",
            SettingKind::Choice(vec!["top".into(), "bottom".into()]),
            Some(Value::String("bottom".into())),
            Surface,
        ),
        SettingDescriptor::new(
            "dock.icon_size",
            "Dock icon size",
            Integer { min: 16, max: 128 },
            Some(Value::Integer(40)),
            Surface,
        ),
        SettingDescriptor::new(
            "dock.margin",
            "Dock margin",
            Integer { min: 0, max: 1024 },
            Some(Value::Integer(12)),
            Surface,
        ),
        SettingDescriptor::new(
            "dock.reserve_space",
            "Reserve dock space",
            Boolean,
            Some(Value::Boolean(false)),
            Surface,
        ),
        SettingDescriptor::new(
            "desktop.enabled",
            "Enable desktop widgets",
            Boolean,
            Some(Value::Boolean(true)),
            Surface,
        ),
        SettingDescriptor::new(
            "desktop.output_fallback",
            "Widget output fallback",
            Boolean,
            Some(Value::Boolean(true)),
            Surface,
        ),
        SettingDescriptor::new(
            "desktop.grid",
            "Placement grid",
            Integer { min: 1, max: 128 },
            Some(Value::Integer(16)),
            Live,
        ),
        SettingDescriptor::new(
            "desktop.widgets",
            "Desktop widget instances",
            SettingKind::TableList,
            Some(Value::Array(Vec::new())),
            Surface,
        ),
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
    .into_iter()
    .chain(crate::theme::schema())
    .chain(crate::overview::schema())
    .chain(crate::shortcuts::schema())
    .chain(super::toasts::schema())
    .collect()
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
        "menus",
        "osd",
        "toasts",
        "startup",
        "instance",
        "ipc",
        "monitor",
        "dock",
        "desktop",
        "theme",
        "overview",
        "shortcuts",
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
    super::bars::validate_profiles(config, schema)?;
    crate::monitor::configuration::parse(
        config
            .get("monitor")
            .and_then(|value| value.get("profiles")),
    )?;
    super::desktop::widgets(config)?;
    crate::shortcuts::Config::read(config)?;
    let theme = oxiced::theme::snapshot::ThemeSnapshot::from_config(config, 0)?;
    oxiced::theme::snapshot::check_capacity(&theme)?;
    Ok(())
}

pub fn validate_value(field: &SettingDescriptor, value: &Value) -> Result<(), String> {
    let valid = match &field.kind {
        SettingKind::Boolean => value.is_bool(),
        SettingKind::Text => value.is_str(),
        SettingKind::Color => value
            .as_str()
            .is_some_and(|value| oxiced::theme::snapshot::parse_color(value).is_ok()),
        SettingKind::Arguments => value.as_str().is_some_and(|value| {
            shell_words::split(value).is_ok_and(|arguments| !arguments.is_empty())
        }),
        SettingKind::TextList => value
            .as_array()
            .is_some_and(|values| values.iter().all(Value::is_str)),
        SettingKind::TableList => value
            .as_array()
            .is_some_and(|values| values.iter().all(Value::is_table)),
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
    if field.path.starts_with("shortcuts.") {
        crate::shortcuts::validate_value(value.as_str().expect("validated text"))
            .map_err(|error| format!("{}: {error}", field.path))?;
    }
    if ["plugins", "launcher.default_plugins"].contains(&field.path.as_str())
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
    pub shortcuts: crate::shortcuts::Config,
    pub overview: crate::overview::Config,
    pub menus: super::menus::MenusConfig,
    pub dock: super::desktop::DockConfig,
    pub desktop: super::desktop::DesktopConfig,
    pub osd: crate::osd::OsdConfig,
    pub toasts: super::toasts::ToastConfig,
    pub bar_width: Option<u32>,
    pub bar_height: u32,
    pub bar_auto_hide: bool,
    pub bar_reveal_zone: u32,
    pub bar_show_labels: bool,
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
            shortcuts: crate::shortcuts::Config::read(table).expect("validated shortcuts"),
            overview: crate::overview::Config::read(table),
            dock: super::desktop::dock(table),
            menus: super::menus::MenusConfig::from_validated(table),
            desktop: super::desktop::desktop(table),
            osd: crate::osd::OsdConfig {
                enabled: boolean("osd.enabled"),
                size: (integer("osd.width"), integer("osd.height")),
                bottom: get_path(table, "osd.position").and_then(Value::as_str) == Some("bottom"),
                margin: integer("osd.margin"),
                timeout_ms: integer("osd.timeout_ms"),
                placement: None,
            },
            toasts: super::toasts::ToastConfig::from_validated(table),
            bar_width: get_path(table, "bar.width")
                .and_then(Value::as_integer)
                .map(|value| value as u32),
            bar_height: integer("bar.height"),
            bar_auto_hide: boolean("bar.auto_hide"),
            bar_reveal_zone: integer("bar.reveal_zone"),
            bar_show_labels: boolean("bar.show_labels"),
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
        .chain(host.menus.session_enabled.then_some("session".to_owned()))
        .chain(order.iter().cloned())
        .collect()
}
