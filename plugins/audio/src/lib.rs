//! Audio management plugin backed by PulseAudio/PipeWire `pactl` and MPRIS.

use std::collections::{BTreeMap, HashMap};
use std::process::Command;
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Duration;

use iced::{
    Alignment, Background, Border, Color, ContentFit, Element, Length, Shadow, Task,
    futures::Stream,
    stream,
    widget::{Column, Container, Row, Space, button, image, text},
};
use iced_anim::{AnimationBuilder, Motion};
use oxibar_plugin_api::{
    ABI_VERSION, HOST_REQUEST_TOGGLE_POPUP, OxiAny, PluginModel, PluginMsg, PluginStream,
    toml::Table,
};
use oxiced::{
    theme::theme_impl::OXITHEME,
    widgets::{oxi_button, oxi_picklist, oxi_slider},
};
use zbus::{
    blocking::{Connection, Proxy, fdo::DBusProxy},
    zvariant::OwnedValue,
};

const DEFAULT_POLL_SECONDS: u64 = 4;
const AUDIO_ICON: &str = "󰕾";
const PLAYER_PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER_IFACE: &str = "org.mpris.MediaPlayer2.Player";
const ROOT_IFACE: &str = "org.mpris.MediaPlayer2";

static POLL_INTERVAL: OnceLock<Duration> = OnceLock::new();

#[derive(Debug)]
struct Model {
    snapshot: AudioSnapshot,
    pending: Option<String>,
    errors: Vec<String>,
}

