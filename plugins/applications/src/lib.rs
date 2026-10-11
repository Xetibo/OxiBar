use std::{
    sync::{Arc, OnceLock},
    time::Duration,
};

use iced::{
    Element, Task,
    futures::{Stream, StreamExt},
};
use oxibar_plugin_api::{
    ABI_VERSION, PluginModel, PluginMsg, PluginStream, RunnerEntry, drain_model_errors,
    plugin_model, toml::Table, with_model_read, with_model_write,
};
use oxibar_plugin_api::{
    runner::{Usage, UsageStore},
    worker::LatestWorker,
};

mod config;
mod system;
mod worker;

use config::{Config, read_config};
use system::{Entry, Match};

static REFRESH_INTERVAL: OnceLock<Duration> = OnceLock::new();

#[derive(Debug)]
struct Model {
    identities: std::collections::BTreeMap<String, usize>,
    next_identity: usize,
    dock: Arc<Vec<oxibar_plugin_api::DockEntry>>,
    config: Config,
    applications: Arc<Vec<Entry>>,
    matches: Vec<Match>,
    query: String,
    generation: u64,
    refreshing: bool,
    reindex_pending: bool,
    index_generation: u64,
    index_worker: Result<LatestWorker<Arc<Vec<Entry>>>, String>,
    query_worker: Result<LatestWorker<Vec<Match>>, String>,
    usage: Usage,
    usage_store: Arc<UsageStore>,
    usage_generation: u64,
    usage_loaded: bool,
    usage_error: Option<String>,
    query_active: bool,
    health: Option<String>,
    errors: Vec<String>,
}

#[derive(Debug)]
enum Message {
    Refresh,
    Indexed(u64, Result<Arc<Vec<Entry>>, String>),
    Searched(u64, Result<Vec<Match>, String>),
    Launched(String, Result<(), String>),
    UsageLoaded(Result<Usage, String>),
    UsageSaved(u64, Result<(), String>),
}

impl Model {
    fn new(config: Config, usage_store: Arc<UsageStore>) -> Self {
        Self {
            identities: std::collections::BTreeMap::new(),
            next_identity: 0,
            dock: Arc::new(Vec::new()),
            config,
            applications: Arc::new(Vec::new()),
            matches: Vec::new(),
            query: String::new(),
            generation: 0,
            refreshing: false,
            reindex_pending: false,
            index_generation: 0,
            index_worker: LatestWorker::new("oxibar-app-index"),
            query_worker: LatestWorker::new("oxibar-app-query"),
            usage: Usage::default(),
            usage_store,
            usage_generation: 0,
            usage_loaded: false,
            usage_error: None,
            query_active: true,
            health: None,
            errors: Vec::new(),
        }
    }
}

fn save_usage(model: &Model) -> Task<PluginMsg> {
    if !model.usage_loaded || model.usage_generation == 0 {
        return Task::none();
    }
    let revision = model.usage_generation;
    let usage = model.usage.clone();
    let store = model.usage_store.clone();
    Task::perform(
        async move {
            worker::run(move || store.save(&usage, revision))
                .await
                .and_then(|result| result)
        },
        move |result| Arc::new(Message::UsageSaved(revision, result)) as PluginMsg,
    )
}

fn index_task(model: &mut Model) -> Task<PluginMsg> {
    model.index_generation = model.index_generation.wrapping_add(1);
    model.refreshing = true;
    let generation = model.index_generation;
    let config = model.config.clone();
    let future = match &model.index_worker {
        Ok(worker) => worker
            .submit(move |cancel| Arc::new(system::load_entries_cancellable(&config, &cancel))),
        Err(error) => Box::pin(iced::futures::future::ready(Err(error.clone()))),
    };
    Task::perform(future, move |entries| {
        Arc::new(Message::Indexed(generation, entries)) as PluginMsg
    })
}

