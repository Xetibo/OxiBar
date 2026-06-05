//! Bluetooth plugin backed by `bluetoothctl`.

use std::io::Write;
use std::process::{Command, Stdio};
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

const DEFAULT_REFRESH_SECONDS: u64 = 20;
const ICON: &str = "󰂯";

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
struct BluetoothDevice {
    mac: String,
    name: String,
    icon: String,
    paired: bool,
}

#[derive(Clone, Debug)]
struct Snapshot {
    connected: Vec<BluetoothDevice>,
    available: Vec<BluetoothDevice>,
}

#[derive(Clone, Debug)]
struct PairingState {
    device: BluetoothDevice,
    code: String,
}

#[derive(Clone, Debug)]
enum PairOutcome {
    Done,
    NeedsCode,
}

#[derive(Clone, Debug)]
enum BluetoothAction {
    Pair { mac: String, code: Option<String> },
    Disconnect { mac: String },
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
                    let Some(mac) = mac else {
                        return None;
                    };
                    if let Some(device) = find_device(model, &mac) {
                        model.pairing = Some(PairingState {
                            device,
                            code: String::new(),
                        });
                        Some(Task::done(Arc::new(HOST_REQUEST_OPEN_MODAL.to_owned())))
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
            let Some(pairing) = model.pairing.clone() else {
                return None;
            };
            model.pending = Some(format!("Pairing {}", pairing.device.name));
            Some(run_action(BluetoothAction::Pair {
                mac: pairing.device.mac,
                code: Some(pairing.code),
            }))
        }
        Message::CancelCode => {
            model.pairing = None;
            Some(Task::done(Arc::new(HOST_REQUEST_CLOSE_MODAL.to_owned())))
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
    let count = model.connected.len();
    let label = if count == 0 {
        ICON.to_owned()
    } else {
        format!("{ICON} {count}")
    };
    Ok(vec![bar_button(label).into()])
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
    let scan = oxi_button::button(
        text(if model.pending.is_some() {
            "Working..."
        } else {
            "Scan"
        })
        .size(12),
        oxi_button::ButtonVariant::SecondaryBg,
    )
    .on_press(msg(Message::Scan))
    .padding([5, 8]);
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
    let Some(pairing) = model.pairing.clone() else {
        return Ok(vec![
            Column::new()
                .push(section_title("No pairing request"))
                .into(),
        ]);
    };
    let input = oxi_text_input::text_input("PIN / pairing code", &pairing.code, |value| {
        msg(Message::CodeChanged(value))
    })
    .width(Length::Fill);
    let submit = oxi_button::button(text("Pair").size(13), oxi_button::ButtonVariant::Primary)
        .on_press(msg(Message::SubmitCode))
        .padding([8, 12]);
    let cancel = oxi_button::button(
        text("Cancel").size(13),
        oxi_button::ButtonVariant::SecondaryBg,
    )
    .on_press(msg(Message::CancelCode))
    .padding([8, 12]);
    Ok(vec![
        Column::new()
            .push(section_title("Bluetooth Pairing"))
            .push(text(pairing.device.name).size(14).style(text_primary))
            .push(
                text("Enter the PIN or pairing code shown by the device.")
                    .size(11)
                    .style(text_muted),
            )
            .push(input)
            .push(
                Row::new()
                    .push(Space::new().width(Length::Fill))
                    .push(cancel)
                    .push(submit)
                    .spacing(8)
                    .align_y(Alignment::Center),
            )
            .spacing(12)
            .width(Length::Fill)
            .into(),
    ])
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
    button(
        text(label)
            .size(14)
            .align_y(Alignment::Center)
            .align_x(Alignment::Center),
    )
    .on_press(msg(Message::TogglePopup))
    .style(bar_button_style)
    .padding([0, 8])
    .height(22.5)
    .width(Length::Shrink)
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
                    .push(text(icon).size(16).style(text_primary))
                    .push(
                        Column::new()
                            .push(text(label.clone()).size(13).style(text_primary))
                            .push(text(details.clone()).size(10).style(text_muted))
                            .spacing(2)
                            .width(Length::Fill),
                    )
                    .spacing(8)
                    .align_y(Alignment::Center),
            )
            .style(move |_, status| card_button_style(status, bg))
            .padding([8, 10])
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
                        radius: 9.0.into(),
                        color: Color::TRANSPARENT,
                        width: 0.0,
                    },
                    shadow: Shadow::default(),
                    ..Default::default()
                })
                .padding([4, 5])
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
        .size(12)
        .style(|_| iced::widget::text::Style {
            color: Some(OXITHEME.primary),
        })
        .into()
}