impl Model {
    fn new(global_config: Table) -> Self {
        let poll_seconds = read_poll_interval(&global_config);
        let _ = POLL_INTERVAL.set(Duration::from_secs(poll_seconds));
        Self {
            snapshot: AudioSnapshot::default(),
            pending: None,
            errors: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default)]
struct AudioSnapshot {
    player: Option<PlayerInfo>,
    outputs: Vec<AudioDevice>,
    inputs: Vec<AudioDevice>,
    default_output: Option<String>,
    default_input: Option<String>,
}

#[derive(Clone, Debug)]
struct PlayerInfo {
    service: String,
    identity: String,
    title: String,
    artist: String,
    status: String,
    volume: u8,
    art_url: Option<String>,
}

#[derive(Clone, Debug)]
struct AudioDevice {
    name: String,
    label: String,
    volume: u8,
}

impl AudioDevice {
    fn choice(&self) -> DeviceChoice {
        DeviceChoice {
            name: self.name.clone(),
            label: self.label.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DeviceChoice {
    name: String,
    label: String,
}

impl std::fmt::Display for DeviceChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

#[derive(Clone, Copy, Debug)]
enum DeviceKind {
    Output,
    Input,
}

#[derive(Clone, Copy, Debug)]
enum PlayerAction {
    Previous,
    PlayPause,
    Next,
}

#[derive(Clone, Debug)]
enum Message {
    TogglePopup,
    Refresh,
    Snapshot(Result<AudioSnapshot, String>),
    PlayerControl(PlayerAction),
    PlayerVolumeChanged(u8),
    SetDefaultOutput(DeviceChoice),
    SetDefaultInput(DeviceChoice),
    OutputVolumeChanged(u8),
    InputVolumeChanged(u8),
    ActionDone {
        result: Result<(), String>,
        refresh: bool,
    },
}

fn msg(m: Message) -> PluginMsg {
    Arc::new(m)
}

#[unsafe(no_mangle)]
pub extern "Rust" fn abi_version() -> u32 {
    ABI_VERSION
}

#[unsafe(no_mangle)]
pub extern "Rust" fn name() -> &'static str {
    "Audio"
}

#[unsafe(no_mangle)]
pub extern "Rust" fn model(global_config: Table) -> (PluginModel, Option<Task<PluginMsg>>) {
    let m: Box<dyn OxiAny> = Box::new(Model::new(global_config));
    (
        Arc::new(RwLock::new(m)),
        Some(Task::done(msg(Message::Refresh))),
    )
}

#[unsafe(no_mangle)]
pub extern "Rust" fn update(model: PluginModel, msg_in: PluginMsg) -> Option<Task<PluginMsg>> {
    let mut guard = model.try_write().ok()?;
    let model = guard.downcast_mut::<Model>()?;
    let m = msg_in.downcast_ref::<Message>()?.clone();
    match m {
        Message::TogglePopup => Some(Task::done(Arc::new(HOST_REQUEST_TOGGLE_POPUP.to_owned()))),
        Message::Refresh => Some(Task::perform(async { load_snapshot() }, |result| {
            msg(Message::Snapshot(result))
        })),
        Message::Snapshot(result) => {
            match result {
                Ok(snapshot) => model.snapshot = snapshot,
                Err(error) => model.errors.push(error),
            }
            None
        }
        Message::PlayerControl(action) => {
            let Some(player) = model.snapshot.player.clone() else {
                return None;
            };
            run_action(model, format!("media {action:?}"), true, move || {
                control_player(&player.service, action)
            })
        }
        Message::PlayerVolumeChanged(volume) => {
            let Some(player) = model.snapshot.player.as_mut() else {
                return None;
            };
            player.volume = volume;
            let service = player.service.clone();
            run_action(model, "media volume".to_owned(), false, move || {
                set_player_volume(&service, volume)
            })
        }
        Message::SetDefaultOutput(choice) => {
            model.snapshot.default_output = Some(choice.name.clone());
            run_action(model, "set output".to_owned(), true, move || {
                set_default_device(DeviceKind::Output, &choice.name)
            })
        }
        Message::SetDefaultInput(choice) => {
            model.snapshot.default_input = Some(choice.name.clone());
            run_action(model, "set input".to_owned(), true, move || {
                set_default_device(DeviceKind::Input, &choice.name)
            })
        }
        Message::OutputVolumeChanged(volume) => {
            let Some(name) = model.snapshot.default_output.clone() else {
                return None;
            };
            if let Some(device) = model.snapshot.outputs.iter_mut().find(|d| d.name == name) {
                device.volume = volume;
            }
            run_action(model, "output volume".to_owned(), false, move || {
                set_device_volume(DeviceKind::Output, &name, volume)
            })
        }
        Message::InputVolumeChanged(volume) => {
            let Some(name) = model.snapshot.default_input.clone() else {
                return None;
            };
            if let Some(device) = model.snapshot.inputs.iter_mut().find(|d| d.name == name) {
                device.volume = volume;
            }
            run_action(model, "input volume".to_owned(), false, move || {
                set_device_volume(DeviceKind::Input, &name, volume)
            })
        }
        Message::ActionDone { result, refresh } => {
            model.pending = None;
            match result {
                Ok(()) if refresh => Some(Task::done(msg(Message::Refresh))),
                Ok(()) => None,
                Err(error) => {
                    model.errors.push(error);
                    None
                }
            }
        }
    }
}

#[unsafe(no_mangle)]
pub extern "Rust" fn launch(_focused_index: usize, _model: PluginModel) -> Option<Task<PluginMsg>> {
    Some(Task::done(msg(Message::TogglePopup)))
}

#[unsafe(no_mangle)]
pub extern "Rust" fn errors(model: PluginModel) -> Vec<String> {
    let Ok(mut guard) = model.try_write() else {
        return Vec::new();
    };
    let Some(m) = guard.downcast_mut::<Model>() else {
        return Vec::new();
    };
    std::mem::take(&mut m.errors)
}

#[unsafe(no_mangle)]
pub extern "Rust" fn view(
    model: PluginModel,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    let lock = model.try_read().map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::WouldBlock, "model is write-locked")
    })?;
    let model = lock.downcast_ref::<Model>().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "model has wrong type")
    })?;

    let volume = selected_device(
        &model.snapshot.outputs,
        model.snapshot.default_output.as_deref(),
    )
    .map(|device| device.volume);
    let label = volume
        .map(|volume| format!("{AUDIO_ICON} {volume}%"))
        .unwrap_or_else(|| AUDIO_ICON.to_owned());
    let btn = button(
        text(label)
            .size(14)
            .align_y(Alignment::Center)
            .align_x(Alignment::Center),
    )
    .on_press(msg(Message::TogglePopup))
    .style(bar_button_style)
    .padding([0, 8])
    .height(22.5)
    .width(Length::Shrink);

    Ok(vec![btn.into()])
}

