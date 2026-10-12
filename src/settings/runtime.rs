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
        let toggle_page = matches!(&action, Action::TogglePage(_));
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
                    self.close_overview(),
                    close_launcher,
                    close_modal,
                    close_panel,
                    close_control_center,
                    self.close_bar_dropdown(),
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
                self.settings.navigation.select(category.id(), false);
                self.settings.category = category;
                self.settings.search.clear();
                self.settings.widget_drag = None;
                if category == super::Category::Monitors
                    && self.settings.monitors.inventory.is_none()
                {
                    return self.update_monitors(crate::monitor::runtime::Action::Refresh);
                }
                Task::none()
            }
            Action::Page(id) | Action::TogglePage(id) => {
                let Some(category) = self.settings.navigation.select(&id, toggle_page) else {
                    return Task::none();
                };
                self.settings.category = category;
                self.settings.widget_drag = None;
                self.settings.search.clear();
                if category == super::Category::Monitors
                    && self.settings.monitors.inventory.is_none()
                {
                    return self.update_monitors(crate::monitor::runtime::Action::Refresh);
                }
                Task::none()
            }
            Action::Search(query) => {
                self.settings.search = query;
                Task::none()
            }
            Action::NavigationBounds(id, bounds) => {
                if (self.settings.navigation.find(&id).is_some()
                    || (id.starts_with('@') && id.len() <= 256))
                    && [bounds.x, bounds.y, bounds.width, bounds.height]
                        .into_iter()
                        .all(f32::is_finite)
                {
                    self.settings.navigation_bounds.insert(id, bounds);
                }
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
                        value: edit_value(self.setting_value(&path).as_ref(), &field.kind),
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
            Action::BeginWidgetDrag {
                section,
                index,
                name,
                revision,
            } if self.config.jobs == 0 => {
                self.settings.widget_drag = Some(super::WidgetDrag {
                    section,
                    index,
                    name,
                    revision,
                });
                Task::none()
            }
            Action::DropWidget(section, index) if self.config.jobs == 0 => {
                self.drop_bar_widget(&section, index)
            }
            Action::CancelWidgetDrag => {
                self.settings.widget_drag = None;
                Task::none()
            }
            Action::EnableDefaultProviders(enabled) if self.config.jobs == 0 => {
                let mut files: Vec<String> =
                    get_path(&self.config.snapshot.effective, "launcher.default_plugins")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect();
                files
                    .retain(|name| !crate::config::DEFAULT_RUNNER_PLUGINS.contains(&name.as_str()));
                if enabled {
                    files.extend(
                        crate::config::DEFAULT_RUNNER_PLUGINS
                            .into_iter()
                            .map(str::to_owned),
                    );
                }
                self.config_job(
                    Some(Mutation::Set(
                        "launcher.default_plugins".into(),
                        Value::Array(files.into_iter().map(Value::String).collect()),
                    )),
                    Some(self.config.snapshot.revision),
                    None,
                    None,
                    false,
                )
            }
            Action::EnablePlugin(filename, enabled) if self.config.jobs == 0 => {
                let mut files: Vec<_> = get_path(&self.config.snapshot.effective, "plugins")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect();
                if enabled && !files.contains(&filename) {
                    files.push(filename.clone());
                } else if !enabled {
                    files.retain(|file| file != &filename);
                }
                let mut overrides = self.config.snapshot.overrides.clone();
                let mut defaults: Vec<_> =
                    get_path(&self.config.snapshot.effective, "launcher.default_plugins")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect();
                if !enabled {
                    defaults.retain(|file| file != &filename);
                }
                set_path(
                    &mut overrides,
                    "plugins",
                    Value::Array(files.into_iter().map(Value::String).collect()),
                )
                .expect("known path");
                set_path(
                    &mut overrides,
                    "launcher.default_plugins",
                    Value::Array(defaults.into_iter().map(Value::String).collect()),
                )
                .expect("known path");
                self.config_job(
                    Some(Mutation::Replace(overrides)),
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
        self.settings.widget_drag = None;
        let revert = if self.settings.monitors.preview.is_some() {
            self.update_monitors(crate::monitor::runtime::Action::Revert)
        } else {
            Task::none()
        };
        let close = self
            .settings
            .window_id
            .take()
            .map(|id| {
                self.shell.registry.remove(id);
                Task::done(Message::CloseSettingsLayer(id))
            })
            .unwrap_or_else(Task::none);
        Task::batch([revert, close])
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
        self.store_widgets(start, center, end, self.config.snapshot.revision)
    }

    fn drop_bar_widget(&mut self, section: &str, index: usize) -> Task<Message> {
        let Some(drag) = self.settings.widget_drag.take() else {
            return Task::none();
        };
        if drag.revision != self.config.snapshot.revision {
            self.settings.error =
                Some("Settings changed during drag; drag the widget again.".into());
            return Task::none();
        }
        let mut sections = [
            self.start_widgets.clone(),
            self.center_widgets.clone(),
            self.end_widgets.clone(),
        ];
        let locate = |section: &str| {
            ["start", "center", "end"]
                .iter()
                .position(|name| *name == section)
        };
        let (Some(from), Some(to)) = (locate(&drag.section), locate(section)) else {
            return Task::none();
        };
        if sections[from].get(drag.index) != Some(&drag.name) || index > sections[to].len() {
            return Task::none();
        }
        let value = sections[from].remove(drag.index);
        let index = if from == to && index > drag.index {
            index - 1
        } else {
            index
        };
        sections[to].insert(index, value);
        let [start, center, end] = sections;
        self.store_widgets(start, center, end, drag.revision)
    }

    fn store_widgets(
        &mut self,
        start: Vec<String>,
        center: Vec<String>,
        end: Vec<String>,
        revision: u64,
    ) -> Task<Message> {
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
            Some(revision),
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
        self.settings.error = outcome.error.clone();
        let old = self.config.snapshot.clone();
        let mut tasks = Vec::new();
        if outcome.snapshot.revision >= old.revision {
            self.config.snapshot = outcome.snapshot.clone();
            self.config.watch_paths = crate::config::WatchPaths::new(&self.config.directory);
            if crate::config::get_allowed_plugins(&old.effective)
                != crate::config::get_allowed_plugins(&outcome.snapshot.effective)
            {
                tasks.push(self.reconcile_plugins());
            }
            if old.effective != outcome.snapshot.effective
                || old.overrides != outcome.snapshot.overrides
            {
                if old.effective.get("theme") != self.config.snapshot.effective.get("theme") {
                    tasks.push(self.apply_theme(false));
                }
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
                if self.launcher.window_id.is_some() {
                    tasks.push(self.query_runner_plugins());
                }
                self.launcher.refresh(&self.plugins);
            }
        }
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
        self.settings
            .navigation
            .reconcile(&self.config.snapshot.schema);
        self.settings.category = self.settings.navigation.current().category;
        self.settings
            .navigation_bounds
            .retain(|id, _| id.starts_with('@') || self.settings.navigation.find(id).is_some());
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
        if !host.control_center_enabled
            || old.host.menus.controls_dropdown != host.menus.controls_dropdown
        {
            tasks.push(self.close_control_center());
        }
        if let Some(id) = self.control_center.window_id
            && self.control_center.attached_to.is_none()
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
        if old.host.menus.notifications_dropdown != host.menus.notifications_dropdown
            || !host.menus.session_enabled && self.shell.session_window.is_some()
        {
            tasks.push(self.close_bar_dropdown());
        }
        tasks.push(self.reconcile_docks());
        tasks.push(self.reconcile_desktop());
        if old.host.overview != host.overview {
            if !host.overview.enabled {
                tasks.push(self.close_overview());
            } else if self.overview.client.is_some()
                && (old.host.overview.fps != host.overview.fps
                    || old.host.overview.thumbnail_size != host.overview.thumbnail_size)
            {
                tasks.push(self.close_overview());
                tasks.push(self.update_overview(crate::overview::Action::Open));
            } else {
                tasks.push(self.reconcile_overview());
            }
        }
        if old.host.osd != host.osd {
            tasks.push(self.apply_osd_config());
        }
        if old.host.toasts != host.toasts {
            tasks.extend(self.toast_layout_tasks());
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
        files.extend(["libapplications.so", "libtools.so", "libsystem.so"].map(str::to_owned));
        let primary = self.config.directory.join("plugins");
        for directory in crate::plugins::plugin_directories(&primary) {
            if let Ok(entries) = std::fs::read_dir(directory) {
                files.extend(
                    entries
                        .flatten()
                        .filter_map(|entry| entry.file_name().into_string().ok())
                        .filter(|name| name.ends_with(".so")),
                );
            }
        }
        self.settings.catalog = files
            .into_iter()
            .map(|filename| {
                let path = crate::plugins::library_path(&primary, &filename);
                let info = self.plugin_runtime.library_info(&path);
                let loaded = self
                    .plugins
                    .iter()
                    .find(|(_, (_, funcs))| funcs.filename == filename);
                PluginStatus {
                    label: loaded
                        .map(|(name, _)| name.clone())
                        .or(info.name)
                        .unwrap_or_else(|| {
                            filename
                                .trim_start_matches("lib")
                                .trim_end_matches(".so")
                                .to_owned()
                        }),
                    state: match (loaded.is_some(), info.error) {
                        (true, Some(error)) => format!("Running · installed library: {error}"),
                        (true, None) => "Running".to_owned(),
                        (false, Some(_)) if !path.is_file() => {
                            "Not installed · add this plugin package to your Oxibar installation"
                                .to_owned()
                        }
                        (false, Some(error)) => error,
                        (false, None) => self
                            .plugin_failures
                            .get(&filename)
                            .cloned()
                            .unwrap_or_else(|| {
                                if self.plugin_runtime.loading {
                                    "Loading providers…".to_owned()
                                } else {
                                    "Installed / disabled".to_owned()
                                }
                            }),
                    },
                    abi: info.abi,
                    capabilities: info.capabilities,
                    enabled: crate::config::get_allowed_plugins(&self.config.snapshot.effective)
                        .contains(&filename.as_str()),
                    filename,
                }
            })
            .collect();
    }

    pub(crate) fn status_data(&self) -> serde_json::Value {
        let mut data = json!({"launcher_open": self.launcher.window_id.is_some(), "settings_open": self.settings.window_id.is_some(),
            "launcher_results": self.launcher.entries.iter().map(|entry| json!({"provider": entry.plugin_id, "id": entry.result.index, "score": entry.result.score, "favorite": entry.favorite})).collect::<Vec<_>>(),
            "launcher_query_revision":self.launcher.query_revision,"launcher_query_us":self.launcher.query_elapsed_us,
            "popup_open":self.popup_open,"popup_provider":self.popup_plugin,
            "session_open": self.shell.session_window.is_some(),
            "dropdown": self.shell.popup_bar.filter(|_| self.popup_open).map(|id| { let metrics = self.popup_metrics(self.popup_plugin.as_deref().unwrap_or_default()); serde_json::json!({"output":self.output_for_bar(Some(id)),"x":self.dropdown_x(id,self.popup_plugin.as_deref().unwrap_or_default()),"width":metrics.connector_width,"height":metrics.height}) }),
            "modal_open":self.modal_window_id.is_some(),"modal_provider":self.modal_plugin,"panel_open":self.panel_window_id.is_some(),"panel_provider":self.panel_plugin,"toast_count":self.toasts.len(),
            "compositor_backend":self.compositor.as_ref().map(|snapshot|&snapshot.backend),"compositor_error":self.compositor_error,"compositor_action_error":self.compositor_action_error,
            "docks":self.docks.instances.values().map(|dock|json!({"instance":format!("{:?}",dock.id),"output":dock.output,"width":dock.size.0,"height":dock.size.1,"items":dock.items.len()})).collect::<Vec<_>>(),
            "desktop_editing":self.desktop.draft.is_some(),"desktop_widgets":self.desktop.instances.values().map(|widget|json!({"id":widget.spec.id,"instance":format!("{:?}",widget.id),"plugin":widget.spec.plugin,"output":widget.output,"x":widget.rect.x,"y":widget.rect.y,"width":widget.rect.width,"height":widget.rect.height})).collect::<Vec<_>>(),
            "osds": self.osds.instances.iter().map(|(output, item)| json!({"instance": format!("{:?}", item.id), "output": output, "provider": item.plugin, "topic": item.topic, "generation": item.generation})).collect::<Vec<_>>(),
            "control_center_open": self.control_center.window_id.is_some(), "control_center_selected": self.control_center.selected, "control_center_widgets": self.control_center_plugins(),
            "restart_required": self.restart_fields(), "loaded_plugins": self.plugins.keys().collect::<Vec<_>>(), "active_subscriptions": self.plugins.iter().filter(|(name, (_, funcs))| self.subscription_enabled(name, funcs)).map(|(name, _)| name).collect::<Vec<_>>(), "config_error": self.settings.error,
            "monitor_backend": self.shell.inventory.as_ref().map(|inventory| &inventory.backend), "monitor_error": self.shell.inventory_error,
            "outputs": self.shell.inventory.as_ref().map(|inventory| inventory.outputs.iter().map(|output| output.json()).collect::<Vec<_>>()).unwrap_or_default(),
            "bars": self.shell.bars.iter().map(|(instance, bar)| json!({"instance": instance, "output": bar.output, "width": bar.size.width, "height": bar.size.height, "position": format!("{:?}", bar.config.position), "auto_hide":bar.config.auto_hide, "visible":self.bar_is_visible(bar.id)})).collect::<Vec<_>>()});
        data["control_center_attached"] = json!(self.control_center.attached_to.is_some());
        data["plugins_loading"] = json!(self.plugin_runtime.loading);
        data["settings_navigation"] = json!({
            "page": self.settings.navigation.selected,
            "expanded": self.settings.navigation.expanded,
            "rows": self.settings.navigation_bounds.iter().filter(|(_, bounds)| bounds.width > 0.0 && bounds.height > 0.0)
                .map(|(page, bounds)| json!({"page":page,"x":bounds.x,"y":bounds.y,"width":bounds.width,"height":bounds.height})).collect::<Vec<_>>()
        });
        data["overview"] = json!({"open":self.overview.client.is_some(),"active_output":self.overview.active,"selected":self.overview.selected,"backend":self.overview.snapshot.backend,"error":self.overview.snapshot.error,"in_flight":self.overview.snapshot.in_flight,"staging_bytes":self.overview.snapshot.staging_bytes,"thumbnail_bytes":self.overview.snapshot.thumbnail_bytes,"captures":self.overview.snapshot.captures,"windows":self.overview.snapshot.windows.iter().map(|w|json!({"id":w.id,"title":w.title,"app_id":w.app_id,"outputs":w.outputs,"focused":w.focused,"preview":w.image.is_some(),"frames":w.frames,"error":w.error,"activatable":w.activatable})).collect::<Vec<_>>()});
        data["overview"]["outputs"] = json!(self.overview.surfaces.keys().collect::<Vec<_>>());
        data["theme"] = json!({"revision": self.appearance.snapshot.revision, "font": self.appearance.snapshot.font_family, "font_loading": self.appearance.font_loading, "preset": self.appearance.snapshot.preset, "density": self.appearance.snapshot.density, "font_scale": self.appearance.snapshot.font_scale, "reduced_motion": self.appearance.snapshot.reduced_motion, "animation_ms": self.appearance.snapshot.animation_ms, "primary": oxiced::theme::snapshot::color_hex(self.appearance.snapshot.tokens.primary), "mantle": oxiced::theme::snapshot::color_hex(self.appearance.snapshot.tokens.mantle), "accent":self.appearance.snapshot.accent.map(oxiced::theme::snapshot::color_hex), "base24":oxiced::theme::base24::SLOTS.iter().zip(self.appearance.snapshot.base24.colors.iter()).map(|(slot,color)|(slot.to_string(),json!(oxiced::theme::snapshot::color_hex(*color)))).collect::<serde_json::Map<_,_>>(), "failures": self.appearance.failures, "upgraded_plugins": self.plugins.iter().filter(|(_, (_, funcs))| funcs.theme_changed.is_some()).map(|(name, _)| name).collect::<Vec<_>>()});
        data["monitor_editor"] = json!({"busy":self.settings.monitors.busy,"preview":self.settings.monitors.preview,"error":self.settings.monitors.error,"notice":self.settings.monitors.notice,"draft":self.settings.monitors.draft.iter().map(|output|output.value()).collect::<Vec<_>>()});
        data["monitor_editor"]["diagram"] = self.monitor_diagram_status();
        data["bar_buttons"] = json!(self.shell.bars.values().flat_map(|bar|bar.anchors.iter().map(move |(key,(provider,bounds))| json!({"output":bar.output,"key":key,"provider":provider,"x":bounds.x,"y":bounds.y,"width":bounds.width,"height":bounds.height}))).collect::<Vec<_>>());
        data
    }

    pub(crate) fn control_request(&mut self, request: ControlRequest) -> Task<Message> {
        let snapshot = &self.config.snapshot;
        match request.command {
            Command::Overview(command) => {
                if !snapshot.host.overview.enabled && !matches!(command, WindowCommand::Close) {
                    request.reply.send(ipc::response(
                        snapshot.revision,
                        self.status_data(),
                        Some("Overview is disabled"),
                    ));
                    return Task::none();
                }
                let task = self.update_overview(match command {
                    WindowCommand::Open => crate::overview::Action::Open,
                    WindowCommand::Close => crate::overview::Action::Close,
                    WindowCommand::Toggle => crate::overview::Action::Toggle,
                });
                request.reply.send(ipc::response(
                    self.config.snapshot.revision,
                    self.status_data(),
                    None,
                ));
                task
            }
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
            Command::Plugins => {
                self.refresh_plugin_catalog();
                request.reply.send(ipc::response(self.config.snapshot.revision, json!({"plugins":self.settings.catalog.iter().map(|plugin| json!({"filename":plugin.filename,"name":plugin.label,"state":plugin.state,"enabled":plugin.enabled,"abi":plugin.abi,"capabilities":plugin.capabilities})).collect::<Vec<_>>()}), None));
                Task::none()
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
            Command::Session(action) => {
                let disabled = !self.config.snapshot.host.menus.session_enabled
                    && !matches!(action, WindowCommand::Close);
                let task = self.update_session_menu(match action {
                    WindowCommand::Open => crate::session_menu::Action::Open,
                    WindowCommand::Close => crate::session_menu::Action::Close,
                    WindowCommand::Toggle => crate::session_menu::Action::Toggle,
                });
                request.reply.send(ipc::response(
                    self.config.snapshot.revision,
                    self.status_data(),
                    disabled.then_some("Session menu disabled in configuration"),
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
        SettingKind::Color => json!({"type": "color"}),
        SettingKind::TextList => json!({"type": "string-array"}),
        SettingKind::TableList => json!({"type": "table-array"}),
        SettingKind::Choice(options) => json!({"type": "enum", "options": options}),
        SettingKind::Arguments => json!({"type": "command-arguments"}),
    }
}
