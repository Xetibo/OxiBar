//! Bluetooth plugin backed by `bluetoothctl`.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use iced::{
    Alignment, Background, Border, Color, Element, Length, Shadow, Task,
    futures::Stream,
    stream,
    widget::{Column, Container, Row, Space, button, scrollable, text},
};
use iced_anim::{AnimationBuilder, Motion};
use oxibar_plugin_api::{
    ABI_VERSION, HOST_REQUEST_CLOSE_MODAL, HOST_REQUEST_OPEN_MODAL, HOST_REQUEST_TOGGLE_POPUP,
    PluginAvailability, PluginMetadata, PluginModel, PluginMsg, PluginStream, drain_model_errors,
    plugin_model, toml::Table, with_model_read, with_model_write,
};
use oxiced::{
    theme::theme_impl::OXITHEME,
    widgets::{
        oxi_button, oxi_plugin, oxi_plugin::text_muted, oxi_plugin::text_primary, oxi_text_input,
    },
};

mod system;

use system::{
    BluetoothAction, BluetoothDevice, PairOutcome, Snapshot, run_bluetooth_action, scan_devices,
};

const DEFAULT_REFRESH_SECONDS: u64 = 20;
const ICON: &str = "󰂯";
const POPUP_SIZE: (u32, u32) = (460, 420);

static REFRESH_INTERVAL: OnceLock<Duration> = OnceLock::new();

#[derive(Debug)]
pub struct Model {
    connected: Vec<BluetoothDevice>,
    available: Vec<BluetoothDevice>,
    hovered: Option<String>,
    pending: Option<String>,
    pairing: Option<PairingState>,
    errors: Vec<String>,
}

