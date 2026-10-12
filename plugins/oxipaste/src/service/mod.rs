mod history;
mod native;

use iced::futures::{StreamExt, stream::BoxStream};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use zbus::{fdo, interface};

use crate::backend::{self, Item};
pub(crate) use native::Snapshot;
use native::{Client, Command};

const NAME: &str = "org.Xetibo.OxiPasteDaemon";
const PATH: &str = "/org/Xetibo/OxiPasteDaemon";
static ACTIVE: AtomicBool = AtomicBool::new(false);

struct Owner;
impl Drop for Owner {
    fn drop(&mut self) {
        ACTIVE.store(false, Ordering::Release);
    }
}

struct HistoryService {
    client: Arc<Client>,
}
impl HistoryService {
    fn item(&self, index: usize) -> fdo::Result<Arc<Item>> {
        self.client
            .items()
            .map_err(fdo::Error::Failed)?
            .get(index)
            .cloned()
            .ok_or_else(|| fdo::Error::Failed("Clipboard item no longer exists".into()))
    }
    async fn execute(&self, command: Command) -> fdo::Result<()> {
        self.client
            .request(command)
            .await
            .map_err(fdo::Error::Failed)
    }
}

#[interface(name = "org.Xetibo.OxiPasteDaemon")]
impl HistoryService {
    #[zbus(name = "GetAll")]
    fn get_all(&self) -> fdo::Result<Vec<(Vec<u8>, String)>> {
        Ok(self
            .client
            .items()
            .map_err(fdo::Error::Failed)?
            .iter()
            .map(|item| (item.bytes.clone(), item.mime.clone()))
            .collect())
    }
    #[zbus(name = "GetLatest")]
    fn get_latest(&self) -> fdo::Result<(Vec<u8>, String)> {
        Ok(self
            .client
            .items()
            .map_err(fdo::Error::Failed)?
            .first()
            .map(|item| (item.bytes.clone(), item.mime.clone()))
            .unwrap_or_default())
    }
    #[zbus(name = "Capture")]
    async fn capture(&self, content: Vec<u8>, mime_type: String) -> fdo::Result<()> {
        let item = Arc::new(Item {
            bytes: content,
            mime: mime_type,
        });
        history::validate(&item).map_err(fdo::Error::Failed)?;
        self.execute(Command::Capture(item)).await
    }
    #[zbus(name = "CopyContent")]
    async fn copy_content(&self, content: Vec<u8>, mime_type: String) -> fdo::Result<()> {
        let item = Arc::new(Item {
            bytes: content,
            mime: mime_type,
        });
        history::validate(&item).map_err(fdo::Error::Failed)?;
        self.execute(Command::Copy(item)).await
    }
    #[zbus(name = "DeleteContent")]
    async fn delete_content(&self, content: Vec<u8>, mime_type: String) -> fdo::Result<()> {
        self.execute(Command::Delete(Arc::new(Item {
            bytes: content,
            mime: mime_type,
        })))
        .await
    }
    #[zbus(name = "DeleteAll")]
    async fn delete_all(&self) -> fdo::Result<()> {
        self.execute(Command::Clear).await
    }
    #[zbus(name = "DeleteAtIndex")]
    async fn delete_at_index(&self, index: u32) -> fdo::Result<()> {
        self.execute(Command::Delete(self.item(index as usize)?))
            .await
    }
    #[zbus(name = "Copy")]
    async fn copy(&self) -> fdo::Result<()> {
        let item = self
            .client
            .current()
            .ok_or_else(|| fdo::Error::Failed("No readable clipboard selection".into()))?;
        self.execute(Command::Capture(item)).await
    }
    #[zbus(name = "Paste")]
    async fn paste(&self, index: u32) -> fdo::Result<()> {
        self.execute(Command::Copy(self.item(index as usize)?))
            .await
    }
    #[zbus(name = "PasteAndDelete")]
    async fn paste_and_delete(&self, index: u32) -> fdo::Result<()> {
        let item = self.item(index as usize)?;
        self.execute(Command::Copy(item.clone())).await?;
        self.execute(Command::Delete(item)).await
    }
    #[zbus(name = "ShutDown")]
    fn shut_down(&self) -> fdo::Result<()> {
        Err(fdo::Error::NotSupported(
            "Clipboard backend lifetime follows Oxibar. Disable the OxiPaste plugin to stop it."
                .into(),
        ))
    }
}

