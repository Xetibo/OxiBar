//! Network plugin backed by NetworkManager's `nmcli`.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use iced::{
    Alignment, Background, Border, Color, Element, Length, Shadow, Task,
    futures::{Stream, StreamExt},
    widget::{Column, Container, Row, Space, button, scrollable},
};
use iced_anim::{AnimationBuilder, transition::Easing};
use oxibar_plugin_api::{
    ABI_VERSION, HOST_REQUEST_CLOSE_MODAL, HOST_REQUEST_OPEN_MODAL, HOST_REQUEST_TOGGLE_POPUP,
    PluginMetadata, PluginModel, PluginMsg, PluginStream, drain_model_errors, plugin_model,
    toml::Table, with_model_read, with_model_write,
};
use oxiced::widgets::oxi_plugin::text;
use oxiced::{
    theme::theme_impl::OXITHEME,
    widgets::{
        oxi_button, oxi_plugin, oxi_plugin::text_muted, oxi_plugin::text_primary, oxi_text_input,
    },
};

mod system;

use system::{
    ActiveNetwork, NetworkAction, SavedConnection, Snapshot, WifiNetwork, run_network_action,
    scan_networks,
};

const DEFAULT_SCAN_SECONDS: u64 = 15;

#[unsafe(no_mangle)]
pub extern "Rust" fn settings_schema() -> Vec<oxibar_plugin_api::SettingDescriptor> {
    use oxibar_plugin_api::{ApplyMode, SettingDescriptor, SettingKind, toml::Value};
    vec![SettingDescriptor::new(
        "network.scan_seconds",
        "Network scan interval",
        SettingKind::Integer {
            min: 1,
            max: i64::MAX,
        },
        Some(Value::Integer(DEFAULT_SCAN_SECONDS as i64)),
        ApplyMode::Restart,
    )]
}
const WIFI_ICON: &str = "󰤨";
const POPUP_SIZE: (u32, u32) = (460, 420);

static SCAN_INTERVAL: OnceLock<Duration> = OnceLock::new();

#[derive(Debug)]
pub struct Model {
    connected: Vec<ActiveNetwork>,
    saved: Vec<SavedConnection>,
    wifi: Vec<WifiNetwork>,
    wifi_enabled: Option<bool>,
    scanning: bool,
    editing: Option<EditState>,
    hovered_entry: Option<String>,
    saved_expanded: bool,
    pending: Option<String>,
    action_generation: u64,
    edit_generation: u64,
    errors: Vec<String>,
}

