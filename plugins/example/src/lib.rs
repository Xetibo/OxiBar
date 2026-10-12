use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

use iced::{
    Element, Task,
    futures::{StreamExt, stream},
};
use oxibar_plugin_api::{
    ApplyMode, CommandOutcome, PluginMetadata, PluginModel, PluginMsg, PluginStream,
    PluginViewContext, SettingDescriptor, SettingKind, SubscriptionPolicy, drain_model_errors,
    plugin_model,
    toml::{Table, Value},
    with_model_read, with_model_write,
};
use oxiced::{
    theme::theme_impl::OXITHEME,
    widgets::oxi_plugin::{self, text},
};

#[derive(Debug)]
struct Model {
    label: String,
    count: u64,
    ticks: u64,
    confirmation: bool,
    errors: Vec<String>,
}

#[derive(Debug, Clone)]
enum Message {
    Popup,
    Osd,
    Tick,
    Increment,
    RequestReset,
    ConfirmReset,
    CancelReset,
}

type ModelOwner = Weak<std::sync::RwLock<Box<dyn oxibar_plugin_api::OxiAny>>>;
static OWNER: OnceLock<Mutex<ModelOwner>> = OnceLock::new();
static INITIALIZATIONS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn label(config: &Table) -> String {
    config
        .get("example")
        .and_then(Value::as_table)
        .and_then(|table| table.get("label"))
        .and_then(Value::as_str)
        .unwrap_or("Example")
        .to_owned()
}

#[unsafe(no_mangle)]
pub extern "Rust" fn abi_version() -> u32 {
    oxibar_plugin_api::ABI_VERSION
}
oxibar_plugin_api::export_theme_changed!();

#[unsafe(no_mangle)]
pub extern "Rust" fn name() -> &'static str {
    "Example"
}

#[unsafe(no_mangle)]
pub extern "Rust" fn availability(config: Table) -> oxibar_plugin_api::PluginAvailability {
    if config
        .get("example")
        .and_then(Value::as_table)
        .and_then(|table| table.get("available"))
        .and_then(Value::as_bool)
        == Some(false)
    {
        oxibar_plugin_api::PluginAvailability::Unavailable("disabled by example.available")
    } else {
        oxibar_plugin_api::PluginAvailability::Available
    }
}

// Example-only contract probe: catalog inspection and unavailable loads must not initialize models.
#[unsafe(no_mangle)]
pub extern "Rust" fn example_initializations() -> u64 {
    INITIALIZATIONS.load(std::sync::atomic::Ordering::SeqCst)
}

#[unsafe(no_mangle)]
pub extern "Rust" fn model(config: Table) -> (PluginModel, Option<Task<PluginMsg>>) {
    INITIALIZATIONS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let model = plugin_model(Model {
        label: label(&config),
        count: 0,
        ticks: 0,
        confirmation: false,
        errors: Vec::new(),
    });
    *OWNER
        .get_or_init(|| Mutex::new(Weak::new()))
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = Arc::downgrade(&model);
    (model, None)
}

#[unsafe(no_mangle)]
pub extern "Rust" fn update(model: PluginModel, message: PluginMsg) -> Option<Task<PluginMsg>> {
    let message = message.downcast_ref::<Message>()?;
    with_model_write::<Model, _>(&model, |model| match message {
        Message::Popup => Some(Task::done(Arc::new(
            oxibar_plugin_api::HOST_REQUEST_TOGGLE_POPUP.to_owned(),
        ) as PluginMsg)),
        Message::Osd => Some(Task::done(
            oxibar_plugin_api::HostOsdRequest::new("counter").into_message(),
        )),
        Message::Tick => {
            model.ticks = model.ticks.saturating_add(1);
            None
        }
        Message::Increment => {
            model.count = model.count.saturating_add(1);
            None
        }
        Message::RequestReset => {
            model.confirmation = true;
            Some(Task::done(
                Arc::new(oxibar_plugin_api::HOST_REQUEST_OPEN_MODAL.to_owned()) as PluginMsg,
            ))
        }
        Message::ConfirmReset => {
            model.count = 0;
            model.confirmation = false;
            Some(Task::done(
                Arc::new(oxibar_plugin_api::HOST_REQUEST_CLOSE_MODAL.to_owned()) as PluginMsg,
            ))
        }
        Message::CancelReset => {
            model.confirmation = false;
            Some(Task::done(
                Arc::new(oxibar_plugin_api::HOST_REQUEST_CLOSE_MODAL.to_owned()) as PluginMsg,
            ))
        }
    })
    .flatten()
}

