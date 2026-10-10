use std::collections::{BTreeMap, HashMap};
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use zbus::{Connection, Proxy, fdo::DBusProxy, zvariant::OwnedValue};

const PLAYER_PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER_IFACE: &str = "org.mpris.MediaPlayer2.Player";
const ROOT_IFACE: &str = "org.mpris.MediaPlayer2";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct AudioSnapshot {
    pub(crate) player: Option<PlayerInfo>,
    pub(crate) outputs: Vec<AudioDevice>,
    pub(crate) inputs: Vec<AudioDevice>,
    pub(crate) default_output: Option<String>,
    pub(crate) default_input: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct DeviceSnapshot {
    pub(crate) outputs: Vec<AudioDevice>,
    pub(crate) inputs: Vec<AudioDevice>,
    pub(crate) default_output: Option<String>,
    pub(crate) default_input: Option<String>,
}

impl AudioSnapshot {
    pub(crate) fn apply_devices(&mut self, snapshot: DeviceSnapshot) {
        self.outputs = snapshot.outputs;
        self.inputs = snapshot.inputs;
        self.default_output = snapshot.default_output;
        self.default_input = snapshot.default_input;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PlayerInfo {
    pub(crate) service: String,
    pub(crate) identity: String,
    pub(crate) title: String,
    pub(crate) artist: String,
    pub(crate) status: String,
    pub(crate) volume: u8,
    pub(crate) art_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AudioDevice {
    pub(crate) name: String,
    pub(crate) label: String,
    pub(crate) volume: u8,
    pub(crate) muted: bool,
}

impl AudioDevice {
    pub(crate) fn choice(&self) -> DeviceChoice {
        DeviceChoice {
            name: self.name.clone(),
            label: self.label.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DeviceChoice {
    pub(crate) name: String,
    pub(crate) label: String,
}

impl std::fmt::Display for DeviceChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum DeviceKind {
    Output,
    Input,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DeviceAction {
    Default(DeviceKind, String),
    Volume(DeviceKind, String, u8),
    Mute(DeviceKind, String, bool),
}

impl DeviceAction {
    pub(crate) fn target(&self) -> (DeviceKind, &str) {
        match self {
            Self::Default(kind, name) | Self::Volume(kind, name, _) | Self::Mute(kind, name, _) => {
                (*kind, name)
            }
        }
    }

    pub(crate) fn supersedes(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (Self::Volume(..), Self::Volume(..)) | (Self::Mute(..), Self::Mute(..))
        ) && self.target() == other.target()
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        let (_, name) = self.target();
        if name.is_empty() || name.contains('\0') {
            Err("audio: invalid device name".to_owned())
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum PlayerAction {
    Previous,
    PlayPause,
    Next,
}

pub(crate) fn selected_device<'a>(
    devices: &'a [AudioDevice],
    default: Option<&str>,
) -> Option<&'a AudioDevice> {
    default
        .and_then(|name| devices.iter().find(|device| device.name == name))
        .or_else(|| devices.first())
}

pub(crate) fn local_art_path(url: &str) -> Option<String> {
    let path = if let Some(rest) = url.strip_prefix("file://") {
        percent_decode(rest.strip_prefix("localhost").unwrap_or(rest))
    } else if url.starts_with('/') {
        url.to_owned()
    } else {
        return None;
    };
    std::fs::metadata(&path).ok()?;
    Some(path)
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3])
            && let Ok(value) = u8::from_str_radix(hex, 16)
        {
            out.push(value);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub(crate) fn load_devices() -> Result<DeviceSnapshot, String> {
    let default_output = run_pactl(&["get-default-sink"]).ok();
    let default_input = run_pactl(&["get-default-source"]).ok();
    let outputs = list_devices(DeviceKind::Output)?;
    let inputs = list_devices(DeviceKind::Input)?;
    Ok(DeviceSnapshot {
        outputs,
        inputs,
        default_output,
        default_input,
    })
}

fn list_devices(kind: DeviceKind) -> Result<Vec<AudioDevice>, String> {
    let list_kind = match kind {
        DeviceKind::Output => "sinks",
        DeviceKind::Input => "sources",
    };
    let short = run_pactl(&["list", "short", list_kind])?;
    let descriptions = device_descriptions(kind).unwrap_or_default();
    let default = match kind {
        DeviceKind::Output => run_pactl(&["get-default-sink"]).ok(),
        DeviceKind::Input => run_pactl(&["get-default-source"]).ok(),
    };

    let mut devices = Vec::new();
    for line in short.lines().filter(|line| !line.trim().is_empty()) {
        let fields = line.split('\t').collect::<Vec<_>>();
        let Some(name) = fields
            .get(1)
            .map(|name| name.trim())
            .filter(|name| !name.is_empty())
        else {
            continue;
        };
        if matches!(kind, DeviceKind::Input) && name.ends_with(".monitor") {
            continue;
        }
        let label = descriptions
            .get(name)
            .filter(|label| !label.is_empty())
            .cloned()
            .unwrap_or_else(|| name.replace(['_', '-'], " "));
        let volume = device_volume(kind, name).unwrap_or(0);
        let muted = device_mute(kind, name).unwrap_or(false);
        devices.push(AudioDevice {
            name: name.to_owned(),
            label,
            volume,
            muted,
        });
    }

    devices.sort_by(|a, b| {
        let a_default = default.as_deref() == Some(a.name.as_str());
        let b_default = default.as_deref() == Some(b.name.as_str());
        b_default.cmp(&a_default).then(a.label.cmp(&b.label))
    });
    Ok(devices)
}

fn device_descriptions(kind: DeviceKind) -> Result<BTreeMap<String, String>, String> {
    let list_kind = match kind {
        DeviceKind::Output => "sinks",
        DeviceKind::Input => "sources",
    };
    let output = run_pactl(&["list", list_kind])?;
    let mut descriptions = BTreeMap::new();
    let mut current_name: Option<String> = None;
    for line in output.lines() {
        let trimmed = line.trim();
        if let Some(name) = trimmed.strip_prefix("Name: ") {
            current_name = Some(name.to_owned());
        } else if let Some(description) = trimmed.strip_prefix("Description: ")
            && let Some(name) = current_name.take()
        {
            descriptions.insert(name, description.to_owned());
        }
    }
    Ok(descriptions)
}

fn device_volume(kind: DeviceKind, name: &str) -> Result<u8, String> {
    let command = match kind {
        DeviceKind::Output => "get-sink-volume",
        DeviceKind::Input => "get-source-volume",
    };
    let output = run_pactl(&[command, name])?;
    parse_percent(&output).ok_or_else(|| format!("audio: could not parse volume for {name}"))
}

fn device_mute(kind: DeviceKind, name: &str) -> Result<bool, String> {
    let command = match kind {
        DeviceKind::Output => "get-sink-mute",
        DeviceKind::Input => "get-source-mute",
    };
    run_pactl(&[command, name]).map(|value| value.ends_with("yes"))
}

pub(crate) fn device_action(action: DeviceAction) -> Result<(), String> {
    action.validate()?;
    match action {
        DeviceAction::Default(kind, name) => set_default_device(kind, &name),
        DeviceAction::Volume(kind, name, volume) => set_device_volume(kind, &name, volume.min(150)),
        DeviceAction::Mute(kind, name, muted) => {
            let command = match kind {
                DeviceKind::Output => "set-sink-mute",
                DeviceKind::Input => "set-source-mute",
            };
            run_pactl(&[command, &name, if muted { "1" } else { "0" }]).map(|_| ())
        }
    }
}

fn set_default_device(kind: DeviceKind, name: &str) -> Result<(), String> {
    let command = match kind {
        DeviceKind::Output => "set-default-sink",
        DeviceKind::Input => "set-default-source",
    };
    run_pactl(&[command, name]).map(|_| ())
}

fn set_device_volume(kind: DeviceKind, name: &str, volume: u8) -> Result<(), String> {
    let command = match kind {
        DeviceKind::Output => "set-sink-volume",
        DeviceKind::Input => "set-source-volume",
    };
    let value = format!("{volume}%");
    run_pactl(&[command, name, &value]).map(|_| ())
}

async fn session() -> Result<Connection, String> {
    zbus::connection::Builder::session()
        .map_err(|e| format!("audio: DBus session failed: {e}"))?
        .method_timeout(Duration::from_secs(2))
        .build()
        .await
        .map_err(|e| format!("audio: DBus session failed: {e}"))
}

pub(crate) async fn current_player() -> Result<Option<PlayerInfo>, String> {
    let connection = session().await?;
    let dbus = DBusProxy::new(&connection)
        .await
        .map_err(|e| format!("audio: DBus proxy failed: {e}"))?;
    let mut players = Vec::new();
    for name in dbus
        .list_names()
        .await
        .map_err(|e| format!("audio: DBus list names failed: {e}"))?
    {
        let service = name.to_string();
        if !service.starts_with("org.mpris.MediaPlayer2.") {
            continue;
        }
        if let Ok(player) = read_player(&connection, &service).await {
            players.push(player);
        }
    }
    players.sort_by_key(|player| match player.status.as_str() {
        "Playing" => 0,
        "Paused" => 1,
        _ => 2,
    });
    Ok(players.into_iter().next())
}

async fn read_player(connection: &Connection, service: &str) -> Result<PlayerInfo, String> {
    let player = Proxy::new(connection, service, PLAYER_PATH, PLAYER_IFACE)
        .await
        .map_err(|e| format!("audio: MPRIS player proxy failed: {e}"))?;
    let root = Proxy::new(connection, service, PLAYER_PATH, ROOT_IFACE)
        .await
        .map_err(|e| format!("audio: MPRIS root proxy failed: {e}"))?;
    let identity = root
        .get_property::<String>("Identity")
        .await
        .unwrap_or_else(|_| {
            service
                .trim_start_matches("org.mpris.MediaPlayer2.")
                .to_owned()
        });
    let status = player
        .get_property::<String>("PlaybackStatus")
        .await
        .unwrap_or_else(|_| "Unknown".to_owned());
    let volume = player
        .get_property::<f64>("Volume")
        .await
        .map(|volume| (volume * 100.0).round().clamp(0.0, 100.0) as u8)
        .unwrap_or(100);
    let metadata = player
        .get_property::<HashMap<String, OwnedValue>>("Metadata")
        .await
        .unwrap_or_default();
    let title = metadata_string(&metadata, "xesam:title").unwrap_or_default();
    let artist = metadata
        .get("xesam:artist")
        .and_then(|value| Vec::<String>::try_from(value.clone()).ok())
        .map(|artists| artists.join(", "))
        .unwrap_or_default();
    let art_url = metadata_string(&metadata, "mpris:artUrl");
    Ok(PlayerInfo {
        service: service.to_owned(),
        identity,
        title,
        artist,
        status,
        volume,
        art_url,
    })
}

fn metadata_string(metadata: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    metadata
        .get(key)
        .and_then(|value| String::try_from(value.clone()).ok())
}

pub(crate) async fn control_player(service: &str, action: PlayerAction) -> Result<(), String> {
    let connection = session().await?;
    let player = Proxy::new(&connection, service, PLAYER_PATH, PLAYER_IFACE)
        .await
        .map_err(|e| format!("audio: MPRIS player proxy failed: {e}"))?;
    let method = match action {
        PlayerAction::Previous => "Previous",
        PlayerAction::PlayPause => "PlayPause",
        PlayerAction::Next => "Next",
    };
    player
        .call::<_, _, ()>(method, &())
        .await
        .map_err(|e| format!("audio: MPRIS {method} failed: {e}"))
}

pub(crate) async fn set_player_volume(service: &str, volume: u8) -> Result<(), String> {
    let connection = session().await?;
    let player = Proxy::new(&connection, service, PLAYER_PATH, PLAYER_IFACE)
        .await
        .map_err(|e| format!("audio: MPRIS player proxy failed: {e}"))?;
    player
        .set_property("Volume", (volume as f64 / 100.0).clamp(0.0, 1.0))
        .await
        .map_err(|e| format!("audio: MPRIS volume failed: {e}"))
}

fn run_pactl(args: &[&str]) -> Result<String, String> {
    let mut child = Command::new("pactl")
        .args(args)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("audio: failed to run pactl: {e}"))?;
    let stdout = std::thread::spawn({
        let pipe = child.stdout.take().expect("pactl stdout is piped");
        move || read_output(pipe)
    });
    let stderr = std::thread::spawn({
        let pipe = child.stderr.take().expect("pactl stderr is piped");
        move || read_output(pipe)
    });
    let deadline = Instant::now() + Duration::from_secs(3);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            result => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(match result {
                    Err(error) => format!("audio: pactl wait failed: {error}"),
                    _ => "audio: pactl timed out".to_owned(),
                });
            }
        }
    };
    let stdout = stdout
        .join()
        .map_err(|_| "audio: pactl stdout worker failed".to_owned())??;
    let stderr = stderr
        .join()
        .map_err(|_| "audio: pactl stderr worker failed".to_owned())??;
    if status?.success() {
        Ok(stdout)
    } else if stderr.is_empty() {
        Err(format!("audio: pactl failed: {stdout}"))
    } else {
        Err(format!("audio: pactl failed: {stderr}"))
    }
}

fn read_output(pipe: impl Read) -> Result<String, String> {
    let mut bytes = Vec::new();
    pipe.take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 1024 * 1024 {
        Err("audio: pactl output exceeded 1 MiB".to_owned())
    } else {
        Ok(String::from_utf8_lossy(&bytes).trim().to_owned())
    }
}

fn parse_percent(output: &str) -> Option<u8> {
    output.split_whitespace().find_map(|token| {
        token
            .strip_suffix('%')
            .and_then(|value| value.parse::<u16>().ok())
            .map(|value| value.min(150) as u8)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_percent_values_with_cap() {
        assert_eq!(parse_percent("front-left: 42% / -10.00 dB"), Some(42));
        assert_eq!(parse_percent("volume: 200%"), Some(150));
        assert_eq!(parse_percent("no percentage"), None);
    }

    #[test]
    fn percent_decode_handles_valid_and_invalid_escapes() {
        assert_eq!(
            percent_decode("cover%20art%2Ffile.png"),
            "cover art/file.png"
        );
        assert_eq!(percent_decode("bad%zzescape"), "bad%zzescape");
    }

    #[test]
    fn selected_device_prefers_configured_default_then_first() {
        let devices = vec![
            AudioDevice {
                name: "one".to_owned(),
                label: "One".to_owned(),
                volume: 10,
                muted: false,
            },
            AudioDevice {
                name: "two".to_owned(),
                label: "Two".to_owned(),
                volume: 20,
                muted: false,
            },
        ];

        assert_eq!(selected_device(&devices, Some("two")).unwrap().volume, 20);
        assert_eq!(
            selected_device(&devices, Some("missing")).unwrap().volume,
            10
        );
        assert!(selected_device(&[], Some("missing")).is_none());
    }
}
