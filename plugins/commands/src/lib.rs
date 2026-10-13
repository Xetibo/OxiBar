mod config;
mod index;
mod query;
#[cfg(test)]
mod tests;

use iced::{Element, Task};
use oxibar_plugin_api::{
    ApplyMode, CommandOutcome, PluginAvailability, PluginModel, PluginMsg, RunnerEntry,
    RunnerFavorite, RunnerStatus, SettingDescriptor, SettingKind, drain_model_errors,
    fswatch::FileWatch,
    plugin_model,
    runner::{Usage, UsageStore},
    toml::Table as TomlTable,
    with_model_read, with_model_write,
    worker::LatestWorker,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, OnceLock, Weak},
    time::Duration,
};

#[derive(Debug, Clone)]
struct Entry {
    id: usize,
    found: query::Found,
    argv: Vec<String>,
}

#[derive(Debug)]
struct Model {
    config: config::Config,
    index: Arc<index::Index>,
    entries: Vec<Entry>,
    identities: BTreeMap<(String, Vec<String>), usize>,
    next_id: usize,
    query: String,
    active: bool,
    generation: u64,
    index_generation: u64,
    refreshing: bool,
    refresh_pending: bool,
    index_worker: Result<LatestWorker<Arc<index::Index>>, String>,
    query_worker: Result<LatestWorker<Result<Vec<query::Found>, String>>, String>,
    watch: Arc<FileWatch>,
    usage: Usage,
    store: Arc<UsageStore>,
    usage_loaded: bool,
    usage_generation: u64,
    health: Option<String>,
    query_error: Option<String>,
    errors: Vec<String>,
}

#[derive(Debug, Clone)]
enum Message {
    Refresh,
    Indexed(u64, Result<Arc<index::Index>, String>),
    Matches(u64, Result<Vec<query::Found>, String>),
    Launched(String, Result<(), String>),
    UsageLoaded(Result<Usage, String>),
    UsageSaved(u64, Result<(), String>),
}

static STREAMS: OnceLock<Mutex<(Weak<FileWatch>, Duration)>> = OnceLock::new();

fn msg(message: Message) -> PluginMsg {
    Arc::new(message)
}

impl Model {
    fn new(config: config::Config, store: Arc<UsageStore>) -> Self {
        Self {
            config,
            index: Arc::new(index::Index::new()),
            entries: Vec::new(),
            identities: BTreeMap::new(),
            next_id: 0,
            query: String::new(),
            active: true,
            generation: 0,
            index_generation: 0,
            refreshing: false,
            refresh_pending: false,
            index_worker: LatestWorker::new("oxibar-commands-index"),
            query_worker: LatestWorker::new("oxibar-commands-query"),
            watch: FileWatch::new(index::roots()),
            usage: Usage::default(),
            store,
            usage_loaded: false,
            usage_generation: 0,
            health: None,
            query_error: None,
            errors: Vec::new(),
        }
    }

    fn index_task(&mut self) -> Task<PluginMsg> {
        self.index_generation = self.index_generation.wrapping_add(1);
        let generation = self.index_generation;
        self.refreshing = true;
        let roots = index::roots();
        self.watch.configure(roots.clone());
        let job = self
            .index_worker
            .as_ref()
            .map(|worker| worker.submit(move |cancel| Arc::new(index::read(&roots, &cancel))))
            .unwrap_or_else(|error| Box::pin(iced::futures::future::ready(Err(error.clone()))));
        Task::perform(job, move |result| msg(Message::Indexed(generation, result)))
    }

    fn query_task(&mut self) -> Task<PluginMsg> {
        self.generation = self.generation.wrapping_add(1);
        self.entries.clear();
        self.query_error = None;
        if !self.active {
            return Task::none();
        }
        let generation = self.generation;
        let config = self.config.clone();
        let index = self.index.clone();
        let query = self.query.clone();
        let ranking = self.usage.ranking(&config.favorites);
        let job = self
            .query_worker
            .as_ref()
            .map(|worker| {
                worker
                    .submit(move |cancel| query::search(&config, &index, &query, &ranking, &cancel))
            })
            .unwrap_or_else(|error| Box::pin(iced::futures::future::ready(Err(error.clone()))));
        Task::perform(job, move |result| {
            msg(Message::Matches(
                generation,
                result.and_then(|result| result),
            ))
        })
    }

