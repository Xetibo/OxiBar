use std::{
    collections::{BTreeSet, HashMap},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
    },
    thread,
    time::Duration,
};

use iced::{
    Alignment, Background, Border, Color, ContentFit, Element, Length, Shadow, Theme,
    futures::channel::mpsc,
    widget::{Column, Row, Space, button, column, container, image, mouse_area, row, text},
};
use oxibar_plugin_api::{PluginMsg, toml::Table};
use oxiced::{
    theme::theme_impl::OXITHEME,
    widgets::{
        oxi_button::{ButtonVariant, button as oxi_button},
        oxi_plugin,
        oxi_progress::progress_bar,
        oxi_text_input,
    },
};
use zbus::{
    block_on,
    blocking::Connection,
    fdo::{RequestNameFlags, RequestNameReply},
    interface,
    object_server::SignalEmitter,
    zvariant::OwnedValue,
};

const BUS_NAME: &str = "org.freedesktop.Notifications";
const OBJECT_PATH: &str = "/org/freedesktop/Notifications";
const NAME_RETRY_SECONDS: u64 = 5;

pub(crate) const ICON: &str = "󰂚";
pub(crate) const TOAST_WIDTH: u32 = 380;

const TOAST_MIN_HEIGHT: u32 = 112;
const TOAST_IMAGE_HEIGHT: u32 = 196;
const TOAST_MAX_HEIGHT: u32 = 360;

type RawImageData = (i32, i32, i32, bool, i32, i32, Vec<u8>);
type NotificationTuple = (
    String,
    u32,
    String,
    String,
    String,
    Vec<String>,
    i32,
    i32,
    String,
    i32,
    RawImageData,
);

#[derive(Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub(crate) struct ImageData {
    pub width: i32,
    pub height: i32,
    pub rowstride: i32,
    pub has_alpha: bool,
    pub bits_per_sample: i32,
    pub channels: i32,
    pub data: Vec<u8>,
}

impl ImageData {
    fn empty() -> Self {
        Self {
            width: -1,
            height: -1,
            rowstride: -1,
            has_alpha: false,
            bits_per_sample: -1,
            channels: -1,
            data: Vec::new(),
        }
    }

    fn into_tuple(self) -> RawImageData {
        (
            self.width,
            self.height,
            self.rowstride,
            self.has_alpha,
            self.bits_per_sample,
            self.channels,
            self.data,
        )
    }

