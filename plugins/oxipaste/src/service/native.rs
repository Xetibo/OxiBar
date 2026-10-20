use iced::futures::{
    StreamExt,
    channel::{mpsc, oneshot},
    stream::BoxStream,
};
use std::{
    collections::{BTreeMap, VecDeque},
    fs::File,
    io::{Read, Write},
    os::{fd::AsFd, unix::net::UnixStream},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle,
    protocol::{wl_callback, wl_registry, wl_seat},
};
use wayland_protocols::ext::data_control::v1::client::{
    ext_data_control_device_v1 as ed, ext_data_control_manager_v1 as em,
    ext_data_control_offer_v1 as eo, ext_data_control_source_v1 as es,
};
use wayland_protocols_wlr::data_control::v1::client::{
    zwlr_data_control_device_v1 as wd, zwlr_data_control_manager_v1 as wm,
    zwlr_data_control_offer_v1 as wo, zwlr_data_control_source_v1 as ws,
};

use super::history::{History, validate};
use crate::backend::{Item, MAX_HISTORY_BYTES, MAX_ITEM_BYTES};

const OWN_SELECTION: &str = "application/x-oxibar-clipboard-owner";
const IO_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone)]
pub(crate) struct Snapshot {
    pub items: Option<Vec<Arc<Item>>>,
    pub error: Option<String>,
}

pub(super) enum Command {
    Capture(Arc<Item>),
    Copy(Arc<Item>),
    Delete(Arc<Item>),
    Clear,
}
struct Request {
    command: Command,
    queued: Instant,
    reply: oneshot::Sender<Result<(), String>>,
}

pub(super) struct Client {
    path: PathBuf,
    limit: usize,
    stopped: AtomicBool,
    ready: AtomicBool,
    history: Mutex<Option<History>>,
    current: Mutex<Option<Arc<Item>>>,
    error: Mutex<Option<String>>,
    queue: Mutex<VecDeque<Request>>,
    wake: Mutex<UnixStream>,
    reader: UnixStream,
    notifier: Mutex<mpsc::Sender<()>>,
    #[cfg(test)]
    socket: Option<PathBuf>,
}

impl Client {
    pub fn new(path: PathBuf, limit: usize) -> Result<(Arc<Self>, mpsc::Receiver<()>), String> {
        let (reader, wake) = UnixStream::pair().map_err(|error| error.to_string())?;
        reader
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        wake.set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        let (sender, receiver) = mpsc::channel(1);
        Ok((
            Arc::new(Self {
                path,
                limit,
                stopped: AtomicBool::new(false),
                ready: AtomicBool::new(false),
                history: Mutex::new(None),
                current: Mutex::new(None),
                error: Mutex::new(Some("Starting clipboard backend…".into())),
                queue: Mutex::new(VecDeque::new()),
                wake: Mutex::new(wake),
                reader,
                notifier: Mutex::new(sender),
                #[cfg(test)]
                socket: None,
            }),
            receiver,
        ))
    }

    pub fn items(&self) -> Result<Vec<Arc<Item>>, String> {
        self.history
            .lock()
            .unwrap()
            .as_ref()
            .map(|history| history.items.clone())
            .ok_or_else(|| {
                self.error
                    .lock()
                    .unwrap()
                    .clone()
                    .unwrap_or_else(|| "Clipboard history is not ready".into())
            })
    }

    pub fn current(&self) -> Option<Arc<Item>> {
        self.ready
            .load(Ordering::Acquire)
            .then(|| self.current.lock().unwrap().clone())
            .flatten()
    }