    fn matches(&mut self, found: &[query::Found]) {
        if self.identities.len() > 8192 {
            self.identities.clear();
        }
        self.entries = found
            .iter()
            .map(|found| {
                let argv = if self.config.command_terminal {
                    self.config
                        .terminal
                        .iter()
                        .chain(&found.argv)
                        .cloned()
                        .collect()
                } else {
                    found.argv.clone()
                };
                let id = *self
                    .identities
                    .entry((found.key.clone(), argv.clone()))
                    .or_insert_with(|| {
                        let id = self.next_id;
                        self.next_id = self.next_id.saturating_add(1);
                        id
                    });
                Entry {
                    id,
                    found: found.clone(),
                    argv,
                }
            })
            .collect();
    }

    fn save_usage(&self) -> Task<PluginMsg> {
        if !self.usage_loaded || self.usage_generation == 0 {
            return Task::none();
        }
        let generation = self.usage_generation;
        let store = self.store.clone();
        let usage = self.usage.clone();
        Task::perform(
            short_job(move || store.save(&usage, generation)),
            move |result| {
                msg(Message::UsageSaved(
                    generation,
                    result.and_then(|result| result),
                ))
            },
        )
    }
}

async fn short_job<T: Send + 'static>(
    job: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String> {
    let (sender, receiver) = iced::futures::channel::oneshot::channel();
    std::thread::Builder::new()
        .name("oxibar-commands-action".into())
        .spawn(move || {
            let _ = sender.send(job());
        })
        .map_err(|error| error.to_string())?;
    receiver
        .await
        .map_err(|_| "Commands action worker stopped".into())
}

#[unsafe(no_mangle)]
pub extern "Rust" fn abi_version() -> u32 {
    oxibar_plugin_api::ABI_VERSION
}
oxibar_plugin_api::export_theme_changed!();
#[unsafe(no_mangle)]
pub extern "Rust" fn name() -> &'static str {
    "Commands"
}
#[unsafe(no_mangle)]
pub extern "Rust" fn availability(_: TomlTable) -> PluginAvailability {
    PluginAvailability::Available
}

#[unsafe(no_mangle)]
pub extern "Rust" fn model(table: TomlTable) -> (PluginModel, Option<Task<PluginMsg>>) {
    let configured = config::read(&table);
    let mut state = Model::new(
        configured.clone().unwrap_or_else(|_| {
            config::read(&TomlTable::new()).expect("built-in commands configuration")
        }),
        Arc::new(UsageStore::new("commands")),
    );
    if let Err(error) = configured {
        state.health = Some(error.clone());
        state.errors.push(error);
    }
    *STREAMS
        .get_or_init(|| Mutex::new((Weak::new(), Duration::from_secs(300))))
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = (
        Arc::downgrade(&state.watch),
        Duration::from_secs(state.config.refresh_seconds),
    );
    let store = state.store.clone();
    let initial = Task::batch([
        state.index_task(),
        Task::perform(short_job(move || store.load()), |result| {
            msg(Message::UsageLoaded(result.and_then(|result| result)))
        }),
    ]);
    (plugin_model(state), Some(initial))
}

