use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use iced::{futures::SinkExt, stream};
use notify::{RecursiveMode, Watcher};

use crate::messages::Message;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct WatchPaths {
    pub directory: PathBuf,
    pub base_target: Option<PathBuf>,
    pub override_target: Option<PathBuf>,
}

impl WatchPaths {
    pub(crate) fn new(directory: &Path) -> Self {
        Self {
            directory: directory.to_owned(),
            base_target: directory.join("config.toml").canonicalize().ok(),
            override_target: directory.join("overrides.toml").canonicalize().ok(),
        }
    }
}

pub(crate) fn watch_changes(
    paths: &WatchPaths,
) -> impl iced::futures::Stream<Item = Message> + use<> {
    let paths = paths.clone();
    stream::channel(1, async move |mut output| {
        let (mut sender, mut events) = iced::futures::channel::mpsc::channel(1);
        let directory = paths.directory.clone();
        let targets = [paths.base_target.clone(), paths.override_target.clone()];
        let mut watcher =
            match notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                if let Ok(event) = event
                    && !matches!(event.kind, notify::EventKind::Access(_))
                    && event.paths.iter().any(|path| {
                        path == &directory.join("config.toml")
                            || path == &directory.join("overrides.toml")
                            || targets.iter().flatten().any(|target| target == path)
                    })
                {
                    let _ = sender.try_send(());
                }
            }) {
                Ok(watcher) => watcher,
                Err(error) => {
                    tracing::warn!(%error, "could not start configuration watcher");
                    return;
                }
            };
        if let Err(error) = watcher.watch(&paths.directory, RecursiveMode::NonRecursive) {
            tracing::warn!(%error, "could not watch configuration directory");
            return;
        }
        for target in [paths.base_target, paths.override_target]
            .into_iter()
            .flatten()
        {
            let _ = watcher.watch(&target, RecursiveMode::NonRecursive);
        }
        use iced::futures::StreamExt;
        while events.next().await.is_some() {
            // Coalesce atomic-save events on the host reactor, not the UI thread.
            tokio::time::sleep(Duration::from_millis(40)).await;
            if output.send(Message::ReloadConfig).await.is_err() {
                break;
            }
        }
    })
}
