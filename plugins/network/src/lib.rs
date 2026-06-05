//! Network plugin backed by NetworkManager's `nmcli`.

use std::collections::BTreeMap;
use std::process::Command;
use std::sync::{Arc, OnceLock, RwLock};
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
    OxiAny, PluginModel, PluginMsg, PluginStream, toml::Table,
};
use oxiced::{
    theme::theme_impl::OXITHEME,
    widgets::{oxi_button, oxi_text_input},
};

const DEFAULT_SCAN_SECONDS: u64 = 15;
const WIFI_ICON: &str = "󰤨";

static SCAN_INTERVAL: OnceLock<Duration> = OnceLock::new();

#[derive(Debug)]
pub struct Model {
    connected: Vec<ActiveNetwork>,
    saved: Vec<SavedConnection>,
    wifi: Vec<WifiNetwork>,
    editing: Option<EditState>,
    hovered_entry: Option<String>,
    saved_expanded: bool,
    pending: Option<String>,
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
            editing: None,
            hovered_entry: None,
            saved_expanded: false,
            pending: None,
            errors: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
struct Snapshot {
    connected: Vec<ActiveNetwork>,
    saved: Vec<SavedConnection>,
    wifi: Vec<WifiNetwork>,
}

#[derive(Clone, Debug)]
struct ActiveNetwork {
    name: String,
    uuid: String,
    kind: String,
    device: String,
}

#[derive(Clone, Debug)]
struct SavedConnection {
    name: String,
    uuid: String,
    kind: String,
}

#[derive(Clone, Debug)]
struct WifiNetwork {
    ssid: String,
    bssid: String,
    signal: u8,
    security: String,
    connected: bool,
    active_uuid: Option<String>,
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
enum NetworkAction {
    ConnectWifi { ssid: String },
    ConnectConnection { uuid: String },
    Disconnect { uuid: String },
    SaveWifi { ssid: String, password: String },
    SaveConnection { uuid: String, password: String },
}

#[derive(Clone, Debug)]
enum Message {
    TogglePopup,
    Refresh,
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
    "Network"
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
        Message::Refresh => Some(Task::perform(async { scan_networks() }, |result| {
            msg(Message::ScanResult(result))
        })),
        Message::ScanResult(result) => {
            match result {
                Ok(snapshot) => {
                    model.connected = snapshot.connected;
                    model.saved = snapshot.saved;
                    model.wifi = snapshot.wifi;
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
            model.editing = Some(EditState {
                label: ssid.clone(),
                target: EditTarget::Wifi { ssid },
                password: String::new(),
            });
            Some(Task::done(Arc::new(HOST_REQUEST_OPEN_MODAL.to_owned())))
        }
        Message::EditConnection { uuid, name } => {
            model.editing = Some(EditState {
                label: name,
                target: EditTarget::Connection { uuid },
                password: String::new(),
            });
            Some(Task::done(Arc::new(HOST_REQUEST_OPEN_MODAL.to_owned())))
        }
        Message::PasswordChanged(password) => {
            if let Some(editing) = model.editing.as_mut() {
                editing.password = password;
            }
            None
        }
        Message::SaveEdit => {
            let Some(editing) = model.editing.clone() else {
                return None;
            };
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
            model.editing = None;
            Some(Task::done(Arc::new(HOST_REQUEST_CLOSE_MODAL.to_owned())))
        }
        Message::ActionDone {
            result,
            close_modal,
        } => {
            model.pending = None;
            match result {
                Ok(_) if close_modal => {
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

    let connected = model.connected.len();
    let label = if connected == 0 {
        WIFI_ICON.to_owned()
    } else {
        format!("{WIFI_ICON} {connected}")
    };
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
        .spacing(8)
        .padding([12, 14])
        .width(Length::Fill);

    let refresh = oxi_button::button(
        text(if model.pending.is_some() {
            "Working..."
        } else {
            "Refresh"
        })
        .size(12),
        oxi_button::ButtonVariant::SecondaryBg,
    )
    .on_press(msg(Message::Refresh))
    .padding([5, 8]);
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

    Ok(vec![scrollable(content).height(Length::Fill).into()])
}

#[unsafe(no_mangle)]
pub extern "Rust" fn modal_view(
    model: PluginModel,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    let lock = model.try_read().map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::WouldBlock, "model is write-locked")
    })?;
    let model = lock.downcast_ref::<Model>().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "model has wrong type")
    })?;