#[unsafe(no_mangle)]
pub extern "Rust" fn update(model: PluginModel, message: PluginMsg) -> Option<Task<PluginMsg>> {
    let message = message.downcast_ref::<Message>()?;
    with_model_write::<Model, _>(&model, |state| match message {
        Message::Refresh => {
            if state.refreshing {
                state.refresh_pending = true;
                Task::none()
            } else {
                state.index_task()
            }
        }
        Message::Indexed(generation, result) if *generation == state.index_generation => {
            state.refreshing = false;
            if state.refresh_pending {
                state.refresh_pending = false;
                return state.index_task();
            }
            match result {
                Ok(index) => state.index = index.clone(),
                Err(error) => {
                    state.health = Some(error.clone());
                    state.errors.push(error.clone());
                }
            }
            state.query_task()
        }
        Message::Matches(generation, result) if *generation == state.generation => {
            match result {
                Ok(found) => state.matches(found),
                Err(error)
                    if !error.to_lowercase().contains("cancel")
                        && !error.to_lowercase().contains("supersed") =>
                {
                    state.query_error = Some(error.clone())
                }
                Err(_) => (),
            }
            Task::none()
        }
        Message::Launched(key, result) => {
            match result {
                Ok(()) => {
                    state.usage.record(key);
                    state.usage_generation = state.usage_generation.wrapping_add(1);
                    return Task::batch([state.save_usage(), state.query_task()]);
                }
                Err(error) => {
                    state.query_error = Some(error.clone());
                    state.errors.push(error.clone());
                }
            }
            Task::none()
        }
        Message::UsageLoaded(result) => {
            match result {
                Ok(usage) => {
                    let mut merged = usage.clone();
                    merged.merge_local(&state.usage);
                    state.usage = merged;
                    state.usage_loaded = true;
                }
                Err(error) => state.errors.push(error.clone()),
            }
            Task::batch([state.save_usage(), state.query_task()])
        }
        Message::UsageSaved(generation, Err(error)) if *generation == state.usage_generation => {
            state.errors.push(error.clone());
            Task::none()
        }
        _ => Task::none(),
    })
}