    pub fn request(
        &self,
        command: Command,
    ) -> iced::futures::future::BoxFuture<'static, Result<(), String>> {
        let (reply, receiver) = oneshot::channel();
        let mut queue = self.queue.lock().unwrap();
        if self.stopped.load(Ordering::Acquire) {
            let _ = reply.send(Err("Clipboard backend stopped".into()));
        } else if queue.len() >= 64
            || queue
                .iter()
                .map(|request| command_bytes(&request.command))
                .sum::<usize>()
                + command_bytes(&command)
                > MAX_HISTORY_BYTES
        {
            let _ = reply.send(Err("Clipboard command queue is full".into()));
        } else {
            queue.push_back(Request {
                command,
                queued: Instant::now(),
                reply,
            });
            self.wake();
        }
        Box::pin(async move {
            receiver.await.unwrap_or_else(|_| {
                Err("Clipboard backend stopped before completing the request".into())
            })
        })
    }

    fn wake(&self) {
        let _ = self.wake.lock().unwrap().write(&[1]);
    }

    fn publish(&self, error: Option<String>) {
        *self.error.lock().unwrap() = error;
        let _ = self.notifier.lock().unwrap().try_send(());
    }

    fn snapshot(&self) -> Snapshot {
        let items = self
            .history
            .lock()
            .unwrap()
            .as_ref()
            .map(|history| history.items.clone());
        Snapshot {
            items,
            error: self.error.lock().unwrap().clone(),
        }
    }

    fn capture(&self, item: Arc<Item>) {
        let result = self
            .history
            .lock()
            .unwrap()
            .as_mut()
            .ok_or_else(|| "Clipboard history is unavailable".to_owned())
            .and_then(|history| history.capture(item));
        match result {
            Ok(_) => self.publish(None),
            Err(error) => self.publish(Some(error)),
        }
    }

    fn edit(&self, command: Command) -> Result<(), String> {
        let result = {
            let mut history = self.history.lock().unwrap();
            let history = history.as_mut().ok_or("Clipboard history is unavailable")?;
            match command {
                Command::Capture(item) => history.capture(item).map(|_| ()),
                Command::Delete(item) => history.delete(&item),
                Command::Clear => history.clear(),
                Command::Copy(_) => return Err("Native clipboard ownership is unavailable".into()),
            }
        };
        let error = if self.ready.load(Ordering::Acquire) {
            result.as_ref().err().cloned()
        } else {
            self.error.lock().unwrap().clone()
        };
        self.publish(error);
        result
    }

    fn reject(&self) {
        while let Some(request) = self.queue.lock().unwrap().pop_front() {
            let _ = request.reply.send(Err("Clipboard backend stopped".into()));
        }
    }
}

struct Worker {
    client: Arc<Client>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.client.stopped.store(true, Ordering::Release);
        self.client.wake();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        self.client.ready.store(false, Ordering::Release);
        self.client.reject();
    }
}

pub(super) fn events(
    client: Arc<Client>,
    receiver: mpsc::Receiver<()>,
) -> BoxStream<'static, Snapshot> {
    let owner = client.clone();
    let thread = std::thread::Builder::new()
        .name("oxibar-oxipaste".into())
        .spawn(move || {
            let mut reader = owner
                .reader
                .try_clone()
                .expect("owned clipboard wake socket");
            while !owner.stopped.load(Ordering::Acquire) {
                let loaded = if owner.history.lock().unwrap().is_none() {
                    match History::load(owner.path.clone(), owner.limit) {
                        Ok(history) => {
                            *owner.history.lock().unwrap() = Some(history);
                            true
                        }
                        Err(error) => {
                            owner.publish(Some(error));
                            false
                        }
                    }
                } else {
                    true
                };
                if loaded {
                    offline_commands(&owner);
                    if let Err(error) = session(owner.clone(), &mut reader) {
                        owner.ready.store(false, Ordering::Release);
                        owner.publish(Some(error));
                    }
                }
                if !owner.stopped.load(Ordering::Acquire) {
                    offline_commands(&owner);
                    let mut fds = [rustix::event::PollFd::new(
                        &reader,
                        rustix::event::PollFlags::IN,
                    )];
                    let _ = rustix::event::poll(
                        &mut fds,
                        Some(&rustix::event::Timespec {
                            tv_sec: 2,
                            tv_nsec: 0,
                        }),
                    );
                    drain(&mut reader);
                }
            }
        })
        .ok();
    if thread.is_none() {
        client.publish(Some("Cannot start clipboard backend worker".into()));
    }
    let worker = Worker { client, thread };
    iced::futures::stream::unfold((receiver, worker), |(mut receiver, worker)| async move {
        receiver.next().await?;
        let snapshot = worker.client.snapshot();
        Some((snapshot, (receiver, worker)))
    })
    .boxed()
}