    fn from_tuple(tuple: RawImageData) -> Self {
        let (width, height, rowstride, has_alpha, bits_per_sample, channels, data) = tuple;
        Self {
            width,
            height,
            rowstride,
            has_alpha,
            bits_per_sample,
            channels,
            data,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub(crate) enum Urgency {
    Low,
    Normal,
    Urgent,
}

impl Urgency {
    fn from_i32(value: i32) -> Self {
        match value {
            0 => Self::Low,
            2 => Self::Urgent,
            _ => Self::Normal,
        }
    }

    fn to_i32(&self) -> i32 {
        match self {
            Self::Low => 0,
            Self::Normal => 1,
            Self::Urgent => 2,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub(crate) struct Notification {
    pub app_name: String,
    pub replaces_id: u32,
    pub app_icon: String,
    pub summary: String,
    pub body: String,
    pub actions: Vec<String>,
    pub expire_timeout: i32,
    pub urgency: Urgency,
    pub image_path: Option<String>,
    pub progress: Option<i32>,
    pub image_data: Option<ImageData>,
    reply_placeholder: Option<String>,
    reply_submit_label: Option<String>,
}

impl Notification {
    #[allow(clippy::too_many_arguments)]
    fn from_dbus(
        id: u32,
        app_name: String,
        app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        expire_timeout: i32,
    ) -> Self {
        Self {
            app_name,
            replaces_id: id,
            app_icon,
            summary,
            body,
            actions,
            expire_timeout,
            urgency: Urgency::from_i32(int_hint(&hints, &["urgency"]).unwrap_or(1)),
            image_path: string_hint(&hints, &["image-path", "image_path"]),
            progress: int_hint(&hints, &["progress", "value"]).map(|value| value.clamp(-1, 100)),
            image_data: image_data_hint(&hints),
            reply_placeholder: string_hint(
                &hints,
                &[
                    "x-kde-reply-placeholder-text",
                    "reply-placeholder",
                    "reply_placeholder",
                ],
            ),
            reply_submit_label: string_hint(
                &hints,
                &[
                    "x-kde-reply-submit-button-text",
                    "reply-submit-label",
                    "reply_submit_label",
                ],
            ),
        }
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(id: u32, summary: &str) -> Self {
        Self {
            app_name: "app".to_owned(),
            replaces_id: id,
            app_icon: String::new(),
            summary: summary.to_owned(),
            body: "body".to_owned(),
            actions: Vec::new(),
            expire_timeout: -1,
            urgency: Urgency::Normal,
            image_path: None,
            progress: None,
            image_data: None,
            reply_placeholder: None,
            reply_submit_label: None,
        }
    }

    pub(crate) fn id(&self) -> u32 {
        self.replaces_id
    }

    pub(crate) fn toast_id(&self) -> String {
        self.replaces_id.to_string()
    }

    pub(crate) fn toast_size(&self) -> (u32, u32) {
        (TOAST_WIDTH, notification_height(self))
    }

    pub(crate) fn primary_action(&self) -> Option<String> {
        self.action_pairs()
            .find(|(key, _)| *key == "default")
            .or_else(|| {
                self.action_pairs()
                    .find(|(key, _)| !is_inline_reply_action(key))
            })
            .map(|(key, _)| key.to_owned())
    }

    pub(crate) fn allows_inline_reply(&self) -> bool {
        self.reply_placeholder.is_some()
            || self
                .action_pairs()
                .any(|(key, _)| is_inline_reply_action(key))
    }

    fn reply_placeholder(&self) -> &str {
        self.reply_placeholder.as_deref().unwrap_or("Reply...")
    }

    fn reply_submit_label(&self) -> &str {
        self.reply_submit_label.as_deref().unwrap_or("Send")
    }

    fn action_pairs(&self) -> impl Iterator<Item = (&str, &str)> {
        self.actions.chunks(2).filter_map(|action| {
            let [key, label] = action else {
                return None;
            };
            Some((key.as_str(), label.as_str()))
        })
    }

    fn into_dbus_tuple(self) -> NotificationTuple {
        let image_data = self
            .image_data
            .unwrap_or_else(ImageData::empty)
            .into_tuple();
        (
            self.app_name,
            self.replaces_id,
            self.app_icon,
            self.summary,
            self.body,
            self.actions,
            self.expire_timeout,
            self.urgency.to_i32(),
            self.image_path.unwrap_or_default(),
            self.progress.unwrap_or(-1),
            image_data,
        )
    }
}

#[derive(Clone, Debug)]
pub(crate) enum Event {
    TogglePanel,
    ToggleDoNotDisturb,
    SetDoNotDisturb(bool),
    ClearAll,
    Add(Box<Notification>),
    Remove(u32),
    Close(u32),
    Invoke(u32, String),
    ReplyChanged(u32, String),
    SubmitReply(u32),
    HoverChanged(u32, bool),
    ToastExpired(u32, u64),
    Error(String),
}

pub(crate) fn msg(event: Event) -> PluginMsg {
    Arc::new(event)
}

pub(crate) fn read_timeout(global_config: &Table) -> Duration {
    let seconds = global_config
        .get("notifications")
        .and_then(|value| value.as_table())
        .and_then(|table| table.get("timeout"))
        .and_then(|value| value.as_integer())
        .filter(|seconds| *seconds > 0)
        .unwrap_or(3) as u64;
    Duration::from_secs(seconds)
}

pub(crate) fn bar_button(count: usize) -> button::Button<'static, PluginMsg> {
    let content: Element<'static, PluginMsg> = if count == 0 {
        text(ICON)
            .size(14)
            .align_y(Alignment::Center)
            .align_x(Alignment::Center)
            .into()
    } else {
        row![
            text(ICON).size(14).align_y(Alignment::Center),
            text(count.to_string()).size(14).align_y(Alignment::Center)
        ]
        .spacing(10)
        .height(Length::Fill)
        .align_y(Alignment::Center)
        .into()
    };
    oxi_plugin::bar_button(content).on_press(msg(Event::TogglePanel))
}

pub(crate) fn panel_view(
    notifications: &[Notification],
    reply_drafts: &std::collections::BTreeMap<u32, String>,
    hovered_notifications: &BTreeSet<u32>,
    do_not_disturb: bool,
) -> Element<'static, PluginMsg> {
    let dnd_label = if do_not_disturb { "DND on" } else { "DND off" };
    let header = Row::new()
        .push(
            text("Notifications")
                .size(18)
                .style(|_| iced::widget::text::Style {
                    color: Some(OXITHEME.primary),
                }),
        )
        .push(Space::new().width(Length::Fill))
        .push(settings_button(dnd_label, Event::ToggleDoNotDisturb))
        .push(settings_button("Clear", Event::ClearAll))
        .spacing(6)
        .align_y(Alignment::Center);

    let mut list = Column::new().spacing(10).width(Length::Fill);
    if notifications.is_empty() {
        list = list.push(
            text("No notifications")
                .size(13)
                .style(|_| iced::widget::text::Style {
                    color: Some(OXITHEME.text_muted),
                }),
        );
    } else {
        for notification in notifications {
            list = list.push(notification_card(
                notification,
                reply_drafts.get(&notification.id()).map(String::as_str),
                hovered_notifications.contains(&notification.id()),
            ));
        }
    }

    Column::new()
        .push(header)
        .push(iced::widget::scrollable(list).height(Length::Fill))
        .spacing(12)
        .padding([14, 14])
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

pub(crate) fn toast_view(
    notifications: &[Notification],
    reply_drafts: &std::collections::BTreeMap<u32, String>,
    hovered_notifications: &BTreeSet<u32>,
    toast_id: &str,
) -> Option<Element<'static, PluginMsg>> {
    notifications
        .iter()
        .find(|notification| notification.toast_id() == toast_id)
        .map(|notification| {
            notification_card(
                notification,
                reply_drafts.get(&notification.id()).map(String::as_str),
                hovered_notifications.contains(&notification.id()),
            )
        })
}

fn settings_button(label: &'static str, event: Event) -> button::Button<'static, PluginMsg> {
    button(text(label).size(11))
        .on_press(msg(event))
        .style(settings_button_style)
        .padding([5, 8])
}

fn settings_button_style(_: &Theme, status: button::Status) -> button::Style {
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

fn notification_card(
    notification: &Notification,
    reply_draft: Option<&str>,
    hovered: bool,
) -> Element<'static, PluginMsg> {
    let id = notification.replaces_id;
    let urgency = notification.urgency.clone();
    let palette = &OXITHEME;
    let primary_action = notification.primary_action();

    let app_name = text(if notification.app_name.is_empty() {
        "notification".to_owned()
    } else {
        notification.app_name.clone()
    })
    .size(palette.font_sm)
    .color(palette.text_muted);

    let close_button = oxi_button(text("x").size(palette.font_sm), ButtonVariant::SecondaryBg)
        .padding([palette.padding_xs, palette.padding_sm])
        .on_press(msg(Event::Close(id)));

    let header = row![app_name, Space::new().width(Length::Fill), close_button]
        .align_y(Alignment::Center)
        .spacing(palette.padding_sm);

    let (body_text, body_image) = body_text_and_image(&notification.body);
    let mut text_column = column![]
        .width(Length::Fill)
        .spacing(palette.padding_xs)
        .align_x(Alignment::Start);

    if !notification.summary.is_empty() {
        text_column = text_column.push(
            text(clean_markup(&notification.summary))
                .size(palette.font_lg)
                .color(palette.text)
                .wrapping(iced::widget::text::Wrapping::Word),
        );
    }

    if !body_text.is_empty() {
        text_column = text_column.push(
            text(body_text)
                .size(palette.font_md)
                .color(palette.text)
                .wrapping(iced::widget::text::Wrapping::Word),
        );
    }

    let mut body_row = Row::new()
        .spacing(palette.padding_md)
        .align_y(Alignment::Center)
        .push(text_column);
    if let Some(image) = notification_image(notification, body_image) {
        body_row = body_row.push(image);
    }

    let mut clickable_body = Column::new()
        .push(body_row)
        .spacing(palette.padding_sm)
        .width(Length::Fill);

    if let Some(progress) = notification.progress
        && progress >= 0
    {
        clickable_body = clickable_body.push(progress_bar(0.0..=100.0, progress as f32));
    }

    let card_event = primary_action
        .clone()
        .map(|action| Event::Invoke(id, action))
        .unwrap_or(Event::Close(id));
    let clickable_body: Element<'static, PluginMsg> = clickable_body.into();

    let mut body = column![header, clickable_body]
        .spacing(palette.padding_sm)
        .width(Length::Fill);

    if let Some(actions) = action_buttons(notification, primary_action.as_deref()) {
        body = body.push(actions);
    }

    if let Some(reply) = reply_row(notification, reply_draft.unwrap_or_default()) {
        body = body.push(reply);
    }

    let card = container(body)
        .width(Length::Fill)
        .padding(palette.padding_md)
        .style(move |theme| notification_style(theme, &urgency, hovered));

    mouse_area(card)
        .on_enter(msg(Event::HoverChanged(id, true)))
        .on_exit(msg(Event::HoverChanged(id, false)))
        .on_press(msg(card_event))
        .into()
}

fn notification_height(notification: &Notification) -> u32 {
    let (_, body_image) = body_text_and_image(&notification.body);
    let has_image = body_image.is_some()
        || notification
            .image_path
            .as_ref()
            .is_some_and(|path| Path::new(path).is_file())
        || notification.image_data.is_some()
        || (!notification.app_icon.is_empty() && Path::new(&notification.app_icon).is_file());

    let mut height = if has_image {
        TOAST_IMAGE_HEIGHT
    } else {
        TOAST_MIN_HEIGHT
    };

    let text_len = notification.summary.len() + notification.body.len();
    if text_len > 90 {
        height += ((text_len - 90) / 42) as u32 * 22;
    }

    if notification.progress.is_some_and(|progress| progress >= 0) {
        height += 20;
    }

    if has_visible_action_buttons(notification) {
        height += 34;
    }

    if notification.allows_inline_reply() {
        height += 54;
    }

    height.min(TOAST_MAX_HEIGHT)
}

fn action_buttons(
    notification: &Notification,
    primary_action: Option<&str>,
) -> Option<Row<'static, PluginMsg>> {
    let mut actions = Row::new()
        .spacing(OXITHEME.padding_sm)
        .align_y(Alignment::Center);
    let mut has_actions = false;

    for (key, label) in notification.action_pairs() {
        if Some(key) == primary_action || is_inline_reply_action(key) {
            continue;
        }
        has_actions = true;
        actions = actions.push(
            oxi_button(text(label.to_owned()), ButtonVariant::SecondaryBg)
                .on_press(msg(Event::Invoke(notification.replaces_id, key.to_owned()))),
        );
    }

    has_actions.then_some(actions)
}

fn has_visible_action_buttons(notification: &Notification) -> bool {
    let primary_action = notification.primary_action();
    notification
        .action_pairs()
        .any(|(key, _)| Some(key) != primary_action.as_deref() && !is_inline_reply_action(key))
}

fn reply_row(notification: &Notification, draft: &str) -> Option<Element<'static, PluginMsg>> {
    if !notification.allows_inline_reply() {
        return None;
    }

    let id = notification.id();
    let placeholder = notification.reply_placeholder().to_owned();
    let draft = draft.to_owned();
    let submit = msg(Event::SubmitReply(id));
    let input = oxi_text_input::text_input(&placeholder, &draft, move |value| {
        msg(Event::ReplyChanged(id, value))
    })
    .on_submit(submit.clone())
    .width(Length::Fill);

    let mut send = oxi_button(
        text(notification.reply_submit_label().to_owned()).size(OXITHEME.font_sm),
        ButtonVariant::PrimaryBg,
    )
    .padding([OXITHEME.padding_sm, OXITHEME.padding_md]);
    if !draft.trim().is_empty() {
        send = send.on_press(submit);
    }

    Some(
        row![input, send]
            .spacing(OXITHEME.padding_sm)
            .align_y(Alignment::Center)
            .into(),
    )
}

fn is_inline_reply_action(key: &str) -> bool {
    key == "inline-reply" || key == "reply"
}

fn notification_style(theme: &Theme, urgency: &Urgency, hovered: bool) -> container::Style {
    let palette = &OXITHEME;
    let border_color = match urgency {
        Urgency::Low => palette.good,
        Urgency::Normal => palette.primary,
        Urgency::Urgent => palette.bad,
    };

    container::Style {
        background: Some(Background::Color(if hovered {
            palette.primary_bg_hover
        } else {
            palette.mantle_hover
        })),
        border: Border {
            radius: palette.border_radius.into(),
            width: 1.0,
            color: border_color,
        },
        ..container::rounded_box(theme)
    }
}

fn notification_image(
    notification: &Notification,
    body_image: Option<String>,
) -> Option<Element<'static, PluginMsg>> {
    if let Some(path) = body_image
        .or_else(|| notification.image_path.clone())
        .filter(|path| Path::new(path).is_file())
    {
        return Some(image_element(iced::widget::image::Handle::from_path(path)));
    }

    if let Some(image_data) = notification.image_data.as_ref()
        && let Some(handle) = image_data_handle(image_data)
    {
        return Some(image_element(handle));
    }

    if !notification.app_icon.is_empty() && Path::new(&notification.app_icon).is_file() {
        return Some(image_element(iced::widget::image::Handle::from_path(
            notification.app_icon.clone(),
        )));
    }

    None
}

fn image_element(handle: iced::widget::image::Handle) -> Element<'static, PluginMsg> {
    image(handle)
        .width(Length::Fixed(88.0))
        .height(Length::Fixed(88.0))
        .content_fit(ContentFit::Contain)
        .into()
}

fn image_data_handle(image_data: &ImageData) -> Option<iced::widget::image::Handle> {
    if image_data.width <= 0
        || image_data.height <= 0
        || image_data.rowstride <= 0
        || image_data.bits_per_sample != 8
        || image_data.channels < 3
    {
        return None;
    }

    let width = image_data.width as usize;
    let height = image_data.height as usize;
    let rowstride = image_data.rowstride as usize;
    let channels = image_data.channels as usize;
    let mut rgba = Vec::with_capacity(width.checked_mul(height)?.checked_mul(4)?);

    for y in 0..height {
        let row_start = y.checked_mul(rowstride)?;
        for x in 0..width {
            let pixel_start = row_start.checked_add(x.checked_mul(channels)?)?;
            if pixel_start.checked_add(channels)? > image_data.data.len() {
                return None;
            }
            rgba.push(image_data.data[pixel_start]);
            rgba.push(image_data.data[pixel_start + 1]);
            rgba.push(image_data.data[pixel_start + 2]);
            rgba.push(if image_data.has_alpha && channels >= 4 {
                image_data.data[pixel_start + 3]
            } else {
                255
            });
        }
    }

    Some(iced::widget::image::Handle::from_rgba(
        image_data.width as u32,
        image_data.height as u32,
        rgba,
    ))
}

fn body_text_and_image(body: &str) -> (String, Option<String>) {
    let Some((prefix, rest)) = body.split_once("<br><img src=\"file:///") else {
        return (clean_markup(body), None);
    };
    let path = rest.split_once('"').map(|(path, _)| path.to_owned());
    let text = if prefix.is_empty() {
        "sent an image.".to_owned()
    } else {
        format!("{}\nsent an image.", clean_markup(prefix))
    };
    (text, path)
}

pub(crate) fn clean_markup(input: &str) -> String {
    let input = input.replace("<br>", "\n").replace("<br/>", "\n");
    let mut output = String::with_capacity(input.len());
    let mut in_tag = false;

    for character in input.chars() {
        match character {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => output.push(character),
            _ => {}
        }
    }

    output
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

fn string_hint(hints: &HashMap<String, OwnedValue>, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        hints
            .get(*key)
            .and_then(|value| <&str>::try_from(value).ok())
            .map(ToOwned::to_owned)
            .filter(|value| !value.is_empty())
    })
}