enum Mode {
    Owned {
        events: BoxStream<'static, Snapshot>,
        _connection: zbus::Connection,
    },
    External {
        first: bool,
    },
}

async fn mode(limit: usize) -> Result<Mode, String> {
    let connection = zbus::Connection::session()
        .await
        .map_err(|error| error.to_string())?;
    let bus = fdo::DBusProxy::new(&connection)
        .await
        .map_err(|error| error.to_string())?;
    let name: zbus::names::BusName<'_> = NAME
        .try_into()
        .map_err(|error: zbus::names::Error| error.to_string())?;
    while bus
        .name_has_owner(name.clone())
        .await
        .map_err(|error| error.to_string())?
    {
        if bus
            .get_connection_unix_process_id(name.clone())
            .await
            .is_ok_and(|pid| pid == std::process::id())
        {
            // A dropped subscription's connection may still be releasing our name.
            async_io::Timer::after(Duration::from_millis(25)).await;
            continue;
        }
        return Ok(Mode::External { first: true });
    }
    let (client, receiver) = Client::new(history::path()?, limit)?;
    let connection = zbus::connection::Builder::session()
        .map_err(|error| error.to_string())?
        .name(NAME)
        .map_err(|error| error.to_string())?
        .serve_at(
            PATH,
            HistoryService {
                client: client.clone(),
            },
        )
        .map_err(|error| error.to_string())?
        .build()
        .await
        .map_err(|error| error.to_string())?;
    Ok(Mode::Owned {
        _connection: connection,
        events: native::events(client, receiver),
    })
}

pub(crate) fn events(interval: Duration, limit: usize) -> BoxStream<'static, Snapshot> {
    struct State {
        // Stop the worker and connection before another stream can claim ownership.
        mode: Option<Mode>,
        owner: Option<Owner>,
        first: bool,
    }
    iced::futures::stream::unfold(
        State {
            owner: None,
            mode: None,
            first: true,
        },
        move |mut state| async move {
            if state.owner.is_none() {
                while ACTIVE.swap(true, Ordering::AcqRel) {
                    async_io::Timer::after(Duration::from_millis(50)).await;
                }
                state.owner = Some(Owner);
            }
            if !state.first && state.mode.is_none() {
                async_io::Timer::after(interval).await;
            }
            state.first = false;
            if state.mode.is_none() {
                let started = iced::futures::future::select(
                    Box::pin(mode(limit)),
                    Box::pin(async_io::Timer::after(Duration::from_secs(8))),
                )
                .await;
                let result = match started {
                    iced::futures::future::Either::Left((result, _)) => result,
                    iced::futures::future::Either::Right(_) => {
                        Err("Clipboard backend startup timed out".into())
                    }
                };
                match result {
                    Ok(mode) => state.mode = Some(mode),
                    Err(error) => {
                        return Some((
                            Snapshot {
                                items: None,
                                error: Some(format!("Cannot start clipboard backend: {error}")),
                            },
                            state,
                        ));
                    }
                }
            }
            let snapshot = match state.mode.as_mut().expect("clipboard backend mode") {
                Mode::Owned { events, .. } => events.next().await?,
                Mode::External { first } => {
                    if !*first {
                        async_io::Timer::after(interval).await;
                    }
                    *first = false;
                    let result = backend::read().await;
                    match result {
                        Ok(items) => Snapshot {
                            items: Some(items),
                            error: None,
                        },
                        Err(error) => {
                            state.mode = None;
                            Snapshot {
                                items: None,
                                error: Some(error),
                            }
                        }
                    }
                }
            };
            Some((snapshot, state))
        },
    )
    .boxed()
}
