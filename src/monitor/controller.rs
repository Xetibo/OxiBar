use super::{
    OutputSnapshot,
    configuration::{DisplayConfig, complete},
};
use iced::futures::{
    StreamExt,
    channel::oneshot,
    stream::{self, BoxStream},
};
use std::{
    hash::{Hash, Hasher},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

const PREVIEW_TIME: Duration = Duration::from_secs(20);
pub(super) trait Backend: Send {
    fn inventory(&mut self) -> Result<OutputSnapshot, String>;
    fn apply(&mut self, config: &[DisplayConfig]) -> Result<(), String>;
    fn label(&self) -> &str;
}
#[derive(Debug, Clone)]
pub struct Preview {
    pub id: u64,
    pub seconds: u64,
    pub inventory: OutputSnapshot,
}
#[derive(Debug, Clone)]
pub struct Event {
    pub id: u64,
    pub result: Result<OutputSnapshot, String>,
}
type Save = Box<dyn FnOnce() -> Result<Arc<crate::config::ConfigSnapshot>, String> + Send>;
enum Request {
    Discover(oneshot::Sender<Result<OutputSnapshot, String>>),
    Preview(Vec<DisplayConfig>, oneshot::Sender<Result<Preview, String>>),
    Revert(u64, oneshot::Sender<Result<OutputSnapshot, String>>),
    Keep(
        u64,
        Save,
        oneshot::Sender<Result<Arc<crate::config::ConfigSnapshot>, String>>,
    ),
    Saved(
        Vec<DisplayConfig>,
        oneshot::Sender<Result<OutputSnapshot, String>>,
    ),
}
struct Lease {
    id: u64,
    baseline: Vec<DisplayConfig>,
    deadline: Instant,
}
struct Owner {
    sender: mpsc::SyncSender<Request>,
    stopped: Arc<AtomicBool>,
    worker: Mutex<Option<thread::JoinHandle<()>>>,
    events: Mutex<Option<iced::futures::channel::mpsc::Receiver<Event>>>,
}
impl Drop for Owner {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        if let Some(worker) = self.worker.get_mut().ok().and_then(Option::take) {
            let _ = worker.join();
        }
    }
}
#[derive(Clone)]
pub struct Controller(Arc<Owner>);
impl std::fmt::Debug for Controller {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DisplayController").finish_non_exhaustive()
    }
}
impl PartialEq for Controller {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for Controller {}
impl Hash for Controller {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.0).hash(state);
    }
}

