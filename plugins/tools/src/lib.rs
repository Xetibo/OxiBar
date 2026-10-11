mod calculator;
mod clipboard;
mod config;
mod index;
mod query;
#[cfg(test)]
mod tests;

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, OnceLock, Weak},
    time::Duration,
};

use iced::{Element, Task};
use oxibar_plugin_api::toml::Table as TomlTable;
use oxibar_plugin_api::{
    ApplyMode, CommandOutcome, PluginAvailability, PluginModel, PluginMsg, RunnerEntry,
    RunnerFavorite, RunnerStatus, SettingDescriptor, SettingKind, drain_model_errors,
    fswatch::FileWatch,
    plugin_model,
    runner::{Usage, UsageStore},
    with_model_read, with_model_write,
    worker::LatestWorker,
};

#[derive(Debug, Clone)]
struct Entry {
    id: usize,
    found: query::Found,
}

#[derive(Debug)]
struct Model {
    config: config::Config,
    index: Arc<index::Index>,
    entries: Vec<Entry>,
    identities: BTreeMap<String, usize>,
    next_id: usize,
    query: String,
    query_active: bool,
    generation: u64,
    index_generation: u64,
    refreshing: bool,
    reindex_pending: bool,
    index_worker: Result<LatestWorker<Arc<index::Index>>, String>,
    query_worker: Result<LatestWorker<Result<Vec<query::Found>, String>>, String>,
    clipboard: Option<Arc<clipboard::Client>>,
    clipboard_snapshot: clipboard::Snapshot,
    watch: Arc<FileWatch>,
    usage: Usage,
    usage_store: Arc<UsageStore>,
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
    Clipboard(clipboard::Snapshot),
    Launched(Option<String>, Result<(), String>),
    UsageLoaded(Result<Usage, String>),
    UsageSaved(u64, Result<(), String>),
}

struct Streams {
    clipboard: Weak<clipboard::Client>,
    watch: Weak<FileWatch>,
    interval: Duration,
}
static STREAMS: OnceLock<Mutex<Streams>> = OnceLock::new();

fn msg(message: Message) -> PluginMsg {
    Arc::new(message)
}

impl Model {
    fn new(config: config::Config, store: Arc<UsageStore>) -> Self {
        let clipboard = clipboard::Client::new(config.clipboard, config.clipboard_limit);
        let mut clipboard_snapshot = clipboard::Snapshot {
            enabled: config.clipboard,
            ..Default::default()
        };
        let clipboard = match clipboard {
            Ok(client) => Some(client),
            Err(error) => {
                clipboard_snapshot.error = Some(error);
                None
            }
        };
        let errors = clipboard_snapshot.error.iter().cloned().collect();
        let watch = FileWatch::new(index::roots(&config));
        Self {
            config,
            index: Arc::new(index::Index::default()),
            entries: Vec::new(),
            identities: BTreeMap::new(),
            next_id: 0,
            query: String::new(),
            query_active: true,
            generation: 0,
            index_generation: 0,
            refreshing: false,
            reindex_pending: false,
            index_worker: LatestWorker::new("oxibar-tools-index"),
            query_worker: LatestWorker::new("oxibar-tools-query"),
            clipboard,
            clipboard_snapshot,
            watch,
            usage: Usage::default(),
            usage_store: store,
            usage_loaded: false,
            usage_generation: 0,
            health: None,
            query_error: None,
            errors,
        }
    }

    fn index_task(&mut self) -> Task<PluginMsg> {
        self.index_generation = self.index_generation.wrapping_add(1);
        let generation = self.index_generation;
        self.refreshing = true;
        let config = self.config.clone();
        let job = self
            .index_worker
            .as_ref()
            .map(|worker| worker.submit(move |cancel| Arc::new(index::read(&config, &cancel))))
            .unwrap_or_else(|error| Box::pin(iced::futures::future::ready(Err(error.clone()))));
        Task::perform(job, move |result| msg(Message::Indexed(generation, result)))
    }

