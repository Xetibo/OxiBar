use toml::{Table, Value};

use super::{
    ConfigSnapshot,
    schema::{self, HostConfig},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Position {
    Top,
    Bottom,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BarConfig {
    pub width: Option<u32>,
    pub height: u32,
    pub transparent: bool,
    pub position: Position,
    pub start: Vec<String>,
    pub center: Vec<String>,
    pub end: Vec<String>,
}

impl BarConfig {
    pub fn for_output(snapshot: &ConfigSnapshot, output: Option<&str>, order: &[String]) -> Self {
        let global = snapshot
            .effective
            .get("bar")
            .and_then(Value::as_table)
            .expect("validated bar");
        let mut table = global.clone();
        let profile = output.and_then(|name| {
            global
                .get("profiles")
                .and_then(Value::as_table)
                .and_then(|profiles| profiles.get(name))
                .and_then(Value::as_table)
        });
        if let Some(profile) = profile {
            super::store::merge(&mut table, profile);
        }
        let mut effective = snapshot.effective.clone();
        effective.insert("bar".to_owned(), Value::Table(table.clone()));
        let host = HostConfig::from_validated(&effective);
        let explicit_profile_layout = profile.is_some_and(|profile| {
            ["start", "center", "end"]
                .iter()
                .any(|key| profile.contains_key(*key))
        });
        let mut start = host.start.clone();
        if !explicit_profile_layout && schema::fallback_layout(&host, &snapshot.overrides) {
            start = schema::fallback_widgets(&host, order);
        }
        Self {
            width: host.bar_width,
            height: host.bar_height,
            transparent: host.transparent,
            position: if table.get("position").and_then(Value::as_str) == Some("bottom") {
                Position::Bottom
            } else {
                Position::Top
            },
            start,
            center: host.center,
            end: host.end,
        }
    }
}

pub(crate) fn validate_profiles(
    table: &Table,
    schema: &[oxibar_plugin_api::SettingDescriptor],
) -> Result<(), String> {
    let Some(profiles) = table.get("bar").and_then(|value| value.get("profiles")) else {
        return Ok(());
    };
    let profiles = profiles
        .as_table()
        .ok_or("bar.profiles: expected per-output tables")?;
    for (name, value) in profiles {
        if name.is_empty() {
            return Err("bar.profiles: output names must be nonempty".to_owned());
        }
        let profile = value
            .as_table()
            .ok_or_else(|| format!("bar.profiles.{name}: expected a table"))?;
        for (key, value) in profile {
            if ![
                "height",
                "width",
                "transparent",
                "position",
                "start",
                "center",
                "end",
            ]
            .contains(&key.as_str())
            {
                return Err(format!(
                    "bar.profiles.{name}.{key}: unsupported per-output setting"
                ));
            }
            if let Some(field) = schema
                .iter()
                .find(|field| field.path == format!("bar.{key}"))
            {
                schema::validate_value(field, value)?;
            }
        }
    }
    Ok(())
}
