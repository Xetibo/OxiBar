use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use iced::Task;

use super::{LoadedPlugins, PluginMap, load_missing_plugins};
use crate::{
    app::OxiBar,
    config::get_allowed_plugins,
    messages::{Message, map_plugin_message},
};

#[derive(Default)]
pub(crate) struct PluginRuntime {
    inactive: PluginMap,
    pub loading: bool,
    queued: bool,
    catalog: BTreeMap<std::path::PathBuf, CachedLibrary>,
}

struct CachedLibrary {
    stamp: Option<(std::path::PathBuf, u64, Option<std::time::SystemTime>)>,
    info: super::LibraryInfo,
}

impl PluginRuntime {
    pub(crate) fn library_info(&mut self, path: &std::path::Path) -> super::LibraryInfo {
        let stamp = std::fs::metadata(path).ok().map(|metadata| {
            (
                std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned()),
                metadata.len(),
                metadata.modified().ok(),
            )
        });
        if let Some(cached) = self.catalog.get(path)
            && cached.stamp == stamp
        {
            return cached.info.clone();
        }
        let info = super::inspect_library(path);
        self.catalog.insert(
            path.to_owned(),
            CachedLibrary {
                stamp,
                info: info.clone(),
            },
        );
        info
    }
}

pub struct LoadOutcome(Mutex<Option<Result<LoadedPlugins, String>>>);

impl std::fmt::Debug for LoadOutcome {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LoadOutcome")
            .finish_non_exhaustive()
    }
}

impl OxiBar {
    pub(crate) fn resident_plugin_update(
        &self,
        name: &str,
        message: oxibar_plugin_api::PluginMsg,
    ) -> Task<Message> {
        let Some((model, funcs)) = self.plugin_runtime.inactive.get(name) else {
            return Task::none();
        };
        let task = super::dispatch_update(funcs, model.clone(), message);
        super::drain_errors(funcs, model);
        let name = name.to_owned();
        task.map_or_else(Task::none, |task| {
            task.map(move |message| map_plugin_message(name.clone(), message))
        })
    }

    pub(crate) fn reconcile_plugins(&mut self) -> Task<Message> {
        let allowed: Vec<String> = get_allowed_plugins(&self.config.snapshot.effective)
            .into_iter()
            .map(str::to_owned)
            .collect();
        let disabled: Vec<String> = self
            .plugins
            .iter()
            .filter(|(_, (_, funcs))| !allowed.contains(&funcs.filename))
            .map(|(name, _)| name.clone())
            .collect();
        let mut tasks = Vec::new();
        for name in disabled {
            if name.eq_ignore_ascii_case("OxiPaste") {
                tasks.push(self.close_oxipaste());
            }
            if self.popup_plugin.as_deref() == Some(&name) {
                tasks.push(self.close_bar_dropdown());
            }
            if self.modal_plugin.as_deref() == Some(&name) {
                tasks.push(self.close_modal());
            }
            if self.panel_plugin.as_deref() == Some(&name) {
                tasks.push(self.close_panel());
            }
            let toasts: Vec<_> = self
                .toasts
                .iter()
                .filter(|toast| toast.plugin_id == name)
                .map(|toast| toast.toast_id.clone())
                .collect();
            for toast in toasts {
                tasks.push(self.close_plugin_toast(&name, &toast));
            }
            let plugin = self.plugins.remove(&name).expect("selected active plugin");
            if let Some(cancel) = plugin.1.runner_cancel {
                unsafe { cancel(plugin.0.clone()) };
            }
            self.plugin_runtime.inactive.insert(name, plugin);
        }
        let enabled: Vec<_> = self
            .plugin_runtime
            .inactive
            .iter()
            .filter(|(_, (_, funcs))| allowed.contains(&funcs.filename))
            .map(|(name, _)| name.clone())
            .collect();
        for name in enabled {
            let plugin = self
                .plugin_runtime
                .inactive
                .remove(&name)
                .expect("selected resident plugin");
            self.plugins.insert(name.clone(), plugin);
            tasks.extend(self.activate_plugin(&name));
        }
        self.order_plugins(&allowed);
        self.launcher.refresh(&self.plugins);
        self.normalize_control_center_selection();
        if self.plugin_runtime.loading {
            self.plugin_runtime.queued = true;
            return Task::batch(tasks);
        }
        let existing: BTreeMap<_, _> = self
            .plugins
            .iter()
            .chain(&self.plugin_runtime.inactive)
            .map(|(name, (_, funcs))| (name.clone(), funcs.filename.clone()))
            .collect();
        if allowed
            .iter()
            .all(|filename| existing.values().any(|existing| existing == filename))
        {
            return Task::batch(tasks);
        }
        self.plugin_runtime.loading = true;
        let config = self.config.snapshot.effective.clone();
        let directory = self.config.directory.join("plugins");
        tasks.push(Task::perform(
            async move {
                Arc::new(LoadOutcome(Mutex::new(Some(
                    crate::background::run(move || {
                        load_missing_plugins(&config, &directory, &existing)
                    })
                    .await,
                ))))
            },
            Message::PluginsLoaded,
        ));
        Task::batch(tasks)
    }

