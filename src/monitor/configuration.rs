use super::{Output, OutputMode, OutputSnapshot};
use std::{collections::BTreeSet, fmt};
use toml::{Table, Value};

#[derive(Debug, Clone, PartialEq)]
pub struct DisplayConfig {
    pub name: String,
    pub enabled: bool,
    pub mode: OutputMode,
    pub scale: f64,
    pub position: (i32, i32),
    pub transform: u32,
    pub vrr: Option<bool>,
}

impl From<&Output> for DisplayConfig {
    fn from(output: &Output) -> Self {
        let mode = if output.physical_size.0 > 0 && output.physical_size.1 > 0 {
            OutputMode {
                width: output.physical_size.0,
                height: output.physical_size.1,
                refresh_hz: output.refresh_hz,
            }
        } else {
            output.modes.first().cloned().unwrap_or(OutputMode {
                width: 1920,
                height: 1080,
                refresh_hz: 60.0,
            })
        };
        Self {
            name: output.name.clone(),
            enabled: output.enabled,
            mode,
            scale: output.scale,
            position: output.position,
            transform: output.transform,
            vrr: output.vrr,
        }
    }
}

impl DisplayConfig {
    pub fn value(&self) -> Value {
        let mut value = Table::from_iter([
            ("name".into(), Value::String(self.name.clone())),
            ("enabled".into(), Value::Boolean(self.enabled)),
            ("width".into(), Value::Integer(i64::from(self.mode.width))),
            ("height".into(), Value::Integer(i64::from(self.mode.height))),
            ("refresh_hz".into(), Value::Float(self.mode.refresh_hz)),
            ("scale".into(), Value::Float(self.scale)),
            ("x".into(), Value::Integer(i64::from(self.position.0))),
            ("y".into(), Value::Integer(i64::from(self.position.1))),
            (
                "transform".into(),
                Value::Integer(i64::from(self.transform)),
            ),
        ]);
        if let Some(vrr) = self.vrr {
            value.insert("vrr".into(), Value::Boolean(vrr));
        }
        Value::Table(value)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModeChoice(pub OutputMode);
impl fmt::Display for ModeChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.refresh_hz == 0.0 {
            return write!(
                f,
                "{} × {} · compositor refresh",
                self.0.width, self.0.height
            );
        }
        write!(
            f,
            "{} × {} · {:.3} Hz",
            self.0.width, self.0.height, self.0.refresh_hz
        )
    }
}

pub fn parse(value: Option<&Value>) -> Result<Vec<DisplayConfig>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let values = value
        .as_array()
        .ok_or("monitor.profiles: expected an array of tables")?;
    if values.len() > 32 {
        return Err("monitor.profiles: at most 32 displays".into());
    }
    let mut result = Vec::new();
    for value in values {
        let table = value
            .as_table()
            .ok_or("monitor.profiles: expected display tables")?;
        let integer = |key: &str, default: i64| -> Result<i64, String> {
            table.get(key).map_or(Ok(default), |value| {
                value
                    .as_integer()
                    .ok_or_else(|| format!("monitor.{key}: expected an integer"))
            })
        };
        let number = |key: &str, default: f64| -> Result<f64, String> {
            table.get(key).map_or(Ok(default), |value| {
                value
                    .as_float()
                    .or_else(|| value.as_integer().map(|v| v as f64))
                    .ok_or_else(|| format!("monitor.{key}: expected a number"))
            })
        };
        let boolean = |key: &str, default: bool| -> Result<bool, String> {
            table.get(key).map_or(Ok(default), |value| {
                value
                    .as_bool()
                    .ok_or_else(|| format!("monitor.{key}: expected a boolean"))
            })
        };
        result.push(DisplayConfig {
            name: table
                .get("name")
                .and_then(Value::as_str)
                .ok_or("monitor.name: expected a connector name")?
                .into(),
            enabled: boolean("enabled", true)?,
            mode: OutputMode {
                width: u32::try_from(integer("width", 1920)?)
                    .map_err(|_| "monitor.width: invalid size")?,
                height: u32::try_from(integer("height", 1080)?)
                    .map_err(|_| "monitor.height: invalid size")?,
                refresh_hz: number("refresh_hz", 60.0)?,
            },
            scale: number("scale", 1.0)?,
            position: (
                i32::try_from(integer("x", 0)?).map_err(|_| "monitor.x: invalid position")?,
                i32::try_from(integer("y", 0)?).map_err(|_| "monitor.y: invalid position")?,
            ),
            transform: u32::try_from(integer("transform", 0)?)
                .map_err(|_| "monitor.transform: invalid rotation")?,
            vrr: table
                .get("vrr")
                .map(|value| value.as_bool().ok_or("monitor.vrr: expected a boolean"))
                .transpose()?,
        });
    }
    validate(&result)?;
    Ok(result)
}

pub fn validate(config: &[DisplayConfig]) -> Result<(), String> {
    if config.len() > 32 {
        return Err("At most 32 displays can be configured".into());
    }
    let mut names = BTreeSet::new();
    for output in config {
        if output.name.is_empty()
            || output.name.len() > 128
            || !output
                .name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_.:".contains(&c))
            || !names.insert(&output.name)
        {
            return Err("Display names must be unique connector identifiers".into());
        }
        if !(1..=16384).contains(&output.mode.width)
            || !(1..=16384).contains(&output.mode.height)
            || !output.mode.refresh_hz.is_finite()
            || !(0.0..=1000.0).contains(&output.mode.refresh_hz)
        {
            return Err(format!(
                "{}: invalid resolution or refresh rate",
                output.name
            ));
        }
        if !output.scale.is_finite()
            || !(0.25..=8.0).contains(&output.scale)
            || output.transform > 7
            || output.position.0.abs_diff(0) > 32768
            || output.position.1.abs_diff(0) > 32768
        {
            return Err(format!(
                "{}: invalid scale, rotation or position",
                output.name
            ));
        }
    }
    if !config.is_empty() && !config.iter().any(|output| output.enabled) {
        return Err("Keep at least one display enabled".into());
    }
    Ok(())
}

pub fn complete(
    config: &[DisplayConfig],
    inventory: &OutputSnapshot,
) -> Result<Vec<DisplayConfig>, String> {
    if config.iter().any(|output| {
        !inventory
            .outputs
            .iter()
            .any(|current| current.name == output.name)
    }) {
        return Err("A configured display was disconnected. Refresh the display list.".into());
    }
    let outputs: Vec<_> = inventory
        .outputs
        .iter()
        .map(|output| {
            config
                .iter()
                .find(|desired| desired.name == output.name)
                .cloned()
                .unwrap_or_else(|| output.into())
        })
        .collect();
    validate(&outputs)?;
    for output in outputs.iter().filter(|output| output.enabled) {
        let current = inventory
            .outputs
            .iter()
            .find(|current| current.name == output.name)
            .expect("completed inventory");
        let mode_matches = |mode: &OutputMode| {
            mode.width == output.mode.width
                && mode.height == output.mode.height
                && (mode.refresh_hz - output.mode.refresh_hz).abs() <= 0.02
        };
        if !current.modes.iter().any(mode_matches)
            && !mode_matches(&DisplayConfig::from(current).mode)
        {
            return Err(format!(
                "{}: mode is no longer advertised by this display",
                output.name
            ));
        }
    }
    Ok(outputs)
}
