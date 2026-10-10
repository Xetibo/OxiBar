use std::{
    sync::{Arc, OnceLock},
    time::Duration,
};

use iced::{Element, Task, futures::Stream, stream};
use oxibar_plugin_api::{
    ABI_VERSION, PluginModel, PluginMsg, PluginStream, RunnerEntry, drain_model_errors,
    plugin_model, toml::Table, with_model_read, with_model_write,
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
    errors: Vec<String>,
}

#[derive(Debug)]
enum Message {
    Refresh,
    Indexed(Result<Arc<Vec<Entry>>, String>),
    Searched(u64, Result<Vec<Match>, String>),
    Launched(Result<(), String>),
}

fn index_task(config: Config) -> Task<PluginMsg> {
    Task::perform(
        async move {
            worker::run(move || system::load_entries(&config))
                .await
                .map(Arc::new)
                .map_err(|error| format!("application indexing failed: {error}"))
        },
        |entries| Arc::new(Message::Indexed(entries)) as PluginMsg,
    )
}

fn query_task(model: &mut Model) -> Task<PluginMsg> {
    model.generation = model.generation.wrapping_add(1);
    model.matches.clear();
    let generation = model.generation;
    let entries = model.applications.clone();
    let query = model.query.clone();
    let limit = model.config.max_entries;
    Task::perform(
        async move { worker::run(move || system::search(&entries, &query, limit)).await },
        move |entries| Arc::new(Message::Searched(generation, entries)) as PluginMsg,
    )
}

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
    let task = index_task(config.clone());
    (
        plugin_model(Model {
            identities: std::collections::BTreeMap::new(),
            next_identity: 0,
            dock: Arc::new(Vec::new()),
            config,
            applications: Arc::new(Vec::new()),
            matches: Vec::new(),
            query: String::new(),
            generation: 0,
            refreshing: true,
            reindex_pending: false,
            errors: Vec::new(),
        }),
        Some(task),
    )
}

#[unsafe(no_mangle)]
pub extern "Rust" fn runner_query(query: String, model: PluginModel) -> Option<Task<PluginMsg>> {
    with_model_write::<Model, _>(&model, |model| {
        if model.query == query {
            return Task::none();
        }
        model.query = query;
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
        Message::Refresh if !model.refreshing => {
            model.refreshing = true;
            Some(index_task(model.config.clone()))
        }
        Message::Refresh => None,
        Message::Indexed(entries) => {
            if model.reindex_pending {
                model.reindex_pending = false;
                return Some(index_task(model.config.clone()));
            }
            model.refreshing = false;
            match entries {
                Ok(entries) => {
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
                    model.errors.push(error.clone());
                    None
                }
            }
        }
        Message::Searched(generation, entries) => {
            if *generation == model.generation {
                match entries {
                    Ok(entries) => model.matches = entries.clone(),
                    Err(error) => model.errors.push(error.clone()),
                }
            }
            None
        }
        Message::Launched(result) => {
            if let Err(error) = result {
                model.errors.push(error.clone());
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
    Some(Task::perform(
        async move {
            worker::run(move || system::launch(&entry))
                .await
                .unwrap_or_else(|error| Err(format!("application launch failed: {error}")))
        },
        |result| Arc::new(Message::Launched(result)) as PluginMsg,
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
    stream::channel(1, async move |output| {
        let _timer = worker::RefreshTimer::new(interval, output, || Arc::new(Message::Refresh));
        std::future::pending::<()>().await;
    })
}

#[unsafe(no_mangle)]
pub extern "Rust" fn settings_schema() -> Vec<oxibar_plugin_api::SettingDescriptor> {
    use oxibar_plugin_api::toml::Value;
    use oxibar_plugin_api::{ApplyMode, SettingDescriptor, SettingKind};
    vec![
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
        let terminal_changed = model.config.terminal != next.terminal;
        if !terminal_changed && model.config.max_entries == next.max_entries {
            return Task::none();
        }
        model.config.max_entries = next.max_entries;
        model.config.terminal = next.terminal;
        if terminal_changed {
            model.dock = Arc::new(Vec::new());
            model.applications = Arc::new(Vec::new());
            model.matches.clear();
            model.generation = model.generation.wrapping_add(1);
            if model.refreshing {
                model.reindex_pending = true;
                Task::none()
            } else {
                model.refreshing = true;
                index_task(model.config.clone())
            }
        } else {
            query_task(model)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxibar_plugin_api::toml;

    #[test]
    fn live_config_changes_limits_and_invalidates_terminal_indexes() {
        let (model, _) = model(Table::new());
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
            Arc::new(Message::Indexed(Ok(Arc::new(Vec::new())))),
        );
        assert!(next.is_some());
        assert!(!with_model_read::<Model, _>(&model, |model| model.reindex_pending).unwrap());
    }

    #[test]
    fn stale_queries_cannot_replace_newer_results() {
        let (model, _) = model(Table::new());
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
        let (model, _) = model(Table::new());
        assert!(launch(0, model.clone()).is_none());
        let _ = update(
            model.clone(),
            Arc::new(Message::Launched(Err("missing executable".to_owned()))),
        );
        assert_eq!(errors(model.clone()), ["missing executable"]);
        assert!(errors(model).is_empty());
    }

    #[test]
    fn catalog_and_runner_share_stable_launch_ids_across_reindex_and_removal() {
        let (model, _) = model(Table::new());
        let index = |entries| {
            let _ = update(
                model.clone(),
                Arc::new(Message::Indexed(Ok(Arc::new(entries)))),
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
}
