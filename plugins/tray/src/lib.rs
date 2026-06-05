//! System tray plugin using the StatusNotifierItem watcher protocol.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use iced::{
    Alignment, Background, Border, Color, ContentFit, Element, Length, Shadow, Task,
    futures::Stream,
    stream,
    widget::{Column, Row, button, image, mouse_area, svg, text},
};
use oxibar_plugin_api::{
    ABI_VERSION, HOST_REQUEST_TOGGLE_POPUP, OxiAny, PluginModel, PluginMsg, PluginStream,
    toml::Table,
};
use oxiced::theme::theme_impl::OXITHEME;
use zbus::{
    blocking::Connection, fdo::RequestNameFlags, interface, message::Header, zvariant::OwnedValue,
};

const WATCHER_BUS_NAME: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";
const ITEM_IFACE: &str = "org.kde.StatusNotifierItem";

#[derive(Debug, Default)]
pub struct Model {
    items: BTreeMap<String, TrayItem>,
    hovered_item: Option<String>,
    errors: Vec<String>,
}

impl Model {
    fn new(_global_config: Table) -> Self {
        Self::default()
    }
}

#[derive(Clone, Debug)]
pub struct TrayItem {
    key: String,
    service: String,
    path: String,
    id: String,
    title: String,
    icon_path: Option<String>,
}

impl TrayItem {
    fn label(&self) -> String {
        if !self.title.is_empty() {
            self.title.clone()
        } else {
            let label = if !self.id.is_empty() {
                &self.id
            } else {
                &self.service
            };
            label
                .rsplit('.')
                .next()
                .unwrap_or(label)
                .trim_matches(':')
                .to_owned()
        }
    }
}

#[derive(Clone, Debug)]
pub enum Message {
    TogglePopup,
    ItemsChanged(Vec<TrayItem>),
    HoverItem(Option<String>),
    Activate { key: String, action: TrayAction },
    Error(String),
}

#[derive(Clone, Copy, Debug)]
pub enum TrayAction {
    Activate,
    SecondaryActivate,
    ContextMenu,
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
    "Tray"
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
        Message::TogglePopup => {
            let request: PluginMsg = Arc::new(HOST_REQUEST_TOGGLE_POPUP.to_owned());
            Some(Task::done(request))
        }
        Message::ItemsChanged(items) => {
            model.items = items
                .into_iter()
                .map(|item| (item.key.clone(), item))
                .collect();
            None
        }
        Message::HoverItem(key) => {
            model.hovered_item = key;
            None
        }
        Message::Activate { key, action } => {
            let Some(item) = model.items.get(&key).cloned() else {
                return None;
            };
            std::thread::spawn(move || {
                if let Err(e) = call_item_action(&item, action) {
                    tracing::warn!(item = item.key, "tray action failed: {e}");
                }
            });
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

    let count = model.items.len();
    let label = if count == 0 {
        "󰀻".to_owned()
    } else {
        format!("󰀻 {count}")
    };

    let btn = button(
        text(label)
            .size(14)
            .align_y(Alignment::Center)
            .align_x(Alignment::Center),
    )
    .on_press(msg(Message::TogglePopup))
    .style(tray_button_style)
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

    let mut list = Column::new()
        .spacing(6)
        .padding([12, 14])
        .width(Length::Fill);

    if model.items.is_empty() {
        list = list.push(
            text("No tray items")
                .size(14)
                .style(|_| iced::widget::text::Style {
                    color: Some(OXITHEME.primary),
                }),
        );
    }

    for item in model.items.values() {
        list = list.push(tray_item_row(
            item,
            model.hovered_item.as_deref() == Some(&item.key),
        ));
    }

    Ok(vec![list.into()])
}

fn tray_item_row(item: &TrayItem, _hovered: bool) -> Element<'static, PluginMsg> {
    let key = item.key.clone();
    let label = item.label();
    let icon = tray_icon(item.icon_path.as_deref());

    let item_button = button(
        Row::new()
            .push(icon)
            .push(text(label).size(13).width(Length::Fill))
            .spacing(8)
            .align_y(Alignment::Center),
    )
    .on_press(msg(Message::Activate {
        key: key.clone(),
        action: TrayAction::Activate,
    }))
    .style(tray_item_button_style)
    .padding([6, 8])
    .width(Length::Fill);

    let row = Row::new().push(item_button).spacing(8);

    mouse_area(row)
        .on_enter(msg(Message::HoverItem(Some(item.key.clone()))))
        .on_exit(msg(Message::HoverItem(None)))
        .on_right_press(msg(Message::Activate {
            key: item.key.clone(),
            action: TrayAction::ContextMenu,
        }))
        .into()
}