fn int_hint(hints: &HashMap<String, OwnedValue>, keys: &[&str]) -> Option<i32> {
    keys.iter().find_map(|key| {
        let value = hints.get(*key)?;
        i32::try_from(value).ok().or_else(|| {
            u8::try_from(value)
                .ok()
                .map(i32::from)
                .or_else(|| {
                    u32::try_from(value)
                        .ok()
                        .and_then(|value| i32::try_from(value).ok())
                })
                .or_else(|| {
                    i64::try_from(value)
                        .ok()
                        .and_then(|value| i32::try_from(value).ok())
                })
        })
    })
}

fn image_data_hint(hints: &HashMap<String, OwnedValue>) -> Option<ImageData> {
    ["image-data", "image_data", "icon_data"]
        .iter()
        .find_map(|key| {
            hints
                .get(*key)
                .and_then(|value| value.try_clone().ok())
                .and_then(|value| RawImageData::try_from(value).ok())
                .map(ImageData::from_tuple)
        })
}

#[derive(Default)]
struct ServerState {
    notifications: HashMap<u32, Notification>,
    do_not_disturb: bool,
    notification_center: bool,
}

#[derive(Clone)]
struct NotificationServer {
    next_id: Arc<AtomicU32>,
    output: Arc<Mutex<mpsc::Sender<PluginMsg>>>,
    state: Arc<Mutex<ServerState>>,
}

