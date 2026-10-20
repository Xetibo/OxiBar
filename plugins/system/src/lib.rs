use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, OnceLock, Weak},
};

use iced::{Element, Task};
use oxibar_plugin_api::{
    ABI_VERSION, CommandOutcome, HOST_REQUEST_CLOSE_MODAL, HOST_REQUEST_OPEN_MODAL,
    HOST_REQUEST_TOGGLE_POPUP, PluginAvailability, PluginMetadata, PluginModel, PluginMsg,
    PluginStream, drain_model_errors, plugin_model,
    toml::{Table, Value},
    with_model_read, with_model_write,
};

mod native;
mod runtime;
mod system;
mod views;
mod worker;

#[cfg(test)]
mod integration_tests;
#[cfg(test)]
mod tests;

use system::{Domain, SessionAction, Snapshot};

#[derive(Debug)]
struct Model {
    client: Arc<native::Client>,
    snapshot: Snapshot,
    preferred: String,
    preferred_config: String,
    locker: Option<Vec<String>>,
    lock_enabled: bool,
    confirmation: Option<SessionAction>,
    pending: BTreeMap<Domain, u64>,
    brightness_intent: Option<(String, u8)>,
    generation: u64,
    error: Option<String>,
    errors: Vec<String>,
    initialized: bool,
}

static CLIENT: OnceLock<Mutex<Weak<native::Client>>> = OnceLock::new();

#[derive(Debug, Clone)]
enum Message {
    TogglePopup,
    Snapshot(Snapshot),
    Refresh,
    Brightness(u8),
    SelectBacklight(String),
    Profile(String),
    Request(SessionAction),
    Confirm,
    Cancel,
    Lock,
    Done {
        domain: Domain,
        generation: u64,
        result: Result<Snapshot, String>,
    },
}

fn plugin_message(message: Message) -> PluginMsg {
    Arc::new(message)
}

oxibar_plugin_api::export_theme_changed!();

#[unsafe(no_mangle)]
pub extern "Rust" fn abi_version() -> u32 {
    ABI_VERSION
}
#[unsafe(no_mangle)]
pub extern "Rust" fn name() -> &'static str {
    "System"
}
#[unsafe(no_mangle)]
pub extern "Rust" fn availability(_: &Table) -> PluginAvailability {
    PluginAvailability::Available
}
#[unsafe(no_mangle)]
pub extern "Rust" fn metadata() -> PluginMetadata {
    PluginMetadata {
        popup_size: Some((460, 420)),
        ..PluginMetadata::default()
    }
}
#[unsafe(no_mangle)]
pub extern "Rust" fn model(config: Table) -> (PluginModel, Option<Task<PluginMsg>>) {
    let client = native::Client::new(system::Limits::from_config(&config));
    *CLIENT
        .get_or_init(|| Mutex::new(Weak::new()))
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = Arc::downgrade(&client);
    (plugin_model(Model::new(client, &config)), None)
}

impl Model {
    fn new(client: Arc<native::Client>, config: &Table) -> Self {
        let mut model = Self {
            client,
            snapshot: Snapshot::default(),
            preferred: String::new(),
            preferred_config: String::new(),
            locker: None,
            lock_enabled: true,
            confirmation: None,
            pending: BTreeMap::new(),
            brightness_intent: None,
            generation: 0,
            error: None,
            errors: Vec::new(),
            initialized: false,
        };
        runtime::configure(&mut model, config);
        model
    }
}
#[unsafe(no_mangle)]
pub extern "Rust" fn update(model: PluginModel, message: PluginMsg) -> Option<Task<PluginMsg>> {
    let message = message.downcast_ref::<Message>()?.clone();
    with_model_write::<Model, _>(&model, |model| runtime::update(model, message)).flatten()
}
#[unsafe(no_mangle)]
pub extern "Rust" fn launch(_: usize, _: PluginModel) -> Option<Task<PluginMsg>> {
    None
}