#[unsafe(no_mangle)]
pub extern "Rust" fn launch(_: usize, model: PluginModel) -> Option<Task<PluginMsg>> {
    update(model, Arc::new(Message::Increment))
}

#[unsafe(no_mangle)]
pub extern "Rust" fn errors(model: PluginModel) -> Vec<String> {
    drain_model_errors::<Model>(&model, |model| &mut model.errors)
}

#[unsafe(no_mangle)]
pub extern "Rust" fn metadata() -> PluginMetadata {
    PluginMetadata {
        popup_size: Some((360, 320)),
        popup_input_size: None,
    }
}

#[unsafe(no_mangle)]
pub extern "Rust" fn view(
    model: PluginModel,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    with_model_read::<Model, _>(&model, |model| {
        vec![
            oxi_plugin::bar_button(text(format!("{} {}", model.label, model.count)))
                .on_press(
                    Arc::new(oxibar_plugin_api::HOST_REQUEST_TOGGLE_POPUP.to_owned()) as PluginMsg,
                )
                .into(),
        ]
    })
}

fn body(model: &Model, instance: Option<&str>) -> Element<'static, PluginMsg> {
    let mut column = iced::widget::Column::new()
        .spacing(OXITHEME.padding_lg)
        .push(text(model.label.clone()).size(OXITHEME.font_xl))
        .push(
            text(format!(
                "Count {} · visible ticks {}",
                model.count, model.ticks
            ))
            .size(OXITHEME.font_md),
        )
        .push(
            oxi_plugin::surface_button(text("Increment"), true)
                .on_press(Arc::new(Message::Increment) as PluginMsg),
        )
        .push(
            oxi_plugin::surface_button(text("Reset…"), false)
                .on_press(Arc::new(Message::RequestReset) as PluginMsg),
        );
    if let Some(instance) = instance {
        column = column.push(text(instance.to_owned()).size(OXITHEME.font_sm));
    }
    column.into()
}

#[unsafe(no_mangle)]
pub extern "Rust" fn bar_view(
    model: PluginModel,
    _: PluginViewContext,
    show_labels: bool,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    if show_labels {
        return view(model);
    }
    with_model_read::<Model, _>(&model, |_| {
        vec![
            oxi_plugin::bar_button(oxi_plugin::icon(oxi_plugin::ShellIcon::Application))
                .on_press(
                    Arc::new(oxibar_plugin_api::HOST_REQUEST_TOGGLE_POPUP.to_owned()) as PluginMsg,
                )
                .into(),
        ]
    })
}

#[unsafe(no_mangle)]
pub extern "Rust" fn popup_view(
    model: PluginModel,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    with_model_read::<Model, _>(&model, |model| vec![body(model, None)])
}

#[unsafe(no_mangle)]
pub extern "Rust" fn control_center_view(
    model: PluginModel,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    popup_view(model)
}

#[unsafe(no_mangle)]
pub extern "Rust" fn desktop_view(
    model: PluginModel,
    context: PluginViewContext,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    with_model_read::<Model, _>(&model, |model| {
        vec![body(model, Some(&context.instance_id))]
    })
}

#[unsafe(no_mangle)]
pub extern "Rust" fn modal_view(
    model: PluginModel,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    with_model_read::<Model, _>(&model, |_| {
        vec![
            iced::widget::Column::new()
                .spacing(OXITHEME.padding_lg)
                .push(text("Reset shared counter?").size(OXITHEME.font_xl))
                .push(
                    oxi_plugin::surface_button(text("Cancel"), false)
                        .on_press(Arc::new(Message::CancelReset) as PluginMsg),
                )
                .push(
                    oxi_plugin::surface_button(text("Reset"), true)
                        .on_press(Arc::new(Message::ConfirmReset) as PluginMsg),
                )
                .into(),
        ]
    })
}

#[unsafe(no_mangle)]
pub extern "Rust" fn modal_closed(model: PluginModel) -> Option<Task<PluginMsg>> {
    with_model_write::<Model, _>(&model, |model| model.confirmation = false);
    None
}

#[unsafe(no_mangle)]
pub extern "Rust" fn osd_view(
    model: PluginModel,
    topic: &str,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    if topic != "counter" {
        return Err(std::io::Error::other("Unknown Example OSD topic"));
    }
    with_model_read::<Model, _>(&model, |model| {
        vec![oxi_plugin::osd_content(
            "+",
            model.label.clone(),
            model.count.to_string(),
            None,
        )]
    })
}

