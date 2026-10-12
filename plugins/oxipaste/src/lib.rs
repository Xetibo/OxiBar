mod backend;
mod view;

use backend::{Action, Item};
use iced::{Element, Task};
use oxibar_plugin_api::{
    ABI_VERSION, ApplyMode, HostPasteRequest, PluginMetadata, PluginModel, PluginMsg, PluginStream,
    PluginViewContext, SettingDescriptor, SettingKind, SubscriptionPolicy, drain_model_errors,
    plugin_model,
    toml::{Table, Value},
    with_model_read, with_model_write,
};
use std::{
    sync::{Arc, OnceLock},
    time::Duration,
};

static INTERVAL: OnceLock<Duration> = OnceLock::new();

#[derive(Debug)]
struct Model {
    items: Vec<Arc<Item>>,
    search: String,
    shortcut: String,
    refreshing: bool,
    busy: bool,
    status: Option<String>,
    health: Option<String>,
    errors: Vec<String>,
}

#[derive(Debug, Clone)]
enum Message {
    Open,
    Refresh,
    Snapshot(Result<Vec<Arc<Item>>, String>),
    Search(String),
    Act(Action, bool),
    Finished(Result<(), String>, bool),
    Pasted(Result<(), String>),
}

fn msg(message: Message) -> PluginMsg {
    Arc::new(message)
}

fn problem(model: &mut Model, error: String) {
    if model.health.as_ref() != Some(&error) {
        model.errors.push(error.clone());
    }
    model.health = Some(error);
}

fn action_problem(model: &mut Model, error: String) {
    model.errors.push(error.clone());
    model.status = Some(error);
}

oxibar_plugin_api::export_theme_changed!();

#[unsafe(no_mangle)]
pub extern "Rust" fn abi_version() -> u32 {
    ABI_VERSION
}
#[unsafe(no_mangle)]
pub extern "Rust" fn name() -> &'static str {
    "OxiPaste"
}
#[unsafe(no_mangle)]
pub extern "Rust" fn model(config: Table) -> (PluginModel, Option<Task<PluginMsg>>) {
    let settings = config.get("oxipaste").and_then(Value::as_table);
    INTERVAL.get_or_init(|| {
        Duration::from_secs(
            settings
                .and_then(|table| table.get("poll_seconds"))
                .and_then(Value::as_integer)
                .unwrap_or(2)
                .clamp(1, 3600) as u64,
        )
    });
    let shortcut = settings
        .and_then(|table| table.get("paste_shortcut"))
        .and_then(Value::as_str)
        .unwrap_or("ctrl+v")
        .to_owned();
    (
        plugin_model(Model {
            items: Vec::new(),
            search: String::new(),
            shortcut,
            refreshing: false,
            busy: false,
            status: None,
            health: None,
            errors: Vec::new(),
        }),
        Some(Task::done(msg(Message::Refresh))),
    )
}

#[unsafe(no_mangle)]
pub extern "Rust" fn update(model: PluginModel, message: PluginMsg) -> Option<Task<PluginMsg>> {
    let message = message.downcast_ref::<Message>().cloned().or_else(|| {
        message
            .downcast_ref::<String>()
            .and_then(|value| HostPasteRequest::parse_result(value))
            .map(Message::Pasted)
    })?;
    with_model_write::<Model, _>(&model, |model| match message {
        Message::Open => Some(Task::done(Arc::new(
            oxibar_plugin_api::HOST_REQUEST_TOGGLE_POPUP.to_owned(),
        ) as PluginMsg)),
        Message::Refresh => {
            if model.refreshing || model.busy {
                return None;
            }
            model.refreshing = true;
            Some(Task::perform(backend::read(), |result| {
                msg(Message::Snapshot(result))
            }))
        }
        Message::Snapshot(result) => {
            model.refreshing = false;
            match result {
                Ok(items) => {
                    model.items = items;
                    model.health = None;
                }
                Err(error) => problem(model, error),
            }
            None
        }
        Message::Search(query) => {
            model.search = query;
            None
        }
        Message::Act(action, paste) => {
            if model.busy {
                return None;
            }
            model.busy = true;
            model.status = Some(
                if paste {
                    "Preparing paste…"
                } else {
                    "Working…"
                }
                .into(),
            );
            model.health = None;
            Some(Task::perform(backend::act(action), move |result| {
                msg(Message::Finished(result, paste))
            }))
        }
        Message::Finished(result, paste) => {
            match result {
                Ok(()) if paste => {
                    model.status = Some("Returning keyboard focus…".into());
                    return Some(Task::done(
                        HostPasteRequest {
                            shortcut: model.shortcut.clone(),
                        }
                        .into_message(),
                    ));
                }
                Ok(()) => model.status = Some("Done".into()),
                Err(error) => action_problem(model, error),
            }
            model.busy = false;
            Some(Task::done(msg(Message::Refresh)))
        }
        Message::Pasted(result) => {
            model.busy = false;
            match result {
                Ok(()) => model.status = Some("Pasted".into()),
                Err(error) => action_problem(model, error),
            }
            None
        }
    })
    .flatten()
}