#[unsafe(no_mangle)]
pub extern "Rust" fn session_menu(model: PluginModel) -> Vec<oxibar_plugin_api::SessionMenuEntry> {
    with_model_read::<Model, _>(&model, |model| {
        let mut entries = vec![oxibar_plugin_api::SessionMenuEntry {
            action: "lock".into(),
            label: "Lock".into(),
            description: "Start the configured screen locker".into(),
            enabled: model.lock_enabled && model.locker.is_some(),
        }];
        for action in [
            SessionAction::Logout,
            SessionAction::Suspend,
            SessionAction::Hibernate,
            SessionAction::Reboot,
            SessionAction::PowerOff,
        ] {
            let command = match action {
                SessionAction::Logout => "logout",
                SessionAction::Suspend => "suspend",
                SessionAction::Hibernate => "hibernate",
                SessionAction::Reboot => "reboot",
                SessionAction::PowerOff => "power-off",
            };
            entries.push(oxibar_plugin_api::SessionMenuEntry {
                action: command.into(),
                label: action.label().into(),
                description: "Confirm before applying this session action".into(),
                enabled: model.snapshot.can(action)
                    && !model.pending.contains_key(&Domain::Session),
            });
        }
        entries
    })
    .unwrap_or_default()
}
#[unsafe(no_mangle)]
pub extern "Rust" fn errors(model: PluginModel) -> Vec<String> {
    drain_model_errors::<Model>(&model, |model| &mut model.errors)
}
#[unsafe(no_mangle)]
pub extern "Rust" fn view(
    model: PluginModel,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    with_model_read::<Model, _>(&model, |model| vec![views::bar(model, true)])
}
#[unsafe(no_mangle)]
pub extern "Rust" fn bar_view(
    model: PluginModel,
    _: oxibar_plugin_api::PluginViewContext,
    show_labels: bool,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    with_model_read::<Model, _>(&model, |model| vec![views::bar(model, show_labels)])
}
#[unsafe(no_mangle)]
pub extern "Rust" fn popup_view(
    model: PluginModel,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    with_model_read::<Model, _>(&model, |model| {
        vec![
            iced::widget::scrollable(views::controls(model, true))
                .height(iced::Length::Fill)
                .into(),
        ]
    })
}
#[unsafe(no_mangle)]
pub extern "Rust" fn control_center_view(
    model: PluginModel,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    with_model_read::<Model, _>(&model, |model| vec![views::controls(model, false)])
}
#[unsafe(no_mangle)]
pub extern "Rust" fn control_center_available(model: PluginModel) -> bool {
    with_model_read::<Model, _>(&model, |model| {
        model.snapshot.backlight(&model.preferred).is_some() || !model.snapshot.profiles.is_empty()
    })
    .unwrap_or(false)
}
#[unsafe(no_mangle)]
pub extern "Rust" fn modal_view(
    model: PluginModel,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    with_model_read::<Model, _>(&model, |model| vec![views::confirmation(model)])
}
#[unsafe(no_mangle)]
pub extern "Rust" fn modal_closed(model: PluginModel) -> Option<Task<PluginMsg>> {
    with_model_write::<Model, _>(&model, |model| {
        model.confirmation = None;
    });
    None
}
#[unsafe(no_mangle)]
pub extern "Rust" fn subscription() -> *mut PluginStream {
    let client = CLIENT
        .get()
        .and_then(|slot| slot.lock().ok())
        .and_then(|client| client.upgrade());
    use iced::futures::StreamExt;
    let stream = match client {
        Some(client) => native::events(client)
            .map(|snapshot| plugin_message(Message::Snapshot(snapshot)))
            .boxed(),
        None => iced::futures::stream::empty().boxed(),
    };
    Box::into_raw(Box::new(stream)) as *mut PluginStream
}
#[unsafe(no_mangle)]
pub extern "Rust" fn config_changed(model: PluginModel, config: Table) -> Option<Task<PluginMsg>> {
    with_model_write::<Model, _>(&model, |model| runtime::configure(model, &config));
    None
}
#[unsafe(no_mangle)]
pub extern "Rust" fn settings_schema() -> Vec<oxibar_plugin_api::SettingDescriptor> {
    use oxibar_plugin_api::{
        ApplyMode::{Live, Restart},
        SettingDescriptor, SettingKind,
    };
    vec![
        SettingDescriptor::new(
            "system.preferred_backlight",
            "Preferred backlight",
            SettingKind::Text,
            Some(Value::String(String::new())),
            Live,
        ),
        SettingDescriptor::new(
            "system.backlight_root",
            "Backlight directory",
            SettingKind::Text,
            Some(Value::String("/sys/class/backlight".to_owned())),
            Restart,
        ),
        SettingDescriptor::new(
            "system.backlight_poll_seconds",
            "Backlight fallback interval (s)",
            SettingKind::Integer { min: 1, max: 60 },
            Some(Value::Integer(2)),
            Restart,
        )
        .description("Sysfs fallback only; power profiles and session state use D-Bus signals."),
        SettingDescriptor::new(
            "system.session_id",
            "Login session ID",
            SettingKind::Text,
            Some(Value::String(String::new())),
            Restart,
        )
        .description(
            "Empty resolves the process's session. Only a session owned by this user is accepted.",
        ),
        SettingDescriptor::new(
            "system.lock_enabled",
            "Enable locking",
            SettingKind::Boolean,
            Some(Value::Boolean(true)),
            Live,
        ),
        SettingDescriptor::new(
            "system.lock_command",
            "Dedicated locker command",
            SettingKind::Arguments,
            None,
            Live,
        ),
    ]
}
#[unsafe(no_mangle)]
pub extern "Rust" fn osd_view(
    model: PluginModel,
    topic: &str,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    with_model_read::<Model, _>(&model, |model| {
        views::osd(model, topic).map(|view| vec![view])
    })?
}
#[unsafe(no_mangle)]
pub extern "Rust" fn command(
    model: PluginModel,
    command: String,
    arguments: Vec<String>,
) -> Result<Task<CommandOutcome>, String> {
    with_model_write::<Model, _>(&model, |model| {
        runtime::command(model, &command, &arguments)
    })
    .ok_or("System model is busy")?
}

const _: oxibar_plugin_api::NameFn = name;
const _: oxibar_plugin_api::ModelFn = model;
const _: oxibar_plugin_api::UpdateFn = update;
const _: oxibar_plugin_api::SubscriptionFn = subscription;
const _: oxibar_plugin_api::CommandFn = command;