#[interface(name = "org.freedesktop.Notifications")]
impl NotificationServer {
    #[zbus(name = "Notify")]
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: String,
        replaces_id: u32,
        app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        expire_timeout: i32,
    ) -> u32 {
        let id = if replaces_id == 0 {
            self.next_id.fetch_add(1, Ordering::Relaxed)
        } else {
            replaces_id
        };
        self.next_id
            .fetch_max(id.saturating_add(1), Ordering::Relaxed);

        let notification = Notification::from_dbus(
            id,
            app_name,
            app_icon,
            summary,
            body,
            actions,
            hints,
            expire_timeout,
        );
        self.state
            .lock()
            .unwrap()
            .notifications
            .insert(id, notification.clone());
        send_event(&self.output, Event::Add(Box::new(notification)));
        id
    }

    #[zbus(name = "CloseNotification")]
    fn close_notification(&self, id: u32) {
        self.state.lock().unwrap().notifications.remove(&id);
        send_event(&self.output, Event::Remove(id));
    }

    #[zbus(name = "GetAllNotifications")]
    fn get_all_notifications(&self) -> Vec<NotificationTuple> {
        self.state
            .lock()
            .unwrap()
            .notifications
            .values()
            .cloned()
            .map(Notification::into_dbus_tuple)
            .collect()
    }

    #[zbus(name = "RemoveAllNotifications")]
    fn remove_all_notifications(&self) -> String {
        self.state.lock().unwrap().notifications.clear();
        send_event(&self.output, Event::ClearAll);
        "ok".to_owned()
    }

    #[zbus(name = "DoNotDisturb")]
    fn do_not_disturb(&self) -> bool {
        let mut state = self.state.lock().unwrap();
        state.do_not_disturb = !state.do_not_disturb;
        let enabled = state.do_not_disturb;
        drop(state);
        send_event(&self.output, Event::SetDoNotDisturb(enabled));
        enabled
    }

    #[zbus(name = "ToggleNotificationCenter")]
    fn toggle_notification_center(&self) -> bool {
        let mut state = self.state.lock().unwrap();
        state.notification_center = !state.notification_center;
        let open = state.notification_center;
        drop(state);
        send_event(&self.output, Event::TogglePanel);
        open
    }

    #[zbus(name = "InvokeAction")]
    fn invoke_action(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        id: u32,
        action: String,
    ) -> zbus::fdo::Result<()> {
        block_on(Self::action_invoked(&emitter, id, &action))?;
        Ok(())
    }

    #[zbus(name = "InlineReply")]
    fn inline_reply(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        id: u32,
        text: String,
    ) -> zbus::fdo::Result<()> {
        block_on(Self::notification_replied(&emitter, id, &text))?;
        Ok(())
    }

    #[zbus(name = "GetCapabilities")]
    fn get_capabilities(&self) -> Vec<String> {
        get_capabilities()
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

    #[zbus(signal, name = "ActionInvoked")]
    async fn action_invoked(
        emitter: &SignalEmitter<'_>,
        id: u32,
        action_key: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal, name = "NotificationReplied")]
    async fn notification_replied(
        emitter: &SignalEmitter<'_>,
        id: u32,
        text: &str,
    ) -> zbus::Result<()>;
}