fn query_task(model: &mut Model) -> Task<PluginMsg> {
    model.generation = model.generation.wrapping_add(1);
    model.matches.clear();
    if !model.query_active {
        return Task::none();
    }
    let generation = model.generation;
    let entries = model.applications.clone();
    let query = model.query.clone();
    let limit = model.config.max_entries;
    let query = query
        .strip_prefix(&model.config.prefix)
        .unwrap_or(&query)
        .trim()
        .to_owned();
    let ranking = model.usage.ranking(&model.config.favorites);
    let future = match &model.query_worker {
        Ok(worker) => worker.submit(move |cancel| {
            system::search_ranked(&entries, &query, limit, &ranking, &cancel)
        }),
        Err(error) => Box::pin(iced::futures::future::ready(Err(error.clone()))),
    };
    Task::perform(future, move |entries| {
        Arc::new(Message::Searched(generation, entries)) as PluginMsg
    })
}

oxibar_plugin_api::export_theme_changed!();

#[unsafe(no_mangle)]
pub extern "Rust" fn abi_version() -> u32 {
    ABI_VERSION
}

#[unsafe(no_mangle)]
pub extern "Rust" fn name() -> &'static str {
    "Applications"
}

#[unsafe(no_mangle)]
pub extern "Rust" fn model(global: Table) -> (PluginModel, Option<Task<PluginMsg>>) {
    let config = read_config(&global);
    let _ = REFRESH_INTERVAL.set(Duration::from_secs(config.refresh_seconds));
    let usage_store = Arc::new(UsageStore::new("applications"));
    let mut state = Model::new(config, usage_store.clone());
    let task = index_task(&mut state);
    let usage = Task::perform(
        async move {
            worker::run(move || usage_store.load())
                .await
                .and_then(|result| result)
        },
        |result| Arc::new(Message::UsageLoaded(result)) as PluginMsg,
    );
    (plugin_model(state), Some(Task::batch([task, usage])))
}

#[unsafe(no_mangle)]
pub extern "Rust" fn runner_prefixes(model: PluginModel) -> Vec<String> {
    with_model_read::<Model, _>(&model, |model| vec![model.config.prefix.clone()])
        .unwrap_or_default()
}

#[unsafe(no_mangle)]
pub extern "Rust" fn runner_status(model: PluginModel) -> oxibar_plugin_api::RunnerStatus {
    with_model_read::<Model, _>(&model, |model| oxibar_plugin_api::RunnerStatus {
        available: true,
        indexing: model.refreshing,
        message: model.health.clone().or_else(|| model.usage_error.clone()),
    })
    .unwrap_or_default()
}

#[unsafe(no_mangle)]
pub extern "Rust" fn runner_cancel(model: PluginModel) {
    with_model_write::<Model, _>(&model, |model| {
        model.generation = model.generation.wrapping_add(1);
        model.matches.clear();
        model.query_active = false;
        if let Ok(worker) = &model.query_worker {
            worker.cancel();
        }
    });
}

#[unsafe(no_mangle)]
pub extern "Rust" fn runner_favorite(
    model: PluginModel,
    index: usize,
) -> Option<oxibar_plugin_api::RunnerFavorite> {
    with_model_read::<Model, _>(&model, |model| {
        let entry = model.applications.iter().find(|entry| entry.id == index)?;
        let active = model.config.favorites.contains(&entry.desktop_id);
        let mut values = model.config.favorites.clone();
        if active {
            values.retain(|value| value != &entry.desktop_id);
        } else {
            values.push(entry.desktop_id.clone());
        }
        Some(oxibar_plugin_api::RunnerFavorite {
            path: "applications.favorites".to_owned(),
            values,
            active,
        })
    })
    .ok()
    .flatten()
}

#[unsafe(no_mangle)]
pub extern "Rust" fn runner_query(query: String, model: PluginModel) -> Option<Task<PluginMsg>> {
    with_model_write::<Model, _>(&model, |model| {
        if model.query == query && model.query_active {
            return Task::none();
        }
        model.query = query;
        model.query_active = true;
        query_task(model)
    })
}