impl Model {
    fn new(global_config: Table) -> Self {
        let interval = read_scan_interval(&global_config);
        let _ = SCAN_INTERVAL.set(Duration::from_secs(interval));
        Self {
            connected: Vec::new(),
            saved: Vec::new(),
            wifi: Vec::new(),
            wifi_enabled: None,
            scanning: false,
            editing: None,
            hovered_entry: None,
            saved_expanded: false,
            pending: None,
            action_generation: 0,
            edit_generation: 0,
            errors: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
struct EditState {
    label: String,
    target: EditTarget,
    password: String,
}

#[derive(Clone, Debug)]
enum EditTarget {
    Wifi { ssid: String },
    Connection { uuid: String },
}

#[derive(Clone, Debug)]
enum Message {
    TogglePopup,
    Refresh,
    WifiPower(bool),
    ScanResult(Result<Snapshot, String>),
    ConnectWifi(String),
    ConnectConnection(String),
    Disconnect(String),
    HoverEntry(Option<String>),
    ToggleSavedConnections,
    EditWifi(String),
    EditConnection {
        uuid: String,
        name: String,
    },
    PasswordChanged(String),
    SaveEdit,
    CancelEdit,
    ActionDone {
        result: Result<String, String>,
        close_modal: bool,
        generation: u64,
        edit_generation: u64,
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
    "Network"
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
        Message::Refresh => {
            if model.scanning {
                return None;
            }
            model.scanning = true;
            Some(Task::perform(
                oxibar_plugin_api::worker::run(scan_networks),
                |result| msg(Message::ScanResult(result)),
            ))
        }
        Message::WifiPower(enabled) => {
            if model.wifi_enabled.is_none() || model.pending.is_some() {
                return None;
            }
            run_action(model, NetworkAction::WifiPower(enabled), false)
        }
        Message::ScanResult(result) => {
            model.scanning = false;
            match result {
                Ok(snapshot) => {
                    model.connected = snapshot.connected;
                    model.saved = snapshot.saved;
                    model.wifi = snapshot.wifi;
                    model.wifi_enabled = snapshot.wifi_enabled;
                }
                Err(error) => model.errors.push(error),
            }
            None
        }
        Message::ConnectWifi(ssid) => run_action(model, NetworkAction::ConnectWifi { ssid }, false),
        Message::ConnectConnection(uuid) => {
            run_action(model, NetworkAction::ConnectConnection { uuid }, false)
        }
        Message::Disconnect(uuid) => run_action(model, NetworkAction::Disconnect { uuid }, false),
        Message::HoverEntry(entry) => {
            model.hovered_entry = entry;
            None
        }
        Message::ToggleSavedConnections => {
            model.saved_expanded = !model.saved_expanded;
            None
        }
        Message::EditWifi(ssid) => {
            model.edit_generation = model.edit_generation.wrapping_add(1);
            model.editing = Some(EditState {
                label: ssid.clone(),
                target: EditTarget::Wifi { ssid },
                password: String::new(),
            });
            Some(Task::done(
                Arc::new(HOST_REQUEST_OPEN_MODAL.to_owned()) as PluginMsg
            ))
        }
        Message::EditConnection { uuid, name } => {
            model.edit_generation = model.edit_generation.wrapping_add(1);
            model.editing = Some(EditState {
                label: name,
                target: EditTarget::Connection { uuid },
                password: String::new(),
            });
            Some(Task::done(
                Arc::new(HOST_REQUEST_OPEN_MODAL.to_owned()) as PluginMsg
            ))
        }
        Message::PasswordChanged(password) => {
            if let Some(editing) = model.editing.as_mut() {
                editing.password = password;
            }
            None
        }
        Message::SaveEdit => {
            let editing = model.editing.clone()?;
            let action = match editing.target {
                EditTarget::Wifi { ssid } => NetworkAction::SaveWifi {
                    ssid,
                    password: editing.password,
                },
                EditTarget::Connection { uuid } => NetworkAction::SaveConnection {
                    uuid,
                    password: editing.password,
                },
            };
            run_action(model, action, true)
        }
        Message::CancelEdit => {
            model.edit_generation = model.edit_generation.wrapping_add(1);
            model.editing = None;
            Some(Task::done(
                Arc::new(HOST_REQUEST_CLOSE_MODAL.to_owned()) as PluginMsg
            ))
        }
        Message::ActionDone {
            result,
            close_modal,
            generation,
            edit_generation,
        } => {
            if generation != model.action_generation {
                return None;
            }
            model.pending = None;
            match result {
                Ok(_) if close_modal && edit_generation == model.edit_generation => {
                    model.editing = None;
                    let close: PluginMsg = Arc::new(HOST_REQUEST_CLOSE_MODAL.to_owned());
                    Some(Task::batch(vec![
                        Task::done(close),
                        Task::done(msg(Message::Refresh)),
                    ]))
                }
                Ok(_) => Some(Task::done(msg(Message::Refresh))),
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
pub extern "Rust" fn command(
    model: PluginModel,
    command: String,
    arguments: Vec<String>,
) -> Result<Task<oxibar_plugin_api::CommandOutcome>, String> {
    let message = match (command.as_str(), arguments.as_slice()) {
        ("popup", []) => Message::TogglePopup,
        ("refresh", []) => Message::Refresh,
        ("edit", [uuid]) => with_model_read::<Model, _>(&model, |model| {
            model
                .saved
                .iter()
                .find(|connection| &connection.uuid == uuid)
                .map(|connection| Message::EditConnection {
                    uuid: connection.uuid.clone(),
                    name: connection.name.clone(),
                })
                .or_else(|| {
                    model
                        .connected
                        .iter()
                        .find(|connection| &connection.uuid == uuid)
                        .map(|connection| Message::EditConnection {
                            uuid: connection.uuid.clone(),
                            name: connection.name.clone(),
                        })
                })
        })
        .map_err(|error| error.to_string())?
        .ok_or("Unknown cached network connection")?,
        _ => return Err("Use Network popup, refresh, or edit <cached connection UUID>".to_owned()),
    };
    Ok(Task::done(oxibar_plugin_api::CommandOutcome {
        message: msg(message),
        result: Ok(()),
    }))
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
        let connected = model.connected.len();
        let content: Element<'static, PluginMsg> = if connected == 0 || !show_labels {
            text(WIFI_ICON)
                .size(OXITHEME.font_md)
                .align_y(Alignment::Center)
                .align_x(Alignment::Center)
                .into()
        } else {
            Row::new()
                .push(
                    text(WIFI_ICON)
                        .size(OXITHEME.font_md)
                        .align_y(Alignment::Center),
                )
                .push(
                    text(connected.to_string())
                        .size(OXITHEME.font_md)
                        .align_y(Alignment::Center),
                )
                .spacing(OXITHEME.padding_md)
                .align_y(Alignment::Center)
                .height(Length::Fill)
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
    with_model_read::<Model, _>(&model, |model| vec![network_controls(model, false)])
}

#[unsafe(no_mangle)]
pub extern "Rust" fn control_center_view(
    model: PluginModel,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    with_model_read::<Model, _>(&model, |model| vec![network_controls(model, true)])
}

#[unsafe(no_mangle)]
pub extern "Rust" fn subwidgets(_: PluginModel) -> Vec<oxibar_plugin_api::SubwidgetDescriptor> {
    vec![oxibar_plugin_api::SubwidgetDescriptor {
        id: "wifi".into(),
        title: "Wi-Fi".into(),
    }]
}
#[unsafe(no_mangle)]
pub extern "Rust" fn subwidget_view(
    model: PluginModel,
    id: &str,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    with_model_read::<Model, _>(&model, |model| {
        if id == "wifi" {
            vec![wifi_toggle(model)]
        } else {
            Vec::new()
        }
    })
}
fn wifi_toggle(model: &Model) -> Element<'static, PluginMsg> {
    let Some(enabled) = model.wifi_enabled else {
        return empty_text("No Wi-Fi adapter");
    };
    Column::new()
        .spacing(OXITHEME.padding_md)
        .push(
            oxiced::widgets::oxi_toggler::toggler::<PluginMsg>(enabled)
                .label("Wi-Fi")
                .on_toggle_maybe(
                    model
                        .pending
                        .is_none()
                        .then_some(|enabled| msg(Message::WifiPower(enabled))),
                ),
        )
        .push(
            text(
                model
                    .connected
                    .iter()
                    .find(|network| network.kind.contains("wireless"))
                    .map(|network| network.name.clone())
                    .unwrap_or_else(|| {
                        if enabled {
                            "Not connected"
                        } else {
                            "Wi-Fi off"
                        }
                        .into()
                    }),
            )
            .size(OXITHEME.font_md)
            .style(text_muted),
        )
        .into()
}
#[unsafe(no_mangle)]
pub extern "Rust" fn popup_metrics(model: PluginModel) -> oxibar_plugin_api::PluginPopupMetrics {
    let height = with_model_read::<Model, _>(&model, |model| {
        let line = OXITHEME.font_md * 1.3;
        let row = line + OXITHEME.font_sm * 1.3 + OXITHEME.padding_xs * 3.0 + OXITHEME.padding_sm;
        let count = model.connected.len()
            + model.wifi.len()
            + if model.saved_expanded {
                model.saved.len()
            } else {
                0
            };
        let empty = usize::from(model.connected.is_empty()) + usize::from(model.wifi.is_empty());
        let mut height = OXITHEME.padding_md * 2.0
            + 20.0
            + (line + OXITHEME.padding_md * 2.0) * 3.0
            + row * count as f32
            + (line + OXITHEME.padding_sm) * empty as f32;
        if model.wifi_enabled.is_some() {
            height += line * 2.0 + OXITHEME.padding_md + OXITHEME.padding_sm;
        }
        for name in model
            .connected
            .iter()
            .map(|item| &item.name)
            .chain(model.wifi.iter().map(|item| &item.ssid))
            .chain(
                model
                    .saved
                    .iter()
                    .filter(|_| model.saved_expanded)
                    .map(|item| &item.name),
            )
        {
            height += line * (name.chars().count().saturating_sub(1) / 32) as f32;
        }
        height.ceil() as u32
    })
    .unwrap_or(POPUP_SIZE.1)
    .min(900);
    oxibar_plugin_api::PluginPopupMetrics {
        popup_size: Some((POPUP_SIZE.0, height)),
        ..Default::default()
    }
}

fn network_controls(model: &Model, embedded: bool) -> Element<'static, PluginMsg> {
    let mut content = Column::new()
        .spacing(if embedded {
            OXITHEME.padding_lg
        } else {
            OXITHEME.padding_sm
        })
        .padding(if embedded {
            [0.0, 0.0]
        } else {
            [OXITHEME.padding_md, OXITHEME.padding_lg]
        })
        .width(Length::Fill);

    if model.wifi_enabled.is_some() {
        content = content.push(wifi_toggle(model));
    }
    let label = if model.pending.is_some() {
        "Working..."
    } else {
        "Refresh"
    };
    let refresh = if embedded {
        oxi_plugin::surface_button(text(label).size(OXITHEME.font_md), false)
    } else {
        oxi_button::button(
            text(label).size(OXITHEME.font_md),
            oxi_button::ButtonVariant::SecondaryBg,
        )
        .padding([OXITHEME.padding_xs, OXITHEME.padding_sm])
    }
    .on_press(msg(Message::Refresh));
    content = content.push(
        Row::new()
            .push(section_title("Connected"))
            .push(Space::new().width(Length::Fill))
            .push(refresh)
            .align_y(Alignment::Center),
    );

    if model.connected.is_empty() {
        content = content.push(empty_text("No active connections"));
    } else {
        for network in &model.connected {
            content = content.push(active_row(
                network,
                model.pending.is_some(),
                model.hovered_entry.as_deref() == Some(entry_key("active", &network.uuid).as_str()),
            ));
        }
    }

    content = content.push(section_title("Available Wi-Fi"));
    if model.wifi.is_empty() {
        content = content.push(empty_text("No Wi-Fi networks found"));
    } else {
        for network in &model.wifi {
            content = content.push(wifi_row(
                network,
                model.pending.is_some(),
                model.hovered_entry.as_deref() == Some(entry_key("wifi", &network.ssid).as_str()),
            ));
        }
    }

    content = content.push(saved_connections_accordion(model));
    if model.saved_expanded {
        for connection in &model.saved {
            content = content.push(saved_row(
                connection,
                model.pending.is_some(),
                model.hovered_entry.as_deref()
                    == Some(entry_key("saved", &connection.uuid).as_str()),
            ));
        }
    }

    if embedded {
        content.into()
    } else {
        scrollable(content).height(Length::Fill).into()
    }
}

#[unsafe(no_mangle)]
pub extern "Rust" fn modal_view(
    model: PluginModel,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    with_model_read::<Model, _>(&model, |model| {
        let Some(editing) = model.editing.clone() else {
            return vec![
                Column::new()
                    .push(section_title("No network selected"))
                    .push(cancel_button("Close"))
                    .spacing(OXITHEME.padding_md)
                    .into(),
            ];
        };

        let password = oxi_text_input::text_input("Password", &editing.password, |value| {
            msg(Message::PasswordChanged(value))
        })
        .secure(true)
        .width(Length::Fill);
        let save = oxi_button::button(
            text("Save").size(OXITHEME.font_md),
            oxi_button::ButtonVariant::Primary,
        )
        .on_press(msg(Message::SaveEdit))
        .padding([OXITHEME.padding_sm, OXITHEME.padding_md]);
        let cancel = cancel_button("Cancel");

        let body =
            Column::new()
                .push(text("Network Settings").size(OXITHEME.font_lg).style(|_| {
                    iced::widget::text::Style {
                        color: Some(OXITHEME.primary),
                    }
                }))
                .push(text(editing.label).size(OXITHEME.font_md).style(|_| {
                    iced::widget::text::Style {
                        color: Some(OXITHEME.text),
                    }
                }))
                .push(
                    text("Leave password empty to reuse the saved NetworkManager profile.")
                        .size(OXITHEME.font_sm)
                        .style(|_| iced::widget::text::Style {
                            color: Some(OXITHEME.text_muted),
                        }),
                )
                .push(password)
                .push(
                    Row::new()
                        .push(Space::new().width(Length::Fill))
                        .push(cancel)
                        .push(save)
                        .spacing(OXITHEME.padding_sm)
                        .align_y(Alignment::Center),
                )
                .spacing(OXITHEME.padding_md)
                .width(Length::Fill)
                .height(Length::Fill);

        vec![body.into()]
    })
}

#[unsafe(no_mangle)]
pub extern "Rust" fn modal_closed(model: PluginModel) -> Option<Task<PluginMsg>> {
    with_model_write::<Model, _>(&model, |model| {
        model.edit_generation = model.edit_generation.wrapping_add(1);
        model.editing = None;
    });
    None
}

#[unsafe(no_mangle)]
pub extern "Rust" fn subscription() -> *mut PluginStream {
    let interval = *SCAN_INTERVAL
        .get()
        .unwrap_or(&Duration::from_secs(DEFAULT_SCAN_SECONDS));
    let s = oxibar_plugin_api::worker::interval(interval).map(|()| msg(Message::Refresh));

    Box::into_raw(Box::new(s)) as *mut PluginStream
}

fn run_action(
    model: &mut Model,
    action: NetworkAction,
    close_modal: bool,
) -> Option<Task<PluginMsg>> {
    model.pending = Some(action.label());
    model.action_generation = model.action_generation.wrapping_add(1);
    let generation = model.action_generation;
    let edit_generation = model.edit_generation;
    Some(Task::perform(
        oxibar_plugin_api::worker::run(move || run_network_action(action)),
        move |result| {
            msg(Message::ActionDone {
                result,
                close_modal,
                generation,
                edit_generation,
            })
        },
    ))
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
    text(label)
        .size(OXITHEME.font_md)
        .style(|_| iced::widget::text::Style {
            color: Some(OXITHEME.text_muted),
        })
        .into()
}

fn network_row_button_style(_: &iced::Theme, status: button::Status) -> button::Style {
    let base = button::Style {
        background: None,
        text_color: OXITHEME.text,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: OXITHEME.border_radius.into(),
        },
        shadow: Shadow::default(),
        snap: false,
    };
    match status {
        button::Status::Hovered | button::Status::Pressed => button::Style {
            text_color: OXITHEME.primary,
            ..base
        },
        button::Status::Active | button::Status::Disabled => base,
    }
}

fn network_edit_button_style(_: &iced::Theme, status: button::Status) -> button::Style {
    let base = button::Style {
        background: Some(Background::Color(OXITHEME.primary_bg)),
        text_color: OXITHEME.primary,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: OXITHEME.border_radius.into(),
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

fn saved_connections_accordion(model: &Model) -> Element<'static, PluginMsg> {
    let icon = if model.saved_expanded { "󰅀" } else { "󰅂" };
    let label = format!("{icon} Saved Connections ({})", model.saved.len());
    button(
        Row::new()
            .push(
                text(label)
                    .size(OXITHEME.font_md)
                    .style(|_| iced::widget::text::Style {
                        color: Some(OXITHEME.primary),
                    }),
            )
            .push(Space::new().width(Length::Fill))
            .align_y(Alignment::Center),
    )
    .on_press(msg(Message::ToggleSavedConnections))
    .style(network_row_button_style)
    .padding([OXITHEME.padding_xs, OXITHEME.padding_sm])
    .width(Length::Fill)
    .into()
}

fn active_row(network: &ActiveNetwork, busy: bool, hovered: bool) -> Element<'static, PluginMsg> {
    let key = entry_key("active", &network.uuid);
    let uuid = network.uuid.clone();
    let edit_uuid = network.uuid.clone();
    let edit_name = network.name.clone();
    let name = network.name.clone();
    let details = format!("{} on {}", network.kind, network.device);
    animated_network_row(hovered, move || {
        let mut main = row_button(
            Row::new()
                .push(text("󰤨").size(OXITHEME.font_md).style(text_primary))
                .push(
                    Column::new()
                        .push(
                            text(name.clone())
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
        );
        if !busy {
            main = main.on_press(msg(Message::Disconnect(uuid.clone())));
        }
        let edit = edit_button(msg(Message::EditConnection {
            uuid: edit_uuid.clone(),
            name: edit_name.clone(),
        }));
        Row::new()
            .push(main)
            .push(edit)
            .spacing(OXITHEME.padding_xs)
            .align_y(Alignment::Center)
            .into()
    })
    .pipe_mouse_area(key)
}

fn saved_row(
    connection: &SavedConnection,
    busy: bool,
    hovered: bool,
) -> Element<'static, PluginMsg> {
    let key = entry_key("saved", &connection.uuid);
    let uuid = connection.uuid.clone();
    let edit_uuid = connection.uuid.clone();
    let edit_name = connection.name.clone();
    let name = connection.name.clone();
    let kind = connection.kind.clone();
    let details = format!("Saved {kind} connection");
    animated_network_row(hovered, move || {
        let mut main = row_button(
            Row::new()
                .push(
                    text(connection_icon(&kind))
                        .size(OXITHEME.font_md)
                        .style(text_primary),
                )
                .push(
                    Column::new()
                        .push(
                            text(name.clone())
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
        );
        if !busy {
            main = main.on_press(msg(Message::ConnectConnection(uuid.clone())));
        }
        let edit = edit_button(msg(Message::EditConnection {
            uuid: edit_uuid.clone(),
            name: edit_name.clone(),
        }));
        Row::new()
            .push(main)
            .push(edit)
            .spacing(OXITHEME.padding_xs)
            .align_y(Alignment::Center)
            .into()
    })
    .pipe_mouse_area(key)
}

fn wifi_row(network: &WifiNetwork, busy: bool, hovered: bool) -> Element<'static, PluginMsg> {
    let key = entry_key("wifi", &network.ssid);
    let ssid = network.ssid.clone();
    let edit_ssid = network.ssid.clone();
    let action_uuid = network.active_uuid.clone();
    let connected = network.connected;
    let security = if network.security.is_empty() {
        "Open".to_owned()
    } else {
        network.security.clone()
    };
    let details = format!("{}% • {} • {}", network.signal, security, network.bssid);

    animated_network_row(hovered, move || {
        let action = if connected {
            action_uuid.clone().map(Message::Disconnect)
        } else {
            Some(Message::ConnectWifi(ssid.clone()))
        };
        let mut main = row_button(
            Row::new()
                .push(
                    text(signal_icon(connected))
                        .size(OXITHEME.font_md)
                        .style(text_primary),
                )
                .push(
                    Column::new()
                        .push(
                            text(ssid.clone())
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
        );
        if !busy && let Some(action) = action {
            main = main.on_press(msg(action));
        }
        let edit = edit_button(msg(Message::EditWifi(edit_ssid.clone())));
        Row::new()
            .push(main)
            .push(edit)
            .spacing(OXITHEME.padding_xs)
            .align_y(Alignment::Center)
            .into()
    })
    .pipe_mouse_area(key)
}

fn animated_network_row(
    hovered: bool,
    build: impl Fn() -> Element<'static, PluginMsg> + 'static,
) -> Element<'static, PluginMsg> {
    let bg = if hovered {
        OXITHEME.primary_bg_hover
    } else {
        OXITHEME.mantle_hover
    };

    let build = move |bg| -> Element<'static, PluginMsg> {
        Container::new(build())
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

trait MouseAreaExt {
    fn pipe_mouse_area(self, key: String) -> Element<'static, PluginMsg>;
}

impl MouseAreaExt for Element<'static, PluginMsg> {
    fn pipe_mouse_area(self, key: String) -> Element<'static, PluginMsg> {
        iced::widget::mouse_area(self)
            .on_enter(msg(Message::HoverEntry(Some(key))))
            .on_exit(msg(Message::HoverEntry(None)))
            .into()
    }
}

fn row_button<'a>(content: impl Into<Element<'a, PluginMsg>>) -> button::Button<'a, PluginMsg> {
    button(content)
        .style(network_row_button_style)
        .padding([OXITHEME.padding_xs, OXITHEME.padding_sm])
        .width(Length::Fill)
}

fn edit_button(message: PluginMsg) -> button::Button<'static, PluginMsg> {
    button(
        text("󰏫")
            .size(OXITHEME.font_md)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center),
    )
    .style(network_edit_button_style)
    .on_press(message)
    .padding([OXITHEME.padding_xs, OXITHEME.padding_sm])
}

fn cancel_button(label: &'static str) -> button::Button<'static, PluginMsg> {
    oxi_button::button(
        text(label).size(OXITHEME.font_md),
        oxi_button::ButtonVariant::SecondaryBg,
    )
    .on_press(msg(Message::CancelEdit))
    .padding([OXITHEME.padding_sm, OXITHEME.padding_md])
}

fn signal_icon(connected: bool) -> &'static str {
    if connected { "󰤨" } else { "󰤯" }
}

fn connection_icon(kind: &str) -> &'static str {
    if kind.contains("wireless") || kind.contains("wifi") {
        "󰤨"
    } else {
        "󰈀"
    }
}

fn entry_key(kind: &str, id: &str) -> String {
    format!("{kind}:{id}")
}

fn read_scan_interval(global: &Table) -> u64 {
    global
        .get("network")
        .and_then(|v| v.as_table())
        .and_then(|t| t.get("scan_seconds"))
        .and_then(|v| v.as_integer())
        .filter(|n| *n > 0)
        .map(|n| n as u64)
        .unwrap_or(DEFAULT_SCAN_SECONDS)
}

const _: fn() = || {
    fn assert_stream<S: Stream<Item = PluginMsg> + Send + 'static>(_: &S) {}
    let _ = |s: &iced::futures::stream::BoxStream<'static, PluginMsg>| assert_stream(s);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn popup_height_tracks_scan_results_and_saved_accordion_contents() {
        let owner = plugin_model(Model::new(Table::new()));
        let height = || popup_metrics(owner.clone()).popup_size.unwrap().1;
        let empty = height();
        with_model_write::<Model, _>(&owner, |model| {
            model.wifi_enabled = Some(true);
            model.wifi = (0..2)
                .map(|index| WifiNetwork {
                    ssid: format!("Wi-Fi {index}"),
                    bssid: String::new(),
                    signal: 80,
                    security: "WPA2".into(),
                    connected: false,
                    active_uuid: None,
                })
                .collect();
            model.saved = (0..3)
                .map(|index| SavedConnection {
                    name: format!("Saved {index}"),
                    uuid: index.to_string(),
                    kind: "wifi".into(),
                })
                .collect();
        });
        let collapsed = height();
        assert!(collapsed > empty);
        update(owner.clone(), msg(Message::ToggleSavedConnections));
        assert!(height() > collapsed);
        with_model_write::<Model, _>(&owner, |model| {
            model.wifi.extend(vec![model.wifi[0].clone(); 100])
        });
        assert_eq!(height(), 900);
        assert_eq!(popup_view(owner).unwrap().len(), 1);
    }