impl Controller {
    pub fn new() -> Result<Self, String> {
        Self::spawn(
            Box::new(|| {
                if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
                    Ok(Box::new(super::hyprland::Control::new()?) as Box<dyn Backend>)
                } else {
                    Ok(Box::new(super::management::Control::new()?) as Box<dyn Backend>)
                }
            }),
            PREVIEW_TIME,
        )
    }
    pub(super) fn spawn(
        factory: Box<dyn Fn() -> Result<Box<dyn Backend>, String> + Send>,
        timeout: Duration,
    ) -> Result<Self, String> {
        let (sender, receiver) = mpsc::sync_channel(8);
        let (events_tx, events) = iced::futures::channel::mpsc::channel(4);
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = stopped.clone();
        let worker = thread::Builder::new()
            .name("oxibar-displays".into())
            .spawn(move || run(factory, receiver, stop, events_tx, timeout))
            .map_err(|error| error.to_string())?;
        Ok(Self(Arc::new(Owner {
            sender,
            stopped,
            worker: Mutex::new(Some(worker)),
            events: Mutex::new(Some(events)),
        })))
    }
    pub fn events(&self) -> BoxStream<'static, Event> {
        self.0
            .events
            .lock()
            .ok()
            .and_then(|mut events| events.take())
            .map_or_else(|| stream::empty().boxed(), StreamExt::boxed)
    }
    pub fn discover(
        &self,
    ) -> impl Future<Output = Result<OutputSnapshot, String>> + Send + 'static {
        let (tx, rx) = oneshot::channel();
        let result = self
            .0
            .sender
            .try_send(Request::Discover(tx))
            .map_err(|_| "Display controller is busy or unavailable".to_owned());
        async move {
            result?;
            rx.await
                .map_err(|_| "Display controller stopped".to_owned())?
        }
    }
    pub fn preview(
        &self,
        config: Vec<DisplayConfig>,
    ) -> impl Future<Output = Result<Preview, String>> + Send + 'static {
        let (tx, rx) = oneshot::channel();
        let result = self
            .0
            .sender
            .try_send(Request::Preview(config, tx))
            .map_err(|_| "Display controller is busy or unavailable".to_owned());
        async move {
            result?;
            rx.await
                .map_err(|_| "Display controller stopped".to_owned())?
        }
    }
    pub fn revert(
        &self,
        id: u64,
    ) -> impl Future<Output = Result<OutputSnapshot, String>> + Send + 'static {
        let (tx, rx) = oneshot::channel();
        let result = self
            .0
            .sender
            .try_send(Request::Revert(id, tx))
            .map_err(|_| "Display controller is busy or unavailable".to_owned());
        async move {
            result?;
            rx.await
                .map_err(|_| "Display controller stopped".to_owned())?
        }
    }
    pub fn keep(
        &self,
        id: u64,
        save: Save,
    ) -> impl Future<Output = Result<Arc<crate::config::ConfigSnapshot>, String>> + Send + 'static
    {
        let (tx, rx) = oneshot::channel();
        let result = self
            .0
            .sender
            .try_send(Request::Keep(id, save, tx))
            .map_err(|_| "Display controller is busy or unavailable".to_owned());
        async move {
            result?;
            rx.await
                .map_err(|_| "Display controller stopped".to_owned())?
        }
    }
    pub fn saved(
        &self,
        config: Vec<DisplayConfig>,
    ) -> impl Future<Output = Result<OutputSnapshot, String>> + Send + 'static {
        let (tx, rx) = oneshot::channel();
        let result = self
            .0
            .sender
            .try_send(Request::Saved(config, tx))
            .map_err(|_| "Display controller is busy or unavailable".to_owned());
        async move {
            result?;
            rx.await
                .map_err(|_| "Display controller stopped".to_owned())?
        }
    }
}