#[unsafe(no_mangle)]
pub extern "Rust" fn runner_entries(model: PluginModel) -> Vec<RunnerEntry> {
    with_model_read::<Model, _>(&model, |model| {
        model
            .matches
            .iter()
            .filter_map(|matched| {
                let entry = model.applications.get(matched.index)?;
                Some(RunnerEntry {
                    index: entry.id,
                    score: matched.score,
                    title: entry.name.clone(),
                    description: entry.description.clone(),
                    icon: entry.icon.clone(),
                })
            })
            .collect()
    })
    .unwrap_or_default()
}

#[unsafe(no_mangle)]
pub extern "Rust" fn update(model: PluginModel, message: PluginMsg) -> Option<Task<PluginMsg>> {
    let message = message.downcast_ref::<Message>()?;
    with_model_write::<Model, _>(&model, |model| match message {
        Message::Refresh if !model.refreshing => Some(index_task(model)),
        Message::Refresh => {
            model.reindex_pending = true;
            None
        }
        Message::Indexed(generation, entries) if *generation == model.index_generation => {
            if model.reindex_pending {
                model.reindex_pending = false;
                return Some(index_task(model));
            }
            model.refreshing = false;
            match entries {
                Ok(entries) => {
                    model.health = None;
                    let mut entries = (**entries).clone();
                    for entry in &mut entries {
                        entry.id = *model
                            .identities
                            .entry(entry.desktop_id.clone())
                            .or_insert_with(|| {
                                let id = model.next_identity;
                                model.next_identity += 1;
                                id
                            });
                    }
                    model.dock = Arc::new(
                        entries
                            .iter()
                            .filter(|entry| !entry.action)
                            .map(|entry| oxibar_plugin_api::DockEntry {
                                index: entry.id,
                                desktop_id: entry.desktop_id.clone(),
                                title: entry.name.clone(),
                                icon: entry.icon.clone(),
                                window_class: entry.window_class.clone(),
                                executable: entry.executable.clone(),
                            })
                            .collect(),
                    );
                    model.applications = Arc::new(entries);
                    Some(query_task(model))
                }
                Err(error) => {
                    model.health = Some(error.clone());
                    model.errors.push(error.clone());
                    None
                }
            }
        }
        Message::Indexed(_, _) => None,
        Message::Searched(generation, entries) => {
            if *generation == model.generation {
                match entries {
                    Ok(entries) => model.matches = entries.clone(),
                    Err(error) if error != "provider work superseded" => {
                        model.errors.push(error.clone())
                    }
                    Err(_) => {}
                }
            }
            None
        }
        Message::Launched(key, result) => {
            if let Err(error) = result {
                model.errors.push(error.clone());
                return None;
            }
            model.usage.record(key);
            model.usage_generation = model.usage_generation.wrapping_add(1);
            Some(Task::batch([save_usage(model), query_task(model)]))
        }
        Message::UsageLoaded(result) => {
            match result {
                Ok(usage) => {
                    let mut usage = usage.clone();
                    usage.merge_local(&model.usage);
                    model.usage = usage;
                    model.usage_loaded = true;
                    model.usage_error = None;
                }
                Err(error) => {
                    model.usage_error = Some(error.clone());
                    model.errors.push(error.clone());
                }
            }
            Some(Task::batch([save_usage(model), query_task(model)]))
        }
        Message::UsageSaved(generation, result) => {
            if *generation == model.usage_generation {
                model.usage_error = result.as_ref().err().cloned();
                if let Err(error) = result {
                    model.errors.push(error.clone());
                }
            }
            None
        }
    })
    .flatten()
}

#[unsafe(no_mangle)]
pub extern "Rust" fn launch(index: usize, model: PluginModel) -> Option<Task<PluginMsg>> {
    let entry = with_model_read::<Model, _>(&model, |model| {
        model
            .applications
            .iter()
            .find(|entry| entry.id == index)
            .cloned()
    })
    .ok()??;
    let key = entry.desktop_id.clone();
    Some(Task::perform(
        async move {
            worker::run(move || system::launch(&entry))
                .await
                .unwrap_or_else(|error| Err(format!("application launch failed: {error}")))
        },
        move |result| Arc::new(Message::Launched(key.clone(), result)) as PluginMsg,
    ))
}