#[unsafe(no_mangle)]
pub extern "Rust" fn popup_view(
    model: PluginModel,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    let lock = model.try_read().map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::WouldBlock, "model is write-locked")
    })?;
    let model = lock.downcast_ref::<Model>().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "model has wrong type")
    })?;

    let mut content = Column::new()
        .spacing(10)
        .padding([12, 14])
        .width(Length::Fill);
    content = content.push(media_card(
        model.snapshot.player.as_ref(),
        model.pending.is_some(),
    ));
    content = content.push(output_section(&model.snapshot));
    content = content.push(input_section(&model.snapshot));

    Ok(vec![content.height(Length::Fill).into()])
}

#[unsafe(no_mangle)]
pub extern "Rust" fn subscription() -> *mut PluginStream {
    let interval = *POLL_INTERVAL
        .get()
        .unwrap_or(&Duration::from_secs(DEFAULT_POLL_SECONDS));
    let s = stream::channel(
        32,
        move |mut output: iced::futures::channel::mpsc::Sender<PluginMsg>| async move {
            std::thread::spawn(move || {
                loop {
                    std::thread::sleep(interval);
                    let _ = output.try_send(msg(Message::Refresh));
                }
            });
            std::future::pending::<()>().await;
        },
    );

    Box::into_raw(Box::new(s)) as *mut PluginStream
}

fn run_action(
    model: &mut Model,
    label: String,
    refresh: bool,
    action: impl FnOnce() -> Result<(), String> + Send + 'static,
) -> Option<Task<PluginMsg>> {
    model.pending = Some(label);
    Some(Task::perform(async move { action() }, move |result| {
        msg(Message::ActionDone { result, refresh })
    }))
}

fn media_card(player: Option<&PlayerInfo>, busy: bool) -> Element<'static, PluginMsg> {
    let player = player.cloned();
    let bg = if player.is_some() {
        OXITHEME.primary_bg
    } else {
        OXITHEME.mantle_hover
    };
    AnimationBuilder::new(bg, move |bg| {
        let body: Element<'static, PluginMsg> = if let Some(player) = player.clone() {
            let title = if player.title.is_empty() {
                player.identity.clone()
            } else {
                player.title.clone()
            };
            let subtitle = if player.artist.is_empty() {
                player.status.clone()
            } else {
                format!("{} • {}", player.artist, player.status)
            };
            let play_label = if player.status == "Playing" {
                "󰏤"
            } else {
                "󰐊"
            };
            let info = Column::new()
                .push(text(title).size(13).style(text_primary))
                .push(text(subtitle).size(10).style(text_muted))
                .push(
                    Row::new()
                        .push(media_button(
                            "󰒮",
                            Message::PlayerControl(PlayerAction::Previous),
                            busy,
                        ))
                        .push(media_button(
                            play_label,
                            Message::PlayerControl(PlayerAction::PlayPause),
                            busy,
                        ))
                        .push(media_button(
                            "󰒭",
                            Message::PlayerControl(PlayerAction::Next),
                            busy,
                        ))
                        .push(Space::new().width(Length::Fill))
                        .push(
                            text(format!("{}%", player.volume))
                                .size(11)
                                .style(text_muted),
                        )
                        .spacing(6)
                        .align_y(Alignment::Center),
                )
                .push(
                    oxi_slider::slider(0u8..=100u8, player.volume, |value| {
                        msg(Message::PlayerVolumeChanged(value))
                    })
                    .width(Length::Fill),
                )
                .spacing(8)
                .width(Length::Fill);

            Row::new()
                .push(artwork_preview(player.art_url.as_deref()))
                .push(info)
                .spacing(10)
                .align_y(Alignment::Center)
                .into()
        } else {
            Row::new()
                .push(artwork_preview(None))
                .push(
                    Column::new()
                        .push(text("No active media stream").size(13).style(text_primary))
                        .push(
                            text("Open music or video player to show MPRIS controls.")
                                .size(10)
                                .style(text_muted),
                        )
                        .spacing(3)
                        .width(Length::Fill),
                )
                .spacing(10)
                .align_y(Alignment::Center)
                .into()
        };
        card(body, bg).into()
    })
    .animation(Motion::SMOOTH)
    .into()
}

