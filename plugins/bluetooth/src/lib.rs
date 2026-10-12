//! Event-driven BlueZ controls with a user-mediated, cancellable pairing agent.

use std::sync::{Arc, Mutex, OnceLock, Weak};

use iced::{
    Element, Task,
    futures::{StreamExt, stream},
};
use oxibar_plugin_api::{
    ABI_VERSION, PluginAvailability, PluginMetadata, PluginModel, PluginMsg, PluginStream,
    drain_model_errors, plugin_model, toml::Table, with_model_read, with_model_write,
};

mod agent;
mod native;
mod runtime;
mod system;
mod views;
mod worker;

#[cfg(test)]
mod integration_tests;

use agent::{PairEvent, Prompt, Reply};
use native::{Client, Limits};
use system::{Action, Snapshot};

const ICON: &str = "󰂯";
const POPUP_SIZE: (u32, u32) = (460, 420);
static CLIENT: OnceLock<Mutex<Weak<Client>>> = OnceLock::new();

#[derive(Debug)]
pub struct Model {
    snapshot: Snapshot,
    adapter: Option<String>,
    client: Arc<Client>,
    hovered: Option<String>,
    pending: Option<String>,
    generation: u64,
    active_pair: Option<u64>,
    pair_label: Option<String>,
    cancel_requested: bool,
    pairing: Option<PairingState>,
    health: Option<String>,
    action_error: Option<String>,
    errors: Vec<String>,
}

impl Model {
    fn new(config: Table) -> Self {
        let section = config.get("bluetooth").and_then(|value| value.as_table());
        let seconds = |key: &str, default| {
            section
                .and_then(|section| section.get(key))
                .and_then(|value| value.as_integer())
                .filter(|value| *value > 0)
                .map(|value| value as u64)
                .unwrap_or(default)
        };
        let limits = Limits {
            discovery: std::time::Duration::from_secs(seconds("refresh_seconds", 20).min(3600)),
            pairing: std::time::Duration::from_secs(
                seconds("pairing_timeout_seconds", 90).clamp(10, 300),
            ),
        };
        let client = Client::new(limits);
        *CLIENT
            .get_or_init(|| Mutex::new(Weak::new()))
            .lock()
            .unwrap() = Arc::downgrade(&client);
        Self {
            snapshot: Snapshot::default(),
            adapter: None,
            client,
            hovered: None,
            pending: None,
            generation: 0,
            active_pair: None,
            pair_label: None,
            cancel_requested: false,
            pairing: None,
            health: None,
            action_error: None,
            errors: Vec::new(),
        }
    }

    fn busy(&self) -> bool {
        self.pending.is_some() || self.active_pair.is_some()
    }
}

#[derive(Clone, Debug)]
struct PairingState {
    prompt: Prompt,
    code: String,
    waiting: bool,
    error: Option<String>,
}

#[derive(Clone, Debug)]
enum Message {
    TogglePopup,
    Refresh,
    Snapshot(Result<Snapshot, String>),
    SelectAdapter(String),
    Scan,
    Power(bool),
    Hover(Option<String>),
    Pair(String),
    Disconnect(String),
    Trust(String, bool),
    Forget(String),
    ActionDone {
        generation: u64,
        result: Result<(), String>,
    },
    PairEvent(u64, PairEvent),
    CodeChanged(String),
    SubmitCode,
    Answer(bool),
    CancelPair,
}

fn msg(message: Message) -> PluginMsg {
    Arc::new(message)
}

oxibar_plugin_api::export_theme_changed!();

#[unsafe(no_mangle)]
pub extern "Rust" fn abi_version() -> u32 {
    ABI_VERSION
}
#[unsafe(no_mangle)]
pub extern "Rust" fn name() -> &'static str {
    "Bluetooth"
}
#[unsafe(no_mangle)]
pub extern "Rust" fn availability(_config: Table) -> PluginAvailability {
    PluginAvailability::Available
}
#[unsafe(no_mangle)]
pub extern "Rust" fn metadata() -> PluginMetadata {
    PluginMetadata {
        popup_size: Some(POPUP_SIZE),
        ..PluginMetadata::default()
    }
}