fn tray_icon(path: Option<&str>) -> Element<'static, PluginMsg> {
    let Some(path) = path else {
        return text("󰀻")
            .size(15)
            .align_x(Alignment::Center)
            .width(22)
            .into();
    };
    if path.ends_with(".svg") || path.ends_with(".svgz") {
        svg(path).width(18).height(18).into()
    } else {
        image(path)
            .width(18)
            .height(18)
            .content_fit(ContentFit::Contain)
            .into()
    }
}

fn tray_button_style(_: &iced::Theme, status: button::Status) -> button::Style {
    let palette = &OXITHEME;
    let base = button::Style {
        background: None,
        text_color: palette.primary,
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
            background: Some(Background::Color(palette.primary_bg_hover)),
            ..base
        },
        button::Status::Pressed => button::Style {
            background: Some(Background::Color(palette.primary_bg_active)),
            ..base
        },
        button::Status::Active | button::Status::Disabled => base,
    }
}

fn tray_item_button_style(_: &iced::Theme, status: button::Status) -> button::Style {
    let palette = &OXITHEME;
    let base = button::Style {
        background: Some(Background::Color(palette.primary_bg)),
        text_color: palette.primary,
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
            background: Some(Background::Color(palette.primary_bg_hover)),
            ..base
        },
        button::Status::Pressed => button::Style {
            background: Some(Background::Color(palette.primary_bg_active)),
            ..base
        },
        button::Status::Active | button::Status::Disabled => base,
    }
}

#[derive(Clone)]
struct Watcher {
    items: Arc<Mutex<BTreeMap<String, TrayItem>>>,
    output: Arc<Mutex<iced::futures::channel::mpsc::Sender<PluginMsg>>>,
}

#[interface(name = "org.kde.StatusNotifierWatcher")]
impl Watcher {
    #[zbus(name = "RegisterStatusNotifierItem")]
    fn register_status_notifier_item(
        &self,
        service_or_path: String,
        #[zbus(header)] header: Header<'_>,
    ) {
        let sender = header
            .sender()
            .map(|name| name.to_string())
            .unwrap_or_default();
        let (service, path) = normalize_item_address(&sender, &service_or_path);
        if service.is_empty() || path.is_empty() {
            return;
        }
        let item = query_item(&service, &path).unwrap_or_else(|| TrayItem {
            key: format!("{service}{path}"),
            service,
            path,
            id: String::new(),
            title: service_or_path,
            icon_path: None,
        });
        self.items.lock().unwrap().insert(item.key.clone(), item);
        publish_snapshot(&self.items, &self.output);
    }

    #[zbus(name = "RegisterStatusNotifierHost")]
    fn register_status_notifier_host(&self, _service: String) {}

    #[zbus(property, name = "RegisteredStatusNotifierItems")]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.items.lock().unwrap().keys().cloned().collect()
    }

    #[zbus(property, name = "IsStatusNotifierHostRegistered")]
    fn is_status_notifier_host_registered(&self) -> bool {
        true
    }

    #[zbus(property, name = "ProtocolVersion")]
    fn protocol_version(&self) -> i32 {
        0
    }
}

#[unsafe(no_mangle)]
pub extern "Rust" fn subscription() -> *mut PluginStream {
    let s = stream::channel(
        32,
        move |output: iced::futures::channel::mpsc::Sender<PluginMsg>| async move {
            let output = Arc::new(Mutex::new(output));
            let watcher_output = output.clone();
            std::thread::spawn(move || run_watcher(watcher_output));
            std::future::pending::<()>().await;
        },
    );

    Box::into_raw(Box::new(s)) as *mut PluginStream
}

fn run_watcher(output: Arc<Mutex<iced::futures::channel::mpsc::Sender<PluginMsg>>>) {
    let items = Arc::new(Mutex::new(BTreeMap::new()));
    let watcher = Watcher {
        items: items.clone(),
        output: output.clone(),
    };

    let connection = match Connection::session() {
        Ok(connection) => connection,
        Err(e) => {
            let _ = output.lock().unwrap().try_send(msg(Message::Error(format!(
                "tray: could not connect to session bus: {e}"
            ))));
            return;
        }
    };

    if let Err(e) = connection.object_server().at(WATCHER_PATH, watcher) {
        let _ = output.lock().unwrap().try_send(msg(Message::Error(format!(
            "tray: could not register watcher object: {e}"
        ))));
        return;
    }

    let flags = RequestNameFlags::ReplaceExisting | RequestNameFlags::DoNotQueue;
    if let Err(e) = connection.request_name_with_flags(WATCHER_BUS_NAME, flags) {
        let _ = output.lock().unwrap().try_send(msg(Message::Error(format!(
            "tray: could not own {WATCHER_BUS_NAME}: {e}"
        ))));
        return;
    }

    loop {
        std::thread::sleep(Duration::from_secs(30));
        publish_snapshot(&items, &output);
    }
}