#[unsafe(no_mangle)]
pub extern "Rust" fn view(
    _model: PluginModel,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    Ok(Vec::new())
}

#[unsafe(no_mangle)]
pub extern "Rust" fn dock_entries(model: PluginModel) -> Arc<Vec<oxibar_plugin_api::DockEntry>> {
    with_model_read::<Model, _>(&model, |model| model.dock.clone()).unwrap_or_default()
}

#[unsafe(no_mangle)]
pub extern "Rust" fn errors(model: PluginModel) -> Vec<String> {
    drain_model_errors::<Model>(&model, |model| &mut model.errors)
}

#[unsafe(no_mangle)]
pub extern "Rust" fn subscription() -> *mut PluginStream {
    let interval = *REFRESH_INTERVAL.get().unwrap_or(&Duration::from_secs(300));
    let stream = refresh_stream(interval);
    Box::into_raw(Box::new(stream)) as *mut PluginStream
}

fn refresh_stream(interval: Duration) -> impl Stream<Item = PluginMsg> {
    oxibar_plugin_api::fswatch::FileWatch::new(system::watch_roots())
        .events(interval)
        .map(|()| Arc::new(Message::Refresh) as PluginMsg)
}

#[unsafe(no_mangle)]
pub extern "Rust" fn settings_schema() -> Vec<oxibar_plugin_api::SettingDescriptor> {
    use oxibar_plugin_api::toml::Value;
    use oxibar_plugin_api::{ApplyMode, SettingDescriptor, SettingKind};
    vec![
        SettingDescriptor::new(
            "applications.prefix",
            "Application prefix",
            SettingKind::Text,
            Some(Value::String("a ".to_owned())),
            ApplyMode::Live,
        ),
        SettingDescriptor::new(
            "applications.locale",
            "Desktop entry locale",
            SettingKind::Text,
            Some(Value::String(String::new())),
            ApplyMode::Live,
        ),
        SettingDescriptor::new(
            "applications.icon_theme",
            "Icon theme",
            SettingKind::Text,
            Some(Value::String("hicolor".to_owned())),
            ApplyMode::Live,
        ),
        SettingDescriptor::new(
            "applications.favorites",
            "Favorite desktop actions",
            SettingKind::TextList,
            Some(Value::Array(Vec::new())),
            ApplyMode::Live,
        ),
        SettingDescriptor::new(
            "applications.max_entries",
            "Application result limit",
            SettingKind::Integer { min: 1, max: 50 },
            Some(Value::Integer(7)),
            ApplyMode::Live,
        ),
        SettingDescriptor::new(
            "applications.terminal",
            "Terminal command",
            SettingKind::Arguments,
            Some(Value::String("kitty".to_owned())),
            ApplyMode::Live,
        )
        .description(
            "Include required execution flags. Changing this rebuilds the background index.",
        ),
        SettingDescriptor::new(
            "applications.refresh_seconds",
            "Index refresh interval",
            SettingKind::Integer {
                min: 1,
                max: i64::MAX,
            },
            Some(Value::Integer(300)),
            ApplyMode::Restart,
        ),
    ]
}

#[unsafe(no_mangle)]
pub extern "Rust" fn config_changed(model: PluginModel, global: Table) -> Option<Task<PluginMsg>> {
    let next = read_config(&global);
    with_model_write::<Model, _>(&model, |model| {
        let terminal_changed = model.config.terminal != next.terminal
            || model.config.locale != next.locale
            || model.config.icon_theme != next.icon_theme;
        if !terminal_changed
            && model.config.max_entries == next.max_entries
            && model.config.prefix == next.prefix
            && model.config.favorites == next.favorites
        {
            return Task::none();
        }
        model.config.max_entries = next.max_entries;
        model.config.terminal = next.terminal;
        model.config.locale = next.locale;
        model.config.icon_theme = next.icon_theme;
        model.config.prefix = next.prefix;
        model.config.favorites = next.favorites;
        if terminal_changed {
            model.dock = Arc::new(Vec::new());
            model.applications = Arc::new(Vec::new());
            model.matches.clear();
            model.generation = model.generation.wrapping_add(1);
            if model.refreshing {
                model.reindex_pending = true;
                if let Ok(worker) = &model.index_worker {
                    worker.cancel();
                }
                Task::none()
            } else {
                index_task(model)
            }
        } else {
            query_task(model)
        }
    })
}

