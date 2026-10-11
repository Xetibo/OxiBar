use std::{
    collections::BTreeMap,
    os::unix::fs::MetadataExt,
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
        let callback_client = client.clone();
        let watcher = RecommendedWatcher::new(
            move |result: notify::Result<notify::Event>| {
                let relevant = match result {
                    Ok(event) => {
                        let state = callback_client
                            .state
                            .lock()
                            .unwrap_or_else(|error| error.into_inner());
                        !matches!(event.kind, notify::EventKind::Access(_))
                            && (event.paths.is_empty()
                                || event.paths.iter().any(|path| {
                                    state.roots.iter().any(|root| {
                                        path.starts_with(root) || root.starts_with(path)
                                    })
                                }))
                    }
                    Err(_) => true,
                };
                if relevant {
                    let _ = sender.clone().try_send(());
                }
            },
            notify::Config::default(),
        )
        .ok();
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
    watched: BTreeMap<PathBuf, Watched>,
    next: std::time::Instant,
    interval: Duration,
}

#[derive(Clone, PartialEq, Eq)]
struct Watched {
    mode: RecursiveMode,
    device: u64,
    inode: u64,
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
        let wanted: BTreeMap<_, _> = watched_roots(&roots)
            .into_iter()
            .filter_map(|(path, mode)| {
                let metadata = std::fs::metadata(&path).ok()?;
                Some((
                    path,
                    Watched {
                        mode,
                        device: metadata.dev(),
                        inode: metadata.ino(),
                    },
                ))
            })
            .collect();
        if let Some(watcher) = &mut self.watcher {
            let removed: Vec<_> = self
                .watched
                .iter()
                .filter(|(path, old)| wanted.get(*path) != Some(*old))
                .map(|(path, _)| path.clone())
                .collect();
            for path in removed {
                let _ = watcher.unwatch(&path);
                self.watched.remove(&path);
            }
            for (path, watched) in wanted {
                if self.watched.get(&path) != Some(&watched)
                    && watcher.watch(&path, watched.mode).is_ok()
                {
                    self.watched.insert(path, watched);
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
        std::fs::write(root.join("unrelated-trace.log"), "sibling noise").unwrap();
        iced::futures::executor::block_on(async {
            assert!(
                matches!(
                    iced::futures::future::select(
                        stream.next(),
                        Box::pin(async_io::Timer::after(Duration::from_millis(350)))
                    )
                    .await,
                    iced::futures::future::Either::Right(_)
                ),
                "missing-root parent watches must ignore unrelated siblings"
            );
        });
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
        iced::futures::executor::block_on(async {
            assert!(
                matches!(
                    iced::futures::future::select(
                        stream.next(),
                        Box::pin(async_io::Timer::after(Duration::from_millis(350)))
                    )
                    .await,
                    iced::futures::future::Either::Right(_)
                ),
                "unchanged watches must not generate self-refresh loops"
            );
        });
        let before = std::time::Instant::now();
        drop(stream);
        assert!(before.elapsed() < Duration::from_secs(1));
        std::fs::remove_dir_all(root).unwrap();
    }
}