pub(crate) fn run_server(output: Arc<Mutex<mpsc::Sender<PluginMsg>>>) {
    let connection = match Connection::session() {
        Ok(connection) => connection,
        Err(e) => {
            send_error(
                &output,
                format!("notifications: could not connect to session bus: {e}"),
            );
            return;
        }
    };
    let server = NotificationServer {
        next_id: Arc::new(AtomicU32::new(1)),
        output: output.clone(),
        state: Arc::new(Mutex::new(ServerState::default())),
    };
    if let Err(e) = connection.object_server().at(OBJECT_PATH, server) {
        send_error(
            &output,
            format!("notifications: could not register object: {e}"),
        );
        return;
    }
    if !own_notification_name(&connection, &output) {
        return;
    }

    loop {
        thread::sleep(Duration::from_secs(30));
    }
}

fn own_notification_name(
    connection: &Connection,
    output: &Arc<Mutex<mpsc::Sender<PluginMsg>>>,
) -> bool {
    let flags = RequestNameFlags::DoNotQueue | RequestNameFlags::ReplaceExisting;
    let mut conflict_reported = false;

    loop {
        match connection.request_name_with_flags(BUS_NAME, flags) {
            Ok(reply) => {
                if let Some(error) = request_name_error(reply) {
                    if !conflict_reported {
                        send_error(output, error);
                        conflict_reported = true;
                    }
                    thread::sleep(Duration::from_secs(NAME_RETRY_SECONDS));
                } else {
                    return true;
                }
            }
            Err(e) => {
                send_error(
                    output,
                    format!("notifications: could not own {BUS_NAME}: {e}"),
                );
                return false;
            }
        }
    }
}

