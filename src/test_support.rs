use std::sync::{Arc, RwLock};

use iced::{Element, Task, Theme, widget::text};
use iced_runtime::Action;
use oxibar_plugin_api::{
    HOST_REQUEST_CLOSE_MODAL, HOST_REQUEST_OPEN_MODAL, HOST_REQUEST_TOGGLE_PANEL,
    HOST_REQUEST_TOGGLE_POPUP, HostToastRequest, OxiAny, PluginMetadata, PluginModel, PluginMsg,
    PluginPopupMetrics, PluginStream, toml::Table,
};

use crate::{
    app::OxiBar,
    layout,
    messages::Message,
    plugins::{PluginFuncs, PluginMap},
};

pub use oxibar_plugin_api::{
    HOST_REQUEST_CLOSE_MODAL as CLOSE_MODAL, HOST_REQUEST_OPEN_MODAL as OPEN_MODAL,
    HOST_REQUEST_TOGGLE_PANEL as TOGGLE_PANEL, HOST_REQUEST_TOGGLE_POPUP as TOGGLE_POPUP,
};

const MAX_DRIVE_STEPS: usize = 32;

#[derive(Clone, Copy, Debug, Default)]
pub struct FakePluginOptions {
    pub popup: bool,
    pub modal: bool,
    pub panel: bool,
    pub toast: bool,
    pub popup_size: Option<(u32, u32)>,
    pub popup_input_size: Option<(u32, u32)>,
    pub dynamic_popup_metrics: Option<PluginPopupMetrics>,
}

impl FakePluginOptions {
    pub fn popup() -> Self {
        Self {
            popup: true,
            ..Self::default()
        }
    }

    pub fn modal() -> Self {
        Self {
            modal: true,
            ..Self::default()
        }
    }

    pub fn panel() -> Self {
        Self {
            panel: true,
            ..Self::default()
        }
    }

    pub fn toast() -> Self {
        Self {
            toast: true,
            ..Self::default()
        }
    }

    pub fn all_surfaces() -> Self {
        Self {
            popup: true,
            modal: true,
            panel: true,
            toast: true,
            ..Self::default()
        }
    }

    pub fn with_popup_size(mut self, width: u32, height: u32) -> Self {
        self.popup_size = Some((width, height));
        self
    }

    pub fn with_popup_input_size(mut self, width: u32, height: u32) -> Self {
        self.popup_input_size = Some((width, height));
        self
    }

    pub fn with_dynamic_popup_metrics(
        mut self,
        popup_size: (u32, u32),
        popup_input_size: Option<(u32, u32)>,
    ) -> Self {
        self.dynamic_popup_metrics = Some(PluginPopupMetrics {
            popup_size: Some(popup_size),
            popup_input_size,
        });
        self
    }
}

#[derive(Debug, Default)]
struct FakePluginModel {
    seen: Vec<String>,
    errors: Vec<String>,
    dynamic_popup_metrics: Option<PluginPopupMetrics>,
}

#[derive(Clone, Debug)]
enum FakePluginMessage {
    Record(String),
    EmitHostRequest(String),
    EmitToastRequest(HostToastRequest),
    Error(String),
}

pub struct TestHost {
    bar: OxiBar,
}

impl TestHost {
    pub fn with_plugins<S>(plugins: impl IntoIterator<Item = (S, FakePluginOptions)>) -> Self
    where
        S: Into<String>,
    {
        let mut plugin_map = PluginMap::new();
        let mut order = Vec::new();

        for (name, options) in plugins {
            let name = name.into();
            let model = fake_model(options.dynamic_popup_metrics);
            let funcs = Arc::new(fake_funcs(options));
            plugin_map.insert(name.clone(), (model, funcs));
            order.push(name);
        }

        Self {
            bar: OxiBar {
                theme: Theme::Dark,
                plugins: plugin_map,
                transparent: false,
                bar_window_id: None,
                bar_size: layout::DEFAULT_BAR_SIZE,
                start_widgets: order,
                center_widgets: Vec::new(),
                end_widgets: Vec::new(),
                popup_plugin: None,
                popup_open: false,
                modal_plugin: None,
                modal_window_id: None,
                panel_plugin: None,
                panel_window_id: None,
                toasts: Vec::new(),
            },
        }
    }

    pub fn drive_plugin_request(&mut self, plugin_id: &str, request: &str) -> Vec<Message> {
        self.drive(Message::PluginSubMsg(
            plugin_id.to_owned(),
            emit_host_request(request),
        ))
    }

