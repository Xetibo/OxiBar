use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use iced::futures::{FutureExt, StreamExt, channel::mpsc, stream::BoxStream};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};

#[derive(Debug)]
pub struct FileWatch {
    state: Mutex<State>,
}

#[derive(Debug)]
struct State {
    roots: Vec<PathBuf>,
    sender: Option<mpsc::Sender<()>>,
}

impl FileWatch {
    pub fn new(roots: Vec<PathBuf>) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State {
                roots,
                sender: None,
            }),
        })
    }

    pub fn configure(&self, roots: Vec<PathBuf>) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.roots != roots {
            state.roots = roots;
            if let Some(sender) = &mut state.sender {
                let _ = sender.try_send(());
            }
        }
    }

    pub fn events(self: &Arc<Self>, interval: Duration) -> BoxStream<'static, ()> {
        let client = self.clone();
        let (sender, receiver) = mpsc::channel(1);
        client
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .sender = Some(sender.clone());
        let watcher = RecommendedWatcher::new(move |result: notify::Result<notify::Event>| {
            if !matches!(result, Ok(event) if matches!(event.kind, notify::EventKind::Access(_))) {
                let _ = sender.clone().try_send(());
            }
        }, notify::Config::default()).ok();
        let state = Events {
            client,
            receiver,
            watcher,
            watched: BTreeMap::new(),
            next: std::time::Instant::now() + interval,
            interval,
        };
        iced::futures::stream::unfold(state, |mut state| async move {
            state.configure();
            let changed = {
                let timer = FutureExt::fuse(async_io::Timer::at(state.next));
                let changed = state.receiver.next().fuse();
                iced::futures::pin_mut!(timer, changed);
                iced::futures::select! {
                    _ = timer => false,
                    event = changed => { event?; true }
                }
            };
            if changed {
                async_io::Timer::after(Duration::from_millis(150)).await;
                while state.receiver.try_recv().is_ok() {}
            } else {
                state.next = std::time::Instant::now() + state.interval;
            }
            state.configure();
            Some(((), state))
        })
        .boxed()
    }
}

struct Events {
    client: Arc<FileWatch>,
    receiver: mpsc::Receiver<()>,
    watcher: Option<RecommendedWatcher>,
    watched: BTreeMap<PathBuf, RecursiveMode>,
    next: std::time::Instant,
    interval: Duration,
}

impl Events {
    fn configure(&mut self) {
        let roots = self
            .client
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .roots
            .clone();
        let wanted = watched_roots(&roots);
        if let Some(watcher) = &mut self.watcher {
            for path in self.watched.keys() {
                let _ = watcher.unwatch(path);
            }
            self.watched.clear();
            for (path, mode) in wanted {
                if watcher.watch(&path, mode).is_ok() {
                    self.watched.insert(path, mode);
                }
            }
        }
    }
}

fn watched_roots(roots: &[PathBuf]) -> BTreeMap<PathBuf, RecursiveMode> {
    let mut result = BTreeMap::new();
    for root in roots {
        let Some(path) = root.ancestors().find(|path| path.is_dir()) else {
            continue;
        };
        let mode = if path == root {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        };
        let value = result.entry(path.to_owned()).or_insert(mode);
        if mode == RecursiveMode::Recursive {
            *value = mode;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_roots_watch_only_the_parent_then_recover_and_drop_without_a_reactor() {
        let root = std::env::temp_dir().join(format!(
            "oxibar-watch-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let watched = root.join("applications");
        let client = FileWatch::new(vec![watched.clone()]);
        assert_eq!(
            watched_roots(std::slice::from_ref(&watched)).get(&root),
            Some(&RecursiveMode::NonRecursive)
        );
        let mut stream = client.events(Duration::from_secs(60));
        assert!(stream.next().now_or_never().is_none());
        std::fs::create_dir(&watched).unwrap();
        let wait = |stream: &mut BoxStream<'static, ()>| {
            iced::futures::executor::block_on(async {
                match iced::futures::future::select(
                    stream.next(),
                    Box::pin(async_io::Timer::after(Duration::from_secs(3))),
                )
                .await
                {
                    iced::futures::future::Either::Left((Some(()), _)) => (),
                    _ => panic!("owned filesystem watcher did not recover"),
                }
            });
        };
        wait(&mut stream);
        assert_eq!(
            watched_roots(std::slice::from_ref(&watched)).get(&watched),
            Some(&RecursiveMode::Recursive)
        );
        std::fs::write(watched.join("fixture.desktop"), "fixture").unwrap();
        wait(&mut stream);
        let before = std::time::Instant::now();
        drop(stream);
        assert!(before.elapsed() < Duration::from_secs(1));
        std::fs::remove_dir_all(root).unwrap();
    }
}
