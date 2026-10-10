use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Component, Path, PathBuf},
    time::Duration,
};

use oxibar_plugin_api::toml::Table;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backlight {
    pub name: String,
    pub current: u32,
    pub maximum: u32,
}

impl Backlight {
    pub fn percent(&self) -> u8 {
        ((u64::from(self.current.min(self.maximum)) * 100 + u64::from(self.maximum) / 2)
            / u64::from(self.maximum.max(1))) as u8
    }
    pub fn raw(&self, percent: u8) -> u32 {
        let value = ((u64::from(self.maximum) * u64::from(percent.min(100)) + 50) / 100) as u32;
        if percent > 0 {
            value.max(1).min(self.maximum)
        } else {
            0
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub id: String,
    pub path: String,
    pub uid: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SessionAction {
    Suspend,
    Hibernate,
    Reboot,
    PowerOff,
    Logout,
}

impl SessionAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Suspend => "Suspend",
            Self::Hibernate => "Hibernate",
            Self::Reboot => "Restart",
            Self::PowerOff => "Power off",
            Self::Logout => "Log out",
        }
    }
    pub fn method(self) -> &'static str {
        match self {
            Self::Suspend => "Suspend",
            Self::Hibernate => "Hibernate",
            Self::Reboot => "Reboot",
            Self::PowerOff => "PowerOff",
            Self::Logout => "Terminate",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "suspend" => Some(Self::Suspend),
            "hibernate" => Some(Self::Hibernate),
            "reboot" => Some(Self::Reboot),
            "power-off" => Some(Self::PowerOff),
            "logout" => Some(Self::Logout),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    Yes,
    Challenge,
    No,
    Unavailable,
}
impl Capability {
    pub fn parse(value: &str) -> Self {
        match value {
            "yes" => Self::Yes,
            "challenge" => Self::Challenge,
            "no" => Self::No,
            _ => Self::Unavailable,
        }
    }
    pub fn allowed(self) -> bool {
        matches!(self, Self::Yes | Self::Challenge)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub sequence: u64,
    pub backlights: Vec<Backlight>,
    pub brightness_error: Option<String>,
    pub profiles: Vec<String>,
    pub active_profile: Option<String>,
    pub degraded: Option<String>,
    pub profile_ready: bool,
    pub profile_error: Option<String>,
    pub session: Option<Session>,
    pub login_ready: bool,
    pub capabilities: BTreeMap<SessionAction, Capability>,
    pub session_error: Option<String>,
}

impl Snapshot {
    pub fn backlight(&self, preferred: &str) -> Option<&Backlight> {
        self.backlights
            .iter()
            .find(|item| item.name == preferred)
            .or_else(|| self.backlights.first())
    }
    pub fn can(&self, action: SessionAction) -> bool {
        self.login_ready
            && self
                .capabilities
                .get(&action)
                .is_some_and(|value| value.allowed())
            && (action != SessionAction::Logout || self.session.is_some())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Domain {
    Brightness,
    Profile,
    Session,
}

#[derive(Debug, Clone)]
pub enum Action {
    Brightness(String, u8),
    Profile(String),
    Session(SessionAction),
}
impl Action {
    pub fn domain(&self) -> Domain {
        match self {
            Self::Brightness(..) => Domain::Brightness,
            Self::Profile(..) => Domain::Profile,
            Self::Session(..) => Domain::Session,
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Brightness(name, percent) if !valid_name(name) || *percent > 100 => {
                Err("Invalid backlight or percentage".to_owned())
            }
            Self::Profile(value)
                if value.is_empty() || value.len() > 128 || value.contains('\0') =>
            {
                Err("Invalid power profile".to_owned())
            }
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Limits {
    pub root: PathBuf,
    pub poll: Duration,
    pub session_id: Option<String>,
}
impl Limits {
    pub fn from_config(config: &Table) -> Self {
        let table = config.get("system").and_then(|value| value.as_table());
        let text = |key| {
            table
                .and_then(|table| table.get(key))
                .and_then(|value| value.as_str())
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        };
        Self {
            root: text("backlight_root")
                .map(PathBuf::from)
                .unwrap_or_else(|| "/sys/class/backlight".into()),
            poll: Duration::from_secs(
                table
                    .and_then(|table| table.get("backlight_poll_seconds"))
                    .and_then(|value| value.as_integer())
                    .filter(|value| (1..=60).contains(value))
                    .unwrap_or(2) as u64,
            ),
            session_id: text("session_id").or_else(|| {
                std::env::var("XDG_SESSION_ID")
                    .ok()
                    .filter(|value| !value.is_empty())
            }),
        }
    }
}

pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && !name.contains('\0')
        && matches!(
            Path::new(name).components().collect::<Vec<_>>().as_slice(),
            [Component::Normal(_)]
        )
}

fn numeric_file(path: &Path) -> Result<u32, String> {
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32)
        .open(path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if !file
        .metadata()
        .map_err(|error| error.to_string())?
        .is_file()
    {
        return Err(format!(
            "{} is not a regular brightness attribute",
            path.display()
        ));
    }
    let mut value = String::new();
    file.take(65)
        .read_to_string(&mut value)
        .map_err(|error| error.to_string())?;
    if value.len() > 64 {
        return Err("Brightness attribute is too large".to_owned());
    }
    value
        .trim()
        .parse()
        .map_err(|_| format!("Invalid brightness attribute {}", path.display()))
}

pub fn read_backlight(root: &Path, name: &str) -> Result<Backlight, String> {
    if !valid_name(name) {
        return Err("Invalid backlight name".to_owned());
    }
    let directory = root.join(name);
    let maximum = numeric_file(&directory.join("max_brightness"))?;
    if maximum == 0 {
        return Err("Backlight maximum must be positive".to_owned());
    }
    Ok(Backlight {
        name: name.to_owned(),
        maximum,
        current: numeric_file(&directory.join("brightness"))?.min(maximum),
    })
}

pub fn read_backlights(root: &Path) -> Result<Vec<Backlight>, String> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.to_string()),
    };
    let mut result = Vec::new();
    let mut error = None;
    for entry in entries.take(64).flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !entry.path().is_dir() {
            continue;
        }
        match read_backlight(root, &name) {
            Ok(device) => result.push(device),
            Err(value) => {
                error = Some(value);
            }
        }
    }
    result.sort_by(|left, right| left.name.cmp(&right.name));
    if result.is_empty()
        && let Some(error) = error
    {
        return Err(error);
    }
    Ok(result)
}

pub fn write_brightness(root: &Path, name: &str, raw: u32) -> Result<(), String> {
    let device = read_backlight(root, name)?;
    if raw > device.maximum {
        return Err("Brightness exceeds device maximum".to_owned());
    }
    let path = root.join(name).join("brightness");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32)
        .open(&path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if !file
        .metadata()
        .map_err(|error| error.to_string())?
        .is_file()
    {
        return Err("Brightness target is not a regular attribute".to_owned());
    }
    file.write_all(raw.to_string().as_bytes())
        .map_err(|error| error.to_string())?;
    // Ordinary fixture files need truncation; sysfs ignores/does not support it.
    let _ = file.set_len(raw.to_string().len() as u64);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rounding_and_minimum_preserve_nonzero_brightness() {
        let device = Backlight {
            name: "panel".to_owned(),
            current: 50,
            maximum: 101,
        };
        assert_eq!(device.percent(), 50);
        assert_eq!(device.raw(50), 51);
        assert_eq!(
            Backlight {
                maximum: 3,
                ..device.clone()
            }
            .raw(1),
            1
        );
        assert_eq!(device.raw(0), 0);
        assert_eq!(device.raw(100), 101);
    }
    #[test]
    fn names_and_capabilities_reject_invalid_targets() {
        for name in ["", "../panel", "a/b", ".", "..", "a\0b"] {
            assert!(!valid_name(name));
        }
        assert!(valid_name("intel_backlight"));
        assert!(Capability::parse("challenge").allowed());
        assert!(!Capability::parse("na").allowed());
        assert!(!Snapshot::default().can(SessionAction::Logout));
    }
}
