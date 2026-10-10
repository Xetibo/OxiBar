use std::sync::{Arc, RwLock};

use iced::{Element, Task, Theme, widget::text};
use iced_layershell::reexport::IcedId;
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
    pub visible_subscription: bool,
    pub dock: bool,
    pub desktop: bool,
    pub passive_desktop: bool,
    pub compositor: bool,
    pub command: bool,
    pub osd: bool,
    pub popup: bool,
    pub modal: bool,
    pub modal_closed: bool,
    pub panel: bool,
    pub toast: bool,
    pub popup_size: Option<(u32, u32)>,
    pub popup_input_size: Option<(u32, u32)>,
    pub dynamic_popup_metrics: Option<PluginPopupMetrics>,
    pub runner: bool,
    pub runner_score: i64,
    pub live_settings: bool,
    pub control_center: bool,
}

impl FakePluginOptions {
    pub fn live_settings() -> Self {
        Self {
            live_settings: true,
            ..Self::default()
        }
    }
    pub fn runner(score: i64) -> Self {
        Self {
            runner: true,
            runner_score: score,
            ..Self::default()
        }
    }
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
    runner_score: i64,
}

#[derive(Clone, Debug)]
enum FakePluginMessage {
    Record(String),
    EmitHostRequest(String),
    EmitToastRequest(HostToastRequest),
    EmitOsd(oxibar_plugin_api::HostOsdRequest),
    Error(String),
}

pub struct TestHost {
    bar: OxiBar,
}

#[derive(Debug, Clone)]
pub struct TestOsd {
    pub id: IcedId,
    pub output: Option<String>,
    pub plugin: String,
    pub topic: String,
    pub generation: u64,
}

#[derive(Debug, Clone)]
pub struct TestDesktop {
    pub window: IcedId,
    pub id: String,
    pub plugin: String,
    pub output: Option<String>,
    pub rect: crate::desktop::Rect,
    pub interactive: bool,
}

#[derive(Debug, Clone)]
pub struct TestDock {
    pub window: IcedId,
    pub output: Option<String>,
    pub titles: Vec<String>,
    pub actions: Vec<Option<crate::dock::Action>>,
}

impl TestHost {
    pub fn desktop_widgets(&self) -> Vec<TestDesktop> {
        self.bar
            .desktop
            .instances
            .values()
            .map(|instance| TestDesktop {
                window: instance.id,
                id: instance.spec.id.clone(),
                plugin: instance.spec.plugin.clone(),
                output: instance.output.clone(),
                rect: instance.rect,
                interactive: instance.interactive,
            })
            .collect()
    }