fn offline_commands(client: &Client) {
    loop {
        let request = client.queue.lock().unwrap().pop_front();
        let Some(request) = request else { break };
        let _ = request.reply.send(client.edit(request.command));
    }
}

fn command_bytes(command: &Command) -> usize {
    match command {
        Command::Capture(item) | Command::Copy(item) | Command::Delete(item) => {
            item.bytes.len() + item.mime.len()
        }
        Command::Clear => 0,
    }
}

#[derive(Clone)]
enum Offer {
    Modern(eo::ExtDataControlOfferV1),
    Wlr(wo::ZwlrDataControlOfferV1),
}
impl Offer {
    fn id(&self) -> u32 {
        match self {
            Self::Modern(p) => p.id().protocol_id(),
            Self::Wlr(p) => p.id().protocol_id(),
        }
    }
    fn receive(&self, mime: &str, fd: std::os::fd::BorrowedFd<'_>) {
        match self {
            Self::Modern(p) => p.receive(mime.into(), fd),
            Self::Wlr(p) => p.receive(mime.into(), fd),
        }
    }
    fn destroy(&self) {
        match self {
            Self::Modern(p) => p.destroy(),
            Self::Wlr(p) => p.destroy(),
        }
    }
}
enum Device {
    Modern(ed::ExtDataControlDeviceV1),
    Wlr(wd::ZwlrDataControlDeviceV1),
}
enum Source {
    Modern(es::ExtDataControlSourceV1),
    Wlr(ws::ZwlrDataControlSourceV1),
}
impl Source {
    fn bytes(&self) -> usize {
        let item = match self {
            Self::Modern(p) => p.data::<Arc<Item>>(),
            Self::Wlr(p) => p.data::<Arc<Item>>(),
        };
        item.map_or(0, |item| item.bytes.len() + item.mime.len())
    }
    fn id(&self) -> u32 {
        match self {
            Self::Modern(p) => p.id().protocol_id(),
            Self::Wlr(p) => p.id().protocol_id(),
        }
    }
    fn destroy(&self) {
        match self {
            Self::Modern(p) => p.destroy(),
            Self::Wlr(p) => p.destroy(),
        }
    }
}
struct Transfer {
    file: File,
    bytes: Vec<u8>,
    source: Option<Arc<Item>>,
    mime: String,
    offset: usize,
    selection: u64,
    deadline: Instant,
}
struct Acknowledgement {
    reply: oneshot::Sender<Result<(), String>>,
    deadline: Instant,
    item: Arc<Item>,
}
struct State {
    client: Arc<Client>,
    modern: Option<em::ExtDataControlManagerV1>,
    wlr: Option<wm::ZwlrDataControlManagerV1>,
    seat: Option<wl_seat::WlSeat>,
    globals: Vec<u32>,
    device: Option<Device>,
    offers: BTreeMap<u32, (Offer, Vec<String>)>,
    selected: Option<u32>,
    selection: u64,
    sources: BTreeMap<u32, Source>,
    acknowledgements: BTreeMap<u32, Acknowledgement>,
    io: Vec<Transfer>,
    synced: u64,
    finished: bool,
}

fn preferred_mime(types: &[String]) -> Option<&str> {
    if types.iter().any(|mime| mime == OWN_SELECTION) {
        return None;
    }
    [
        "image/png",
        "image/jpeg",
        "image/webp",
        "image/gif",
        "image/bmp",
        "text/plain;charset=utf-8",
        "text/plain;charset=UTF-8",
        "text/plain",
        "UTF8_STRING",
        "STRING",
        "TEXT",
    ]
    .into_iter()
    .find(|mime| types.iter().any(|value| value == mime))
    .or_else(|| {
        types
            .iter()
            .find(|mime| mime.starts_with("text/") || mime.starts_with("image/"))
            .map(String::as_str)
    })
}