    pub fn drive_plugin_record(&mut self, plugin_id: &str, value: &str) -> Vec<Message> {
        self.drive(Message::PluginSubMsg(plugin_id.to_owned(), record(value)))
    }

    pub fn drive_plugin_toast_show(
        &mut self,
        plugin_id: &str,
        toast_id: &str,
        width: u32,
        height: u32,
    ) -> Vec<Message> {
        self.drive(Message::PluginSubMsg(
            plugin_id.to_owned(),
            emit_toast_request(HostToastRequest::show(toast_id, width, height)),
        ))
    }

    pub fn drive_plugin_toast_close(&mut self, plugin_id: &str, toast_id: &str) -> Vec<Message> {
        self.drive(Message::PluginSubMsg(
            plugin_id.to_owned(),
            emit_toast_request(HostToastRequest::close(toast_id)),
        ))
    }

    pub fn drive(&mut self, message: Message) -> Vec<Message> {
        let mut applied = Vec::new();
        let mut pending = vec![message];

        while let Some(message) = pending.pop() {
            assert!(
                applied.len() < MAX_DRIVE_STEPS,
                "task/message loop exceeded {MAX_DRIVE_STEPS} steps"
            );
            let outputs = collect_task(self.bar.update(message.clone()));
            applied.push(message);
            pending.extend(outputs.into_iter().rev());
        }

        applied
    }

    pub fn popup_plugin(&self) -> Option<&str> {
        self.bar.popup_plugin.as_deref()
    }

    pub fn popup_open(&self) -> bool {
        self.bar.popup_open
    }

    pub fn modal_plugin(&self) -> Option<&str> {
        self.bar.modal_plugin.as_deref()
    }

    pub fn modal_open(&self) -> bool {
        self.bar.modal_window_id.is_some()
    }

    pub fn panel_plugin(&self) -> Option<&str> {
        self.bar.panel_plugin.as_deref()
    }

    pub fn panel_open(&self) -> bool {
        self.bar.panel_window_id.is_some()
    }

    pub fn toast_count(&self) -> usize {
        self.bar.toasts.len()
    }

    pub fn toast_ids(&self) -> Vec<String> {
        self.bar
            .toasts
            .iter()
            .map(|toast| toast.toast_id.clone())
            .collect()
    }

    pub fn seen_by_plugin(&self, plugin_id: &str) -> Vec<String> {
        let Some((model, _)) = self.bar.plugins.get(plugin_id) else {
            return Vec::new();
        };
        let Ok(guard) = model.read() else {
            return Vec::new();
        };
        guard
            .downcast_ref::<FakePluginModel>()
            .map(|model| model.seen.clone())
            .unwrap_or_default()
    }
}

pub fn collect_task<T: Send + 'static>(task: Task<T>) -> Vec<T> {
    let Some(mut stream) = iced_runtime::task::into_stream(task.collect()) else {
        return Vec::new();
    };

    iced::futures::executor::block_on(async move {
        use iced::futures::StreamExt;

        match stream.next().await {
            Some(Action::Output(outputs)) => outputs,
            Some(_) => panic!("task produced a non-output iced runtime action"),
            None => Vec::new(),
        }
    })
}

pub fn record(value: &str) -> PluginMsg {
    Arc::new(FakePluginMessage::Record(value.to_owned()))
}

pub fn emit_host_request(request: &str) -> PluginMsg {
    Arc::new(FakePluginMessage::EmitHostRequest(request.to_owned()))
}

pub fn emit_toast_request(request: HostToastRequest) -> PluginMsg {
    Arc::new(FakePluginMessage::EmitToastRequest(request))
}

pub fn error(value: &str) -> PluginMsg {
    Arc::new(FakePluginMessage::Error(value.to_owned()))
}

pub fn is_host_request(message: &Message, plugin_id: &str, request: &str) -> bool {
    matches!(
        (message, request),
        (Message::TogglePluginPopup(id), HOST_REQUEST_TOGGLE_POPUP) if id == plugin_id
    ) || matches!(
        (message, request),
        (Message::OpenPluginModal(id), HOST_REQUEST_OPEN_MODAL) if id == plugin_id
    ) || matches!(
        (message, request),
        (Message::ClosePluginModal(id), HOST_REQUEST_CLOSE_MODAL) if id == plugin_id
    ) || matches!(
        (message, request),
        (Message::TogglePluginPanel(id), HOST_REQUEST_TOGGLE_PANEL) if id == plugin_id
    )
}