    pub(crate) fn plugins_loaded(&mut self, outcome: &LoadOutcome) -> Task<Message> {
        let Some(result) = outcome.0.lock().expect("plugin load result").take() else {
            return Task::none();
        };
        self.plugin_runtime.loading = false;
        let mut tasks = Vec::new();
        match result {
            Err(error) => self.settings.error = Some(error),
            Ok(loaded) => {
                let mut store = self
                    .config
                    .store
                    .lock()
                    .expect("plugin schema registration");
                for (name, (_, funcs)) in &loaded.plugins {
                    let mut fields = funcs
                        .settings_schema
                        .map(|schema| unsafe { schema() })
                        .unwrap_or_default();
                    if funcs.config_changed.is_none() {
                        for field in &mut fields {
                            field.apply = oxibar_plugin_api::ApplyMode::Restart;
                        }
                    }
                    if let Err(error) = store.register_schema(fields) {
                        self.settings.error = Some(error);
                    }
                    if let Some(validate) = funcs.config_validate
                        && let Err(error) =
                            store.register_validator(name.clone(), validate, funcs._lib.clone())
                    {
                        self.settings.error = Some(error);
                    }
                }
                self.config.snapshot = store.snapshot();
                drop(store);
                let allowed: Vec<String> = get_allowed_plugins(&self.config.snapshot.effective)
                    .into_iter()
                    .map(str::to_owned)
                    .collect();
                tasks.extend(loaded.tasks);
                for (name, plugin) in loaded.plugins {
                    self.plugin_failures.remove(&plugin.1.filename);
                    if allowed.contains(&plugin.1.filename) {
                        self.plugins.insert(name.clone(), plugin);
                        tasks.extend(self.activate_plugin(&name));
                    } else {
                        self.plugin_runtime.inactive.insert(name, plugin);
                    }
                }
                self.plugin_failures.extend(loaded.failures);
                self.order_plugins(&allowed);
            }
        }
        self.launcher.refresh(&self.plugins);
        if self.launcher.window_id.is_some() {
            tasks.push(self.query_runner_plugins());
        }
        self.normalize_control_center_selection();
        tasks.push(self.reconcile_bars());
        tasks.push(self.reconcile_docks());
        tasks.push(self.reconcile_desktop());
        self.settings.sync_editor(&self.config.snapshot);
        self.refresh_plugin_catalog();
        if std::mem::take(&mut self.plugin_runtime.queued) {
            tasks.push(self.reconcile_plugins());
        }
        Task::batch(tasks)
    }

    fn order_plugins(&mut self, allowed: &[String]) {
        self.plugin_order = allowed
            .iter()
            .filter_map(|filename| {
                self.plugins
                    .iter()
                    .find(|(_, (_, funcs))| &funcs.filename == filename)
                    .map(|(name, _)| name.clone())
            })
            .fold(Vec::new(), |mut names, name| {
                if !names.contains(&name) {
                    names.push(name);
                }
                names
            });
        if crate::config::schema::fallback_layout(
            &self.config.snapshot.host,
            &self.config.snapshot.overrides,
        ) {
            self.start_widgets = crate::config::schema::fallback_widgets(
                &self.config.snapshot.host,
                &self.plugin_order,
            );
        }
    }

    fn activate_plugin(&mut self, name: &str) -> Vec<Task<Message>> {
        let (model, funcs) = &self.plugins[name];
        let mut tasks = Vec::new();
        if let Some(changed) = funcs.theme_changed
            && let Err(error) = unsafe { changed(model.clone(), self.appearance.snapshot.clone()) }
        {
            self.settings.error = Some(format!("{name}: {error}"));
            self.appearance.failures.push(name.to_owned());
        }
        if let Some(changed) = funcs.config_changed
            && let Some(task) =
                unsafe { changed(model.clone(), self.config.snapshot.effective.clone()) }
        {
            let name = name.to_owned();
            tasks.push(task.map(move |message| map_plugin_message(name.clone(), message)));
        }
        if let (Some(changed), Some(snapshot)) = (funcs.compositor_changed, &self.compositor)
            && let Some(task) = unsafe { changed(model.clone(), snapshot.clone()) }
        {
            let name = name.to_owned();
            tasks.push(task.map(move |message| map_plugin_message(name.clone(), message)));
        }
        tasks
    }
}