#[unsafe(no_mangle)]
pub extern "Rust" fn subscription_policy() -> SubscriptionPolicy {
    SubscriptionPolicy::WhenVisible
}

fn ticks() -> iced::futures::stream::BoxStream<'static, PluginMsg> {
    stream::unfold((), |()| async {
        async_io::Timer::after(Duration::from_secs(1)).await;
        Some((Arc::new(Message::Tick) as PluginMsg, ()))
    })
    .boxed()
}

#[unsafe(no_mangle)]
pub extern "Rust" fn subscription() -> *mut PluginStream {
    let active = OWNER
        .get()
        .and_then(|owner| owner.lock().ok()?.upgrade())
        .is_some();
    let stream = if active {
        ticks()
    } else {
        stream::empty().boxed()
    };
    Box::into_raw(Box::new(stream)) as *mut PluginStream
}

#[unsafe(no_mangle)]
pub extern "Rust" fn settings_schema() -> Vec<SettingDescriptor> {
    vec![
        SettingDescriptor::new(
            "example.label",
            "Example label",
            SettingKind::Text,
            Some(Value::String("Example".to_owned())),
            ApplyMode::Live,
        ),
        SettingDescriptor::new(
            "example.available",
            "Example availability gate",
            SettingKind::Boolean,
            Some(Value::Boolean(true)),
            ApplyMode::Restart,
        ),
    ]
}

#[unsafe(no_mangle)]
pub extern "Rust" fn config_validate(config: Table) -> Result<(), String> {
    let label = label(&config);
    if label.is_empty() || label.len() > 128 || label.contains('\0') {
        Err("example.label must contain 1–128 bytes without NUL".to_owned())
    } else {
        Ok(())
    }
}

#[unsafe(no_mangle)]
pub extern "Rust" fn config_changed(model: PluginModel, config: Table) -> Option<Task<PluginMsg>> {
    with_model_write::<Model, _>(&model, |model| model.label = label(&config));
    None
}

#[unsafe(no_mangle)]
pub extern "Rust" fn command(
    _: PluginModel,
    command: String,
    arguments: Vec<String>,
) -> Result<Task<CommandOutcome>, String> {
    if !arguments.is_empty() {
        return Err("Example commands take no arguments".to_owned());
    }
    let message = match command.as_str() {
        "popup" => Message::Popup,
        "osd" => Message::Osd,
        "increment" => Message::Increment,
        "reset" => Message::RequestReset,
        _ => return Err("Unknown Example command".to_owned()),
    };
    Ok(Task::done(CommandOutcome {
        message: Arc::new(message),
        result: Ok(()),
    }))
}

const _: oxibar_plugin_api::NameFn = name;
const _: oxibar_plugin_api::ModelFn = model;
const _: oxibar_plugin_api::UpdateFn = update;
const _: oxibar_plugin_api::SubscriptionFn = subscription;

#[cfg(test)]
mod tests {
    use super::*;
    use iced::futures::FutureExt;

    #[test]
    fn shared_state_live_config_and_modal_cancellation_do_not_reset_counter() {
        let (model, _) = model(Table::new());
        update(model.clone(), Arc::new(Message::Increment));
        drop(update(model.clone(), Arc::new(Message::RequestReset)));
        modal_closed(model.clone());
        modal_closed(model.clone());
        config_changed(model.clone(), "[example]\nlabel='Live'".parse().unwrap());
        assert_eq!(
            with_model_read::<Model, _>(&model, |model| (
                model.label.clone(),
                model.count,
                model.confirmation
            ))
            .unwrap(),
            ("Live".to_owned(), 1, false)
        );
        assert!(command(model.clone(), "reset".to_owned(), Vec::new()).is_ok());
        assert!(config_validate("[example]\nlabel=''".parse().unwrap()).is_err());
        assert!(
            !desktop_view(
                model,
                PluginViewContext {
                    instance_id: "desktop:second".to_owned(),
                    output_name: None,
                    output_scale: 1.0
                }
            )
            .unwrap()
            .is_empty()
        );
    }

    #[test]
    fn visible_timer_is_cancellable_without_host_tokio() {
        let mut stream = ticks();
        assert!(stream.next().now_or_never().is_none());
        let started = std::time::Instant::now();
        drop(stream);
        assert!(started.elapsed() < Duration::from_millis(100));
    }
}
