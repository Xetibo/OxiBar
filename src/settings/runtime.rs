use std::{
    collections::BTreeSet,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use iced::Task;
use iced_layershell::reexport::IcedId;
use oxibar_plugin_api::{ApplyMode, SettingDescriptor};
use serde_json::json;
use toml::{Table, Value};

use super::{Action, Draft, PluginStatus, draft_value, edit_value};
use crate::{
    app::OxiBar,
    background,
    config::{
        ConfigSnapshot, ConfigStore, Mutation,
        store::{get_path, set_path},
    },
    ipc::{self, Command, ConfigCommand, ControlRequest, Reply, WindowCommand},
    messages::{Message, map_plugin_message},
};

pub(crate) struct ConfigRuntime {
    pub store: Arc<Mutex<ConfigStore>>,
    pub snapshot: Arc<ConfigSnapshot>,
    pub boot: Arc<ConfigSnapshot>,
    pub directory: PathBuf,
    pub endpoint: ipc::Endpoint,
    pub watch_paths: crate::config::WatchPaths,
    pub jobs: usize,
    pub reload_queued: bool,
}

impl ConfigRuntime {
    pub(crate) fn new(store: ConfigStore) -> Self {
        Self::shared(Arc::new(Mutex::new(store)))
    }

    pub(crate) fn shared(store: Arc<Mutex<ConfigStore>>) -> Self {
        let (snapshot, directory) = {
            let store = store.lock().expect("initial config store");
            (store.snapshot(), store.directory.clone())
        };
        let endpoint = ipc::Endpoint {
            path: ipc::socket_path(&snapshot.effective),
            record: directory.join("control.socket"),
        };
        let watch_paths = crate::config::WatchPaths::new(&directory);
        Self {
            store,
            snapshot: snapshot.clone(),
            boot: snapshot,
            directory,
            endpoint,
            watch_paths,
            jobs: 0,
            reload_queued: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConfigOutcome {
    pub snapshot: Arc<ConfigSnapshot>,
    pub error: Option<String>,
    pub reply: Option<Reply>,
    pub edited_path: Option<String>,
    pub clear_editor: bool,
}

impl OxiBar {
    pub(crate) fn update_settings(&mut self, action: Action) -> Task<Message> {
        match action {
            Action::Open | Action::Toggle => {
                if self.settings.window_id.is_some() {
                    return if matches!(action, Action::Toggle) {
                        self.close_settings()
                    } else {
                        Task::none()
                    };
                }
                self.refresh_plugin_catalog();
                self.settings.sync_editor(&self.config.snapshot);
                let id = IcedId::unique();
                let output = self.overlay_output();
                self.register_overlay(
                    id,
                    crate::shell::SurfaceKind::Settings,
                    self.shell.active_bar,
                    output.clone(),
                );
                self.settings.window_id = Some(id);
                self.settings.awaiting_focus = true;
                self.popup_open = false;
                let close_launcher = self.close_launcher();
                let close_modal = self.close_modal();
                let close_panel = self.close_panel();
                let close_control_center = self.close_control_center();
                let close_editor = if self.desktop.draft.is_some() {
                    self.cancel_desktop_edit()
                } else {
                    Task::none()
                };
                Task::batch([
                    close_launcher,
                    close_modal,
                    close_panel,
                    close_control_center,
                    close_editor,
                    self.current_input_region_task(),
                    Task::done(Message::OnOutput(
                        output,
                        Box::new(Message::OpenSettingsLayer(
                            id,
                            self.config.snapshot.host.settings_size.0,
                            self.config.snapshot.host.settings_size.1,
                        )),
                    )),
                ])
            }
            Action::Close => self.close_settings(),
            Action::Category(category) => {
                self.settings.category = category;
                Task::none()
            }
            Action::Search(query) => {
                self.settings.search = query;
                Task::none()
            }
            Action::Edit(path, value) if self.config.jobs == 0 => {
                self.settings
                    .drafts
                    .entry(path)
                    .and_modify(|draft| draft.value = value.clone())
                    .or_insert(Draft {
                        value,
                        revision: self.config.snapshot.revision,
                    });
                Task::none()
            }
            Action::Apply(path) if self.config.jobs == 0 => {
                let Some(field) = self
                    .config
                    .snapshot
                    .schema
                    .iter()
                    .find(|field| field.path == path)
                else {
                    return Task::none();
                };
                let draft = self
                    .settings
                    .drafts
                    .get(&path)
                    .cloned()
                    .unwrap_or_else(|| Draft {
                        value: edit_value(
                            get_path(&self.config.snapshot.effective, &path),
                            &field.kind,
                        ),
                        revision: self.config.snapshot.revision,
                    });
                match draft_value(field, &draft.value) {
                    Ok(value) => self.config_job(
                        Some(Mutation::Set(path.clone(), value)),
                        Some(draft.revision),
                        None,
                        Some(path),
                        false,
                    ),
                    Err(error) => {
                        self.settings.error = Some(error);
                        Task::none()
                    }
                }
            }
            Action::Set(path, value) if self.config.jobs == 0 => self.config_job(
                Some(Mutation::Set(path.clone(), value)),
                Some(self.config.snapshot.revision),
                None,
                Some(path),
                false,
            ),
            Action::Reset(path) if self.config.jobs == 0 => self.config_job(
                Some(Mutation::Reset(path.clone())),
                Some(self.config.snapshot.revision),
                None,
                Some(path),
                false,
            ),
            Action::ResetAll if self.config.jobs == 0 => self.config_job(
                Some(Mutation::ResetAll),
                Some(self.config.snapshot.revision),
                None,
                None,
                true,
            ),
            Action::Reload => {
                self.settings.drafts.clear();
                self.settings.editor_dirty = false;
                self.reload_config(None)
            }
            Action::SelectWidget(section, name) => {
                self.settings.widget_choice.insert(section, name);
                Task::none()
            }
            Action::AddWidget(section) if self.config.jobs == 0 => {
                let Some(name) = self.settings.widget_choice.remove(&section) else {
                    return Task::none();
                };
                self.edit_widgets(&section, |widgets| widgets.push(name))
            }
            Action::RemoveWidget(section, index) if self.config.jobs == 0 => {
                self.edit_widgets(&section, |widgets| {
                    if index < widgets.len() {
                        widgets.remove(index);
                    }
                })
            }
            Action::MoveWidget(section, index, forward) if self.config.jobs == 0 => self
                .edit_widgets(&section, |widgets| {
                    if let Some(next) = index.checked_add_signed(if forward { 1 } else { -1 })
                        && index < widgets.len()
                        && next < widgets.len()
                    {
                        widgets.swap(index, next);
                    }
                }),
            Action::EnablePlugin(filename, enabled) if self.config.jobs == 0 => {
                let mut files: Vec<_> =
                    crate::config::get_allowed_plugins(&self.config.snapshot.effective)
                        .into_iter()
                        .map(str::to_owned)
                        .collect();
                if enabled && !files.contains(&filename) {
                    files.push(filename);
                } else if !enabled {
                    files.retain(|file| file != &filename);
                }
                self.config_job(
                    Some(Mutation::Set(
                        "plugins".to_owned(),
                        Value::Array(files.into_iter().map(Value::String).collect()),
                    )),
                    Some(self.config.snapshot.revision),
                    None,
                    None,
                    false,
                )
            }
            Action::Advanced(action) if self.config.jobs == 0 => {
                if action.is_edit() {
                    self.settings.editor_dirty = true;
                }
                self.settings.editor.perform(action);
                Task::none()
            }
            Action::SaveAdvanced if self.config.jobs == 0 => {
                match toml::from_str::<Table>(&self.settings.editor.text()) {
                    Ok(table) => self.config_job(
                        Some(Mutation::Replace(table)),
                        Some(self.settings.editor_revision),
                        None,
                        None,
                        true,
                    ),
                    Err(error) => {
                        self.settings.error = Some(error.to_string());
                        Task::none()
                    }
                }
            }
            _ => Task::none(),
        }
    }

    pub(crate) fn close_settings(&mut self) -> Task<Message> {
        self.settings
            .window_id
            .take()
            .map(|id| {
                self.shell.registry.remove(id);
                Task::done(Message::CloseSettingsLayer(id))
            })
            .unwrap_or_else(Task::none)
    }

    fn edit_widgets(
        &mut self,
        section: &str,
        edit: impl FnOnce(&mut Vec<String>),
    ) -> Task<Message> {
        let mut start = self.start_widgets.clone();
        let mut center = self.center_widgets.clone();
        let mut end = self.end_widgets.clone();
        edit(match section {
            "start" => &mut start,
            "center" => &mut center,
            "end" => &mut end,
            _ => return Task::none(),
        });
        let mut overrides = self.config.snapshot.overrides.clone();
        for (section, widgets) in [("start", start), ("center", center), ("end", end)] {
            if let Err(error) = set_path(
                &mut overrides,
                &format!("bar.{section}"),
                Value::Array(widgets.into_iter().map(Value::String).collect()),
            ) {
                self.settings.error = Some(error);
                return Task::none();
            }
        }
        self.config_job(
            Some(Mutation::Replace(overrides)),
            Some(self.config.snapshot.revision),
            None,
            None,
            false,
        )
    }

    pub(crate) fn reload_config(&mut self, reply: Option<Reply>) -> Task<Message> {
        if self.config.jobs > 0 && reply.is_none() {
            self.config.reload_queued = true;
            return Task::none();
        }
        self.config_job(None, None, reply, None, false)
    }

    pub(crate) fn config_job(
        &mut self,
        mutation: Option<Mutation>,
        revision: Option<u64>,
        reply: Option<Reply>,
        edited_path: Option<String>,
        clear_editor: bool,
    ) -> Task<Message> {
        self.config.jobs += 1;
        self.settings.error = None;
        self.settings.notice = None;
        let store = self.config.store.clone();
        let fallback = self.config.snapshot.clone();
        Task::perform(
            async move {
                match background::run(move || {
                    let mut store = store
                        .lock()
                        .map_err(|_| "Configuration store lock failed".to_owned())?;
                    let result = match mutation {
                        Some(mutation) => store.transact(mutation, revision).map(|_| ()),
                        None => store.reload().map(|_| ()),
                    };
                    Ok::<_, String>((store.snapshot(), result.err()))
                })
                .await
                {
                    Ok(Ok((snapshot, error))) => ConfigOutcome {
                        snapshot,
                        error,
                        reply,
                        edited_path,
                        clear_editor,
                    },
                    result => ConfigOutcome {
                        snapshot: fallback,
                        error: Some(match result {
                            Ok(Err(error)) | Err(error) => error,
                            _ => unreachable!(),
                        }),
                        reply,
                        edited_path,
                        clear_editor,
                    },
                }
            },
            |outcome| Message::ConfigUpdated(Arc::new(outcome)),
        )
    }

    pub(crate) fn config_updated(&mut self, outcome: &ConfigOutcome) -> Task<Message> {
        self.config.jobs = self.config.jobs.saturating_sub(1);
        let old = self.config.snapshot.clone();
        let mut tasks = Vec::new();
        if outcome.snapshot.revision >= old.revision {
            self.config.snapshot = outcome.snapshot.clone();
            self.config.watch_paths = crate::config::WatchPaths::new(&self.config.directory);
            if old.effective != outcome.snapshot.effective
                || old.overrides != outcome.snapshot.overrides
            {
                tasks.push(self.apply_host_config(&old));
            }
            if old.effective != outcome.snapshot.effective {
                tasks.extend(
                    self.plugins
                        .iter()
                        .filter_map(|(plugin_id, (model, funcs))| {
                            let changed = funcs.config_changed?;
                            let id = plugin_id.clone();
                            let task = unsafe {
                                changed(model.clone(), self.config.snapshot.effective.clone())
                            };
                            task.map(|task| {
                                task.map(move |message| map_plugin_message(id.clone(), message))
                            })
                        }),
                );
                self.launcher.refresh(&self.plugins);
            }
        }
        self.settings.error = outcome.error.clone();
        if outcome.error.is_none() {
            if let Some(path) = outcome.edited_path.as_ref() {
                self.settings.drafts.remove(path);
            }
            if outcome.clear_editor {
                self.settings.editor_dirty = false;
                self.settings.drafts.clear();
            }
            self.settings.notice = Some("Configuration saved / reloaded".to_owned());
        }
        self.settings.sync_editor(&self.config.snapshot);
        self.refresh_plugin_catalog();
        if let Some(reply) = outcome.reply.as_ref() {
            reply.send(ipc::response(
                self.config.snapshot.revision,
                self.status_data(),
                outcome.error.as_deref(),
            ));
        }
        if self.config.jobs == 0 && self.config.reload_queued {
            self.config.reload_queued = false;
            tasks.push(self.reload_config(None));
        }
        Task::batch(tasks)
    }

    fn apply_host_config(&mut self, old: &ConfigSnapshot) -> Task<Message> {
        let host = self.config.snapshot.host.clone();
        let mut tasks = Vec::new();
        self.transparent = host.transparent;
        self.start_widgets = host.start.clone();
        self.center_widgets = host.center.clone();
        self.end_widgets = host.end.clone();
        if crate::config::schema::fallback_layout(&host, &self.config.snapshot.overrides) {
            self.start_widgets = crate::config::schema::fallback_widgets(&host, &self.plugin_order);
        }
        self.launcher.configure(&host);
        self.normalize_control_center_selection();
        tasks.push(self.close_control_center_toasts());
        if !host.control_center_enabled {
            tasks.push(self.close_control_center());
        }
        if let Some(id) = self.control_center.window_id
            && old.host.control_center_size != host.control_center_size
        {
            tasks.push(Task::done(Message::ResizeLayer(
                id,
                host.control_center_size.0,
                host.control_center_size.1,
            )));
        }
        if !host.launcher_enabled {
            tasks.push(self.close_launcher());
        }
        if let Some(id) = self.launcher.window_id
            && old.host.launcher_size != host.launcher_size
        {
            tasks.push(Task::done(Message::ResizeLayer(
                id,
                host.launcher_size.0,
                host.launcher_size.1,
            )));
        }
        if let Some(id) = self.settings.window_id
            && old.host.settings_size != host.settings_size
        {
            tasks.push(Task::done(Message::ResizeLayer(
                id,
                host.settings_size.0,
                host.settings_size.1,
            )));
        }
        tasks.push(self.reconcile_bars());
        tasks.push(self.reconcile_docks());
        tasks.push(self.reconcile_desktop());
        if old.host.osd != host.osd {
            tasks.push(self.apply_osd_config());
        }
        tasks.push(self.current_input_region_task());
        Task::batch(tasks)
    }

    pub(crate) fn restart_fields(&self) -> Vec<String> {
        let mut paths = BTreeSet::new();
        for field in &self.config.snapshot.schema {
            if field.apply == ApplyMode::Restart
                && get_path(&self.config.snapshot.effective, &field.path)
                    != get_path(&self.config.boot.effective, &field.path)
            {
                paths.insert(field.path.clone());
            }
        }
        for (section, value) in &self.config.snapshot.effective {
            if ["bar", "launcher", "settings"].contains(&section.as_str()) {
                continue;
            }
            if Some(value) == self.config.boot.effective.get(section) {
                continue;
            }
            collect_unknown(
                section,
                value,
                &self.config.boot.effective,
                &self.config.snapshot.schema,
                &mut paths,
            );
        }
        for (section, value) in &self.config.boot.effective {
            if ["bar", "launcher", "settings"].contains(&section.as_str()) {
                continue;
            }
            collect_unknown(
                section,
                value,
                &self.config.snapshot.effective,
                &self.config.snapshot.schema,
                &mut paths,
            );
        }
        paths.into_iter().collect()
    }

    pub(crate) fn refresh_plugin_catalog(&mut self) {
        let mut files: BTreeSet<String> =
            crate::config::get_allowed_plugins(&self.config.snapshot.effective)
                .into_iter()
                .map(str::to_owned)
                .collect();
        if let Ok(entries) = std::fs::read_dir(self.config.directory.join("plugins")) {
            files.extend(
                entries
                    .flatten()
                    .filter_map(|entry| entry.file_name().into_string().ok())
                    .filter(|name| name.ends_with(".so")),
            );
        }
        self.settings.catalog = files
            .into_iter()
            .map(|filename| {
                let loaded = self
                    .plugins
                    .iter()
                    .find(|(_, (_, funcs))| funcs.filename == filename);
                PluginStatus {
                    label: loaded.map(|(name, _)| name.clone()).unwrap_or_else(|| {
                        filename
                            .trim_start_matches("lib")
                            .trim_end_matches(".so")
                            .to_owned()
                    }),
                    state: loaded.map(|_| "Running".to_owned()).unwrap_or_else(|| {
                        self.plugin_failures
                            .get(&filename)
                            .cloned()
                            .unwrap_or_else(|| "Installed / not running".to_owned())
                    }),
                    filename,
                }
            })
            .collect();
    }

    pub(crate) fn status_data(&self) -> serde_json::Value {
        json!({"launcher_open": self.launcher.window_id.is_some(), "settings_open": self.settings.window_id.is_some(),
            "compositor_backend":self.compositor.as_ref().map(|snapshot|&snapshot.backend),"compositor_error":self.compositor_error,"compositor_action_error":self.compositor_action_error,
            "docks":self.docks.instances.values().map(|dock|json!({"instance":format!("{:?}",dock.id),"output":dock.output,"width":dock.size.0,"height":dock.size.1,"items":dock.items.len()})).collect::<Vec<_>>(),
            "desktop_editing":self.desktop.draft.is_some(),"desktop_widgets":self.desktop.instances.values().map(|widget|json!({"id":widget.spec.id,"instance":format!("{:?}",widget.id),"plugin":widget.spec.plugin,"output":widget.output,"x":widget.rect.x,"y":widget.rect.y,"width":widget.rect.width,"height":widget.rect.height})).collect::<Vec<_>>(),
            "osds": self.osds.instances.iter().map(|(output, item)| json!({"instance": format!("{:?}", item.id), "output": output, "provider": item.plugin, "topic": item.topic, "generation": item.generation})).collect::<Vec<_>>(),
            "control_center_open": self.control_center.window_id.is_some(), "control_center_selected": self.control_center.selected, "control_center_widgets": self.control_center_plugins(),
            "restart_required": self.restart_fields(), "loaded_plugins": self.plugins.keys().collect::<Vec<_>>(), "active_subscriptions": self.plugins.iter().filter(|(name, (_, funcs))| self.subscription_enabled(name, funcs)).map(|(name, _)| name).collect::<Vec<_>>(), "config_error": self.settings.error,
            "monitor_backend": self.shell.inventory.as_ref().map(|inventory| &inventory.backend), "monitor_error": self.shell.inventory_error,
            "outputs": self.shell.inventory.as_ref().map(|inventory| inventory.outputs.iter().map(|output| output.json()).collect::<Vec<_>>()).unwrap_or_default(),
            "bars": self.shell.bars.iter().map(|(instance, bar)| json!({"instance": instance, "output": bar.output, "width": bar.size.width, "height": bar.size.height, "position": format!("{:?}", bar.config.position)})).collect::<Vec<_>>()})
    }

    pub(crate) fn control_request(&mut self, request: ControlRequest) -> Task<Message> {
        let snapshot = &self.config.snapshot;
        match request.command {
            Command::Desktop(action) => {
                if matches!(action, crate::desktop::Action::Save) {
                    if self.desktop.draft.as_ref().is_none_or(|draft| draft.saving)
                        || self.config.jobs > 0
                    {
                        request.reply.send(ipc::response(
                            snapshot.revision,
                            self.status_data(),
                            Some("Desktop editor is not ready to save"),
                        ));
                        return Task::none();
                    }
                    let reply = request.reply;
                    return self
                        .update_desktop(action)
                        .map(move |message| match message {
                            Message::ConfigUpdated(mut outcome) => {
                                std::sync::Arc::make_mut(&mut outcome).reply = Some(reply.clone());
                                Message::ConfigUpdated(outcome)
                            }
                            message => message,
                        });
                }
                let task = self.update_desktop(action);
                request.reply.send(ipc::response(
                    self.config.snapshot.revision,
                    self.status_data(),
                    None,
                ));
                task
            }
            Command::Windows => {
                let data=self.compositor.as_ref().map(|snapshot|snapshot.windows.iter().map(|window|json!({"id":window.id,"pid":window.pid,"class":window.class,"title":window.title,"output":window.output,"workspace":window.workspace,"focused":window.focused})).collect::<Vec<_>>()).unwrap_or_default();
                request.reply.send(ipc::response(
                    snapshot.revision,
                    json!({"windows":data,"error":self.compositor_error}),
                    None,
                ));
                Task::none()
            }
            Command::Action {
                plugin,
                action,
                arguments,
            } => self.run_command(plugin, action, arguments, request.reply),
            Command::Osd { plugin, topic } => {
                let result = self.request_osd(
                    plugin,
                    oxibar_plugin_api::HostOsdRequest::new(topic).follow_focus(),
                );
                let error = result.as_ref().err().map(String::as_str);
                request.reply.send(ipc::response(
                    self.config.snapshot.revision,
                    self.status_data(),
                    error,
                ));
                result.unwrap_or_else(|_| Task::none())
            }
            Command::CloseOsds => {
                let task = self.close_osds();
                request.reply.send(ipc::response(
                    self.config.snapshot.revision,
                    self.status_data(),
                    None,
                ));
                task
            }
            Command::Outputs => {
                let data = json!({"backend": self.shell.inventory.as_ref().map(|inventory| &inventory.backend), "error": self.shell.inventory_error,
                    "outputs": self.shell.inventory.as_ref().map(|inventory| inventory.outputs.iter().map(|output| output.json()).collect::<Vec<_>>()).unwrap_or_default()});
                request
                    .reply
                    .send(ipc::response(snapshot.revision, data, None));
                Task::none()
            }
            Command::Launcher(action) => {
                let disabled = !self.launcher.enabled && !matches!(action, WindowCommand::Close);
                let task = self.update_launcher(match action {
                    WindowCommand::Open => crate::launcher::Action::Open,
                    WindowCommand::Close => crate::launcher::Action::Close,
                    WindowCommand::Toggle => crate::launcher::Action::Toggle,
                });
                request.reply.send(ipc::response(
                    self.config.snapshot.revision,
                    self.status_data(),
                    disabled.then_some("Launcher disabled in configuration"),
                ));
                task
            }
            Command::Settings(action) => {
                let task = self.update_settings(match action {
                    WindowCommand::Open => Action::Open,
                    WindowCommand::Close => Action::Close,
                    WindowCommand::Toggle => Action::Toggle,
                });
                request.reply.send(ipc::response(
                    self.config.snapshot.revision,
                    self.status_data(),
                    None,
                ));
                task
            }
            Command::ControlCenter(action) => {
                let disabled = !self.config.snapshot.host.control_center_enabled
                    && !matches!(action, WindowCommand::Close);
                let task = self.update_control_center(match action {
                    WindowCommand::Open => crate::control_center::Action::Open,
                    WindowCommand::Close => crate::control_center::Action::Close,
                    WindowCommand::Toggle => crate::control_center::Action::Toggle,
                });
                request.reply.send(ipc::response(
                    self.config.snapshot.revision,
                    self.status_data(),
                    disabled.then_some("Control center disabled in configuration"),
                ));
                task
            }
            Command::Status => {
                request
                    .reply
                    .send(ipc::response(snapshot.revision, self.status_data(), None));
                Task::none()
            }
            Command::Config(ConfigCommand::Change(mutation, revision)) => {
                self.config_job(Some(mutation), revision, Some(request.reply), None, false)
            }
            Command::Config(ConfigCommand::Reload) => self.reload_config(Some(request.reply)),
            Command::Config(ConfigCommand::Get(path)) => {
                let data = if let Some(path) = path {
                    json!({"path": path, "value": get_path(&snapshot.effective, &path), "base": snapshot.base_value(&path), "source": snapshot.source(&path).label(), "restart_required": self.restart_fields()})
                } else {
                    json!({"effective": snapshot.effective, "base": snapshot.base, "overrides": snapshot.overrides, "restart_required": self.restart_fields()})
                };
                request
                    .reply
                    .send(ipc::response(snapshot.revision, data, None));
                Task::none()
            }
            Command::Config(ConfigCommand::Export(overrides)) => {
                let text = toml::to_string_pretty(if overrides {
                    &snapshot.overrides
                } else {
                    &snapshot.effective
                });
                request.reply.send(match text {
                    Ok(text) => ipc::response(snapshot.revision, json!(text), None),
                    Err(error) => {
                        ipc::response(snapshot.revision, json!(null), Some(&error.to_string()))
                    }
                });
                Task::none()
            }
            Command::Config(ConfigCommand::Schema) => {
                let fields: Vec<_> = snapshot.schema.iter().map(|field| json!({"path": field.path, "label": field.label, "description": field.description,
                    "kind": kind_data(&field.kind), "default": field.default, "apply": field.apply.label(), "source": snapshot.source(&field.path).label()})).collect();
                request
                    .reply
                    .send(ipc::response(snapshot.revision, json!(fields), None));
                Task::none()
            }
        }
    }
}

fn collect_unknown(
    prefix: &str,
    value: &Value,
    boot: &Table,
    schema: &[SettingDescriptor],
    paths: &mut BTreeSet<String>,
) {
    if let Value::Table(table) = value {
        for (key, value) in table {
            collect_unknown(&format!("{prefix}.{key}"), value, boot, schema, paths);
        }
    } else if !schema.iter().any(|field| field.path == prefix)
        && get_path(boot, prefix) != Some(value)
    {
        paths.insert(prefix.to_owned());
    }
}

fn kind_data(kind: &oxibar_plugin_api::SettingKind) -> serde_json::Value {
    use oxibar_plugin_api::SettingKind;
    match kind {
        SettingKind::Boolean => json!({"type": "boolean"}),
        SettingKind::Integer { min, max } => json!({"type": "integer", "min": min, "max": max}),
        SettingKind::Number { min, max } => json!({"type": "number", "min": min, "max": max}),
        SettingKind::Text => json!({"type": "string"}),
        SettingKind::TextList => json!({"type": "string-array"}),
        SettingKind::TableList => json!({"type": "table-array"}),
        SettingKind::Choice(options) => json!({"type": "enum", "options": options}),
        SettingKind::Arguments => json!({"type": "command-arguments"}),
    }
}
