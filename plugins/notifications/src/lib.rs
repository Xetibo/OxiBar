//! Notification center plugin implementing `org.freedesktop.Notifications`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use iced::{
    Alignment, Background, Border, Color, Element, Length, Shadow, Task,
    futures::Stream,
    stream,
    widget::{Column, Container, Row, Space, button, scrollable, text},
};
use oxibar_plugin_api::{
    ABI_VERSION, HOST_REQUEST_TOGGLE_PANEL, OxiAny, PluginModel, PluginMsg, PluginStream,
    toml::Table,
};
use oxiced::theme::theme_impl::OXITHEME;
use zbus::{blocking::Connection, fdo::RequestNameFlags, interface, zvariant::OwnedValue};

const BUS_NAME: &str = "org.freedesktop.Notifications";
const OBJECT_PATH: &str = "/org/freedesktop/Notifications";
const ICON: &str = "󰂚";

#[derive(Debug, Default)]
pub struct Model {
    notifications: Vec<Notification>,
    do_not_disturb: bool,
    errors: Vec<String>,
}

impl Model {
    fn new(_global_config: Table) -> Self {
        Self::default()
    }
}

#[derive(Clone, Debug)]
struct Notification {
    id: u32,
    app_name: String,
    summary: String,
    body: String,
}

#[derive(Clone, Debug)]
enum Message {
    TogglePanel,
    ToggleDoNotDisturb,
    ClearAll,
    Add(Notification),
    Remove(u32),
    Error(String),
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
    "Notifications"
}

#[unsafe(no_mangle)]
pub extern "Rust" fn model(global_config: Table) -> (PluginModel, Option<Task<PluginMsg>>) {
    let m: Box<dyn OxiAny> = Box::new(Model::new(global_config));
    (Arc::new(RwLock::new(m)), None)
}

#[unsafe(no_mangle)]
pub extern "Rust" fn update(model: PluginModel, msg_in: PluginMsg) -> Option<Task<PluginMsg>> {
    let mut guard = model.try_write().ok()?;
    let model = guard.downcast_mut::<Model>()?;
    let m = msg_in.downcast_ref::<Message>()?.clone();
    match m {
        Message::TogglePanel => Some(Task::done(Arc::new(HOST_REQUEST_TOGGLE_PANEL.to_owned()))),
        Message::ToggleDoNotDisturb => {
            model.do_not_disturb = !model.do_not_disturb;
            None
        }
        Message::ClearAll => {
            model.notifications.clear();
            None
        }
        Message::Add(notification) => {
            if let Some(existing) = model
                .notifications
                .iter_mut()
                .find(|existing| existing.id == notification.id)
            {
                *existing = notification;
            } else {
                model.notifications.insert(0, notification);
            }
            None
        }
        Message::Remove(id) => {
            model
                .notifications
                .retain(|notification| notification.id != id);
            None
        }
        Message::Error(error) => {
            model.errors.push(error);
            None
        }
    }
}

#[unsafe(no_mangle)]
pub extern "Rust" fn launch(_focused_index: usize, _model: PluginModel) -> Option<Task<PluginMsg>> {
    Some(Task::done(msg(Message::TogglePanel)))
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
    let count = model.notifications.len();
    let label = if count == 0 {
        ICON.to_owned()
    } else {
        format!("{ICON} {count}")
    };
    Ok(vec![bar_button(label).into()])
}

#[unsafe(no_mangle)]
pub extern "Rust" fn panel_view(
    model: PluginModel,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    let lock = model.try_read().map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::WouldBlock, "model is write-locked")
    })?;
    let model = lock.downcast_ref::<Model>().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "model has wrong type")
    })?;

    let dnd_label = if model.do_not_disturb {
        "DND on"
    } else {
        "DND off"
    };
    let header = Row::new()
        .push(
            text("Notifications")
                .size(18)
                .style(|_| iced::widget::text::Style {
                    color: Some(OXITHEME.primary),
                }),
        )
        .push(Space::new().width(Length::Fill))
        .push(settings_button(dnd_label, Message::ToggleDoNotDisturb))
        .push(settings_button("Clear", Message::ClearAll))
        .spacing(6)
        .align_y(Alignment::Center);

    let mut list = Column::new().spacing(8).width(Length::Fill);
    if model.notifications.is_empty() {
        list = list.push(
            text("No notifications")
                .size(13)
                .style(|_| iced::widget::text::Style {
                    color: Some(OXITHEME.text_muted),
                }),
        );
    } else {
        for notification in &model.notifications {
            list = list.push(notification_card(notification));
        }
    }

    Ok(vec![
        Column::new()
            .push(header)
            .push(scrollable(list).height(Length::Fill))
            .spacing(12)
            .padding([14, 14])
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
    ])
}