    #[test]
    fn reads_scan_interval_from_config() {
        let mut network = Table::new();
        network.insert(
            "scan_seconds".to_owned(),
            oxibar_plugin_api::toml::Value::Integer(3),
        );
        let mut global = Table::new();
        global.insert(
            "network".to_owned(),
            oxibar_plugin_api::toml::Value::Table(network),
        );

        assert_eq!(read_scan_interval(&global), 3);
        assert_eq!(read_scan_interval(&Table::new()), DEFAULT_SCAN_SECONDS);
    }

    #[test]
    fn helper_labels_are_stable() {
        assert_eq!(signal_icon(true), "󰤨");
        assert_eq!(signal_icon(false), "󰤯");
        assert_eq!(connection_icon("802-11-wireless"), "󰤨");
        assert_eq!(connection_icon("ethernet"), "󰈀");
        assert_eq!(entry_key("wifi", "home"), "wifi:home");
    }

    #[test]
    fn model_views_and_error_drain_are_deterministic() {
        let (plugin_model, init_task) = model(Table::new());
        assert!(init_task.is_some());
        assert_eq!(name(), "Network");
        assert_eq!(abi_version(), ABI_VERSION);
        assert_eq!(metadata().popup_size, Some(POPUP_SIZE));
        assert_eq!(view(plugin_model.clone()).unwrap().len(), 1);
        assert_eq!(popup_view(plugin_model.clone()).unwrap().len(), 1);
        assert_eq!(modal_view(plugin_model.clone()).unwrap().len(), 1);

        let task = update(
            plugin_model.clone(),
            msg(Message::ScanResult(Err("network failed".to_owned()))),
        );
        assert!(task.is_none());
        assert_eq!(errors(plugin_model.clone()), vec!["network failed"]);
        assert!(errors(plugin_model).is_empty());
    }