    pub fn subscribed_plugins(&self) -> Vec<String> {
        let mut names: Vec<_> = self
            .bar
            .plugins
            .iter()
            .filter(|(name, (_, funcs))| self.bar.subscription_enabled(name, funcs))
            .map(|(name, _)| name.clone())
            .collect();
        names.sort();
        names
    }
    pub fn desktop_editors(&self) -> Vec<(Option<String>, IcedId)> {
        self.bar
            .desktop
            .editors
            .iter()
            .map(|(output, (id, _))| (output.clone(), *id))
            .collect()
    }
    pub fn desktop_draft(&self) -> Option<Vec<crate::config::desktop::WidgetSpec>> {
        self.bar
            .desktop
            .draft
            .as_ref()
            .map(|draft| draft.widgets.clone())
    }
    pub fn desktop_error(&self) -> Option<&str> {
        self.bar
            .desktop
            .draft
            .as_ref()
            .and_then(|draft| draft.error.as_deref())
    }
    pub fn docks(&self) -> Vec<TestDock> {
        self.bar
            .docks
            .instances
            .values()
            .map(|instance| TestDock {
                window: instance.id,
                output: instance.output.clone(),
                titles: instance
                    .items
                    .iter()
                    .map(|item| item.title.clone())
                    .collect(),
                actions: instance
                    .items
                    .iter()
                    .map(|item| item.action.clone())
                    .collect(),
            })
            .collect()
    }
    pub fn with_plugins<S>(plugins: impl IntoIterator<Item = (S, FakePluginOptions)>) -> Self
    where
        S: Into<String>,
    {
        let mut plugin_map = PluginMap::new();
        let mut order = Vec::new();

        for (name, options) in plugins {
            let name = name.into();
            let model = fake_model(options.dynamic_popup_metrics);
            model
                .write()
                .unwrap()
                .downcast_mut::<FakePluginModel>()
                .unwrap()
                .runner_score = options.runner_score;
            let funcs = Arc::new(fake_funcs(options));
            plugin_map.insert(name.clone(), (model, funcs));
            order.push(name);
        }

        let mut config_store = crate::config::ConfigStore::memory(Table::new(), Table::new());
        config_store
            .register_schema(
                plugin_map
                    .values()
                    .flat_map(|(_, funcs)| {
                        funcs
                            .settings_schema
                            .map(|schema| unsafe { schema() })
                            .unwrap_or_default()
                    })
                    .collect(),
            )
            .unwrap();
        let config = crate::settings::ConfigRuntime::new(config_store);
        let settings = crate::settings::SettingsUi::new(&config.snapshot);
        let mut host = Self {
            bar: OxiBar {
                docks: crate::dock::Docks::default(),
                desktop: crate::desktop::Desktop::default(),
                compositor: None,
                compositor_error: None,
                compositor_action_error: None,
                osds: crate::osd::Osds::untimed(),
                shell: crate::shell::Shell::new(Err("Test inventory".to_owned())),
                config,
                settings,
                control_center: crate::control_center::ControlCenter::default(),
                plugin_order: order.clone(),
                plugin_failures: std::collections::BTreeMap::new(),
                launcher: crate::launcher::Launcher::new(&Table::new()),
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
        };
        let _ = collect_task(host.bar.reconcile_bars());
        host
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

    pub fn osds(&self) -> Vec<TestOsd> {
        self.bar
            .osds
            .instances
            .iter()
            .map(|(output, item)| TestOsd {
                id: item.id,
                output: output.clone(),
                plugin: item.plugin.clone(),
                topic: item.topic.clone(),
                generation: item.generation,
            })
            .collect()
    }

    pub fn action(
        &mut self,
        source: Option<IcedId>,
        plugin: &str,
        action: &str,
        arguments: &[&str],
    ) -> (Vec<Message>, serde_json::Value) {
        let (reply, receiver) = crate::ipc::Reply::channel();
        let request = Message::ControlRequest(crate::ipc::ControlRequest {
            command: crate::ipc::Command::Action {
                plugin: plugin.to_owned(),
                action: action.to_owned(),
                arguments: arguments.iter().map(|value| (*value).to_owned()).collect(),
            },
            reply,
        });
        let request = source
            .map(|source| Message::FromSurface(source, Box::new(request.clone())))
            .unwrap_or(request);
        let messages = self.drive(request);
        (
            messages,
            iced::futures::executor::block_on(receiver).expect("command reply"),
        )
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

    pub fn config_snapshot(&self) -> std::sync::Arc<crate::config::ConfigSnapshot> {
        self.bar.config.snapshot.clone()
    }

    pub fn main_window(&self) -> iced_layershell::reexport::IcedId {
        self.bar.bar_window_id.expect("test bar")
    }
    pub fn bar_windows(&self) -> Vec<iced_layershell::reexport::IcedId> {
        self.bar.shell.bars.values().map(|bar| bar.id).collect()
    }
    pub fn bar_output(&self, id: iced_layershell::reexport::IcedId) -> Option<String> {
        self.bar.output_for_bar(Some(id))
    }
    pub fn bar_dimensions(
        &self,
        id: iced_layershell::reexport::IcedId,
    ) -> Option<crate::layout::BarDimensions> {
        self.bar.shell.bar(id).map(|bar| bar.size)
    }
    pub fn popup_window(&self) -> Option<iced_layershell::reexport::IcedId> {
        self.bar.shell.popup_bar
    }
    pub fn settings_window(&self) -> Option<iced_layershell::reexport::IcedId> {
        self.bar.settings.window_id
    }
    pub fn control_center_window(&self) -> Option<IcedId> {
        self.bar.control_center.window_id
    }
    pub fn control_center_selected(&self) -> Option<&str> {
        self.bar.control_center.selected.as_deref()
    }
    pub fn control_center_widgets(&self) -> Vec<String> {
        self.bar.control_center_plugins()
    }
    pub fn surface_output(&self, id: IcedId) -> Option<&str> {
        self.bar
            .shell
            .registry
            .get(id)
            .and_then(|record| record.output.as_deref())
    }
    pub fn settings_error(&self) -> Option<&str> {
        self.bar.settings.error.as_deref()
    }
    pub fn restart_fields(&self) -> Vec<String> {
        self.bar.restart_fields()
    }
    pub fn bar_size(&self) -> crate::layout::BarDimensions {
        self.bar.bar_size
    }
    pub fn widgets(&self) -> (&[String], &[String], &[String]) {
        (
            &self.bar.start_widgets,
            &self.bar.center_widgets,
            &self.bar.end_widgets,
        )
    }

    pub fn launcher_window(&self) -> Option<iced_layershell::reexport::IcedId> {
        self.bar.launcher.window_id
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

    pub fn modal_window(&self) -> Option<IcedId> {
        self.bar.modal_window_id
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

pub fn emit_osd(request: oxibar_plugin_api::HostOsdRequest) -> PluginMsg {
    Arc::new(FakePluginMessage::EmitOsd(request))
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
        subscription_policy: options
            .visible_subscription
            .then_some(fake_subscription_policy),
        dock_entries: options.dock.then_some(fake_dock_entries),
        desktop_view: options.desktop.then_some(fake_desktop_view),
        desktop_interactive: options.passive_desktop.then_some(fake_desktop_interactive),
        compositor_changed: options.compositor.then_some(fake_compositor_changed),
        command: options.command.then_some(fake_command),
        osd_view: options.osd.then_some(fake_osd_view),
        context_view: None,
        control_center_view: options.control_center.then_some(fake_view),
        filename: "libfake.so".to_owned(),
        settings_schema: options.live_settings.then_some(fake_settings_schema),
        config_changed: options.live_settings.then_some(fake_config_changed),
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
        modal_closed: options.modal_closed.then_some(fake_modal_closed),
        panel_view: options.panel.then_some(fake_panel_view),
        toast_view: options.toast.then_some(fake_toast_view),
        runner_query: options.runner.then_some(fake_runner_query),
        runner_entries: options.runner.then_some(fake_runner_entries),
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

unsafe extern "Rust" fn fake_modal_closed(model: PluginModel) -> Option<Task<PluginMsg>> {
    model
        .write()
        .unwrap()
        .downcast_mut::<FakePluginModel>()
        .unwrap()
        .seen
        .push("modal closed".to_owned());
    Some(Task::done(
        Arc::new(FakePluginMessage::Record("modal cleanup".to_owned())) as PluginMsg,
    ))
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
        FakePluginMessage::EmitOsd(request) => Some(Task::done(Arc::new(request) as PluginMsg)),
        FakePluginMessage::Error(error) => {
            model.errors.push(error);
            None
        }
    }
}

unsafe extern "Rust" fn fake_launch(
    focused_index: usize,
    model: PluginModel,
) -> Option<Task<PluginMsg>> {
    oxibar_plugin_api::with_model_write::<FakePluginModel, _>(&model, |model| {
        model.seen.push(format!("launch:{focused_index}"));
    });
    None
}

unsafe extern "Rust" fn fake_dock_entries(
    _: PluginModel,
) -> Arc<Vec<oxibar_plugin_api::DockEntry>> {
    Arc::new(vec![oxibar_plugin_api::DockEntry {
        index: 7,
        desktop_id: "fixture.desktop".into(),
        title: "Fixture application".into(),
        icon: None,
        window_class: "fixture".into(),
        executable: "fixture".into(),
    }])
}

unsafe extern "Rust" fn fake_desktop_view(
    _: PluginModel,
    context: oxibar_plugin_api::PluginViewContext,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    Ok(vec![
        text(format!("Desktop {}", context.instance_id)).into(),
    ])
}

unsafe extern "Rust" fn fake_desktop_interactive(_model: PluginModel) -> bool {
    false
}

unsafe extern "Rust" fn fake_compositor_changed(
    model: PluginModel,
    snapshot: oxibar_plugin_api::CompositorSnapshot,
) -> Option<Task<PluginMsg>> {
    oxibar_plugin_api::with_model_write::<FakePluginModel, _>(&model, |model| {
        model
            .seen
            .push(format!("compositor:{}", snapshot.windows.len()))
    });
    None
}

unsafe extern "Rust" fn fake_settings_schema() -> Vec<oxibar_plugin_api::SettingDescriptor> {
    vec![oxibar_plugin_api::SettingDescriptor::new(
        "custom.value",
        "Custom value",
        oxibar_plugin_api::SettingKind::Integer { min: 0, max: 100 },
        Some(toml::Value::Integer(1)),
        oxibar_plugin_api::ApplyMode::Live,
    )]
}

unsafe extern "Rust" fn fake_config_changed(
    model: PluginModel,
    config: Table,
) -> Option<Task<PluginMsg>> {
    oxibar_plugin_api::with_model_write::<FakePluginModel, _>(&model, |model| {
        if let Some(value) = crate::config::store::get_path(&config, "custom.value") {
            model.seen.push(format!("config:{value}"));
        }
    });
    None
}

unsafe extern "Rust" fn fake_runner_query(
    query: String,
    model: PluginModel,
) -> Option<Task<PluginMsg>> {
    oxibar_plugin_api::with_model_write::<FakePluginModel, _>(&model, |model| {
        model.seen.push(format!("query:{query}"));
    });
    None
}

unsafe extern "Rust" fn fake_runner_entries(
    model: PluginModel,
) -> Vec<oxibar_plugin_api::RunnerEntry> {
    oxibar_plugin_api::with_model_read::<FakePluginModel, _>(&model, |model| {
        vec![oxibar_plugin_api::RunnerEntry {
            index: 42,
            score: model.runner_score,
            title: "Fake result".to_owned(),
            description: String::new(),
            icon: None,
        }]
    })
    .unwrap_or_default()
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

unsafe extern "Rust" fn fake_osd_view(
    _: PluginModel,
    topic: &str,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    if topic == "bad" {
        return Err(std::io::Error::other("Bad OSD topic"));
    }
    Ok(vec![text(format!("fake OSD {topic}")).into()])
}

unsafe extern "Rust" fn fake_command(
    _: PluginModel,
    action: String,
    arguments: Vec<String>,
) -> Result<Task<oxibar_plugin_api::CommandOutcome>, String> {
    match action.as_str() {
        "record" => Ok(Task::done(oxibar_plugin_api::CommandOutcome {
            message: record(&arguments.join(" ")),
            result: Ok(()),
        })),
        "fail" => Ok(Task::done(oxibar_plugin_api::CommandOutcome {
            message: record("failed action updated"),
            result: Err("Fixture action failed".to_owned()),
        })),
        "volume" => Ok(Task::perform(
            async {
                let (sender, receiver) = iced::futures::channel::oneshot::channel();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    let _ = sender.send(());
                });
                let _ = receiver.await;
            },
            |_| oxibar_plugin_api::CommandOutcome {
                message: emit_osd(oxibar_plugin_api::HostOsdRequest::new("volume")),
                result: Ok(()),
            },
        )),
        _ => Err("Unknown fixture command".to_owned()),
    }
}

unsafe extern "Rust" fn fake_subscription() -> *mut PluginStream {
    let stream = iced::futures::stream::empty::<PluginMsg>();
    Box::into_raw(Box::new(stream)) as *mut PluginStream
}

unsafe extern "Rust" fn fake_subscription_policy() -> oxibar_plugin_api::SubscriptionPolicy {
    oxibar_plugin_api::SubscriptionPolicy::WhenVisible
}