fn send_event(output: &Arc<Mutex<mpsc::Sender<PluginMsg>>>, event: Event) {
    let _ = output.lock().unwrap().try_send(msg(event));
}

fn send_error(output: &Arc<Mutex<mpsc::Sender<PluginMsg>>>, error: String) {
    send_event(output, Event::Error(error));
}

pub(crate) fn request_name_error(reply: RequestNameReply) -> Option<String> {
    match reply {
        RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner => None,
        other => Some(format!(
            "notifications: could not own {BUS_NAME}: request returned {other}; another notification daemon is probably running"
        )),
    }
}

pub(crate) fn spawn_close_notification(id: u32) {
    thread::spawn(move || {
        let Ok(conn) = Connection::session() else {
            return;
        };
        let _ = conn.call_method(
            Some(BUS_NAME),
            OBJECT_PATH,
            Some(BUS_NAME),
            "CloseNotification",
            &(id,),
        );
    });
}

pub(crate) fn spawn_invoke_action(id: u32, action: String) {
    thread::spawn(move || {
        let Ok(conn) = Connection::session() else {
            return;
        };
        let _ = conn.call_method(
            Some(BUS_NAME),
            OBJECT_PATH,
            Some(BUS_NAME),
            "InvokeAction",
            &(id, action),
        );
        let _ = conn.call_method(
            Some(BUS_NAME),
            OBJECT_PATH,
            Some(BUS_NAME),
            "CloseNotification",
            &(id,),
        );
    });
}

pub(crate) fn spawn_inline_reply(id: u32, text: String) {
    thread::spawn(move || {
        let Ok(conn) = Connection::session() else {
            return;
        };
        let _ = conn.call_method(
            Some(BUS_NAME),
            OBJECT_PATH,
            Some(BUS_NAME),
            "InlineReply",
            &(id, text),
        );
        let _ = conn.call_method(
            Some(BUS_NAME),
            OBJECT_PATH,
            Some(BUS_NAME),
            "CloseNotification",
            &(id,),
        );
    });
}

pub(crate) fn get_capabilities() -> Vec<String> {
    [
        "action-icons",
        "actions",
        "body",
        "body-hyperlinks",
        "body-images",
        "body-markup",
        "icon-static",
        "inline-reply",
        "persistence",
    ]
    .into_iter()
    .map(ToOwned::to_owned)
    .collect()
}