    #[test]
    fn dismissed_editor_and_stale_actions_cannot_close_a_new_password_draft() {
        let owner = plugin_model(Model::new(Table::new()));
        drop(update(
            owner.clone(),
            msg(Message::EditWifi("Old".to_owned())),
        ));
        let old_edit = with_model_write::<Model, _>(&owner, |model| {
            model.action_generation = 1;
            model.edit_generation
        })
        .unwrap();
        modal_closed(owner.clone());
        drop(update(
            owner.clone(),
            msg(Message::EditWifi("New".to_owned())),
        ));
        drop(update(
            owner.clone(),
            msg(Message::ActionDone {
                result: Ok(String::new()),
                close_modal: true,
                generation: 1,
                edit_generation: old_edit,
            }),
        ));
        assert_eq!(
            with_model_read::<Model, _>(&owner, |model| model
                .editing
                .as_ref()
                .unwrap()
                .label
                .clone())
            .unwrap(),
            "New"
        );
        with_model_write::<Model, _>(&owner, |model| {
            model.action_generation = 2;
            model.pending = Some("new action".to_owned());
        });
        assert!(
            update(
                owner.clone(),
                msg(Message::ActionDone {
                    result: Err("old error".to_owned()),
                    close_modal: true,
                    generation: 1,
                    edit_generation: old_edit
                })
            )
            .is_none()
        );
        assert_eq!(
            with_model_read::<Model, _>(&owner, |model| model.pending.clone())
                .unwrap()
                .as_deref(),
            Some("new action")
        );
        assert!(errors(owner.clone()).is_empty());
        modal_closed(owner.clone());
        assert!(with_model_read::<Model, _>(&owner, |model| model.editing.is_none()).unwrap());
    }
}