impl Model {
    fn new(global_config: Table) -> Self {
        let seconds = global_config
            .get("bluetooth")
            .and_then(|v| v.as_table())
            .and_then(|t| t.get("refresh_seconds"))
            .and_then(|v| v.as_integer())
            .filter(|n| *n > 0)
            .map(|n| n as u64)
            .unwrap_or(DEFAULT_REFRESH_SECONDS);
        let _ = REFRESH_INTERVAL.set(Duration::from_secs(seconds));
        Self {
            connected: Vec::new(),
            available: Vec::new(),
            hovered: None,
            pending: None,
            pairing: None,
            errors: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
struct PairingState {
    device: BluetoothDevice,
    code: String,
}

#[derive(Clone, Debug)]
enum Message {
    TogglePopup,
    Refresh,
    Scan,
    ScanResult(Result<Snapshot, String>),
    Hover(Option<String>),
    Pair(String),
    Disconnect(String),
    ActionDone {
        mac: Option<String>,
        result: Result<PairOutcome, String>,
    },
    CodeChanged(String),
    SubmitCode,
    CancelCode,
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
    "Bluetooth"
}

#[unsafe(no_mangle)]
pub extern "Rust" fn availability(_global_config: Table) -> PluginAvailability {
    if system::has_bluetooth_controller() {
        PluginAvailability::Available
    } else {
        PluginAvailability::Unavailable("no bluetooth controller found")
    }
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
        Message::Refresh => Some(Task::perform(async { scan_devices(false) }, |result| {
            msg(Message::ScanResult(result))
        })),
        Message::Scan => {
            model.pending = Some("Scanning".to_owned());
            Some(Task::perform(async { scan_devices(true) }, |result| {
                msg(Message::ScanResult(result))
            }))
        }
        Message::ScanResult(result) => {
            model.pending = None;
            match result {
                Ok(snapshot) => {
                    model.connected = snapshot.connected;
                    model.available = snapshot.available;
                }
                Err(error) => model.errors.push(error),
            }
            None
        }
        Message::Hover(key) => {
            model.hovered = key;
            None
        }
        Message::Pair(mac) => {
            model.pending = Some(format!("Pairing {mac}"));
            Some(run_action(BluetoothAction::Pair { mac, code: None }))
        }
        Message::Disconnect(mac) => {
            model.pending = Some(format!("Disconnecting {mac}"));
            Some(run_action(BluetoothAction::Disconnect { mac }))
        }
        Message::ActionDone { mac, result } => {
            model.pending = None;
            match result {
                Ok(PairOutcome::Done) => Some(Task::done(msg(Message::Refresh))),
                Ok(PairOutcome::NeedsCode) => {
                    let mac = mac?;
                    if let Some(device) = find_device(model, &mac) {
                        model.pairing = Some(PairingState {
                            device,
                            code: String::new(),
                        });
                        Some(Task::done(
                            Arc::new(HOST_REQUEST_OPEN_MODAL.to_owned()) as PluginMsg
                        ))
                    } else {
                        model
                            .errors
                            .push(format!("bluetooth: unknown device {mac}"));
                        None
                    }
                }
                Err(error) => {
                    model.errors.push(error);
                    None
                }
            }
        }
        Message::CodeChanged(code) => {
            if let Some(pairing) = model.pairing.as_mut() {
                pairing.code = code;
            }
            None
        }
        Message::SubmitCode => {
            let pairing = model.pairing.clone()?;
            model.pending = Some(format!("Pairing {}", pairing.device.name));
            Some(run_action(BluetoothAction::Pair {
                mac: pairing.device.mac,
                code: Some(pairing.code),
            }))
        }
        Message::CancelCode => {
            model.pairing = None;
            Some(Task::done(
                Arc::new(HOST_REQUEST_CLOSE_MODAL.to_owned()) as PluginMsg
            ))
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
        let count = model.connected.len();
        let label = if count == 0 {
            ICON.to_owned()
        } else {
            format!("{ICON} {count}")
        };
        vec![bar_button(label).into()]
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
        let scan = oxi_button::button(
            text(if model.pending.is_some() {
                "Working..."
            } else {
                "Scan"
            })
            .size(OXITHEME.font_md),
            oxi_button::ButtonVariant::SecondaryBg,
        )
        .on_press(msg(Message::Scan))
        .padding([OXITHEME.padding_xs, OXITHEME.padding_sm]);
        content = content.push(
            Row::new()
                .push(section_title("Bluetooth"))
                .push(Space::new().width(Length::Fill))
                .push(scan)
                .align_y(Alignment::Center),
        );

        content = content.push(section_title("Connected Devices"));
        if model.connected.is_empty() {
            content = content.push(empty_text("No connected devices"));
        } else {
            for device in &model.connected {
                content = content.push(device_card(
                    device,
                    true,
                    model.pending.is_some(),
                    model.hovered.as_deref() == Some(device.mac.as_str()),
                ));
            }
        }

        content = content.push(section_title("Available Devices"));
        if model.available.is_empty() {
            content = content.push(empty_text("No pairable devices found"));
        } else {
            for device in &model.available {
                content = content.push(device_card(
                    device,
                    false,
                    model.pending.is_some(),
                    model.hovered.as_deref() == Some(device.mac.as_str()),
                ));
            }
        }

        vec![scrollable(content).height(Length::Fill).into()]
    })
}

#[unsafe(no_mangle)]
pub extern "Rust" fn modal_view(
    model: PluginModel,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    with_model_read::<Model, _>(&model, |model| {
        let Some(pairing) = model.pairing.clone() else {
            return vec![
                Column::new()
                    .push(section_title("No pairing request"))
                    .into(),
            ];
        };
        let input = oxi_text_input::text_input("PIN / pairing code", &pairing.code, |value| {
            msg(Message::CodeChanged(value))
        })
        .width(Length::Fill);
        let submit = oxi_button::button(
            text("Pair").size(OXITHEME.font_md),
            oxi_button::ButtonVariant::Primary,
        )
        .on_press(msg(Message::SubmitCode))
        .padding([OXITHEME.padding_sm, OXITHEME.padding_md]);
        let cancel = oxi_button::button(
            text("Cancel").size(OXITHEME.font_md),
            oxi_button::ButtonVariant::SecondaryBg,
        )
        .on_press(msg(Message::CancelCode))
        .padding([OXITHEME.padding_sm, OXITHEME.padding_md]);
        vec![
            Column::new()
                .push(section_title("Bluetooth Pairing"))
                .push(
                    text(pairing.device.name)
                        .size(OXITHEME.font_md)
                        .style(text_primary),
                )
                .push(
                    text("Enter the PIN or pairing code shown by the device.")
                        .size(OXITHEME.font_sm)
                        .style(text_muted),
                )
                .push(input)
                .push(
                    Row::new()
                        .push(Space::new().width(Length::Fill))
                        .push(cancel)
                        .push(submit)
                        .spacing(OXITHEME.padding_sm)
                        .align_y(Alignment::Center),
                )
                .spacing(OXITHEME.padding_md)
                .width(Length::Fill)
                .into(),
        ]
    })
}

#[unsafe(no_mangle)]
pub extern "Rust" fn subscription() -> *mut PluginStream {
    let interval = *REFRESH_INTERVAL
        .get()
        .unwrap_or(&Duration::from_secs(DEFAULT_REFRESH_SECONDS));
    let s = stream::channel(
        16,
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

fn bar_button(label: String) -> button::Button<'static, PluginMsg> {
    oxi_plugin::bar_button(
        text(label)
            .size(OXITHEME.font_md)
            .align_y(Alignment::Center)
            .align_x(Alignment::Center),
    )
    .on_press(msg(Message::TogglePopup))
}

fn device_card(
    device: &BluetoothDevice,
    connected: bool,
    busy: bool,
    hovered: bool,
) -> Element<'static, PluginMsg> {
    let mac = device.mac.clone();
    let key = device.mac.clone();
    let label = device.name.clone();
    let icon = icon_for(device);
    let details = if connected {
        "Connected".to_owned()
    } else if device.paired {
        "Paired".to_owned()
    } else {
        "Pairable".to_owned()
    };
    let row: Element<'static, PluginMsg> = AnimationBuilder::new(
        if hovered {
            OXITHEME.primary_bg_hover
        } else {
            OXITHEME.mantle_hover
        },
        move |bg| {
            let mut card: button::Button<'_, PluginMsg> = button(
                Row::new()
                    .push(text(icon).size(OXITHEME.font_lg).style(text_primary))
                    .push(
                        Column::new()
                            .push(
                                text(label.clone())
                                    .size(OXITHEME.font_md)
                                    .style(text_primary),
                            )
                            .push(
                                text(details.clone())
                                    .size(OXITHEME.font_sm)
                                    .style(text_muted),
                            )
                            .spacing(OXITHEME.padding_xs)
                            .width(Length::Fill),
                    )
                    .spacing(OXITHEME.padding_sm)
                    .align_y(Alignment::Center),
            )
            .style(move |_, status| card_button_style(status, bg))
            .padding([OXITHEME.padding_xs, OXITHEME.padding_sm])
            .width(Length::Fill);
            if !busy {
                card = if connected {
                    card.on_press(msg(Message::Disconnect(mac.clone())))
                } else {
                    card.on_press(msg(Message::Pair(mac.clone())))
                };
            }
            Container::new(card)
                .style(move |_| iced::widget::container::Style {
                    background: Some(Background::Color(bg)),
                    border: Border {
                        radius: OXITHEME.border_radius.into(),
                        color: Color::TRANSPARENT,
                        width: 0.0,
                    },
                    shadow: Shadow::default(),
                    ..Default::default()
                })
                .padding(OXITHEME.padding_xs)
                .width(Length::Fill)
                .into()
        },
    )
    .animation(Motion::SMOOTH)
    .into();