fn normalize_item_address(sender: &str, service_or_path: &str) -> (String, String) {
    if service_or_path.starts_with('/') {
        (sender.to_owned(), service_or_path.to_owned())
    } else {
        (service_or_path.to_owned(), "/StatusNotifierItem".to_owned())
    }
}

fn query_item(service: &str, path: &str) -> Option<TrayItem> {
    let connection = Connection::session().ok()?;
    let id = get_item_string_property(&connection, service, path, "Id").unwrap_or_default();
    let title = get_item_string_property(&connection, service, path, "Title").unwrap_or_default();
    let icon_name =
        get_item_string_property(&connection, service, path, "IconName").unwrap_or_default();
    let icon_theme_path =
        get_item_string_property(&connection, service, path, "IconThemePath").unwrap_or_default();
    let icon_path = resolve_icon_path(&icon_name, &icon_theme_path);
    Some(TrayItem {
        key: format!("{service}{path}"),
        service: service.to_owned(),
        path: path.to_owned(),
        id,
        title,
        icon_path,
    })
}

fn resolve_icon_path(icon_name: &str, icon_theme_path: &str) -> Option<String> {
    if icon_name.is_empty() {
        return None;
    }
    let direct = Path::new(icon_name);
    if direct.is_absolute() && direct.exists() {
        return Some(icon_name.to_owned());
    }

    let mut roots = Vec::new();
    if !icon_theme_path.is_empty() {
        roots.push(PathBuf::from(icon_theme_path));
    }
    if let Ok(home) = std::env::var("HOME") {
        roots.push(PathBuf::from(format!("{home}/.local/share/icons")));
        roots.push(PathBuf::from(format!("{home}/.icons")));
        roots.push(PathBuf::from(format!("{home}/.nix-profile/share/icons")));
    }
    if let Ok(data_dirs) = std::env::var("XDG_DATA_DIRS") {
        for dir in data_dirs.split(':').filter(|dir| !dir.is_empty()) {
            roots.push(PathBuf::from(dir).join("icons"));
            roots.push(PathBuf::from(dir).join("pixmaps"));
        }
    } else {
        roots.push(PathBuf::from("/usr/local/share/icons"));
        roots.push(PathBuf::from("/usr/share/icons"));
        roots.push(PathBuf::from("/usr/share/pixmaps"));
    }
    roots.push(PathBuf::from("/run/current-system/sw/share/icons"));
    roots.push(PathBuf::from("/run/current-system/sw/share/pixmaps"));

    roots
        .into_iter()
        .find_map(|root| find_icon_in_dir(&root, icon_name, 0))
        .map(|path| path.to_string_lossy().into_owned())
}

fn find_icon_in_dir(root: &Path, icon_name: &str, depth: u8) -> Option<PathBuf> {
    if depth > 8 || !root.is_dir() {
        return None;
    }
    let candidates = [
        root.join(format!("{icon_name}.svg")),
        root.join(format!("{icon_name}.svgz")),
        root.join(format!("{icon_name}.png")),
        root.join(format!("{icon_name}.xpm")),
    ];
    if let Some(path) = candidates.into_iter().find(|path| path.is_file()) {
        return Some(path);
    }

    let entries = std::fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_icon_in_dir(&path, icon_name, depth + 1) {
                return Some(found);
            }
        }
    }
    None
}

fn get_item_string_property(
    connection: &Connection,
    service: &str,
    path: &str,
    property: &str,
) -> Option<String> {
    let reply = connection
        .call_method(
            Some(service),
            path,
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &(ITEM_IFACE, property),
        )
        .ok()?;
    let value = reply.body().deserialize::<OwnedValue>().ok()?;
    value.try_into().ok()
}

fn call_item_action(item: &TrayItem, action: TrayAction) -> zbus::Result<()> {
    let connection = Connection::session()?;
    let method = match action {
        TrayAction::Activate => "Activate",
        TrayAction::SecondaryActivate => "SecondaryActivate",
        TrayAction::ContextMenu => "ContextMenu",
    };
    connection.call_method(
        Some(item.service.as_str()),
        item.path.as_str(),
        Some(ITEM_IFACE),
        method,
        &(0i32, 0i32),
    )?;
    Ok(())
}

fn publish_snapshot(
    items: &Arc<Mutex<BTreeMap<String, TrayItem>>>,
    output: &Arc<Mutex<iced::futures::channel::mpsc::Sender<PluginMsg>>>,
) {
    let snapshot = items.lock().unwrap().values().cloned().collect::<Vec<_>>();
    let _ = output
        .lock()
        .unwrap()
        .try_send(msg(Message::ItemsChanged(snapshot)));
}

const _: fn() = || {
    fn assert_stream<S: Stream<Item = PluginMsg> + Send + 'static>(_: &S) {}
    let _ = |s: &iced::futures::stream::BoxStream<'static, PluginMsg>| assert_stream(s);
};
