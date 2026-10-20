//! Shared native PulseAudio/PipeWire device state and asynchronous MPRIS controls.

use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

use iced::{
    Alignment, Background, Border, Color, ContentFit, Element, Length, Shadow, Task,
    futures::{Stream, StreamExt},
    widget::{Column, Container, Row, Space, image, scrollable},
};
use iced_anim::{AnimationBuilder, transition::Easing};
use oxibar_plugin_api::{
    ABI_VERSION, CommandOutcome, HOST_REQUEST_TOGGLE_POPUP, HostOsdRequest, PluginMetadata,
    PluginModel, PluginMsg, PluginStream, drain_model_errors, plugin_model, toml::Table,
    with_model_read, with_model_write,
};
use oxiced::widgets::oxi_plugin::text;
use oxiced::{
    theme::theme_impl::OXITHEME,
    widgets::{
        oxi_button, oxi_picklist, oxi_plugin, oxi_plugin::text_muted, oxi_plugin::text_primary,
        oxi_slider, oxi_toggler,
    },
};

#[cfg(test)]
mod integration_tests;
mod native;
mod system;
mod worker;

use system::{
    AudioDevice, AudioSnapshot, DeviceAction, DeviceChoice, DeviceKind, DeviceSnapshot,
    PlayerAction, PlayerInfo, control_player, local_art_path, selected_device, set_player_volume,
};

const DEFAULT_POLL_SECONDS: u64 = 4;

#[unsafe(no_mangle)]
pub extern "Rust" fn settings_schema() -> Vec<oxibar_plugin_api::SettingDescriptor> {
    use oxibar_plugin_api::{ApplyMode, SettingDescriptor, SettingKind, toml::Value};
    vec![
        SettingDescriptor::new(
            "audio.backend",
            "Audio backend",
            SettingKind::Choice(vec!["native".to_owned(), "pactl".to_owned()]),
            Some(Value::String("native".to_owned())),
            ApplyMode::Restart,
        )
        .description("Native PulseAudio/PipeWire events, or explicit pactl compatibility mode."),
        SettingDescriptor::new(
            "audio.poll_seconds",
            "Media refresh interval",
            SettingKind::Integer {
                min: 1,
                max: i64::MAX,
            },
            Some(Value::Integer(DEFAULT_POLL_SECONDS as i64)),
            ApplyMode::Restart,
        )
        .description("MPRIS refresh interval; also refreshes devices in pactl compatibility mode."),
    ]
}
const AUDIO_ICON: &str = "󰕾";
const POPUP_SIZE: (u32, u32) = (460, 640);
const ARTWORK_PREVIEW_SIZE: u32 = 76;
const ARTWORK_ICON_SIZE: f32 = 30.0;
const MAX_DEVICE_VOLUME: u8 = 150;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Backend {
    #[default]
    Native,
    Pactl,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ActionTarget {
    Media,
    Devices,
    NativeDevices,
}

#[derive(Clone, Default)]
struct SubscriptionConfig {
    interval: Option<Duration>,
    client: Option<Weak<native::Client>>,
}

static SUBSCRIPTION_CONFIG: OnceLock<Mutex<SubscriptionConfig>> = OnceLock::new();

fn subscription_config() -> &'static Mutex<SubscriptionConfig> {
    SUBSCRIPTION_CONFIG.get_or_init(Mutex::default)
}

#[derive(Debug)]
struct Model {
    snapshot: AudioSnapshot,
    pending: Option<String>,
    errors: Vec<String>,
    backend: Backend,
    native: Option<Arc<native::Client>>,
    device_error: Option<String>,
    device_refreshing: bool,
    device_refresh_again: bool,
    media_refreshing: Option<u64>,
    media_refresh_again: bool,
    media_generation: u64,
    action_generation: u64,
    media_action_generation: u64,
    device_action_generation: u64,
    device_action_active: bool,
    media_action_active: bool,
    device_osd_topic: Option<&'static str>,
    media_osd_topic: Option<&'static str>,
    device_baseline: Option<DeviceSnapshot>,
    media_baseline: Option<Option<PlayerInfo>>,
}

impl Model {
    fn new(global_config: Table) -> Self {
        let poll_seconds = read_poll_interval(&global_config);
        let backend = read_backend(&global_config);
        let native = (backend == Backend::Native)
            .then(native::Client::new)
            .transpose();
        let (native, errors) = match native {
            Ok(client) => (client, Vec::new()),
            Err(error) => (None, vec![error]),
        };
        *subscription_config()
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = SubscriptionConfig {
            interval: Some(Duration::from_secs(poll_seconds)),
            client: native.as_ref().map(Arc::downgrade),
        };
        Self {
            snapshot: AudioSnapshot::default(),
            pending: None,
            errors,
            backend,
            native,
            device_error: None,
            device_refreshing: false,
            device_refresh_again: false,
            media_refreshing: None,
            media_refresh_again: false,
            media_generation: 0,
            action_generation: 0,
            media_action_generation: 0,
            device_action_generation: 0,
            device_action_active: false,
            media_action_active: false,
            device_osd_topic: None,
            media_osd_topic: None,
            device_baseline: None,
            media_baseline: None,
        }
    }
}

#[derive(Clone, Debug)]
enum Message {
    TogglePopup,
    Refresh,
    Poll,
    RefreshDevices,
    RefreshMedia,
    Devices(Result<DeviceSnapshot, String>),
    Media {
        result: Result<Option<PlayerInfo>, String>,
        generation: u64,
    },
    PlayerControl(PlayerAction),
    PlayerVolumeChanged(u8),
    SetDefaultOutput(DeviceChoice),
    SetDefaultInput(DeviceChoice),
    OutputVolumeChanged(u8),
    InputVolumeChanged(u8),
    OutputMuted(bool),
    InputMuted(bool),
    ActionDone {
        result: Result<(), String>,
        target: ActionTarget,
        generation: u64,
    },
    DeviceOsd {
        result: Result<DeviceSnapshot, String>,
        generation: u64,
        topic: &'static str,
    },
    MediaOsd {
        result: Result<Option<PlayerInfo>, String>,
        generation: u64,
        topic: &'static str,
    },
}