fn empty_text(label: &'static str) -> Element<'static, PluginMsg> {
    text(label).size(12).style(text_muted).into()
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
            radius: 8.0.into(),
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

fn scan_devices(active_scan: bool) -> Result<Snapshot, String> {
    if active_scan {
        let _ = run_bluetoothctl_script("scan on\n", Some(Duration::from_secs(4)));
    }
    let all = parse_devices(&run_bluetoothctl(&["devices"])?);
    let connected_macs = parse_devices(&run_bluetoothctl(&["devices", "Connected"])?)
        .into_iter()
        .map(|device| device.mac)
        .collect::<Vec<_>>();
    let mut connected = Vec::new();
    let mut available = Vec::new();
    for basic in all {
        let info = device_info(&basic.mac).unwrap_or_default();
        let is_connected = connected_macs.contains(&basic.mac) || info.connected;
        let device = BluetoothDevice {
            mac: basic.mac,
            name: basic.name,
            icon: info.icon,
            paired: info.paired,
        };
        if is_connected {
            connected.push(device);
        } else if !is_noise(&device) {
            available.push(device);
        }
    }
    connected.sort_by(|a, b| a.name.cmp(&b.name));
    available.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Snapshot {
        connected,
        available,
    })
}

#[derive(Default)]
struct DeviceInfo {
    icon: String,
    paired: bool,
    connected: bool,
}

fn device_info(mac: &str) -> Result<DeviceInfo, String> {
    let output = run_bluetoothctl(&["info", mac])?;
    let mut info = DeviceInfo::default();
    for line in output.lines().map(str::trim) {
        if let Some(icon) = line.strip_prefix("Icon:") {
            info.icon = icon.trim().to_owned();
        } else if let Some(paired) = line.strip_prefix("Paired:") {
            info.paired = paired.trim() == "yes";
        } else if let Some(connected) = line.strip_prefix("Connected:") {
            info.connected = connected.trim() == "yes";
        }
    }
    Ok(info)
}

fn parse_devices(output: &str) -> Vec<BluetoothDevice> {
    output
        .lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("Device ")?;
            let (mac, name) = rest.split_once(' ')?;
            Some(BluetoothDevice {
                mac: mac.to_owned(),
                name: name.trim().to_owned(),
                icon: String::new(),
                paired: false,
            })
        })
        .collect()
}

fn is_noise(device: &BluetoothDevice) -> bool {
    device.name.is_empty()
        || device.name.eq_ignore_ascii_case(&device.mac)
        || looks_like_mac(&device.name)
        || device.icon.is_empty()
}

fn looks_like_mac(value: &str) -> bool {
    let parts = value.split(':').collect::<Vec<_>>();
    parts.len() == 6
        && parts
            .iter()
            .all(|part| part.len() == 2 && part.chars().all(|ch| ch.is_ascii_hexdigit()))
}

fn run_bluetooth_action(action: BluetoothAction) -> Result<PairOutcome, String> {
    match action {
        BluetoothAction::Pair { mac, code } => pair_device(&mac, code),
        BluetoothAction::Disconnect { mac } => {
            run_bluetoothctl(&["disconnect", &mac])?;
            Ok(PairOutcome::Done)
        }
    }
}

fn pair_device(mac: &str, code: Option<String>) -> Result<PairOutcome, String> {
    let has_code = code.is_some();
    let script = if let Some(code) = code {
        format!(
            "agent KeyboardDisplay\ndefault-agent\npair {mac}\n{code}\nyes\ntrust {mac}\nconnect {mac}\nquit\n"
        )
    } else {
        format!(
            "agent KeyboardDisplay\ndefault-agent\npair {mac}\ntrust {mac}\nconnect {mac}\nquit\n"
        )
    };
    match run_bluetoothctl_script(&script, Some(Duration::from_secs(20))) {
        Ok(_) => Ok(PairOutcome::Done),
        Err(error) if needs_code(&error) && !has_code => Ok(PairOutcome::NeedsCode),
        Err(error) => Err(error),
    }
}

fn needs_code(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    lower.contains("pin")
        || lower.contains("passkey")
        || lower.contains("code")
        || lower.contains("agent")
}

fn run_bluetoothctl(args: &[&str]) -> Result<String, String> {
    let output = Command::new("bluetoothctl")
        .args(args)
        .output()
        .map_err(|e| format!("bluetooth: failed to run bluetoothctl: {e}"))?;
    command_output(output)
}

fn run_bluetoothctl_script(script: &str, timeout: Option<Duration>) -> Result<String, String> {
    let mut child = Command::new("bluetoothctl")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("bluetooth: failed to run bluetoothctl: {e}"))?;
    child
        .stdin
        .as_mut()
        .ok_or_else(|| "bluetooth: could not open bluetoothctl stdin".to_owned())?
        .write_all(script.as_bytes())
        .map_err(|e| format!("bluetooth: failed to write bluetoothctl script: {e}"))?;
    if let Some(timeout) = timeout {
        let start = std::time::Instant::now();
        loop {
            if let Some(_status) = child
                .try_wait()
                .map_err(|e| format!("bluetooth: bluetoothctl wait failed: {e}"))?
            {
                break;
            }
            if start.elapsed() > timeout {
                let _ = child.kill();
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    let output = child
        .wait_with_output()
        .map_err(|e| format!("bluetooth: bluetoothctl output failed: {e}"))?;
    command_output(output)
}

fn command_output(output: std::process::Output) -> Result<String, String> {
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if output.status.success()
        && !stdout.to_ascii_lowercase().contains("failed")
        && !stderr.to_ascii_lowercase().contains("failed")
    {
        Ok(stdout)
    } else if stderr.is_empty() {
        Err(format!("bluetooth: bluetoothctl failed: {stdout}"))
    } else {
        Err(format!(
            "bluetooth: bluetoothctl failed: {stderr}\n{stdout}"
        ))
    }
}

const _: fn() = || {
    fn assert_stream<S: Stream<Item = PluginMsg> + Send + 'static>(_: &S) {}
    let _ = |s: &iced::futures::stream::BoxStream<'static, PluginMsg>| assert_stream(s);
};