fn artwork_preview(art_url: Option<&str>) -> Element<'static, PluginMsg> {
    let content: Element<'static, PluginMsg> = if let Some(path) = art_url.and_then(local_art_path)
    {
        image(path)
            .width(76)
            .height(76)
            .content_fit(ContentFit::Cover)
            .border_radius(12)
            .into()
    } else {
        text("󰎈")
            .size(30)
            .style(|_| iced::widget::text::Style {
                color: Some(OXITHEME.primary),
            })
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .into()
    };

    Container::new(content)
        .style(|_| iced::widget::container::Style {
            background: Some(Background::Color(OXITHEME.primary_bg)),
            border: Border {
                radius: 12.0.into(),
                color: Color::TRANSPARENT,
                width: 0.0,
            },
            shadow: Shadow::default(),
            ..Default::default()
        })
        .width(76)
        .height(76)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .into()
}

fn local_art_path(url: &str) -> Option<String> {
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

fn output_section(snapshot: &AudioSnapshot) -> Element<'static, PluginMsg> {
    device_section(
        "Output Device",
        &snapshot.outputs,
        snapshot.default_output.as_deref(),
        Message::SetDefaultOutput,
        Message::OutputVolumeChanged,
    )
}

fn input_section(snapshot: &AudioSnapshot) -> Element<'static, PluginMsg> {
    device_section(
        "Input Device",
        &snapshot.inputs,
        snapshot.default_input.as_deref(),
        Message::SetDefaultInput,
        Message::InputVolumeChanged,
    )
}

fn device_section(
    label: &'static str,
    devices: &[AudioDevice],
    default: Option<&str>,
    select_msg: fn(DeviceChoice) -> Message,
    volume_msg: fn(u8) -> Message,
) -> Element<'static, PluginMsg> {
    let choices = devices.iter().map(AudioDevice::choice).collect::<Vec<_>>();
    let selected = selected_device(devices, default).map(AudioDevice::choice);
    let volume = selected_device(devices, default)
        .map(|device| device.volume)
        .unwrap_or_default();
    let picker = oxi_picklist::pick_list(choices, selected, move |choice| msg(select_msg(choice)))
        .width(Length::Fill);

    let body = Column::new()
        .push(text(label).size(12).style(text_primary))
        .push(picker)
        .push(
            Row::new()
                .push(
                    oxi_slider::slider(0u8..=150u8, volume, move |value| msg(volume_msg(value)))
                        .width(Length::Fill),
                )
                .push(text(format!("{volume}%")).size(11).style(text_muted))
                .spacing(8)
                .align_y(Alignment::Center),
        )
        .spacing(7)
        .width(Length::Fill);
    card(body.into(), OXITHEME.mantle_hover).into()
}

fn media_button(
    label: &'static str,
    message: Message,
    busy: bool,
) -> iced::widget::Button<'static, PluginMsg> {
    let button = oxi_button::button(text(label).size(13), oxi_button::ButtonVariant::PrimaryBg)
        .padding([5, 8]);
    if busy {
        button
    } else {
        button.on_press(msg(message))
    }
}

fn bar_button_style(_: &iced::Theme, status: button::Status) -> button::Style {
    let base = button::Style {
        background: None,
        text_color: OXITHEME.primary,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 8.0.into(),
        },
        shadow: Shadow::default(),
        snap: false,
    };
    match status {
        button::Status::Hovered => button::Style {
            background: Some(Background::Color(OXITHEME.primary_bg_hover)),
            ..base
        },
        button::Status::Pressed => button::Style {
            background: Some(Background::Color(OXITHEME.primary_bg_active)),
            ..base
        },
        button::Status::Active | button::Status::Disabled => base,
    }
}

fn card<'a>(content: Element<'a, PluginMsg>, bg: Color) -> Container<'a, PluginMsg> {
    Container::new(content)
        .style(move |_| iced::widget::container::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                radius: 12.0.into(),
                color: Color::TRANSPARENT,
                width: 0.0,
            },
            shadow: Shadow::default(),
            ..Default::default()
        })
        .padding(10)
        .width(Length::Fill)
}

fn text_primary(_: &iced::Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(OXITHEME.text),
    }
}

fn text_muted(_: &iced::Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(OXITHEME.text_muted),
    }
}

fn selected_device<'a>(
    devices: &'a [AudioDevice],
    default: Option<&str>,
) -> Option<&'a AudioDevice> {
    default
        .and_then(|name| devices.iter().find(|device| device.name == name))
        .or_else(|| devices.first())
}