fn msg(m: Message) -> PluginMsg {
    Arc::new(m)
}

oxibar_plugin_api::export_theme_changed!();

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
        popup_size: Some(POPUP_SIZE),
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
        Message::Refresh | Message::Poll | Message::RefreshDevices | Message::RefreshMedia => {
            let mut tasks = Vec::new();
            let devices = !matches!(m, Message::RefreshMedia);
            let media = !matches!(m, Message::RefreshDevices);
            let manual = !matches!(m, Message::Poll);
            if devices
                && manual
                && let Some(client) = &model.native
            {
                client.refresh();
            }
            if devices && model.backend == Backend::Pactl {
                if model.device_refreshing {
                    model.device_refresh_again |= manual;
                } else {
                    model.device_refreshing = true;
                    tasks.push(Task::perform(
                        async { worker::run(system::load_devices).await.and_then(|r| r) },
                        |r| msg(Message::Devices(r)),
                    ));
                }
            }
            if media {
                if model.media_refreshing.is_some() {
                    model.media_refresh_again |= manual;
                } else {
                    model.media_generation = model.media_generation.wrapping_add(1);
                    let generation = model.media_generation;
                    model.media_refreshing = Some(generation);
                    tasks.push(Task::perform(
                        worker::bounded(system::current_player()),
                        move |result| msg(Message::Media { result, generation }),
                    ));
                }
            }
            Some(Task::batch(tasks))
        }
        Message::Devices(result) => {
            model.device_refreshing = false;
            let mut tasks = Vec::new();
            match result {
                Ok(snapshot) => {
                    if !model.device_action_active
                        && let Some(old) = &model.device_baseline
                    {
                        for (kind, topic) in [
                            (DeviceKind::Output, "volume"),
                            (DeviceKind::Input, "input-volume"),
                        ] {
                            if device_digest(old, kind) != device_digest(&snapshot, kind)
                                && device_digest(&snapshot, kind).is_some()
                            {
                                tasks.push(show_osd(topic, true));
                            }
                        }
                    }
                    model.device_baseline = Some(snapshot.clone());
                    model.snapshot.apply_devices(snapshot);
                    model.device_error = None;
                }
                Err(error) => {
                    if model.device_error.as_ref() != Some(&error) {
                        model.errors.push(error.clone());
                    }
                    model.device_error = Some(error);
                }
            }
            if std::mem::take(&mut model.device_refresh_again) {
                tasks.push(Task::done(msg(Message::RefreshDevices)));
            }
            (!tasks.is_empty()).then(|| Task::batch(tasks))
        }
        Message::Media { result, generation } => {
            if model.media_refreshing != Some(generation) {
                return None;
            }
            model.media_refreshing = None;
            let mut tasks = Vec::new();
            if generation == model.media_generation
                && let Ok(player) = result
            {
                if !model.media_action_active
                    && let Some(old) = &model.media_baseline
                    && old != &player
                    && player.is_some()
                {
                    tasks.push(show_osd("media", true));
                }
                model.media_baseline = Some(player.clone());
                model.snapshot.player = player;
            }
            if std::mem::take(&mut model.media_refresh_again) {
                tasks.push(Task::done(msg(Message::RefreshMedia)));
            }
            (!tasks.is_empty()).then(|| Task::batch(tasks))
        }
        Message::PlayerControl(action) => {
            let player = model.snapshot.player.clone()?;
            model.media_osd_topic = Some("media");
            model.media_generation = model.media_generation.wrapping_add(1);
            run_action(
                model,
                format!("media {action:?}"),
                ActionTarget::Media,
                worker::bounded(async move { control_player(&player.service, action).await }),
            )
        }
        Message::PlayerVolumeChanged(volume) => {
            let player = model.snapshot.player.as_mut()?;
            player.volume = volume;
            let service = player.service.clone();
            model.media_osd_topic = Some("media-volume");
            model.media_generation = model.media_generation.wrapping_add(1);
            run_action(
                model,
                "media volume".to_owned(),
                ActionTarget::Media,
                worker::bounded(async move { set_player_volume(&service, volume).await }),
            )
        }
        Message::SetDefaultOutput(choice) => {
            model.snapshot.default_output = Some(choice.name.clone());
            run_device_action(
                model,
                "set output",
                DeviceAction::Default(DeviceKind::Output, choice.name),
            )
        }
        Message::SetDefaultInput(choice) => {
            model.snapshot.default_input = Some(choice.name.clone());
            run_device_action(
                model,
                "set input",
                DeviceAction::Default(DeviceKind::Input, choice.name),
            )
        }
        Message::OutputVolumeChanged(volume) => {
            let name = selected_device(
                &model.snapshot.outputs,
                model.snapshot.default_output.as_deref(),
            )?
            .name
            .clone();
            if let Some(device) = model.snapshot.outputs.iter_mut().find(|d| d.name == name) {
                device.volume = volume;
            }
            run_device_action(
                model,
                "output volume",
                DeviceAction::Volume(DeviceKind::Output, name, volume),
            )
        }
        Message::InputVolumeChanged(volume) => {
            let name = selected_device(
                &model.snapshot.inputs,
                model.snapshot.default_input.as_deref(),
            )?
            .name
            .clone();
            if let Some(device) = model.snapshot.inputs.iter_mut().find(|d| d.name == name) {
                device.volume = volume;
            }
            run_device_action(
                model,
                "input volume",
                DeviceAction::Volume(DeviceKind::Input, name, volume),
            )
        }
        Message::OutputMuted(muted) | Message::InputMuted(muted) => {
            let kind = if matches!(m, Message::OutputMuted(_)) {
                DeviceKind::Output
            } else {
                DeviceKind::Input
            };
            let (devices, default) = match kind {
                DeviceKind::Output => (&mut model.snapshot.outputs, &model.snapshot.default_output),
                DeviceKind::Input => (&mut model.snapshot.inputs, &model.snapshot.default_input),
            };
            let name = selected_device(devices, default.as_deref())?.name.clone();
            if let Some(device) = devices.iter_mut().find(|d| d.name == name) {
                device.muted = muted;
            }
            run_device_action(model, "device mute", DeviceAction::Mute(kind, name, muted))
        }
        Message::ActionDone {
            result,
            target,
            generation,
        } => {
            let latest = match target {
                ActionTarget::Media => model.media_action_generation,
                ActionTarget::Devices | ActionTarget::NativeDevices => {
                    model.device_action_generation
                }
            };
            if generation != latest {
                return None;
            }
            if generation == model.action_generation {
                model.pending = None;
            }
            let topic = match target {
                ActionTarget::Media => {
                    model.media_action_active = false;
                    model.media_osd_topic.take()
                }
                ActionTarget::Devices | ActionTarget::NativeDevices => {
                    model.device_action_active = false;
                    model.device_osd_topic.take()
                }
            };
            if result.is_ok()
                && let Some(topic) = topic
            {
                return Some(match target {
                    ActionTarget::NativeDevices => {
                        model.device_baseline = Some(device_snapshot(&model.snapshot));
                        show_osd(topic, false)
                    }
                    ActionTarget::Devices => Task::perform(
                        async {
                            worker::run(system::load_devices)
                                .await
                                .and_then(|result| result)
                        },
                        move |result| {
                            msg(Message::DeviceOsd {
                                result,
                                generation,
                                topic,
                            })
                        },
                    ),
                    ActionTarget::Media => {
                        Task::perform(worker::bounded(system::current_player()), move |result| {
                            msg(Message::MediaOsd {
                                result,
                                generation,
                                topic,
                            })
                        })
                    }
                });
            }
            let refresh = target != ActionTarget::NativeDevices || result.is_err();
            if let Err(error) = result {
                model.errors.push(error);
            }
            refresh.then(|| {
                Task::done(msg(match target {
                    ActionTarget::Media => Message::RefreshMedia,
                    ActionTarget::Devices | ActionTarget::NativeDevices => Message::RefreshDevices,
                }))
            })
        }
        Message::DeviceOsd {
            result,
            generation,
            topic,
        } => {
            if model.device_action_generation != generation {
                return None;
            }
            match result {
                Ok(snapshot) => {
                    model.device_baseline = Some(snapshot.clone());
                    model.snapshot.apply_devices(snapshot);
                    model.device_error = None;
                    Some(show_osd(topic, false))
                }
                Err(error) => {
                    model.errors.push(error);
                    None
                }
            }
        }
        Message::MediaOsd {
            result,
            generation,
            topic,
        } => {
            if model.media_action_generation != generation {
                return None;
            }
            if let Ok(player) = result {
                model.media_baseline = Some(player.clone());
                model.snapshot.player = player;
                model
                    .snapshot
                    .player
                    .as_ref()
                    .map(|_| show_osd(topic, false))
            } else {
                None
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
    bar_elements(model, true)
}

#[unsafe(no_mangle)]
pub extern "Rust" fn bar_view(
    model: PluginModel,
    _: oxibar_plugin_api::PluginViewContext,
    show_labels: bool,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    bar_elements(model, show_labels)
}

fn bar_elements(
    model: PluginModel,
    show_labels: bool,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    with_model_read::<Model, _>(&model, |model| {
        let device = selected_device(
            &model.snapshot.outputs,
            model.snapshot.default_output.as_deref(),
        );
        let content: Element<'static, PluginMsg> =
            if let Some(device) = device.filter(|_| show_labels) {
                Row::new()
                    .push(
                        text(if device.muted { "󰝟" } else { AUDIO_ICON })
                            .size(OXITHEME.font_md)
                            .align_y(Alignment::Center),
                    )
                    .push(
                        text(if device.muted {
                            "Muted".to_owned()
                        } else {
                            format!("{}%", device.volume)
                        })
                        .size(OXITHEME.font_md)
                        .align_y(Alignment::Center),
                    )
                    .spacing(OXITHEME.padding_xs)
                    .height(Length::Fill)
                    .align_y(Alignment::Center)
                    .into()
            } else {
                text(if device.is_some_and(|device| device.muted) {
                    "󰝟"
                } else {
                    AUDIO_ICON
                })
                .size(OXITHEME.font_md)
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
            .spacing(OXITHEME.padding_sm)
            .padding([OXITHEME.padding_md, OXITHEME.padding_lg])
            .width(Length::Fill);
        content = content.push(media_card(
            model.snapshot.player.as_ref(),
            model.pending.is_some(),
        ));
        content = content.push(output_section(&model.snapshot));
        content = content.push(input_section(&model.snapshot));
        if let Some(error) = &model.device_error {
            content = content.push(text(error.clone()).size(OXITHEME.font_md).style(text_muted));
        }

        vec![scrollable(content).height(Length::Fill).into()]
    })
}

#[unsafe(no_mangle)]
pub extern "Rust" fn control_center_view(
    model: PluginModel,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    with_model_read::<Model, _>(&model, |model| {
        let mut content = Column::new()
            .push(media_card(
                model.snapshot.player.as_ref(),
                model.pending.is_some(),
            ))
            .push(output_section(&model.snapshot))
            .push(input_section(&model.snapshot))
            .spacing(OXITHEME.padding_xl)
            .width(Length::Fill);
        if let Some(error) = &model.device_error {
            content = content.push(text(error.clone()).size(OXITHEME.font_md).style(text_muted));
        }
        vec![content.into()]
    })
}

#[unsafe(no_mangle)]
pub extern "Rust" fn subwidgets(_: PluginModel) -> Vec<oxibar_plugin_api::SubwidgetDescriptor> {
    [("media", "Now playing"), ("volume", "Volume")]
        .into_iter()
        .map(|(id, title)| oxibar_plugin_api::SubwidgetDescriptor {
            id: id.into(),
            title: title.into(),
        })
        .collect()
}

#[unsafe(no_mangle)]
pub extern "Rust" fn subwidget_view(
    model: PluginModel,
    id: &str,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    with_model_read::<Model, _>(&model, |model| match id {
        "media" => vec![media_card(
            model.snapshot.player.as_ref(),
            model.pending.is_some(),
        )],
        "volume" => vec![output_section(&model.snapshot)],
        _ => Vec::new(),
    })
}

#[unsafe(no_mangle)]
pub extern "Rust" fn popup_metrics(model: PluginModel) -> oxibar_plugin_api::PluginPopupMetrics {
    let height = with_model_read::<Model, _>(&model, |model| {
        let line = OXITHEME.font_md * 1.3;
        let small = OXITHEME.font_sm * 1.3;
        let card_padding = OXITHEME.padding_md * 2.0;
        let media = model
            .snapshot
            .player
            .as_ref()
            .map_or(ARTWORK_PREVIEW_SIZE as f32, |player| {
                let lines = ((player.title.chars().count() + player.artist.chars().count()) as f32
                    / 28.0)
                    .ceil()
                    .max(2.0);
                (line * lines + line + OXITHEME.padding_xs * 2.0 + 24.0 + OXITHEME.padding_sm * 3.0)
                    .max(ARTWORK_PREVIEW_SIZE as f32)
            })
            + card_padding;
        let full_device = line
            + line
            + OXITHEME.padding_md * 2.0
            + 24.0
            + small.max(16.0)
            + OXITHEME.padding_sm * 3.0
            + card_padding;
        let device_height = |devices: &[AudioDevice]| {
            if devices.is_empty() {
                line * 2.0 + OXITHEME.padding_sm + card_padding
            } else {
                full_device
            }
        };
        let error = model.device_error.as_ref().map_or(0.0, |error| {
            line * (error.chars().count() as f32 / 45.0).ceil().max(1.0) + OXITHEME.padding_sm
        });
        (media
            + device_height(&model.snapshot.outputs)
            + device_height(&model.snapshot.inputs)
            + error
            + OXITHEME.padding_sm * 2.0
            + OXITHEME.padding_md * 2.0
            + 20.0)
            .ceil() as u32
    })
    .unwrap_or(POPUP_SIZE.1)
    .min(900);
    oxibar_plugin_api::PluginPopupMetrics {
        popup_size: Some((POPUP_SIZE.0, height)),
        ..Default::default()
    }
}

#[unsafe(no_mangle)]
pub extern "Rust" fn desktop_view(
    model: PluginModel,
    _: oxibar_plugin_api::PluginViewContext,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    with_model_read::<Model, _>(&model, |model| {
        vec![media_card(
            model.snapshot.player.as_ref(),
            model.pending.is_some(),
        )]
    })
}

#[unsafe(no_mangle)]
pub extern "Rust" fn desktop_interactive(model: PluginModel) -> bool {
    with_model_read::<Model, _>(&model, |model| model.snapshot.player.is_some()).unwrap_or(false)
}

#[unsafe(no_mangle)]
pub extern "Rust" fn subscription() -> *mut PluginStream {
    let config = subscription_config()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let interval = config
        .interval
        .unwrap_or(Duration::from_secs(DEFAULT_POLL_SECONDS));
    let timer = iced::futures::stream::unfold((), move |_| async move {
        async_io::Timer::after(interval).await;
        Some((msg(Message::Poll), ()))
    })
    .boxed();
    let s = if let Some(client) = config.client.and_then(|weak| weak.upgrade()) {
        iced::futures::stream::select(
            native::events(client).map(|r| msg(Message::Devices(r))),
            timer,
        )
        .boxed()
    } else {
        timer
    };

    Box::into_raw(Box::new(s)) as *mut PluginStream
}

fn run_action(
    model: &mut Model,
    label: String,
    target: ActionTarget,
    action: impl std::future::Future<Output = Result<(), String>> + Send + 'static,
) -> Option<Task<PluginMsg>> {
    model.pending = Some(label);
    model.action_generation = model.action_generation.wrapping_add(1);
    let generation = model.action_generation;
    match target {
        ActionTarget::Media => {
            model.media_action_generation = generation;
            model.media_action_active = true;
        }
        ActionTarget::Devices | ActionTarget::NativeDevices => {
            model.device_action_generation = generation;
            model.device_action_active = true;
        }
    }
    Some(Task::perform(action, move |result| {
        msg(Message::ActionDone {
            result,
            target,
            generation,
        })
    }))
}

fn run_device_action(
    model: &mut Model,
    label: &str,
    action: DeviceAction,
) -> Option<Task<PluginMsg>> {
    let kind = match &action {
        DeviceAction::Default(kind, _)
        | DeviceAction::Volume(kind, _, _)
        | DeviceAction::Mute(kind, _, _) => *kind,
    };
    model.device_osd_topic = Some(if kind == DeviceKind::Output {
        "volume"
    } else {
        "input-volume"
    });
    if model.backend == Backend::Pactl {
        run_action(model, label.to_owned(), ActionTarget::Devices, async move {
            worker::run(move || system::device_action(action)).await?
        })
    } else {
        let receiver = model.native.as_ref()?.submit(action);
        run_action(
            model,
            label.to_owned(),
            ActionTarget::NativeDevices,
            worker::deadline(
                async move {
                    receiver
                        .await
                        .map_err(|_| "audio: native worker stopped".to_owned())?
                },
                Duration::from_secs(6),
                "native command",
            ),
        )
    }
}

fn show_osd(topic: &'static str, follow_focus: bool) -> Task<PluginMsg> {
    let request = HostOsdRequest::new(topic);
    Task::done(
        (if follow_focus {
            request.follow_focus()
        } else {
            request
        })
        .into_message(),
    )
}
fn device_snapshot(snapshot: &AudioSnapshot) -> DeviceSnapshot {
    DeviceSnapshot {
        outputs: snapshot.outputs.clone(),
        inputs: snapshot.inputs.clone(),
        default_output: snapshot.default_output.clone(),
        default_input: snapshot.default_input.clone(),
    }
}
fn device_digest(snapshot: &DeviceSnapshot, kind: DeviceKind) -> Option<(String, u8, bool)> {
    let (devices, default) = match kind {
        DeviceKind::Output => (&snapshot.outputs, snapshot.default_output.as_deref()),
        DeviceKind::Input => (&snapshot.inputs, snapshot.default_input.as_deref()),
    };
    selected_device(devices, default)
        .map(|device| (device.name.clone(), device.volume, device.muted))
}

#[unsafe(no_mangle)]
pub extern "Rust" fn osd_view(
    model: PluginModel,
    topic: &str,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    with_model_read::<Model, _>(&model, |model| {
        let content = match topic {
            "volume" | "input-volume" => {
                let (devices, default, icon, title) = if topic == "volume" {
                    (
                        &model.snapshot.outputs,
                        model.snapshot.default_output.as_deref(),
                        AUDIO_ICON,
                        "Volume",
                    )
                } else {
                    (
                        &model.snapshot.inputs,
                        model.snapshot.default_input.as_deref(),
                        "󰍬",
                        "Microphone",
                    )
                };
                let device = selected_device(devices, default)
                    .ok_or_else(|| std::io::Error::other("Audio device is unavailable"))?;
                oxi_plugin::osd_content(
                    if device.muted { "󰝟" } else { icon },
                    title,
                    if device.muted {
                        "Muted".to_owned()
                    } else {
                        format!("{}%", device.volume)
                    },
                    Some((
                        if device.muted {
                            0.0
                        } else {
                            device.volume.into()
                        },
                        MAX_DEVICE_VOLUME.into(),
                    )),
                )
            }
            "media" | "media-volume" => {
                let player = model
                    .snapshot
                    .player
                    .as_ref()
                    .ok_or_else(|| std::io::Error::other("Media player is unavailable"))?;
                let value = if topic == "media-volume" {
                    format!("{}%", player.volume)
                } else {
                    format!(
                        "{} · {}",
                        player.status,
                        player.title.chars().take(28).collect::<String>()
                    )
                };
                oxi_plugin::osd_content(
                    "󰎈",
                    "Media",
                    value,
                    (topic == "media-volume").then_some((player.volume.into(), 100.0)),
                )
            }
            _ => return Err(std::io::Error::other("Unknown Audio OSD topic")),
        };
        Ok(vec![content])
    })?
}

fn command_percentage(value: &str, current: u8, maximum: u8) -> Result<u8, String> {
    let value = value.strip_suffix('%').unwrap_or(value);
    let number: i16 = value
        .parse()
        .map_err(|_| "Expected a volume percentage or signed step")?;
    if value.starts_with(['+', '-']) {
        Ok(i16::from(current)
            .saturating_add(number)
            .clamp(0, i16::from(maximum)) as u8)
    } else if (0..=i16::from(maximum)).contains(&number) {
        Ok(number as u8)
    } else {
        Err(format!("Volume must be between 0 and {maximum}"))
    }
}

fn command_message(model: &Model, command: &str, arguments: &[String]) -> Result<Message, String> {
    let device = |kind| {
        let (devices, default) = if kind == DeviceKind::Output {
            (
                &model.snapshot.outputs,
                model.snapshot.default_output.as_deref(),
            )
        } else {
            (
                &model.snapshot.inputs,
                model.snapshot.default_input.as_deref(),
            )
        };
        selected_device(devices, default).ok_or("Audio device is unavailable".to_owned())
    };
    let muted = |arguments: &[String], current: bool| match arguments {
        [] => Ok(!current),
        [value] if value == "toggle" => Ok(!current),
        [value] if value == "on" || value == "true" => Ok(true),
        [value] if value == "off" || value == "false" => Ok(false),
        _ => Err("Mute expects on, off or toggle".to_owned()),
    };
    match (command, arguments) {
        ("volume", [value]) => Ok(Message::OutputVolumeChanged(command_percentage(
            value,
            device(DeviceKind::Output)?.volume,
            MAX_DEVICE_VOLUME,
        )?)),
        ("input-volume", [value]) => Ok(Message::InputVolumeChanged(command_percentage(
            value,
            device(DeviceKind::Input)?.volume,
            MAX_DEVICE_VOLUME,
        )?)),
        ("mute", arguments) => Ok(Message::OutputMuted(muted(
            arguments,
            device(DeviceKind::Output)?.muted,
        )?)),
        ("input-mute", arguments) => Ok(Message::InputMuted(muted(
            arguments,
            device(DeviceKind::Input)?.muted,
        )?)),
        ("media-volume", [value]) => Ok(Message::PlayerVolumeChanged(command_percentage(
            value,
            model
                .snapshot
                .player
                .as_ref()
                .ok_or("Media player is unavailable")?
                .volume,
            100,
        )?)),
        ("play-pause" | "next" | "previous", []) => {
            if model.snapshot.player.is_none() {
                return Err("Media player is unavailable".to_owned());
            }
            Ok(Message::PlayerControl(match command {
                "next" => PlayerAction::Next,
                "previous" => PlayerAction::Previous,
                _ => PlayerAction::PlayPause,
            }))
        }
        _ => Err("Unknown Audio command or arguments".to_owned()),
    }
}

#[unsafe(no_mangle)]
pub extern "Rust" fn command(
    model: PluginModel,
    command: String,
    arguments: Vec<String>,
) -> Result<Task<CommandOutcome>, String> {
    let message =
        with_model_read::<Model, _>(&model, |model| command_message(model, &command, &arguments))
            .map_err(|error| error.to_string())??;
    let task = update(model, msg(message)).ok_or("Audio action could not start")?;
    Ok(task.map(|message| {
        let result = message
            .downcast_ref::<Message>()
            .and_then(|message| match message {
                Message::ActionDone { result, .. } => Some(result.clone()),
                _ => None,
            })
            .unwrap_or(Err(
                "Audio command did not acknowledge completion".to_owned()
            ));
        CommandOutcome { message, result }
    }))
}

fn media_card(player: Option<&PlayerInfo>, busy: bool) -> Element<'static, PluginMsg> {
    let player = player.cloned();
    let bg = if player.is_some() {
        OXITHEME.primary_bg
    } else {
        OXITHEME.mantle_hover
    };
    let build = move |bg| -> Element<'static, PluginMsg> {
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
                .push(text(title).size(OXITHEME.font_md).style(text_primary))
                .push(text(subtitle).size(OXITHEME.font_sm).style(text_muted))
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
                                .size(OXITHEME.font_sm)
                                .style(text_muted),
                        )
                        .spacing(OXITHEME.padding_sm)
                        .align_y(Alignment::Center),
                )
                .push(
                    oxi_slider::slider(0u8..=100u8, player.volume, |value| {
                        msg(Message::PlayerVolumeChanged(value))
                    })
                    .width(Length::Fill),
                )
                .spacing(OXITHEME.padding_sm)
                .width(Length::Fill);

            Row::new()
                .push(artwork_preview(player.art_url.as_deref()))
                .push(info)
                .spacing(OXITHEME.padding_sm)
                .align_y(Alignment::Center)
                .into()
        } else {
            Row::new()
                .push(artwork_preview(None))
                .push(
                    Column::new()
                        .push(
                            text("No active media stream")
                                .size(OXITHEME.font_md)
                                .style(text_primary),
                        )
                        .push(
                            text("Open music or video player to show MPRIS controls.")
                                .size(OXITHEME.font_sm)
                                .style(text_muted),
                        )
                        .spacing(OXITHEME.padding_xs)
                        .width(Length::Fill),
                )
                .spacing(OXITHEME.padding_sm)
                .align_y(Alignment::Center)
                .into()
        };
        card(body, bg).into()
    };
    let motion = oxiced::theme::snapshot::current_snapshot();
    if motion.reduced_motion || motion.animation_ms == 0 {
        build(bg)
    } else {
        AnimationBuilder::new(bg, build)
            .animation(Easing::EASE_OUT.with_duration(motion.duration()))
            .into()
    }
}