#[unsafe(no_mangle)]
pub extern "Rust" fn settings_schema() -> Vec<oxibar_plugin_api::SettingDescriptor> {
    use oxibar_plugin_api::{ApplyMode, SettingDescriptor, SettingKind, toml::Value};
    vec![
        SettingDescriptor::new(
            "bluetooth.refresh_seconds",
            "Bluetooth scan duration",
            SettingKind::Integer { min: 1, max: 3600 },
            Some(Value::Integer(20)),
            ApplyMode::Restart,
        )
        .description(
            "Duration of a requested discovery session. Native device state follows BlueZ events.",
        ),
        SettingDescriptor::new(
            "bluetooth.pairing_timeout_seconds",
            "Pairing timeout",
            SettingKind::Integer { min: 10, max: 300 },
            Some(Value::Integer(90)),
            ApplyMode::Restart,
        )
        .description("Maximum time for pairing, user replies and connecting the device."),
    ]
}

#[unsafe(no_mangle)]
pub extern "Rust" fn model(config: Table) -> (PluginModel, Option<Task<PluginMsg>>) {
    (
        plugin_model(Model::new(config)),
        Some(Task::done(msg(Message::Refresh))),
    )
}

#[unsafe(no_mangle)]
pub extern "Rust" fn update(model: PluginModel, message: PluginMsg) -> Option<Task<PluginMsg>> {
    let message = message.downcast_ref::<Message>()?.clone();
    with_model_write::<Model, _>(&model, |model| runtime::update(model, message)).flatten()
}

#[unsafe(no_mangle)]
pub extern "Rust" fn modal_closed(model: PluginModel) -> Option<Task<PluginMsg>> {
    with_model_write::<Model, _>(&model, runtime::cancel);
    None
}

#[unsafe(no_mangle)]
pub extern "Rust" fn launch(_index: usize, _model: PluginModel) -> Option<Task<PluginMsg>> {
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
    with_model_read::<Model, _>(&model, |model| vec![views::controls(model, false)])
}
#[unsafe(no_mangle)]
pub extern "Rust" fn control_center_view(
    model: PluginModel,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    with_model_read::<Model, _>(&model, |model| vec![views::controls(model, true)])
}
#[unsafe(no_mangle)]
pub extern "Rust" fn modal_view(
    model: PluginModel,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    with_model_read::<Model, _>(&model, |model| vec![views::modal(model)])
}

#[unsafe(no_mangle)]
pub extern "Rust" fn subscription() -> *mut PluginStream {
    let client = CLIENT
        .get()
        .and_then(|client| client.lock().unwrap().upgrade());
    let stream = match client {
        Some(client) => native::events(client)
            .map(|state| msg(Message::Snapshot(state)))
            .boxed(),
        None => stream::empty().boxed(),
    };
    Box::into_raw(Box::new(stream)) as *mut PluginStream
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_views_and_error_drain_are_deterministic() {
        let (model, task) = model(Table::new());
        assert!(task.is_some());
        assert_eq!(name(), "Bluetooth");
        assert_eq!(abi_version(), ABI_VERSION);
        assert_eq!(metadata().popup_size, Some(POPUP_SIZE));
        assert_eq!(view(model.clone()).unwrap().len(), 1);
        assert_eq!(popup_view(model.clone()).unwrap().len(), 1);
        assert_eq!(modal_view(model.clone()).unwrap().len(), 1);
        update(
            model.clone(),
            msg(Message::Snapshot(Err("fixture offline".to_owned()))),
        );
        assert_eq!(errors(model.clone()), ["fixture offline"]);
        assert!(errors(model).is_empty());
    }
}