fn read_poll_interval(global: &Table) -> u64 {
    global
        .get("audio")
        .and_then(|v| v.as_table())
        .and_then(|t| t.get("poll_seconds"))
        .and_then(|v| v.as_integer())
        .filter(|n| *n > 0)
        .map(|n| n as u64)
        .unwrap_or(DEFAULT_POLL_SECONDS)
}

fn load_snapshot() -> Result<AudioSnapshot, String> {
    let default_output = run_pactl(&["get-default-sink"]).ok();
    let default_input = run_pactl(&["get-default-source"]).ok();
    let outputs = list_devices(DeviceKind::Output)?;
    let inputs = list_devices(DeviceKind::Input)?;
    let player = current_player().ok().flatten();
    Ok(AudioSnapshot {
        player,
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
        devices.push(AudioDevice {
            name: name.to_owned(),
            label,
            volume,
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

fn current_player() -> Result<Option<PlayerInfo>, String> {
    let connection =
        Connection::session().map_err(|e| format!("audio: DBus session failed: {e}"))?;
    let dbus = DBusProxy::new(&connection).map_err(|e| format!("audio: DBus proxy failed: {e}"))?;
    let mut players = Vec::new();
    for name in dbus
        .list_names()
        .map_err(|e| format!("audio: DBus list names failed: {e}"))?
    {
        let service = name.to_string();
        if !service.starts_with("org.mpris.MediaPlayer2.") {
            continue;
        }
        if let Ok(player) = read_player(&connection, &service) {
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

fn read_player(connection: &Connection, service: &str) -> Result<PlayerInfo, String> {
    let player = Proxy::new(connection, service, PLAYER_PATH, PLAYER_IFACE)
        .map_err(|e| format!("audio: MPRIS player proxy failed: {e}"))?;
    let root = Proxy::new(connection, service, PLAYER_PATH, ROOT_IFACE)
        .map_err(|e| format!("audio: MPRIS root proxy failed: {e}"))?;
    let identity = root.get_property::<String>("Identity").unwrap_or_else(|_| {
        service
            .trim_start_matches("org.mpris.MediaPlayer2.")
            .to_owned()
    });
    let status = player
        .get_property::<String>("PlaybackStatus")
        .unwrap_or_else(|_| "Unknown".to_owned());
    let volume = player
        .get_property::<f64>("Volume")
        .map(|volume| (volume * 100.0).round().clamp(0.0, 100.0) as u8)
        .unwrap_or(100);
    let metadata = player
        .get_property::<HashMap<String, OwnedValue>>("Metadata")
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

fn control_player(service: &str, action: PlayerAction) -> Result<(), String> {
    let connection =
        Connection::session().map_err(|e| format!("audio: DBus session failed: {e}"))?;
    let player = Proxy::new(&connection, service, PLAYER_PATH, PLAYER_IFACE)
        .map_err(|e| format!("audio: MPRIS player proxy failed: {e}"))?;
    let method = match action {
        PlayerAction::Previous => "Previous",
        PlayerAction::PlayPause => "PlayPause",
        PlayerAction::Next => "Next",
    };
    player
        .call::<_, _, ()>(method, &())
        .map_err(|e| format!("audio: MPRIS {method} failed: {e}"))
}

fn set_player_volume(service: &str, volume: u8) -> Result<(), String> {
    let connection =
        Connection::session().map_err(|e| format!("audio: DBus session failed: {e}"))?;
    let player = Proxy::new(&connection, service, PLAYER_PATH, PLAYER_IFACE)
        .map_err(|e| format!("audio: MPRIS player proxy failed: {e}"))?;
    player
        .set_property("Volume", (volume as f64 / 100.0).clamp(0.0, 1.0))
        .map_err(|e| format!("audio: MPRIS volume failed: {e}"))
}

fn run_pactl(args: &[&str]) -> Result<String, String> {
    let output = Command::new("pactl")
        .args(args)
        .output()
        .map_err(|e| format!("audio: failed to run pactl: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if output.status.success() {
        Ok(stdout)
    } else if stderr.is_empty() {
        Err(format!("audio: pactl failed: {stdout}"))
    } else {
        Err(format!("audio: pactl failed: {stderr}"))
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

const _: fn() = || {
    fn assert_stream<S: Stream<Item = PluginMsg> + Send + 'static>(_: &S) {}
    let _ = |s: &iced::futures::stream::BoxStream<'static, PluginMsg>| assert_stream(s);
};