impl State {
    fn offer(&mut self, offer: Offer) {
        if self.offers.len() >= 32 {
            offer.destroy();
            return;
        }
        self.offers.insert(offer.id(), (offer, Vec::new()));
    }
    fn mime(&mut self, id: u32, mime: String) {
        if let Some((_, types)) = self.offers.get_mut(&id)
            && types.len() < 64
            && !mime.is_empty()
            && mime.len() <= 256
            && !mime.contains(['\0', '\n', '\r'])
        {
            types.push(mime);
        }
    }
    fn selection(&mut self, selected: Option<u32>) {
        self.selection = self.selection.saturating_add(1);
        self.io.retain(|io| io.source.is_some());
        if let Some(old) = self.selected.take()
            && Some(old) != selected
            && let Some((offer, _)) = self.offers.remove(&old)
        {
            offer.destroy();
        }
        self.selected = selected;
        let Some((offer, types)) = selected.and_then(|id| self.offers.get(&id)) else {
            *self.client.current.lock().unwrap() = None;
            return;
        };
        if !types.iter().any(|mime| mime == OWN_SELECTION) {
            *self.client.current.lock().unwrap() = None;
        }
        let Some(mime) = preferred_mime(types) else {
            return;
        };
        if self.io.len() >= 16 {
            return;
        }
        if let Ok((read, write)) = capture_pipe() {
            offer.receive(mime, write.as_fd());
            self.io.push(Transfer {
                file: File::from(read),
                bytes: Vec::new(),
                source: None,
                mime: mime.into(),
                offset: 0,
                selection: self.selection,
                deadline: Instant::now() + IO_TIMEOUT,
            });
        }
    }
    fn primary(&mut self, selected: Option<u32>) {
        if let Some(id) = selected
            && let Some((offer, _)) = self.offers.remove(&id)
        {
            offer.destroy();
        }
    }
    fn send(&mut self, fd: std::os::fd::OwnedFd, item: &Arc<Item>, mime: &str) {
        if self.io.len() >= 16 || mime == OWN_SELECTION || (mime != item.mime && !item.is_text()) {
            return;
        }
        let Ok(flags) = rustix::fs::fcntl_getfl(&fd) else {
            return;
        };
        if rustix::fs::fcntl_setfl(&fd, flags | rustix::fs::OFlags::NONBLOCK).is_err() {
            return;
        }
        self.io.push(Transfer {
            file: File::from(fd),
            bytes: Vec::new(),
            source: Some(item.clone()),
            mime: mime.into(),
            offset: 0,
            selection: 0,
            deadline: Instant::now() + IO_TIMEOUT,
        });
    }
    fn tick(&mut self) {
        let mut accepted = Vec::new();
        let mut error = None;
        let mut budget =
            MAX_HISTORY_BYTES.saturating_sub(self.io.iter().map(|io| io.bytes.len()).sum());
        self.io.retain_mut(|io| {
            if Instant::now() > io.deadline {
                if io.source.is_none() {
                    error = Some("Clipboard capture timed out".into());
                }
                return false;
            }
            if let Some(source) = &io.source {
                match io.file.write(&source.bytes[io.offset..]) {
                    Ok(0) => false,
                    Ok(count) => {
                        io.offset += count;
                        io.offset < source.bytes.len()
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => true,
                    Err(_) => false,
                }
            } else {
                let mut bytes = [0; 8192];
                loop {
                    match io.file.read(&mut bytes) {
                        Ok(0) => {
                            if io.selection == self.selection && !io.bytes.is_empty() {
                                accepted.push(Arc::new(Item {
                                    bytes: std::mem::take(&mut io.bytes),
                                    mime: io.mime.clone(),
                                }));
                            }
                            return false;
                        }
                        Ok(count) => {
                            if io.bytes.len() + count > MAX_ITEM_BYTES || count > budget {
                                error = Some("Clipboard capture exceeds supported limits".into());
                                return false;
                            }
                            budget -= count;
                            io.bytes.extend_from_slice(&bytes[..count]);
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            return true;
                        }
                        Err(problem) => {
                            error = Some(format!("Clipboard capture failed: {problem}"));
                            return false;
                        }
                    }
                }
            }
        });
        for item in accepted {
            *self.client.current.lock().unwrap() = Some(item.clone());
            self.client.capture(item);
        }
        if let Some(error) = error {
            self.client.publish(Some(error));
        }
        let expired: Vec<_> = self
            .acknowledgements
            .iter()
            .filter(|(_, acknowledgement)| Instant::now() > acknowledgement.deadline)
            .map(|(id, _)| *id)
            .collect();
        for id in expired {
            if let Some(acknowledgement) = self.acknowledgements.remove(&id) {
                let _ = acknowledgement
                    .reply
                    .send(Err("Clipboard ownership confirmation timed out".into()));
            }
        }
    }
    fn commands(&mut self, connection: &Connection, qh: &QueueHandle<Self>) {
        loop {
            let request = self.client.queue.lock().unwrap().pop_front();
            let Some(request) = request else { break };
            if request.queued.elapsed() > IO_TIMEOUT {
                let _ = request.reply.send(Err("Clipboard command expired".into()));
                continue;
            }
            let Command::Copy(item) = request.command else {
                let _ = request.reply.send(self.client.edit(request.command));
                continue;
            };
            if self.sources.len() >= 64
                || self.sources.values().map(Source::bytes).sum::<usize>()
                    + item.bytes.len()
                    + item.mime.len()
                    > MAX_HISTORY_BYTES
            {
                let _ = request
                    .reply
                    .send(Err("Clipboard ownership queue is full".into()));
                continue;
            }
            if let Err(error) = validate(&item) {
                let _ = request.reply.send(Err(error));
                continue;
            }
            let types = if item.is_text() {
                vec![
                    item.mime.as_str(),
                    "text/plain;charset=utf-8",
                    "text/plain",
                    "UTF8_STRING",
                    "STRING",
                    "TEXT",
                    OWN_SELECTION,
                ]
            } else {
                vec![item.mime.as_str(), OWN_SELECTION]
            };
            let source = match self.device.as_ref() {
                Some(Device::Modern(device)) => {
                    let source = self
                        .modern
                        .as_ref()
                        .unwrap()
                        .create_data_source(qh, item.clone());
                    for mime in &types {
                        source.offer((*mime).into());
                    }
                    device.set_selection(Some(&source));
                    Source::Modern(source)
                }
                Some(Device::Wlr(device)) => {
                    let source = self
                        .wlr
                        .as_ref()
                        .unwrap()
                        .create_data_source(qh, item.clone());
                    for mime in &types {
                        source.offer((*mime).into());
                    }
                    device.set_selection(Some(&source));
                    Source::Wlr(source)
                }
                None => {
                    let _ = request
                        .reply
                        .send(Err("Native clipboard ownership is unavailable".into()));
                    continue;
                }
            };
            self.sources.insert(source.id(), source);
            let callback = connection.display().sync(qh, ());
            self.acknowledgements.insert(
                callback.id().protocol_id(),
                Acknowledgement {
                    reply: request.reply,
                    deadline: Instant::now() + IO_TIMEOUT,
                    item,
                },
            );
        }
    }
}

fn drain(reader: &mut UnixStream) {
    let mut bytes = [0; 128];
    while reader.read(&mut bytes).is_ok_and(|count| count > 0) {}
}
fn poll(
    connection: &Connection,
    queue: &mut EventQueue<State>,
    state: &mut State,
    reader: &mut UnixStream,
    timeout: Option<Duration>,
) -> Result<(), String> {
    if state.client.stopped.load(Ordering::Acquire) {
        return Err("Clipboard backend stopped".into());
    }
    queue
        .dispatch_pending(state)
        .map_err(|error| error.to_string())?;
    if state.finished {
        return Err("Clipboard data-control service disconnected".into());
    }
    connection.flush().map_err(|error| error.to_string())?;
    let Some(guard) = queue.prepare_read() else {
        state.tick();
        return Ok(());
    };
    let backend = connection.backend();
    let fd = backend.poll_fd();
    let flags = {
        let mut fds = vec![
            rustix::event::PollFd::new(&fd, rustix::event::PollFlags::IN),
            rustix::event::PollFd::new(&*reader, rustix::event::PollFlags::IN),
        ];
        for io in &state.io {
            fds.push(rustix::event::PollFd::new(
                &io.file,
                if io.source.is_some() {
                    rustix::event::PollFlags::OUT
                } else {
                    rustix::event::PollFlags::IN
                },
            ));
        }
        let duration = timeout.map(|duration| rustix::event::Timespec {
            tv_sec: duration.as_secs() as i64,
            tv_nsec: duration.subsec_nanos() as i64,
        });
        rustix::event::poll(&mut fds, duration.as_ref()).map_err(|error| error.to_string())?;
        fds[0].revents()
    };
    if flags.intersects(
        rustix::event::PollFlags::IN
            | rustix::event::PollFlags::HUP
            | rustix::event::PollFlags::ERR,
    ) {
        guard.read().map_err(|error| error.to_string())?;
    } else {
        drop(guard);
    }
    drain(reader);
    queue
        .dispatch_pending(state)
        .map_err(|error| error.to_string())?;
    state.tick();
    if state.finished {
        Err("Clipboard data-control service disconnected".into())
    } else {
        Ok(())
    }
}
fn session(client: Arc<Client>, reader: &mut UnixStream) -> Result<(), String> {
    let connect = || {
        #[cfg(test)]
        if let Some(path) = &client.socket {
            return UnixStream::connect(path)
                .map_err(|_| wayland_client::ConnectError::NoCompositor)
                .and_then(Connection::from_socket);
        }
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .ok_or(wayland_client::ConnectError::NoCompositor)?;
        let display = oxibar_plugin_api::launch::wayland_display(
            &runtime,
            std::env::var_os("WAYLAND_DISPLAY").as_deref(),
        )
        .ok_or(wayland_client::ConnectError::NoCompositor)?;
        // Independent clients need a fresh socket, never the host's inherited WAYLAND_SOCKET fd.
        UnixStream::connect(runtime.join(display))
            .map_err(|_| wayland_client::ConnectError::NoCompositor)
            .and_then(Connection::from_socket)
    };
    let connection = connect()
        .map_err(|error| format!("Cannot connect clipboard backend to Wayland: {error}"))?;
    let mut queue = connection.new_event_queue();
    let qh = queue.handle();
    let mut state = State {
        client: client.clone(),
        modern: None,
        wlr: None,
        seat: None,
        globals: Vec::new(),
        device: None,
        offers: BTreeMap::new(),
        selected: None,
        selection: 0,
        sources: BTreeMap::new(),
        acknowledgements: BTreeMap::new(),
        io: Vec::new(),
        synced: 0,
        finished: false,
    };
    connection.display().get_registry(&qh, ());
    let before = state.synced;
    connection.display().sync(&qh, ());
    let deadline = Instant::now() + IO_TIMEOUT;
    while state.synced == before {
        if Instant::now() > deadline {
            return Err("Clipboard initialization timed out".into());
        }
        poll(
            &connection,
            &mut queue,
            &mut state,
            reader,
            Some(Duration::from_millis(100)),
        )?;
    }
    let seat = state.seat.as_ref().ok_or("Clipboard seat is unavailable")?;
    state.device = if let Some(manager) = &state.modern {
        Some(Device::Modern(manager.get_data_device(seat, &qh, ())))
    } else if let Some(manager) = &state.wlr {
        Some(Device::Wlr(manager.get_data_device(seat, &qh, ())))
    } else {
        return Err(
            "Compositor exposes no ext-data-control or wlr-data-control clipboard protocol".into(),
        );
    };
    client.ready.store(true, Ordering::Release);
    client.publish(None);
    while !client.stopped.load(Ordering::Acquire) {
        state.commands(&connection, &qh);
        let timeout = state
            .io
            .iter()
            .map(|io| io.deadline)
            .chain(
                state
                    .acknowledgements
                    .values()
                    .map(|acknowledgement| acknowledgement.deadline),
            )
            .min()
            .map(|deadline| deadline.saturating_duration_since(Instant::now()));
        poll(&connection, &mut queue, &mut state, reader, timeout)?;
    }
    Ok(())
}

fn capture_pipe() -> Result<(std::os::fd::OwnedFd, std::os::fd::OwnedFd), rustix::io::Errno> {
    let (read, write) = rustix::pipe::pipe_with(rustix::pipe::PipeFlags::CLOEXEC)?;
    // Clipboard producers expect blocking writes, especially for images larger than pipe capacity.
    rustix::fs::fcntl_setfl(&read, rustix::fs::OFlags::NONBLOCK)?;
    Ok((read, write))
}

impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => {
                match interface.as_str() {
                    "ext_data_control_manager_v1" if state.modern.is_none() => {
                        state.modern = Some(registry.bind(name, version.min(1), qh, ()))
                    }
                    "zwlr_data_control_manager_v1" if state.wlr.is_none() => {
                        state.wlr = Some(registry.bind(name, version.min(2), qh, ()))
                    }
                    "wl_seat" if state.seat.is_none() => {
                        state.seat = Some(registry.bind(name, version.min(2), qh, ()))
                    }
                    _ => return,
                }
                state.globals.push(name);
            }
            wl_registry::Event::GlobalRemove { name } if state.globals.contains(&name) => {
                state.finished = true
            }
            _ => (),
        }
    }
}
impl Dispatch<wl_seat::WlSeat, ()> for State {
    fn event(
        _: &mut Self,
        _: &wl_seat::WlSeat,
        _: wl_seat::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
impl Dispatch<wl_callback::WlCallback, ()> for State {
    fn event(
        state: &mut Self,
        proxy: &wl_callback::WlCallback,
        _: wl_callback::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        state.synced += 1;
        if let Some(acknowledgement) = state.acknowledgements.remove(&proxy.id().protocol_id()) {
            *state.client.current.lock().unwrap() = Some(acknowledgement.item);
            let _ = acknowledgement.reply.send(Ok(()));
        }
    }
}
impl Dispatch<em::ExtDataControlManagerV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &em::ExtDataControlManagerV1,
        _: em::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
impl Dispatch<wm::ZwlrDataControlManagerV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &wm::ZwlrDataControlManagerV1,
        _: wm::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
impl Dispatch<ed::ExtDataControlDeviceV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ed::ExtDataControlDeviceV1,
        event: ed::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ed::Event::DataOffer { id } => state.offer(Offer::Modern(id)),
            ed::Event::Selection { id } => state.selection(id.map(|p| p.id().protocol_id())),
            ed::Event::PrimarySelection { id } => state.primary(id.map(|p| p.id().protocol_id())),
            ed::Event::Finished => state.finished = true,
            _ => (),
        }
    }
    wayland_client::event_created_child!(State, ed::ExtDataControlDeviceV1, [0 => (eo::ExtDataControlOfferV1, ())]);
}
impl Dispatch<wd::ZwlrDataControlDeviceV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &wd::ZwlrDataControlDeviceV1,
        event: wd::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wd::Event::DataOffer { id } => state.offer(Offer::Wlr(id)),
            wd::Event::Selection { id } => state.selection(id.map(|p| p.id().protocol_id())),
            wd::Event::PrimarySelection { id } => state.primary(id.map(|p| p.id().protocol_id())),
            wd::Event::Finished => state.finished = true,
            _ => (),
        }
    }
    wayland_client::event_created_child!(State, wd::ZwlrDataControlDeviceV1, [0 => (wo::ZwlrDataControlOfferV1, ())]);
}
impl Dispatch<eo::ExtDataControlOfferV1, ()> for State {
    fn event(
        state: &mut Self,
        proxy: &eo::ExtDataControlOfferV1,
        event: eo::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let eo::Event::Offer { mime_type } = event {
            state.mime(proxy.id().protocol_id(), mime_type);
        }
    }
}
impl Dispatch<wo::ZwlrDataControlOfferV1, ()> for State {
    fn event(
        state: &mut Self,
        proxy: &wo::ZwlrDataControlOfferV1,
        event: wo::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wo::Event::Offer { mime_type } = event {
            state.mime(proxy.id().protocol_id(), mime_type);
        }
    }
}
impl Dispatch<es::ExtDataControlSourceV1, Arc<Item>> for State {
    fn event(
        state: &mut Self,
        proxy: &es::ExtDataControlSourceV1,
        event: es::Event,
        item: &Arc<Item>,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            es::Event::Send { mime_type, fd } => state.send(fd, item, &mime_type),
            es::Event::Cancelled => {
                if let Some(source) = state.sources.remove(&proxy.id().protocol_id()) {
                    source.destroy();
                }
            }
            _ => (),
        }
    }
}
impl Dispatch<ws::ZwlrDataControlSourceV1, Arc<Item>> for State {
    fn event(
        state: &mut Self,
        proxy: &ws::ZwlrDataControlSourceV1,
        event: ws::Event,
        item: &Arc<Item>,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ws::Event::Send { mime_type, fd } => state.send(fd, item, &mime_type),
            ws::Event::Cancelled => {
                if let Some(source) = state.sources.remove(&proxy.id().protocol_id()) {
                    source.destroy();
                }
            }
            _ => (),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_image_transfer_keeps_producer_blocking_and_captures_every_byte() {
        let (read, write) = capture_pipe().unwrap();
        assert!(
            rustix::fs::fcntl_getfl(&read)
                .unwrap()
                .contains(rustix::fs::OFlags::NONBLOCK)
        );
        assert!(
            !rustix::fs::fcntl_getfl(&write)
                .unwrap()
                .contains(rustix::fs::OFlags::NONBLOCK)
        );
        let payload: Vec<u8> = (0..512 * 1024).map(|index| (index % 251) as u8).collect();
        let expected = payload.clone();
        let producer = std::thread::spawn(move || {
            std::fs::File::from(write).write_all(&payload).unwrap();
        });
        let mut read = std::fs::File::from(read);
        let mut captured = Vec::new();
        let mut buffer = [0; 8192];
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            assert!(Instant::now() < deadline, "clipboard transfer stalled");
            match read.read(&mut buffer) {
                Ok(0) => break,
                Ok(length) => captured.extend_from_slice(&buffer[..length]),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(1))
                }
                Err(error) => panic!("clipboard transfer failed: {error}"),
            }
        }
        producer.join().unwrap();
        assert_eq!(captured, expected);
    }

    #[test]
    fn preferred_capture_preserves_mime_and_skips_our_acknowledged_writes() {
        let types = vec!["text/plain".into(), "image/png".into()];
        assert_eq!(preferred_mime(&types), Some("image/png"));
        assert_eq!(preferred_mime(&["text/html".into()]), Some("text/html"));
        assert_eq!(
            preferred_mime(&["text/plain".into(), OWN_SELECTION.into()]),
            None
        );
        assert_eq!(preferred_mime(&["application/octet-stream".into()]), None);
    }

    #[test]
    fn owned_worker_reports_offline_errors_keeps_history_and_joins_on_stream_drop() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("items");
        let (mut client, receiver) = Client::new(path.clone(), 100).unwrap();
        Arc::get_mut(&mut client).unwrap().socket =
            Some(directory.path().join("missing-wayland-socket"));
        let item = Arc::new(Item {
            bytes: b"retained\0\xff".to_vec(),
            mime: "image/png".into(),
        });
        let capture = client.request(Command::Capture(item.clone()));
        let copy = client.request(Command::Copy(item.clone()));
        let mut stream = events(client.clone(), receiver);
        iced::futures::executor::block_on(async {
            capture.await.unwrap();
            assert!(copy.await.unwrap_err().contains("unavailable"));
            assert_eq!(client.items().unwrap(), vec![item.clone()]);
            loop {
                let snapshot = stream.next().await.unwrap();
                if snapshot
                    .error
                    .as_ref()
                    .is_some_and(|error| error.contains("Wayland"))
                {
                    assert_eq!(snapshot.items, Some(vec![item.clone()]));
                    break;
                }
            }
        });
        let started = Instant::now();
        drop(stream);
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(client.stopped.load(Ordering::Acquire));
        assert!(iced::futures::executor::block_on(client.request(Command::Clear)).is_err());
        assert_eq!(History::load(path, 100).unwrap().items, vec![item]);
    }
}