fn fake_model(dynamic_popup_metrics: Option<PluginPopupMetrics>) -> PluginModel {
    let model: Box<dyn OxiAny> = Box::new(FakePluginModel {
        dynamic_popup_metrics,
        ..FakePluginModel::default()
    });
    Arc::new(RwLock::new(model))
}

fn fake_funcs(options: FakePluginOptions) -> PluginFuncs {
    PluginFuncs {
        _lib: Arc::new(current_library()),
        model: fake_model_fn,
        update: fake_update,
        launch: fake_launch,
        view: fake_view,
        errors: fake_errors,
        name: fake_name,
        metadata: PluginMetadata {
            popup_size: options.popup_size,
            popup_input_size: options.popup_input_size,
        },
        popup_metrics: options
            .dynamic_popup_metrics
            .is_some()
            .then_some(fake_popup_metrics),
        subscription: fake_subscription,
        popup_view: options.popup.then_some(fake_popup_view),
        modal_view: options.modal.then_some(fake_modal_view),
        panel_view: options.panel.then_some(fake_panel_view),
        toast_view: options.toast.then_some(fake_toast_view),
    }
}

#[cfg(unix)]
fn current_library() -> libloading::Library {
    libloading::os::unix::Library::this().into()
}

#[cfg(windows)]
fn current_library() -> libloading::Library {
    libloading::os::windows::Library::this()
        .expect("current process library should load")
        .into()
}

unsafe extern "Rust" fn fake_model_fn(
    _global_config: Table,
) -> (PluginModel, Option<Task<PluginMsg>>) {
    (fake_model(None), None)
}

unsafe extern "Rust" fn fake_popup_metrics(model: PluginModel) -> PluginPopupMetrics {
    let Ok(guard) = model.try_read() else {
        return PluginPopupMetrics::default();
    };
    guard
        .downcast_ref::<FakePluginModel>()
        .and_then(|model| model.dynamic_popup_metrics)
        .unwrap_or_default()
}

unsafe extern "Rust" fn fake_update(
    model: PluginModel,
    msg_in: PluginMsg,
) -> Option<Task<PluginMsg>> {
    let mut guard = model.try_write().ok()?;
    let model = guard.downcast_mut::<FakePluginModel>()?;
    let message = msg_in.downcast_ref::<FakePluginMessage>()?.clone();

    match message {
        FakePluginMessage::Record(value) => {
            model.seen.push(value);
            None
        }
        FakePluginMessage::EmitHostRequest(request) => {
            model.seen.push(request.clone());
            Some(Task::done(Arc::new(request)))
        }
        FakePluginMessage::EmitToastRequest(request) => {
            model.seen.push(request.toast_id.clone());
            Some(Task::done(Arc::new(request.to_host_request_string())))
        }
        FakePluginMessage::Error(error) => {
            model.errors.push(error);
            None
        }
    }
}

unsafe extern "Rust" fn fake_launch(
    _focused_index: usize,
    _model: PluginModel,
) -> Option<Task<PluginMsg>> {
    None
}

unsafe extern "Rust" fn fake_errors(model: PluginModel) -> Vec<String> {
    let Ok(mut guard) = model.try_write() else {
        return Vec::new();
    };
    let Some(model) = guard.downcast_mut::<FakePluginModel>() else {
        return Vec::new();
    };
    std::mem::take(&mut model.errors)
}

unsafe extern "Rust" fn fake_name() -> &'static str {
    "Fake"
}

unsafe extern "Rust" fn fake_view(
    _model: PluginModel,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    Ok(vec![text("fake").into()])
}

unsafe extern "Rust" fn fake_popup_view(
    _model: PluginModel,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    Ok(vec![text("fake popup").into()])
}

unsafe extern "Rust" fn fake_modal_view(
    _model: PluginModel,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    Ok(vec![text("fake modal").into()])
}

unsafe extern "Rust" fn fake_panel_view(
    _model: PluginModel,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    Ok(vec![text("fake panel").into()])
}

unsafe extern "Rust" fn fake_toast_view(
    _model: PluginModel,
    toast_id: &str,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    Ok(vec![text(format!("fake toast {toast_id}")).into()])
}

unsafe extern "Rust" fn fake_subscription() -> *mut PluginStream {
    let stream = iced::futures::stream::empty::<PluginMsg>();
    Box::into_raw(Box::new(stream)) as *mut PluginStream
}