fn restore(
    backend: &mut dyn Backend,
    baseline: &[DisplayConfig],
) -> Result<OutputSnapshot, String> {
    let inventory = backend.inventory()?;
    let connected: Vec<_> = baseline
        .iter()
        .filter(|old| {
            inventory
                .outputs
                .iter()
                .any(|current| current.name == old.name)
        })
        .cloned()
        .collect();
    let mut desired: Vec<_> = inventory
        .outputs
        .iter()
        .map(|current| {
            connected
                .iter()
                .find(|old| old.name == current.name)
                .cloned()
                .unwrap_or_else(|| current.into())
        })
        .collect();
    if !desired.iter().any(|output| output.enabled)
        && let Some(output) = desired.first_mut()
    {
        output.enabled = true;
    }
    if !desired.is_empty() {
        backend.apply(&desired)?;
    }
    backend.inventory()
}
fn run(
    factory: Box<dyn Fn() -> Result<Box<dyn Backend>, String> + Send>,
    receiver: mpsc::Receiver<Request>,
    stopped: Arc<AtomicBool>,
    mut events: iced::futures::channel::mpsc::Sender<Event>,
    timeout: Duration,
) {
    let mut backend = factory();
    let mut lease: Option<Lease> = None;
    let mut generation = 0_u64;
    while !stopped.load(Ordering::Acquire) {
        if lease
            .as_ref()
            .is_some_and(|lease| Instant::now() >= lease.deadline)
        {
            let old = lease.take().expect("expired lease");
            let result = backend
                .as_mut()
                .map_err(|error| error.clone())
                .and_then(|backend| restore(backend.as_mut(), &old.baseline));
            let _ = events.try_send(Event { id: old.id, result });
        }
        let wait = lease
            .as_ref()
            .map(|lease| {
                lease
                    .deadline
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(100))
            })
            .unwrap_or(Duration::from_millis(100));
        let request = match receiver.recv_timeout(wait) {
            Ok(request) => request,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(_) => break,
        };
        if backend.is_err() {
            backend = factory();
        }
        let backend = match &mut backend {
            Ok(backend) => backend.as_mut(),
            Err(error) => {
                match request {
                    Request::Discover(tx) | Request::Revert(_, tx) | Request::Saved(_, tx) => {
                        let _ = tx.send(Err(error.clone()));
                    }
                    Request::Preview(_, tx) => {
                        let _ = tx.send(Err(error.clone()));
                    }
                    Request::Keep(_, _, tx) => {
                        let _ = tx.send(Err(error.clone()));
                    }
                }
                continue;
            }
        };
        match request {
            Request::Discover(tx) => {
                let result = backend.inventory().map(|mut inventory| {
                    inventory.backend = backend.label().into();
                    inventory
                });
                let _ = tx.send(result);
            }
            Request::Preview(config, tx) => {
                let result = (|| {
                    if lease.is_some() {
                        return Err("Keep or revert the current preview first".into());
                    }
                    let inventory = backend.inventory()?;
                    let desired = complete(&config, &inventory)?;
                    let baseline = inventory
                        .outputs
                        .iter()
                        .map(DisplayConfig::from)
                        .collect::<Vec<_>>();
                    if let Err(error) = backend.apply(&desired) {
                        let rollback = restore(backend, &baseline);
                        return Err(format!(
                            "{error}{}",
                            rollback
                                .err()
                                .map(|error| format!("; rollback: {error}"))
                                .unwrap_or_default()
                        ));
                    }
                    generation = generation.wrapping_add(1);
                    lease = Some(Lease {
                        id: generation,
                        baseline,
                        deadline: Instant::now() + timeout,
                    });
                    match backend.inventory() {
                        Ok(inventory) => Ok(Preview {
                            id: generation,
                            seconds: timeout.as_secs(),
                            inventory,
                        }),
                        Err(error) => {
                            let old = lease.take().expect("new lease");
                            let _ = restore(backend, &old.baseline);
                            Err(error)
                        }
                    }
                })();
                let _ = tx.send(result);
            }
            Request::Revert(id, tx) => {
                let result = if lease.as_ref().is_some_and(|lease| lease.id == id) {
                    let old = lease.take().expect("matching lease");
                    restore(backend, &old.baseline)
                } else {
                    Err("This display preview has already ended".into())
                };
                let _ = tx.send(result);
            }
            Request::Keep(id, save, tx) => {
                let result = if lease
                    .as_ref()
                    .is_some_and(|lease| lease.id == id && Instant::now() < lease.deadline)
                {
                    match save() {
                        Ok(snapshot) => {
                            lease = None;
                            Ok(snapshot)
                        }
                        Err(error) => {
                            let old = lease.take().expect("matching lease");
                            let rollback = restore(backend, &old.baseline);
                            Err(format!(
                                "{error}{}",
                                rollback
                                    .err()
                                    .map(|error| format!("; rollback: {error}"))
                                    .unwrap_or_default()
                            ))
                        }
                    }
                } else {
                    Err("This display preview expired before it could be saved".into())
                };
                let _ = tx.send(result);
            }
            Request::Saved(config, tx) => {
                let result = (|| {
                    if lease.is_some() {
                        return Err("A display preview is active".into());
                    }
                    let inventory = backend.inventory()?;
                    let present: Vec<_> = config
                        .into_iter()
                        .filter(|desired| {
                            inventory
                                .outputs
                                .iter()
                                .any(|output| output.name == desired.name)
                        })
                        .collect();
                    if present.is_empty() {
                        return Ok(inventory);
                    }
                    let desired = complete(&present, &inventory)?;
                    let baseline: Vec<_> =
                        inventory.outputs.iter().map(DisplayConfig::from).collect();
                    if let Err(error) = backend.apply(&desired) {
                        let _ = restore(backend, &baseline);
                        return Err(error);
                    }
                    backend.inventory()
                })();
                let _ = tx.send(result);
            }
        }
    }
    if let Some(lease) = lease
        && let Ok(backend) = &mut backend
    {
        let _ = restore(backend.as_mut(), &lease.baseline);
    }
}