#[unsafe(no_mangle)]
pub extern "Rust" fn config_validate(global: Table) -> Result<(), String> {
    let config = read_config(&global);
    if config.prefix.is_empty()
        || config.prefix.len() > 32
        || config.prefix.contains(['\0', '\n', '\r'])
    {
        return Err("applications.prefix must contain 1..32 printable bytes".to_owned());
    }
    if config.locale.len() > 128 || config.locale.contains('\0') {
        return Err("applications.locale exceeds its supported length".to_owned());
    }
    if config.icon_theme.is_empty()
        || config.icon_theme.len() > 128
        || !config
            .icon_theme
            .chars()
            .all(|character| character.is_alphanumeric() || matches!(character, '-' | '_' | '.'))
        || matches!(config.icon_theme.as_str(), "." | "..")
    {
        return Err("applications.icon_theme must be a theme name".to_owned());
    }
    if config.favorites.len() > 4096
        || config
            .favorites
            .iter()
            .any(|id| id.is_empty() || id.len() > 4096 || id.contains('\0'))
    {
        return Err("applications.favorites contains an invalid desktop action ID".to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxibar_plugin_api::toml;

    fn fixture() -> PluginModel {
        let mut state = Model::new(
            read_config(&Table::new()),
            Arc::new(UsageStore::at(
                std::env::temp_dir()
                    .join(format!("oxibar-unused-app-state-{}", std::process::id())),
            )),
        );
        state.refreshing = true;
        state.index_worker = Err("fixture indexing disabled".into());
        state.index_generation = 1;
        plugin_model(state)
    }

    #[test]
    fn live_config_changes_limits_and_invalidates_terminal_indexes() {
        let model = fixture();
        let initial_generation =
            with_model_read::<Model, _>(&model, |model| model.generation).unwrap();
        let _ = config_changed(model.clone(), toml::from_str("[bar]\nheight=38").unwrap());
        assert_eq!(
            with_model_read::<Model, _>(&model, |model| model.generation).unwrap(),
            initial_generation
        );
        let _ = config_changed(
            model.clone(),
            toml::from_str("[applications]\nmax_entries=2\nterminal='foot'").unwrap(),
        );
        assert!(
            with_model_read::<Model, _>(&model, |model| model.reindex_pending
                && model.config.max_entries == 2
                && model.config.terminal == ["foot"])
            .unwrap()
        );
        let next = update(
            model.clone(),
            Arc::new(Message::Indexed(1, Ok(Arc::new(Vec::new())))),
        );
        assert!(next.is_some());
        assert!(!with_model_read::<Model, _>(&model, |model| model.reindex_pending).unwrap());
    }

    #[test]
    fn stale_queries_cannot_replace_newer_results() {
        let model = fixture();
        let _ = runner_query("old".to_owned(), model.clone());
        let _ = runner_query("new".to_owned(), model.clone());
        let _ = update(
            model.clone(),
            Arc::new(Message::Searched(
                1,
                Ok(vec![Match {
                    index: 1,
                    score: 20,
                }]),
            )),
        );
        assert!(with_model_read::<Model, _>(&model, |model| model.matches.is_empty()).unwrap());
        let _ = update(
            model.clone(),
            Arc::new(Message::Searched(
                2,
                Ok(vec![Match {
                    index: 2,
                    score: 50,
                }]),
            )),
        );
        assert_eq!(
            with_model_read::<Model, _>(&model, |model| model.matches.clone()).unwrap(),
            vec![Match {
                index: 2,
                score: 50
            }]
        );
    }

    #[test]
    fn missing_selection_is_noop_and_launch_errors_drain_once() {
        let model = fixture();
        assert!(launch(0, model.clone()).is_none());
        let _ = update(
            model.clone(),
            Arc::new(Message::Launched(
                "missing.desktop".to_owned(),
                Err("missing executable".to_owned()),
            )),
        );
        assert_eq!(errors(model.clone()), ["missing executable"]);
        assert!(errors(model).is_empty());
    }

    #[test]
    fn catalog_and_runner_share_stable_launch_ids_across_reindex_and_removal() {
        let model = fixture();
        let index = |entries| {
            let _ = update(
                model.clone(),
                Arc::new(Message::Indexed(1, Ok(Arc::new(entries)))),
            );
        };
        index(vec![
            system::test_entry("a.desktop", "Alpha"),
            system::test_entry("b.desktop", "Beta"),
        ]);
        let catalog = dock_entries(model.clone());
        let beta = catalog
            .iter()
            .find(|entry| entry.desktop_id == "b.desktop")
            .unwrap()
            .index;
        assert!(Arc::ptr_eq(&catalog, &dock_entries(model.clone())));
        index(vec![
            system::test_entry("b.desktop", "Beta"),
            system::test_entry("new.desktop", "New"),
            system::test_entry("a.desktop", "Alpha"),
        ]);
        assert_eq!(
            dock_entries(model.clone())
                .iter()
                .find(|entry| entry.desktop_id == "b.desktop")
                .unwrap()
                .index,
            beta
        );
        with_model_write::<Model, _>(&model, |model| {
            model.matches = vec![Match {
                index: 0,
                score: 100,
            }]
        });
        assert_eq!(runner_entries(model.clone())[0].index, beta);
        index(vec![
            system::test_entry("new.desktop", "New"),
            system::test_entry("a.desktop", "Alpha"),
        ]);
        assert!(launch(beta, model).is_none());
    }

    #[test]
    fn metadata_load_merges_unsaved_launches_without_overwriting_invalid_state() {
        let model = fixture();
        with_model_write::<Model, _>(&model, |model| model.query_active = false);
        let _ = update(
            model.clone(),
            Arc::new(Message::Launched("a.desktop".to_owned(), Ok(()))),
        );
        assert!(!with_model_read::<Model, _>(&model, |model| model.usage_loaded).unwrap());
        let mut persisted = Usage::default();
        for _ in 0..5 {
            persisted.record("a.desktop");
        }
        let _ = update(model.clone(), Arc::new(Message::UsageLoaded(Ok(persisted))));
        assert!(with_model_read::<Model, _>(&model, |model| model.usage_loaded).unwrap());
        assert_eq!(
            with_model_read::<Model, _>(&model, |model| model.usage.ranking(&[])["a.desktop"])
                .unwrap(),
            260
        );
        let invalid = fixture();
        let _ = update(
            invalid.clone(),
            Arc::new(Message::UsageLoaded(Err("invalid state".to_owned()))),
        );
        let _ = update(
            invalid.clone(),
            Arc::new(Message::Launched("b.desktop".to_owned(), Ok(()))),
        );
        assert!(
            with_model_read::<Model, _>(&invalid, |model| !model.usage_loaded
                && model.usage_error.as_deref() == Some("invalid state"))
            .unwrap()
        );
        assert_eq!(
            runner_status(invalid).message.as_deref(),
            Some("invalid state")
        );
    }

    #[test]
    fn inactive_scopes_cancel_queries_and_desktop_actions_are_not_dock_apps() {
        let model = fixture();
        let parent = system::test_entry("a.desktop", "Alpha");
        let mut action = system::test_entry("a.desktop::new", "Alpha — New");
        action.action = true;
        let _ = update(
            model.clone(),
            Arc::new(Message::Indexed(1, Ok(Arc::new(vec![parent, action])))),
        );
        assert_eq!(dock_entries(model.clone()).len(), 1);
        runner_cancel(model.clone());
        assert!(runner_entries(model.clone()).is_empty());
        assert!(runner_query(String::new(), model.clone()).is_some());
        assert!(with_model_read::<Model, _>(&model, |model| model.query_active).unwrap());
    }
}