#[unsafe(no_mangle)]
pub extern "Rust" fn launch(index: usize, model: PluginModel) -> Option<Task<PluginMsg>> {
    let entry = with_model_read::<Model, _>(&model, |state| {
        state
            .entries
            .iter()
            .find(|entry| entry.id == index)
            .cloned()
    })
    .ok()??;
    Some(Task::perform(
        short_job(move || oxibar_plugin_api::launch::launch(entry.argv, None).map(|_| ())),
        move |result| {
            msg(Message::Launched(
                entry.found.key.clone(),
                result.and_then(|result| result),
            ))
        },
    ))
}
#[unsafe(no_mangle)]
pub extern "Rust" fn errors(model: PluginModel) -> Vec<String> {
    drain_model_errors::<Model>(&model, |state| &mut state.errors)
}
#[unsafe(no_mangle)]
pub extern "Rust" fn view(
    _: PluginModel,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    Ok(Vec::new())
}
#[unsafe(no_mangle)]
pub extern "Rust" fn subscription() -> *mut oxibar_plugin_api::PluginStream {
    use iced::futures::{StreamExt, stream};
    let (watch, interval) = STREAMS
        .get()
        .and_then(|state| state.lock().ok())
        .map(|state| (state.0.upgrade(), state.1))
        .unwrap_or((None, Duration::from_secs(300)));
    let stream = watch
        .map(|watch| {
            watch
                .events(interval)
                .map(|_| msg(Message::Refresh))
                .boxed()
        })
        .unwrap_or_else(|| stream::pending().boxed());
    Box::into_raw(Box::new(stream)) as *mut oxibar_plugin_api::PluginStream
}
#[unsafe(no_mangle)]
pub extern "Rust" fn runner_query(query: String, model: PluginModel) -> Option<Task<PluginMsg>> {
    with_model_write::<Model, _>(&model, |state| {
        if query == state.query && state.active {
            return Task::none();
        }
        state.query = query.chars().take(1024).collect();
        state.active = true;
        state.query_task()
    })
}
#[unsafe(no_mangle)]
pub extern "Rust" fn runner_cancel(model: PluginModel) {
    let _ = with_model_write::<Model, _>(&model, |state| {
        state.active = false;
        state.generation = state.generation.wrapping_add(1);
        state.entries.clear();
        if let Ok(worker) = &state.query_worker {
            worker.cancel();
        }
    });
}
#[unsafe(no_mangle)]
pub extern "Rust" fn runner_prefixes(_: PluginModel) -> Vec<String> {
    vec![">".into()]
}
#[unsafe(no_mangle)]
pub extern "Rust" fn runner_status(model: PluginModel) -> RunnerStatus {
    with_model_read::<Model, _>(&model, |state| RunnerStatus {
        available: true,
        indexing: state.refreshing,
        message: state.query_error.clone().or_else(|| state.health.clone()),
    })
    .unwrap_or_default()
}
#[unsafe(no_mangle)]
pub extern "Rust" fn runner_entries(model: PluginModel) -> Vec<RunnerEntry> {
    with_model_read::<Model, _>(&model, |state| {
        state
            .entries
            .iter()
            .map(|entry| RunnerEntry {
                index: entry.id,
                title: entry.found.title.clone(),
                description: if state.config.command_terminal {
                    "Command · run in terminal"
                } else {
                    "Command · run argument list"
                }
                .into(),
                score: entry.found.score,
                icon: None,
            })
            .collect()
    })
    .unwrap_or_default()
}
#[unsafe(no_mangle)]
pub extern "Rust" fn runner_favorite(model: PluginModel, index: usize) -> Option<RunnerFavorite> {
    with_model_read::<Model, _>(&model, |state| {
        let key = &state
            .entries
            .iter()
            .find(|entry| entry.id == index)?
            .found
            .key;
        let mut values = state.config.favorites.clone();
        let active = values.contains(key);
        if active {
            values.retain(|value| value != key);
        } else {
            values.push(key.clone());
        }
        Some(RunnerFavorite {
            path: "commands.favorites".into(),
            values,
            active,
        })
    })
    .ok()
    .flatten()
}
#[unsafe(no_mangle)]
pub extern "Rust" fn config_validate(table: TomlTable) -> Result<(), String> {
    config::read(&table).map(|_| ())
}
#[unsafe(no_mangle)]
pub extern "Rust" fn config_changed(
    model: PluginModel,
    table: TomlTable,
) -> Option<Task<PluginMsg>> {
    with_model_write::<Model, _>(&model, |state| match config::read(&table) {
        Ok(mut configured) => {
            configured.refresh_seconds = state.config.refresh_seconds;
            state.health = None;
            if configured == state.config {
                return Task::none();
            }
            state.config = configured;
            state.query_task()
        }
        Err(error) => {
            state.health = Some(error);
            Task::none()
        }
    })
}
#[unsafe(no_mangle)]
pub extern "Rust" fn command(
    _: PluginModel,
    command: String,
    arguments: Vec<String>,
) -> Result<Task<CommandOutcome>, String> {
    if command != "refresh" || !arguments.is_empty() {
        return Err("commands: expected refresh".into());
    }
    Ok(Task::done(CommandOutcome {
        message: msg(Message::Refresh),
        result: Ok(()),
    }))
}
#[unsafe(no_mangle)]
pub extern "Rust" fn settings_schema() -> Vec<SettingDescriptor> {
    vec![
        SettingDescriptor::new(
            "commands.max_entries",
            "Command result limit",
            SettingKind::Integer { min: 1, max: 50 },
            Some(7.into()),
            ApplyMode::Live,
        ),
        SettingDescriptor::new(
            "commands.terminal",
            "Command terminal",
            SettingKind::Arguments,
            Some("kitty".into()),
            ApplyMode::Live,
        ),
        SettingDescriptor::new(
            "commands.command_terminal",
            "Run commands in terminal",
            SettingKind::Boolean,
            Some(true.into()),
            ApplyMode::Live,
        ),
        SettingDescriptor::new(
            "commands.search_unprefixed",
            "Include commands in application searches",
            SettingKind::Boolean,
            Some(false.into()),
            ApplyMode::Live,
        ),
        SettingDescriptor::new(
            "commands.favorites",
            "Favorite command identities",
            SettingKind::TextList,
            Some(Vec::<String>::new().into()),
            ApplyMode::Live,
        ),
        SettingDescriptor::new(
            "commands.refresh_seconds",
            "Command index refresh seconds",
            SettingKind::Integer { min: 1, max: 3600 },
            Some(300.into()),
            ApplyMode::Restart,
        ),
    ]
}