    fn query_task(&mut self) -> Task<PluginMsg> {
        self.generation = self.generation.wrapping_add(1);
        self.entries.clear();
        self.query_error = None;
        if !self.query_active {
            return Task::none();
        }
        let generation = self.generation;
        let config = self.config.clone();
        let index = self.index.clone();
        let clipboard = self.clipboard_snapshot.clone();
        let query = self.query.clone();
        let ranking = self.usage.ranking(&config.favorites);
        let job = self
            .query_worker
            .as_ref()
            .map(|worker| {
                worker.submit(move |cancel| {
                    query::search(&config, &index, &clipboard, &query, &ranking, &cancel)
                })
            })
            .unwrap_or_else(|error| Box::pin(iced::futures::future::ready(Err(error.clone()))));
        Task::perform(job, move |result| {
            msg(Message::Matches(
                generation,
                result.and_then(|result| result),
            ))
        })
    }

    fn save_usage(&self) -> Task<PluginMsg> {
        if !self.usage_loaded || self.usage_generation == 0 {
            return Task::none();
        }
        let generation = self.usage_generation;
        let store = self.usage_store.clone();
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
        .name("oxibar-tools-action".into())
        .spawn(move || {
            let _ = sender.send(job());
        })
        .map_err(|error| error.to_string())?;
    receiver
        .await
        .map_err(|_| "Tools action worker stopped".into())
}

fn execute(config: config::Config, intent: query::Intent) -> Result<(), String> {
    let (mut argv, directory) = match intent {
        query::Intent::Execute {
            argv,
            terminal,
            directory,
        } => {
            let arguments = if terminal {
                config.terminal.into_iter().chain(argv).collect()
            } else {
                argv
            };
            (arguments, directory)
        }
        query::Intent::Open(target) => (config.opener.into_iter().chain([target]).collect(), None),
        query::Intent::Copy { .. } => return Err("Clipboard copy requires native owner".into()),
    };
    if argv.is_empty() {
        return Err("Empty action".into());
    }
    let executable = argv.remove(0);
    let mut command = std::process::Command::new(executable);
    command
        .args(argv)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    if let Some(directory) = directory {
        command.current_dir(directory);
    }
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    std::thread::Builder::new()
        .name("oxibar-tools-reaper".into())
        .spawn(move || {
            let _ = child.wait();
        })
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[unsafe(no_mangle)]
pub extern "Rust" fn abi_version() -> u32 {
    oxibar_plugin_api::ABI_VERSION
}
oxibar_plugin_api::export_theme_changed!();
#[unsafe(no_mangle)]
pub extern "Rust" fn name() -> &'static str {
    "Tools"
}
#[unsafe(no_mangle)]
pub extern "Rust" fn availability(_: TomlTable) -> PluginAvailability {
    PluginAvailability::Available
}

#[unsafe(no_mangle)]
pub extern "Rust" fn model(table: TomlTable) -> (PluginModel, Option<Task<PluginMsg>>) {
    let configuration = config::read(&table);
    let mut state = Model::new(
        configuration.clone().unwrap_or_else(|_| {
            config::read(&TomlTable::new()).expect("built-in tools configuration")
        }),
        Arc::new(UsageStore::new("tools")),
    );
    if let Err(error) = configuration {
        state.health = Some(error.clone());
        state.errors.push(error);
    }
    let streams = Streams {
        clipboard: state
            .clipboard
            .as_ref()
            .map(Arc::downgrade)
            .unwrap_or_default(),
        watch: Arc::downgrade(&state.watch),
        interval: Duration::from_secs(state.config.refresh_seconds),
    };
    *STREAMS
        .get_or_init(|| {
            Mutex::new(Streams {
                clipboard: Weak::new(),
                watch: Weak::new(),
                interval: Duration::from_secs(300),
            })
        })
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = streams;
    let store = state.usage_store.clone();
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
                state.reindex_pending = true;
                Task::none()
            } else {
                state.index_task()
            }
        }
        Message::Indexed(generation, result) if *generation == state.index_generation => {
            state.refreshing = false;
            if state.reindex_pending {
                state.reindex_pending = false;
                return state.index_task();
            }
            match result {
                Ok(index) => {
                    state.index = index.clone();
                    state.health = None;
                }
                Err(error) => {
                    state.health = Some(error.clone());
                    state.errors.push(error.clone());
                }
            }
            state.query_task()
        }
        Message::Matches(generation, result) if *generation == state.generation => {
            match result {
                Ok(found) => {
                    if state.identities.len() > 8192 {
                        state.identities.clear();
                    }
                    state.entries = found
                        .iter()
                        .map(|found| {
                            let key = match &found.intent {
                                query::Intent::Copy { identity, .. } => identity.clone(),
                                _ => format!("{:?}:{:?}", found.key, found.intent),
                            };
                            let id = *state.identities.entry(key).or_insert_with(|| {
                                let id = state.next_id;
                                state.next_id = state.next_id.saturating_add(1);
                                id
                            });
                            Entry {
                                id,
                                found: found.clone(),
                            }
                        })
                        .collect();
                }
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
        Message::Clipboard(snapshot) => {
            state.clipboard_snapshot = snapshot.clone();
            if state.query.starts_with(':') || state.query.starts_with('=') {
                state.query_task()
            } else {
                Task::none()
            }
        }
        Message::Launched(key, result) => {
            match result {
                Ok(()) => {
                    if let Some(key) = key {
                        state.usage.record(key);
                        state.usage_generation = state.usage_generation.wrapping_add(1);
                        return Task::batch([state.save_usage(), state.query_task()]);
                    }
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
    let (config, entry, clipboard) = with_model_read::<Model, _>(&model, |state| {
        Some((
            state.config.clone(),
            state
                .entries
                .iter()
                .find(|entry| entry.id == index)?
                .clone(),
            state.clipboard.clone(),
        ))
    })
    .ok()??;
    match entry.found.intent {
        query::Intent::Copy { text, .. } => Some(match clipboard {
            Some(client) => Task::perform(client.copy(text), |result| {
                msg(Message::Launched(None, result))
            }),
            None => Task::done(msg(Message::Launched(
                None,
                Err("native clipboard service is unavailable".into()),
            ))),
        }),
        intent => Some(Task::perform(
            short_job(move || execute(config, intent)),
            move |result| {
                msg(Message::Launched(
                    entry.found.key.clone(),
                    result.and_then(|result| result),
                ))
            },
        )),
    }
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
    let (clipboard, watch, interval) = STREAMS
        .get()
        .and_then(|state| state.lock().ok())
        .map(|state| {
            (
                state.clipboard.upgrade(),
                state.watch.upgrade(),
                state.interval,
            )
        })
        .unwrap_or((None, None, Duration::from_secs(300)));
    let clipboard = clipboard
        .map(|client| {
            clipboard::events(client)
                .map(|snapshot| msg(Message::Clipboard(snapshot)))
                .boxed()
        })
        .unwrap_or_else(|| stream::pending().boxed());
    let watch = watch
        .map(|client| {
            client
                .events(interval)
                .map(|_| msg(Message::Refresh))
                .boxed()
        })
        .unwrap_or_else(|| stream::pending().boxed());
    Box::into_raw(Box::new(stream::select(clipboard, watch).boxed()))
        as *mut oxibar_plugin_api::PluginStream
}

#[unsafe(no_mangle)]
pub extern "Rust" fn runner_query(query: String, model: PluginModel) -> Option<Task<PluginMsg>> {
    with_model_write::<Model, _>(&model, |state| {
        if query == state.query && state.query_active {
            return Task::none();
        }
        state.query = query.chars().take(1024).collect();
        state.query_active = true;
        state.query_task()
    })
}
#[unsafe(no_mangle)]
pub extern "Rust" fn runner_cancel(model: PluginModel) {
    let _ = with_model_write::<Model, _>(&model, |state| {
        state.query_active = false;
        state.generation = state.generation.wrapping_add(1);
        state.entries.clear();
        if let Ok(worker) = &state.query_worker {
            worker.cancel();
        }
    });
}
#[unsafe(no_mangle)]
pub extern "Rust" fn runner_prefixes(_: PluginModel) -> Vec<String> {
    [">", "=", "/", "@", "!", ":"]
        .into_iter()
        .map(str::to_owned)
        .collect()
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
                description: entry.found.description.clone(),
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
        let key = state
            .entries
            .iter()
            .find(|entry| entry.id == index)?
            .found
            .key
            .as_ref()?;
        let mut values = state.config.favorites.clone();
        let active = values.contains(key);
        if active {
            values.retain(|value| value != key);
        } else {
            values.push(key.clone());
        }
        Some(RunnerFavorite {
            path: "tools.favorites".into(),
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
    let configured = match config::read(&table) {
        Ok(config) => config,
        Err(error) => {
            return with_model_write::<Model, _>(&model, |state| {
                state.health = Some(error);
                Task::none()
            });
        }
    };
    with_model_write::<Model, _>(&model, |state| {
        let mut configured = configured;
        configured.refresh_seconds = state.config.refresh_seconds;
        if configured == state.config {
            return Task::none();
        }
        let reindex = configured.roots != state.config.roots
            || configured.file_limit != state.config.file_limit;
        if let Some(client) = &state.clipboard {
            client.configure(configured.clipboard, configured.clipboard_limit);
        }
        state.clipboard_snapshot.enabled = configured.clipboard;
        if !configured.clipboard {
            state.clipboard_snapshot.entries = Arc::new(Vec::new());
        }
        state.watch.configure(index::roots(&configured));
        state.config = configured;
        if reindex {
            state.reindex_pending = false;
            if let Ok(worker) = &state.index_worker {
                worker.cancel();
            }
            return Task::batch([state.index_task(), state.query_task()]);
        }
        state.query_task()
    })
}
#[unsafe(no_mangle)]
pub extern "Rust" fn command(
    model: PluginModel,
    command: String,
    arguments: Vec<String>,
) -> Result<Task<CommandOutcome>, String> {
    match (command.as_str(), arguments.as_slice()) {
        ("copy", [text]) => {
            let client = with_model_read::<Model, _>(&model, |state| state.clipboard.clone())
                .map_err(|error| error.to_string())?
                .ok_or("native clipboard service is unavailable")?;
            let job = client.copy(text.clone());
            Ok(Task::perform(job, |result| CommandOutcome {
                message: msg(Message::Launched(None, result.clone())),
                result,
            }))
        }
        ("refresh", []) => Ok(Task::done(CommandOutcome {
            message: msg(Message::Refresh),
            result: Ok(()),
        })),
        _ => Err("tools: expected copy <text> or refresh".into()),
    }
}
#[unsafe(no_mangle)]
pub extern "Rust" fn settings_schema() -> Vec<SettingDescriptor> {
    vec![
        SettingDescriptor::new(
            "tools.max_entries",
            "Tools result limit",
            SettingKind::Integer { min: 1, max: 50 },
            Some(7.into()),
            ApplyMode::Live,
        ),
        SettingDescriptor::new(
            "tools.terminal",
            "Command terminal",
            SettingKind::Arguments,
            Some("kitty".into()),
            ApplyMode::Live,
        ),
        SettingDescriptor::new(
            "tools.opener",
            "File and bookmark opener",
            SettingKind::Arguments,
            Some("xdg-open".into()),
            ApplyMode::Live,
        ),
        SettingDescriptor::new(
            "tools.command_terminal",
            "Run commands in terminal",
            SettingKind::Boolean,
            Some(true.into()),
            ApplyMode::Live,
        ),
        SettingDescriptor::new(
            "tools.file_roots",
            "Indexed file directories",
            SettingKind::TextList,
            Some(vec!["~/Documents", "~/Downloads"].into()),
            ApplyMode::Live,
        ),
        SettingDescriptor::new(
            "tools.file_limit",
            "File index limit",
            SettingKind::Integer {
                min: 1,
                max: 100_000,
            },
            Some(20_000.into()),
            ApplyMode::Live,
        ),
        SettingDescriptor::new(
            "tools.bookmarks",
            "Bookmarks",
            SettingKind::TableList,
            Some(Vec::<oxibar_plugin_api::toml::Value>::new().into()),
            ApplyMode::Live,
        )
        .description("Records with stable id, name and URL; @ searches bookmarks"),
        SettingDescriptor::new(
            "tools.actions",
            "Named argument-list actions",
            SettingKind::TableList,
            Some(Vec::<oxibar_plugin_api::toml::Value>::new().into()),
            ApplyMode::Live,
        )
        .description("Records with stable id, name and command; ! searches actions"),
        SettingDescriptor::new(
            "tools.favorites",
            "Favorite tool identities",
            SettingKind::TextList,
            Some(Vec::<String>::new().into()),
            ApplyMode::Live,
        ),
        SettingDescriptor::new(
            "tools.clipboard_enabled",
            "Clipboard history",
            SettingKind::Boolean,
            Some(false.into()),
            ApplyMode::Live,
        )
        .description("Opt-in, ephemeral text history; never writes clipboard contents to disk"),
        SettingDescriptor::new(
            "tools.clipboard_limit",
            "Clipboard history limit",
            SettingKind::Integer { min: 1, max: 100 },
            Some(50.into()),
            ApplyMode::Live,
        ),
        SettingDescriptor::new(
            "tools.refresh_seconds",
            "Tool index fallback refresh",
            SettingKind::Integer { min: 1, max: 3600 },
            Some(300.into()),
            ApplyMode::Restart,
        ),
    ]
}