#[unsafe(no_mangle)]
pub extern "Rust" fn launch(_: usize, _: PluginModel) -> Option<Task<PluginMsg>> {
    Some(Task::done(
        Arc::new(oxibar_plugin_api::HOST_REQUEST_TOGGLE_POPUP.to_owned()) as PluginMsg,
    ))
}
#[unsafe(no_mangle)]
pub extern "Rust" fn errors(model: PluginModel) -> Vec<String> {
    drain_model_errors::<Model>(&model, |model| &mut model.errors)
}
#[unsafe(no_mangle)]
pub extern "Rust" fn metadata() -> PluginMetadata {
    PluginMetadata {
        popup_size: Some((540, 560)),
        popup_input_size: None,
    }
}
#[unsafe(no_mangle)]
pub extern "Rust" fn subscription() -> *mut PluginStream {
    let stream = iced::futures::stream::unfold(true, |first| async move {
        if !first {
            async_io::Timer::after(INTERVAL.get().copied().unwrap_or(Duration::from_secs(2))).await;
        }
        Some((msg(Message::Refresh), false))
    });
    Box::into_raw(Box::new(Box::pin(stream))) as *mut PluginStream
}
#[unsafe(no_mangle)]
pub extern "Rust" fn subscription_policy() -> SubscriptionPolicy {
    SubscriptionPolicy::WhenVisible
}
#[unsafe(no_mangle)]
pub extern "Rust" fn view(
    model: PluginModel,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    view::bar(model, true)
}
#[unsafe(no_mangle)]
pub extern "Rust" fn bar_view(
    model: PluginModel,
    _: PluginViewContext,
    show_labels: bool,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    view::bar(model, show_labels)
}
#[unsafe(no_mangle)]
pub extern "Rust" fn popup_view(
    model: PluginModel,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    with_model_read::<Model, _>(&model, |model| vec![view::body(model, true)])
}
#[unsafe(no_mangle)]
pub extern "Rust" fn control_center_view(
    model: PluginModel,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    with_model_read::<Model, _>(&model, |model| vec![view::body(model, false)])
}
#[unsafe(no_mangle)]
pub extern "Rust" fn desktop_view(
    model: PluginModel,
    _: PluginViewContext,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    popup_view(model)
}
#[unsafe(no_mangle)]
pub extern "Rust" fn desktop_interactive(_: PluginModel) -> bool {
    true
}
#[unsafe(no_mangle)]
pub extern "Rust" fn config_validate(config: Table) -> Result<(), String> {
    if let Some(settings) = config.get("oxipaste") {
        let settings = settings.as_table().ok_or("oxipaste must be a table")?;
        if let Some(value) = settings.get("paste_shortcut")
            && !matches!(value.as_str(), Some("ctrl+v" | "ctrl+shift+v"))
        {
            return Err("oxipaste.paste_shortcut must be ctrl+v or ctrl+shift+v".into());
        }
        if let Some(value) = settings.get("poll_seconds")
            && !value
                .as_integer()
                .is_some_and(|seconds| (1..=3600).contains(&seconds))
        {
            return Err("oxipaste.poll_seconds must be 1–3600".into());
        }
    }
    Ok(())
}
#[unsafe(no_mangle)]
pub extern "Rust" fn settings_schema() -> Vec<SettingDescriptor> {
    vec![
        SettingDescriptor::new(
            "oxipaste.paste_shortcut",
            "Clipboard paste shortcut",
            SettingKind::Choice(vec!["ctrl+v".into(), "ctrl+shift+v".into()]),
            Some(Value::String("ctrl+v".into())),
            ApplyMode::Restart,
        ),
        SettingDescriptor::new(
            "oxipaste.poll_seconds",
            "Clipboard history refresh",
            SettingKind::Integer { min: 1, max: 3600 },
            Some(Value::Integer(2)),
            ApplyMode::Restart,
        ),
    ]
}

const _: oxibar_plugin_api::ModelFn = model;
const _: oxibar_plugin_api::UpdateFn = update;
const _: oxibar_plugin_api::SubscriptionFn = subscription;
