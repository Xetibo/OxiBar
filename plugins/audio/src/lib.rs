//! Audio management plugin backed by PulseAudio/PipeWire `pactl` and MPRIS.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use iced::{
    Alignment, Background, Border, Color, ContentFit, Element, Length, Shadow, Task,
    futures::Stream,
    stream,
    widget::{Column, Container, Row, Space, image, text},
};
use iced_anim::{AnimationBuilder, Motion};
use oxibar_plugin_api::{
    ABI_VERSION, HOST_REQUEST_TOGGLE_POPUP, PluginMetadata, PluginModel, PluginMsg, PluginStream,
    drain_model_errors, plugin_model, toml::Table, with_model_read, with_model_write,
};
use oxiced::{
    theme::theme_impl::OXITHEME,
    widgets::{
        oxi_button, oxi_picklist, oxi_plugin, oxi_plugin::text_muted, oxi_plugin::text_primary,
        oxi_slider,
    },
};

mod system;

use system::{
    AudioDevice, AudioSnapshot, DeviceChoice, DeviceKind, PlayerAction, PlayerInfo, control_player,
    load_snapshot, local_art_path, selected_device, set_default_device, set_device_volume,
    set_player_volume,
};

const DEFAULT_POLL_SECONDS: u64 = 4;
const AUDIO_ICON: &str = "󰕾";

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
pub extern "Rust" fn metadata() -> PluginMetadata {
    PluginMetadata {
        popup_size: Some((460, 420)),
        ..PluginMetadata::default()
    }
}

#[unsafe(no_mangle)]
pub extern "Rust" fn model(global_config: Table) -> (PluginModel, Option<Task<PluginMsg>>) {
    (
        plugin_model(Model::new(global_config)),
        Some(Task::done(msg(Message::Refresh))),
    )
}

#[unsafe(no_mangle)]
pub extern "Rust" fn update(model: PluginModel, msg_in: PluginMsg) -> Option<Task<PluginMsg>> {
    let m = msg_in.downcast_ref::<Message>()?.clone();
    with_model_write::<Model, _>(&model, |model| match m {
        Message::TogglePopup => Some(Task::done(
            Arc::new(HOST_REQUEST_TOGGLE_POPUP.to_owned()) as PluginMsg
        )),
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
            let player = model.snapshot.player.clone()?;
            run_action(model, format!("media {action:?}"), true, move || {
                control_player(&player.service, action)
            })
        }
        Message::PlayerVolumeChanged(volume) => {
            let player = model.snapshot.player.as_mut()?;
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
            let name = model.snapshot.default_output.clone()?;
            if let Some(device) = model.snapshot.outputs.iter_mut().find(|d| d.name == name) {
                device.volume = volume;
            }
            run_action(model, "output volume".to_owned(), false, move || {
                set_device_volume(DeviceKind::Output, &name, volume)
            })
        }
        Message::InputVolumeChanged(volume) => {
            let name = model.snapshot.default_input.clone()?;
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
    })
    .flatten()
}

#[unsafe(no_mangle)]
pub extern "Rust" fn launch(_focused_index: usize, _model: PluginModel) -> Option<Task<PluginMsg>> {
    Some(Task::done(msg(Message::TogglePopup)))
}

#[unsafe(no_mangle)]
pub extern "Rust" fn errors(model: PluginModel) -> Vec<String> {
    drain_model_errors::<Model>(&model, |model| &mut model.errors)
}

#[unsafe(no_mangle)]
pub extern "Rust" fn view(
    model: PluginModel,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    with_model_read::<Model, _>(&model, |model| {
        let volume = selected_device(
            &model.snapshot.outputs,
            model.snapshot.default_output.as_deref(),
        )
        .map(|device| device.volume);
        let content: Element<'static, PluginMsg> = if let Some(volume) = volume {
            Row::new()
                .push(text(AUDIO_ICON).size(14).align_y(Alignment::Center))
                .push(
                    text(format!("{volume}%"))
                        .size(14)
                        .align_y(Alignment::Center),
                )
                .spacing(10)
                .height(Length::Fill)
                .align_y(Alignment::Center)
                .into()
        } else {
            text(AUDIO_ICON)
                .size(14)
                .align_y(Alignment::Center)
                .align_x(Alignment::Center)
                .into()
        };
        let btn = oxi_plugin::bar_button(content).on_press(msg(Message::TogglePopup));

        vec![btn.into()]
    })
}

#[unsafe(no_mangle)]
pub extern "Rust" fn popup_view(
    model: PluginModel,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    with_model_read::<Model, _>(&model, |model| {
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

        vec![content.height(Length::Fill).into()]
    })
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

const _: fn() = || {
    fn assert_stream<S: Stream<Item = PluginMsg> + Send + 'static>(_: &S) {}
    let _ = |s: &iced::futures::stream::BoxStream<'static, PluginMsg>| assert_stream(s);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_poll_interval_from_config() {
        let mut audio = Table::new();
        audio.insert(
            "poll_seconds".to_owned(),
            oxibar_plugin_api::toml::Value::Integer(9),
        );
        let mut global = Table::new();
        global.insert(
            "audio".to_owned(),
            oxibar_plugin_api::toml::Value::Table(audio),
        );

        assert_eq!(read_poll_interval(&global), 9);
        assert_eq!(read_poll_interval(&Table::new()), DEFAULT_POLL_SECONDS);
    }

    #[test]
    fn model_views_and_error_drain_are_deterministic() {
        let (plugin_model, init_task) = model(Table::new());
        assert!(init_task.is_some());
        assert_eq!(name(), "Audio");
        assert_eq!(abi_version(), ABI_VERSION);
        assert_eq!(metadata().popup_size, Some((460, 420)));
        assert_eq!(view(plugin_model.clone()).unwrap().len(), 1);
        assert_eq!(popup_view(plugin_model.clone()).unwrap().len(), 1);

        let task = update(
            plugin_model.clone(),
            msg(Message::Snapshot(Err("audio failed".to_owned()))),
        );
        assert!(task.is_none());
        assert_eq!(errors(plugin_model.clone()), vec!["audio failed"]);
        assert!(errors(plugin_model).is_empty());
    }
}
