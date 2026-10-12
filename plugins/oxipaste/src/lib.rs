mod backend;
mod service;
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
static HISTORY_LIMIT: OnceLock<usize> = OnceLock::new();

#[derive(Debug)]
struct Model {
    items: Vec<Arc<Item>>,
    search: String,
    shortcut: String,
    refreshing: bool,
    backend_revision: u64,
    busy: bool,
    status: Option<String>,
    health: Option<String>,
    backend_health: Option<String>,
    errors: Vec<String>,
}

#[derive(Debug, Clone)]
enum Message {
    Open,
    Refresh,
    Snapshot(Result<Vec<Arc<Item>>, String>, u64),
    Backend(service::Snapshot),
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
    HISTORY_LIMIT.get_or_init(|| {
        settings
            .and_then(|table| table.get("max_items"))
            .and_then(Value::as_integer)
            .unwrap_or(100)
            .clamp(1, 5000) as usize
    });
    (
        plugin_model(Model {
            items: Vec::new(),
            search: String::new(),
            shortcut,
            refreshing: false,
            backend_revision: 0,
            busy: false,
            status: None,
            health: None,
            backend_health: Some("Starting clipboard backend…".into()),
            errors: Vec::new(),
        }),
        None,
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
            let revision = model.backend_revision;
            Some(Task::perform(backend::read(), move |result| {
                msg(Message::Snapshot(result, revision))
            }))
        }
        Message::Snapshot(result, revision) => {
            model.refreshing = false;
            if revision != model.backend_revision {
                return None;
            }
            match result {
                Ok(items) => {
                    model.items = items;
                    model.health = None;
                }
                Err(error) => problem(model, error),
            }
            None
        }
        Message::Backend(snapshot) => {
            model.backend_revision = model.backend_revision.wrapping_add(1);
            if let Some(items) = snapshot.items {
                model.items = items;
            }
            if let Some(error) = &snapshot.error
                && model.backend_health.as_ref() != Some(error)
            {
                model.errors.push(error.clone());
            }
            model.backend_health = snapshot.error;
            model.health = None;
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
    use iced::futures::StreamExt;
    let stream = service::events(
        INTERVAL.get().copied().unwrap_or(Duration::from_secs(2)),
        HISTORY_LIMIT.get().copied().unwrap_or(100),
    )
    .map(|snapshot| msg(Message::Backend(snapshot)))
    .boxed();
    Box::into_raw(Box::new(stream)) as *mut PluginStream
}
#[unsafe(no_mangle)]
pub extern "Rust" fn subscription_policy() -> SubscriptionPolicy {
    SubscriptionPolicy::Always
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
        if let Some(value) = settings.get("max_items")
            && !value
                .as_integer()
                .is_some_and(|count| (1..=5000).contains(&count))
        {
            return Err("oxipaste.max_items must be 1–5000".into());
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
            "External clipboard history refresh",
            SettingKind::Integer { min: 1, max: 3600 },
            Some(Value::Integer(2)),
            ApplyMode::Restart,
        ),
        SettingDescriptor::new(
            "oxipaste.max_items",
            "Clipboard history limit",
            SettingKind::Integer { min: 1, max: 5000 },
            Some(Value::Integer(100)),
            ApplyMode::Restart,
        ),
    ]
}

const _: oxibar_plugin_api::ModelFn = model;
const _: oxibar_plugin_api::UpdateFn = update;
const _: oxibar_plugin_api::SubscriptionFn = subscription;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resident_backend_updates_win_over_in_flight_manual_refreshes() {
        let (model, initial) = model(Table::new());
        assert!(initial.is_none());
        assert_eq!(subscription_policy(), SubscriptionPolicy::Always);
        let selected = Arc::new(Item {
            bytes: b"captured while picker closed".to_vec(),
            mime: "text/plain".into(),
        });
        update(
            model.clone(),
            msg(Message::Backend(service::Snapshot {
                items: Some(vec![selected.clone()]),
                error: None,
            })),
        );
        update(model.clone(), msg(Message::Snapshot(Ok(Vec::new()), 0)));
        update(
            model.clone(),
            msg(Message::Backend(service::Snapshot {
                items: None,
                error: Some("Clipboard service unavailable".into()),
            })),
        );
        with_model_read::<Model, _>(&model, |model| {
            assert_eq!(model.items, vec![selected]);
            assert_eq!(
                model.backend_health.as_deref(),
                Some("Clipboard service unavailable")
            );
        })
        .unwrap();
        update(
            model.clone(),
            msg(Message::Backend(service::Snapshot {
                items: Some(Vec::new()),
                error: None,
            })),
        );
        with_model_read::<Model, _>(&model, |model| {
            assert!(model.items.is_empty());
            assert!(model.backend_health.is_none());
        })
        .unwrap();
        assert!(
            config_validate(
                oxibar_plugin_api::toml::from_str("[oxipaste]\nmax_items=5001").unwrap()
            )
            .is_err()
        );
    }
}