fn artwork_preview(art_url: Option<&str>) -> Element<'static, PluginMsg> {
    let content: Element<'static, PluginMsg> = if let Some(path) = art_url.and_then(local_art_path)
    {
        image(path)
            .width(ARTWORK_PREVIEW_SIZE)
            .height(ARTWORK_PREVIEW_SIZE)
            .content_fit(ContentFit::Cover)
            .border_radius(OXITHEME.border_radius)
            .into()
    } else {
        text("󰎈")
            .size(ARTWORK_ICON_SIZE)
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
                radius: OXITHEME.border_radius.into(),
                color: Color::TRANSPARENT,
                width: 0.0,
            },
            shadow: Shadow::default(),
            ..Default::default()
        })
        .width(ARTWORK_PREVIEW_SIZE)
        .height(ARTWORK_PREVIEW_SIZE)
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
        Message::OutputMuted,
    )
}

fn input_section(snapshot: &AudioSnapshot) -> Element<'static, PluginMsg> {
    device_section(
        "Input Device",
        &snapshot.inputs,
        snapshot.default_input.as_deref(),
        Message::SetDefaultInput,
        Message::InputVolumeChanged,
        Message::InputMuted,
    )
}

fn device_section(
    label: &'static str,
    devices: &[AudioDevice],
    default: Option<&str>,
    select_msg: fn(DeviceChoice) -> Message,
    volume_msg: fn(u8) -> Message,
    mute_msg: fn(bool) -> Message,
) -> Element<'static, PluginMsg> {
    if devices.is_empty() {
        return card(
            Column::new()
                .spacing(OXITHEME.padding_sm)
                .push(text(label).size(OXITHEME.font_md).style(text_primary))
                .push(
                    text("No devices available")
                        .size(OXITHEME.font_md)
                        .style(text_muted),
                )
                .into(),
            OXITHEME.mantle_hover,
        )
        .into();
    }
    let choices = devices.iter().map(AudioDevice::choice).collect::<Vec<_>>();
    let selected = selected_device(devices, default).map(AudioDevice::choice);
    let device = selected_device(devices, default);
    let volume = device.map(|device| device.volume).unwrap_or_default();
    let picker = oxi_picklist::pick_list(choices, selected, move |choice| msg(select_msg(choice)))
        .width(Length::Fill);

    let body = Column::new()
        .push(text(label).size(OXITHEME.font_md).style(text_primary))
        .push(picker)
        .push(
            Row::new()
                .push(text("Mute").size(OXITHEME.font_md).style(text_primary))
                .push(Space::new().width(Length::Fill))
                .push(
                    oxi_toggler::toggler(device.is_some_and(|d| d.muted))
                        .on_toggle_maybe(device.map(|_| move |muted| msg(mute_msg(muted)))),
                )
                .align_y(Alignment::Center),
        )
        .push(
            Row::new()
                .push(
                    oxi_slider::slider(0u8..=MAX_DEVICE_VOLUME, volume, move |value| {
                        msg(volume_msg(value))
                    })
                    .width(Length::Fill),
                )
                .push(
                    text(format!("{volume}%"))
                        .size(OXITHEME.font_sm)
                        .style(text_muted),
                )
                .spacing(OXITHEME.padding_sm)
                .align_y(Alignment::Center),
        )
        .spacing(OXITHEME.padding_sm)
        .width(Length::Fill);
    card(body.into(), OXITHEME.mantle_hover).into()
}