    iced::widget::mouse_area(row)
        .on_enter(msg(Message::Hover(Some(key))))
        .on_exit(msg(Message::Hover(None)))
        .into()
}

fn section_title(label: &'static str) -> Element<'static, PluginMsg> {
    text(label)
        .size(OXITHEME.font_md)
        .style(|_| iced::widget::text::Style {
            color: Some(OXITHEME.primary),
        })
        .into()
}

fn empty_text(label: &'static str) -> Element<'static, PluginMsg> {
    text(label).size(OXITHEME.font_md).style(text_muted).into()
}

fn card_button_style(status: button::Status, bg: Color) -> button::Style {
    let hover = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: Some(Background::Color(if hover {
            OXITHEME.primary_bg_hover
        } else {
            bg
        })),
        text_color: OXITHEME.text,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: OXITHEME.border_radius.into(),
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

fn icon_for(device: &BluetoothDevice) -> &'static str {
    if device.icon.contains("audio") || device.icon.contains("headset") {
        "󰥰"
    } else if device.icon.contains("input") || device.icon.contains("keyboard") {
        "󰌌"
    } else if device.icon.contains("phone") {
        "󰏲"
    } else {
        ICON
    }
}

fn find_device(model: &Model, mac: &str) -> Option<BluetoothDevice> {
    model
        .connected
        .iter()
        .chain(model.available.iter())
        .find(|device| device.mac == mac)
        .cloned()
}

fn run_action(action: BluetoothAction) -> Task<PluginMsg> {
    let mac = match &action {
        BluetoothAction::Pair { mac, .. } | BluetoothAction::Disconnect { mac } => {
            Some(mac.clone())
        }
    };
    Task::perform(async move { run_bluetooth_action(action) }, move |result| {
        msg(Message::ActionDone {
            mac: mac.clone(),
            result,
        })
    })
}

const _: fn() = || {
    fn assert_stream<S: Stream<Item = PluginMsg> + Send + 'static>(_: &S) {}
    let _ = |s: &iced::futures::stream::BoxStream<'static, PluginMsg>| assert_stream(s);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_for_detects_audio_devices() {
        let usable = BluetoothDevice {
            mac: "AA:BB:CC:DD:EE:FF".to_owned(),
            name: "Headphones".to_owned(),
            icon: "audio-card".to_owned(),
            paired: false,
        };
        assert_eq!(icon_for(&usable), "󰥰");
    }

    #[test]
    fn model_views_and_error_drain_are_deterministic() {
        let (plugin_model, init_task) = model(Table::new());
        assert!(init_task.is_some());
        assert_eq!(name(), "Bluetooth");
        assert_eq!(abi_version(), ABI_VERSION);
        assert_eq!(metadata().popup_size, Some(POPUP_SIZE));
        assert_eq!(view(plugin_model.clone()).unwrap().len(), 1);
        assert_eq!(popup_view(plugin_model.clone()).unwrap().len(), 1);
        assert_eq!(modal_view(plugin_model.clone()).unwrap().len(), 1);

        let task = update(
            plugin_model.clone(),
            msg(Message::ActionDone {
                mac: None,
                result: Err("bluetooth failed".to_owned()),
            }),
        );
        assert!(task.is_none());
        assert_eq!(errors(plugin_model.clone()), vec!["bluetooth failed"]);
        assert!(errors(plugin_model).is_empty());
    }
}