#[unsafe(no_mangle)]
pub extern "Rust" fn subscription() -> *mut PluginStream {
    let s = stream::channel(
        64,
        move |output: iced::futures::channel::mpsc::Sender<PluginMsg>| async move {
            let output = Arc::new(Mutex::new(output));
            std::thread::spawn(move || run_server(output));
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
    .on_press(msg(Message::TogglePanel))
    .style(bar_button_style)
    .padding([0, 8])
    .height(22.5)
    .width(Length::Shrink)
}

fn settings_button(label: &'static str, message: Message) -> button::Button<'static, PluginMsg> {
    button(text(label).size(11))
        .on_press(msg(message))
        .style(settings_button_style)
        .padding([5, 8])
}

fn notification_card(notification: &Notification) -> Element<'static, PluginMsg> {
    Container::new(
        Column::new()
            .push(
                Row::new()
                    .push(text(ICON).size(14).style(text_primary))
                    .push(
                        text(notification.app_name.clone())
                            .size(11)
                            .style(text_muted)
                            .width(Length::Fill),
                    )
                    .spacing(7)
                    .align_y(Alignment::Center),
            )
            .push(
                text(notification.summary.clone())
                    .size(13)
                    .style(text_primary),
            )
            .push(text(notification.body.clone()).size(11).style(text_muted))
            .spacing(4),
    )
    .style(|_| iced::widget::container::Style {
        background: Some(Background::Color(OXITHEME.mantle_hover)),
        border: Border {
            radius: 10.0.into(),
            color: Color::TRANSPARENT,
            width: 0.0,
        },
        shadow: Shadow::default(),
        ..Default::default()
    })
    .padding(10)
    .width(Length::Fill)
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

fn settings_button_style(_: &iced::Theme, status: button::Status) -> button::Style {
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

#[derive(Clone)]
struct NotificationServer {
    next_id: Arc<AtomicU32>,
    output: Arc<Mutex<iced::futures::channel::mpsc::Sender<PluginMsg>>>,
}

#[interface(name = "org.freedesktop.Notifications")]
impl NotificationServer {
    #[zbus(name = "Notify")]
    fn notify(
        &self,
        app_name: String,
        replaces_id: u32,
        _app_icon: String,
        summary: String,
        body: String,
        _actions: Vec<String>,
        _hints: HashMap<String, OwnedValue>,
        _expire_timeout: i32,
    ) -> u32 {
        let id = if replaces_id == 0 {
            self.next_id.fetch_add(1, Ordering::Relaxed)
        } else {
            replaces_id
        };
        let notification = Notification {
            id,
            app_name,
            summary: strip_markup(&summary),
            body: strip_markup(&body),
        };
        let _ = self
            .output
            .lock()
            .unwrap()
            .try_send(msg(Message::Add(notification)));
        id
    }

    #[zbus(name = "CloseNotification")]
    fn close_notification(&self, id: u32) {
        let _ = self
            .output
            .lock()
            .unwrap()
            .try_send(msg(Message::Remove(id)));
    }

    #[zbus(name = "GetCapabilities")]
    fn get_capabilities(&self) -> Vec<String> {
        vec!["body".to_owned(), "body-markup".to_owned()]
    }

    #[zbus(name = "GetServerInformation")]
    fn get_server_information(&self) -> (String, String, String, String) {
        (
            "Oxibar".to_owned(),
            "Oxibar".to_owned(),
            "0.1.0".to_owned(),
            "1.2".to_owned(),
        )
    }
}

fn run_server(output: Arc<Mutex<iced::futures::channel::mpsc::Sender<PluginMsg>>>) {
    let connection = match Connection::session() {
        Ok(connection) => connection,
        Err(e) => {
            let _ = output.lock().unwrap().try_send(msg(Message::Error(format!(
                "notifications: could not connect to session bus: {e}"
            ))));
            return;
        }
    };
    let server = NotificationServer {
        next_id: Arc::new(AtomicU32::new(1)),
        output: output.clone(),
    };
    if let Err(e) = connection.object_server().at(OBJECT_PATH, server) {
        let _ = output.lock().unwrap().try_send(msg(Message::Error(format!(
            "notifications: could not register object: {e}"
        ))));
        return;
    }
    let flags = RequestNameFlags::DoNotQueue | RequestNameFlags::ReplaceExisting;
    if let Err(e) = connection.request_name_with_flags(BUS_NAME, flags) {
        let _ = output.lock().unwrap().try_send(msg(Message::Error(format!(
            "notifications: could not own {BUS_NAME}: {e}"
        ))));
        return;
    }
    loop {
        std::thread::sleep(Duration::from_secs(30));
    }
}

fn strip_markup(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

const _: fn() = || {
    fn assert_stream<S: Stream<Item = PluginMsg> + Send + 'static>(_: &S) {}
    let _ = |s: &iced::futures::stream::BoxStream<'static, PluginMsg>| assert_stream(s);
};