fn media_button(
    label: &'static str,
    message: Message,
    busy: bool,
) -> iced::widget::Button<'static, PluginMsg> {
    let button = oxi_button::button(
        text(label).size(OXITHEME.font_md),
        oxi_button::ButtonVariant::PrimaryBg,
    )
    .padding([OXITHEME.padding_xs, OXITHEME.padding_sm]);
    if busy {
        button
    } else {
        button.on_press(msg(message))
    }
}

fn card<'a>(content: Element<'a, PluginMsg>, bg: Color) -> Container<'a, PluginMsg> {
    oxi_plugin::compact_card(content, bg)
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

fn read_backend(global: &Table) -> Backend {
    match global
        .get("audio")
        .and_then(|v| v.as_table())
        .and_then(|t| t.get("backend"))
        .and_then(|v| v.as_str())
    {
        Some("pactl") => Backend::Pactl,
        _ => Backend::Native,
    }
}

const _: fn() = || {
    fn assert_stream<S: Stream<Item = PluginMsg> + Send + 'static>(_: &S) {}
    let _ = |s: &iced::futures::stream::BoxStream<'static, PluginMsg>| assert_stream(s);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn popup_height_grows_for_device_controls_and_wrapped_media_and_shrinks_when_removed() {
        let (owner, _) = model(Table::new());
        let height = || popup_metrics(owner.clone()).popup_size.unwrap().1;
        let empty = height();
        with_model_write::<Model, _>(&owner, |model| {
            let device = AudioDevice {
                name: "fixture".into(),
                label: "Fixture".into(),
                volume: 50,
                muted: false,
            };
            model.snapshot.outputs.push(device.clone());
            model.snapshot.inputs.push(device);
        });
        let devices = height();
        assert!(devices > empty);
        with_model_write::<Model, _>(&owner, |model| {
            model.snapshot.player = Some(PlayerInfo {
                service: "fixture".into(),
                identity: "Player".into(),
                title: "Long song title ".repeat(12),
                artist: "Artist".into(),
                status: "Playing".into(),
                volume: 50,
                art_url: None,
            })
        });
        assert!(height() > devices);
        assert!(height() <= 900);
        with_model_write::<Model, _>(&owner, |model| model.snapshot = AudioSnapshot::default());
        assert_eq!(height(), empty);
    }

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
        assert_eq!(metadata().popup_size, Some(POPUP_SIZE));
        assert_eq!(view(plugin_model.clone()).unwrap().len(), 1);
        assert_eq!(popup_view(plugin_model.clone()).unwrap().len(), 1);

        let task = update(
            plugin_model.clone(),
            msg(Message::Devices(Err("audio failed".to_owned()))),
        );
        assert!(task.is_none());
        assert_eq!(errors(plugin_model.clone()), vec!["audio failed"]);
        assert!(errors(plugin_model).is_empty());
    }

    #[test]
    fn device_events_preserve_media_and_disconnect_retains_last_valid_devices() {
        let (model, _) = model(Table::new());
        with_model_write::<Model, _>(&model, |model| {
            model.snapshot.player = Some(PlayerInfo {
                service: "player".to_owned(),
                identity: "Player".to_owned(),
                title: "Song".to_owned(),
                artist: "Artist".to_owned(),
                status: "Playing".to_owned(),
                volume: 50,
                art_url: None,
            });
        });
        let devices = DeviceSnapshot {
            outputs: vec![AudioDevice {
                name: "sink".to_owned(),
                label: "Speaker".to_owned(),
                volume: 65,
                muted: false,
            }],
            default_output: Some("sink".to_owned()),
            ..Default::default()
        };
        update(model.clone(), msg(Message::Devices(Ok(devices.clone()))));
        update(
            model.clone(),
            msg(Message::Devices(Err("offline".to_owned()))),
        );
        with_model_read::<Model, _>(&model, |model| {
            assert_eq!(model.snapshot.player.as_ref().unwrap().title, "Song");
            assert_eq!(model.snapshot.outputs, devices.outputs);
            assert_eq!(model.device_error.as_deref(), Some("offline"));
        })
        .unwrap();
        update(model.clone(), msg(Message::Devices(Ok(devices))));
        with_model_read::<Model, _>(&model, |model| assert!(model.device_error.is_none())).unwrap();
    }

    #[test]
    fn stale_acknowledgements_and_media_queries_cannot_overwrite_newer_actions() {
        let (model, _) = model(Table::new());
        with_model_write::<Model, _>(&model, |model| {
            model.action_generation = 2;
            model.device_action_generation = 2;
            model.pending = Some("new action".to_owned());
            model.media_generation = 3;
            model.media_refreshing = Some(1);
            model.snapshot.player = Some(PlayerInfo {
                service: "player".to_owned(),
                identity: "Player".to_owned(),
                title: "New song".to_owned(),
                artist: "Artist".to_owned(),
                status: "Playing".to_owned(),
                volume: 80,
                art_url: None,
            });
        });
        assert!(
            update(
                model.clone(),
                msg(Message::ActionDone {
                    result: Err("stale error".to_owned()),
                    target: ActionTarget::Devices,
                    generation: 1
                })
            )
            .is_none()
        );
        update(
            model.clone(),
            msg(Message::Media {
                result: Ok(None),
                generation: 1,
            }),
        );
        with_model_read::<Model, _>(&model, |model| {
            assert_eq!(model.pending.as_deref(), Some("new action"));
            assert_eq!(model.snapshot.player.as_ref().unwrap().volume, 80);
            assert!(model.errors.is_empty());
            assert!(model.media_refreshing.is_none());
        })
        .unwrap();
        update(
            model.clone(),
            msg(Message::ActionDone {
                result: Ok(()),
                target: ActionTarget::NativeDevices,
                generation: 2,
            }),
        );
        with_model_read::<Model, _>(&model, |model| assert!(model.pending.is_none())).unwrap();
    }

    #[test]
    fn overlapping_domains_refresh_without_clearing_the_other_pending_action() {
        for (first, second, result) in [
            (ActionTarget::Media, ActionTarget::Devices, Ok(())),
            (ActionTarget::Devices, ActionTarget::Media, Ok(())),
            (
                ActionTarget::NativeDevices,
                ActionTarget::Media,
                Err("offline".to_owned()),
            ),
        ] {
            let (model, _) = model(Table::new());
            with_model_write::<Model, _>(&model, |model| {
                drop(run_action(model, "first".to_owned(), first, async {
                    Ok(())
                }));
                drop(run_action(model, "second".to_owned(), second, async {
                    Ok(())
                }));
            });
            assert!(
                update(
                    model.clone(),
                    msg(Message::ActionDone {
                        result: result.clone(),
                        target: first,
                        generation: 1,
                    }),
                )
                .is_some()
            );
            with_model_read::<Model, _>(&model, |model| {
                assert_eq!(model.pending.as_deref(), Some("second"));
                assert_eq!(model.errors, result.err().into_iter().collect::<Vec<_>>());
            })
            .unwrap();
            update(
                model.clone(),
                msg(Message::ActionDone {
                    result: Ok(()),
                    target: second,
                    generation: 2,
                }),
            );
            with_model_read::<Model, _>(&model, |model| {
                assert!(model.pending.is_none());
            })
            .unwrap();
        }
    }

    fn collect_messages(task: Option<Task<PluginMsg>>) -> Vec<PluginMsg> {
        let Some(mut stream) =
            task.and_then(|task| iced_runtime::task::into_stream(task.collect()))
        else {
            return Vec::new();
        };
        iced::futures::executor::block_on(async {
            use iced::futures::StreamExt;
            match stream.next().await {
                Some(iced_runtime::Action::Output(messages)) => messages,
                Some(_) => panic!("unexpected widget operation"),
                None => Vec::new(),
            }
        })
    }

    #[test]
    fn osd_baselines_are_quiet_and_successful_actions_retain_source_without_echo() {
        let (model, _) = model(Table::new());
        let mut devices = DeviceSnapshot {
            outputs: vec![AudioDevice {
                name: "sink".to_owned(),
                label: "Speaker".to_owned(),
                volume: 50,
                muted: false,
            }],
            default_output: Some("sink".to_owned()),
            ..Default::default()
        };
        assert!(update(model.clone(), msg(Message::Devices(Ok(devices.clone())))).is_none());
        devices.outputs[0].volume = 60;
        let requests = collect_messages(update(
            model.clone(),
            msg(Message::Devices(Ok(devices.clone()))),
        ));
        assert_eq!(
            requests[0]
                .downcast_ref::<String>()
                .and_then(|value| HostOsdRequest::parse_host_request(value)),
            Some(HostOsdRequest::new("volume").follow_focus())
        );
        with_model_write::<Model, _>(&model, |model| {
            model.device_action_generation = 4;
            model.action_generation = 4;
            model.device_action_active = true;
            model.device_osd_topic = Some("volume");
            model.snapshot.outputs[0].volume = 70;
        });
        let requests = collect_messages(update(
            model.clone(),
            msg(Message::ActionDone {
                result: Ok(()),
                target: ActionTarget::NativeDevices,
                generation: 4,
            }),
        ));
        assert_eq!(
            requests[0]
                .downcast_ref::<String>()
                .and_then(|value| HostOsdRequest::parse_host_request(value)),
            Some(HostOsdRequest::new("volume"))
        );
        devices.outputs[0].volume = 70;
        assert!(update(model.clone(), msg(Message::Devices(Ok(devices.clone())))).is_none());
        with_model_write::<Model, _>(&model, |model| {
            model.device_action_generation = 5;
            model.device_action_active = true;
            model.device_osd_topic = Some("volume");
        });
        let messages = collect_messages(update(
            model.clone(),
            msg(Message::ActionDone {
                result: Err("offline".to_owned()),
                target: ActionTarget::NativeDevices,
                generation: 5,
            }),
        ));
        assert!(messages.iter().all(|message| {
            message
                .downcast_ref::<String>()
                .and_then(|value| HostOsdRequest::parse_host_request(value))
                .is_none()
        }));
        assert!(osd_view(model.clone(), "volume").is_ok());
        assert!(osd_view(model, "unknown").is_err());
    }

    #[test]
    fn commands_validate_before_execution_and_return_actual_backend_errors() {
        let (model, _) = model(Table::new());
        assert!(command(model.clone(), "volume".to_owned(), vec!["+5".to_owned()]).is_err());
        update(
            model.clone(),
            msg(Message::Devices(Ok(DeviceSnapshot {
                outputs: vec![AudioDevice {
                    name: "sink".to_owned(),
                    label: "Speaker".to_owned(),
                    volume: 50,
                    muted: false,
                }],
                default_output: Some("sink".to_owned()),
                ..Default::default()
            }))),
        );
        assert!(command(model.clone(), "volume".to_owned(), vec!["151".to_owned()]).is_err());
        assert!(command(model.clone(), "mute".to_owned(), vec!["invalid".to_owned()]).is_err());
        assert!(command(model.clone(), "play-pause".to_owned(), vec![]).is_err());
        let task = command(model.clone(), "volume".to_owned(), vec!["+5".to_owned()]).unwrap();
        let mut stream = iced_runtime::task::into_stream(task.collect()).unwrap();
        let outcomes = iced::futures::executor::block_on(async {
            use iced::futures::StreamExt;
            match stream.next().await {
                Some(iced_runtime::Action::Output(outcomes)) => outcomes,
                _ => panic!("missing command outcome"),
            }
        });
        assert!(outcomes[0].result.is_err());
        assert!(outcomes[0].message.downcast_ref::<Message>().is_some());
    }
}