    let Some(editing) = model.editing.clone() else {
        return Ok(vec![
            Column::new()
                .push(section_title("No network selected"))
                .push(cancel_button("Close"))
                .spacing(12)
                .into(),
        ]);
    };

    let password = oxi_text_input::text_input("Password", &editing.password, |value| {
        msg(Message::PasswordChanged(value))
    })
    .secure(true)
    .width(Length::Fill);
    let save = oxi_button::button(text("Save").size(13), oxi_button::ButtonVariant::Primary)
        .on_press(msg(Message::SaveEdit))
        .padding([8, 12]);
    let cancel = cancel_button("Cancel");

    let body = Column::new()
        .push(
            text("Network Settings")
                .size(18)
                .style(|_| iced::widget::text::Style {
                    color: Some(OXITHEME.primary),
                }),
        )
        .push(
            text(editing.label)
                .size(14)
                .style(|_| iced::widget::text::Style {
                    color: Some(OXITHEME.text),
                }),
        )
        .push(
            text("Leave password empty to reuse the saved NetworkManager profile.")
                .size(11)
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
                .spacing(8)
                .align_y(Alignment::Center),
        )
        .spacing(12)
        .width(Length::Fill)
        .height(Length::Fill);

    Ok(vec![body.into()])
}

#[unsafe(no_mangle)]
pub extern "Rust" fn subscription() -> *mut PluginStream {
    let interval = *SCAN_INTERVAL
        .get()
        .unwrap_or(&Duration::from_secs(DEFAULT_SCAN_SECONDS));
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
    action: NetworkAction,
    close_modal: bool,
) -> Option<Task<PluginMsg>> {
    model.pending = Some(action.label());
    Some(Task::perform(
        async move { run_network_action(action) },
        move |result| {
            msg(Message::ActionDone {
                result,
                close_modal,
            })
        },
    ))
}

impl NetworkAction {
    fn label(&self) -> String {
        match self {
            NetworkAction::ConnectWifi { ssid } => format!("Connecting to {ssid}"),
            NetworkAction::ConnectConnection { .. } => "Connecting".to_owned(),
            NetworkAction::Disconnect { .. } => "Disconnecting".to_owned(),
            NetworkAction::SaveWifi { ssid, .. } => format!("Saving {ssid}"),
            NetworkAction::SaveConnection { .. } => "Saving connection".to_owned(),
        }
    }
}

fn section_title(label: &'static str) -> Element<'static, PluginMsg> {
    text(label)
        .size(12)
        .style(|_| iced::widget::text::Style {
            color: Some(OXITHEME.primary),
        })
        .into()
}

fn empty_text(label: &'static str) -> Element<'static, PluginMsg> {
    text(label)
        .size(12)
        .style(|_| iced::widget::text::Style {
            color: Some(OXITHEME.text_muted),
        })
        .into()
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

fn network_row_button_style(_: &iced::Theme, status: button::Status) -> button::Style {
    let base = button::Style {
        background: None,
        text_color: OXITHEME.text,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 8.0.into(),
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

fn saved_connections_accordion(model: &Model) -> Element<'static, PluginMsg> {
    let icon = if model.saved_expanded { "󰅀" } else { "󰅂" };
    let label = format!("{icon} Saved Connections ({})", model.saved.len());
    button(
        Row::new()
            .push(text(label).size(12).style(|_| iced::widget::text::Style {
                color: Some(OXITHEME.primary),
            }))
            .push(Space::new().width(Length::Fill))
            .align_y(Alignment::Center),
    )
    .on_press(msg(Message::ToggleSavedConnections))
    .style(network_row_button_style)
    .padding([6, 8])
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
                .push(text("󰤨").size(13).style(text_primary))
                .push(
                    Column::new()
                        .push(text(name.clone()).size(13).style(text_primary))
                        .push(text(details.clone()).size(10).style(text_muted))
                        .spacing(2)
                        .width(Length::Fill),
                )
                .spacing(8)
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
            .spacing(4)
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
                .push(text(connection_icon(&kind)).size(13).style(text_primary))
                .push(
                    Column::new()
                        .push(text(name.clone()).size(13).style(text_primary))
                        .push(text(details.clone()).size(10).style(text_muted))
                        .spacing(2)
                        .width(Length::Fill),
                )
                .spacing(8)
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
            .spacing(4)
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
                .push(text(signal_icon(connected)).size(13).style(text_primary))
                .push(
                    Column::new()
                        .push(text(ssid.clone()).size(13).style(text_primary))
                        .push(text(details.clone()).size(10).style(text_muted))
                        .spacing(2)
                        .width(Length::Fill),
                )
                .spacing(8)
                .align_y(Alignment::Center),
        );
        if !busy && let Some(action) = action {
            main = main.on_press(msg(action));
        }
        let edit = edit_button(msg(Message::EditWifi(edit_ssid.clone())));
        Row::new()
            .push(main)
            .push(edit)
            .spacing(4)
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

    AnimationBuilder::new(bg, move |bg| {
        Container::new(build())
            .style(move |_| iced::widget::container::Style {
                background: Some(Background::Color(bg)),
                border: Border {
                    radius: 9.0.into(),
                    color: Color::TRANSPARENT,
                    width: 0.0,
                },
                shadow: Shadow::default(),
                ..Default::default()
            })
            .padding([5, 6])
            .width(Length::Fill)
            .into()
    })
    .animation(Motion::SMOOTH)
    .into()
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
        .padding([6, 8])
        .width(Length::Fill)
}

fn edit_button(message: PluginMsg) -> button::Button<'static, PluginMsg> {
    button(
        text("󰏫")
            .size(14)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center),
    )
    .style(network_edit_button_style)
    .on_press(message)
    .padding([6, 8])
}

fn cancel_button(label: &'static str) -> button::Button<'static, PluginMsg> {
    oxi_button::button(text(label).size(13), oxi_button::ButtonVariant::SecondaryBg)
        .on_press(msg(Message::CancelEdit))
        .padding([8, 12])
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

fn scan_networks() -> Result<Snapshot, String> {
    let connected = scan_active_connections()?;
    let saved = scan_saved_connections(&connected)?;
    let wifi = scan_wifi_networks(&connected)?;
    Ok(Snapshot {
        connected,
        saved,
        wifi,
    })
}

fn scan_active_connections() -> Result<Vec<ActiveNetwork>, String> {
    let output = run_nmcli(&[
        "-t",
        "-f",
        "NAME,UUID,TYPE,DEVICE",
        "connection",
        "show",
        "--active",
    ])?;
    let mut networks = Vec::new();
    for line in output.lines().filter(|line| !line.trim().is_empty()) {
        let fields = split_nmcli_line(line);
        if fields.len() < 4 || fields[1].is_empty() {
            continue;
        }
        networks.push(ActiveNetwork {
            name: fields[0].clone(),
            uuid: fields[1].clone(),
            kind: fields[2].clone(),
            device: fields[3].clone(),
        });
    }
    Ok(networks)
}

fn scan_saved_connections(active: &[ActiveNetwork]) -> Result<Vec<SavedConnection>, String> {
    let output = run_nmcli(&["-t", "-f", "NAME,UUID,TYPE", "connection", "show"])?;
    let mut connections = Vec::new();
    for line in output.lines().filter(|line| !line.trim().is_empty()) {
        let fields = split_nmcli_line(line);
        if fields.len() < 3 || fields[1].is_empty() {
            continue;
        }
        if active.iter().any(|network| network.uuid == fields[1]) {
            continue;
        }
        let kind = fields[2].clone();
        if kind == "loopback" {
            continue;
        }
        connections.push(SavedConnection {
            name: fields[0].clone(),
            uuid: fields[1].clone(),
            kind,
        });
    }
    connections.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(connections)
}

fn scan_wifi_networks(active: &[ActiveNetwork]) -> Result<Vec<WifiNetwork>, String> {
    let output = run_nmcli(&[
        "-t",
        "-f",
        "IN-USE,SSID,BSSID,SIGNAL,SECURITY",
        "device",
        "wifi",
        "list",
        "--rescan",
        "auto",
    ])?;
    let mut by_ssid = BTreeMap::<String, WifiNetwork>::new();
    for line in output.lines().filter(|line| !line.trim().is_empty()) {
        let fields = split_nmcli_line(line);
        if fields.len() < 5 || fields[1].trim().is_empty() {
            continue;
        }
        let ssid = fields[1].clone();
        let signal = fields[3].parse::<u8>().unwrap_or_default();
        let connected = fields[0].trim() == "*";
        let active_uuid = active
            .iter()
            .find(|network| network.name == ssid && network.kind.contains("wireless"))
            .map(|network| network.uuid.clone());
        let network = WifiNetwork {
            ssid: ssid.clone(),
            bssid: fields[2].clone(),
            signal,
            security: fields[4].clone(),
            connected,
            active_uuid,
        };
        match by_ssid.get(&ssid) {
            Some(existing) if existing.signal >= signal => {}
            _ => {
                by_ssid.insert(ssid, network);
            }
        }
    }
    let mut networks = by_ssid.into_values().collect::<Vec<_>>();
    networks.sort_by(|a, b| b.connected.cmp(&a.connected).then(b.signal.cmp(&a.signal)));
    Ok(networks)
}

fn run_network_action(action: NetworkAction) -> Result<String, String> {
    match action {
        NetworkAction::ConnectWifi { ssid } => connect_wifi(&ssid),
        NetworkAction::ConnectConnection { uuid } => run_nmcli_owned(vec![
            "connection".to_owned(),
            "up".to_owned(),
            "uuid".to_owned(),
            uuid,
        ]),
        NetworkAction::Disconnect { uuid } => run_nmcli_owned(vec![
            "connection".to_owned(),
            "down".to_owned(),
            "uuid".to_owned(),
            uuid,
        ]),
        NetworkAction::SaveWifi { ssid, password } => connect_wifi_with_password(&ssid, &password),
        NetworkAction::SaveConnection { uuid, password } => {
            if !password.is_empty() {
                run_nmcli_owned(vec![
                    "connection".to_owned(),
                    "modify".to_owned(),
                    "uuid".to_owned(),
                    uuid.clone(),
                    "802-11-wireless-security.psk".to_owned(),
                    password,
                ])?;
            }
            run_nmcli_owned(vec![
                "connection".to_owned(),
                "up".to_owned(),
                "uuid".to_owned(),
                uuid,
            ])
        }
    }
}

fn connect_wifi(ssid: &str) -> Result<String, String> {
    run_nmcli_owned(vec![
        "connection".to_owned(),
        "up".to_owned(),
        "id".to_owned(),
        ssid.to_owned(),
    ])
    .or_else(|_| {
        run_nmcli_owned(vec![
            "device".to_owned(),
            "wifi".to_owned(),
            "connect".to_owned(),
            ssid.to_owned(),
        ])
    })
}

fn connect_wifi_with_password(ssid: &str, password: &str) -> Result<String, String> {
    let mut args = vec![
        "device".to_owned(),
        "wifi".to_owned(),
        "connect".to_owned(),
        ssid.to_owned(),
    ];
    if !password.is_empty() {
        args.push("password".to_owned());
        args.push(password.to_owned());
    }
    run_nmcli_owned(args)
}

fn run_nmcli(args: &[&str]) -> Result<String, String> {
    let output = Command::new("nmcli")
        .args(args)
        .output()
        .map_err(|e| format!("network: failed to run nmcli: {e}"))?;
    command_output(output)
}

fn run_nmcli_owned(args: Vec<String>) -> Result<String, String> {
    let output = Command::new("nmcli")
        .args(&args)
        .output()
        .map_err(|e| format!("network: failed to run nmcli: {e}"))?;
    command_output(output)
}

fn command_output(output: std::process::Output) -> Result<String, String> {
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if output.status.success() {
        Ok(stdout)
    } else if stderr.is_empty() {
        Err(format!("network: nmcli failed: {stdout}"))
    } else {
        Err(format!("network: nmcli failed: {stderr}"))
    }
}

fn split_nmcli_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut escaped = false;
    for ch in line.chars() {
        if escaped {
            field.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == ':' {
            fields.push(field);
            field = String::new();
        } else {
            field.push(ch);
        }
    }
    fields.push(field);
    fields
}

const _: fn() = || {
    fn assert_stream<S: Stream<Item = PluginMsg> + Send + 'static>(_: &S) {}
    let _ = |s: &iced::futures::stream::BoxStream<'static, PluginMsg>| assert_stream(s);
};
