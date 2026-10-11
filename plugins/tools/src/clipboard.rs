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
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
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

const TEXT_LIMIT: usize = 16_384;
const HISTORY_BYTES: usize = 524_288;
const MIME: &str = "text/plain;charset=utf-8";
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    pub id: u64,
    pub text: String,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Snapshot {
    pub available: bool,
    pub enabled: bool,
    pub entries: Arc<Vec<Entry>>,
    pub error: Option<String>,
}
struct CopyRequest {
    text: String,
    queued: Instant,
    reply: oneshot::Sender<Result<(), String>>,
}
pub(crate) struct Client {
    socket: Option<std::path::PathBuf>,
    legacy_only: bool,
    stopped: AtomicBool,
    started: AtomicBool,
    ready: AtomicBool,
    enabled: AtomicBool,
    limit: AtomicUsize,
    next_id: std::sync::atomic::AtomicU64,
    capture_epoch: std::sync::atomic::AtomicU64,
    queue: Mutex<VecDeque<CopyRequest>>,
    wake: Mutex<UnixStream>,
    reader: Mutex<UnixStream>,
    history: Mutex<Vec<Entry>>,
    mailbox: Mutex<Snapshot>,
    notifier: Mutex<Option<mpsc::Sender<()>>>,
}
impl std::fmt::Debug for Client {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ClipboardClient")
            .field("ready", &self.ready.load(Ordering::Acquire))
            .finish()
    }
}
impl Client {
    pub(crate) fn new(enabled: bool, limit: usize) -> Result<Arc<Self>, String> {
        Self::configured(enabled, limit, None, false)
    }
    fn configured(
        enabled: bool,
        limit: usize,
        socket: Option<std::path::PathBuf>,
        legacy_only: bool,
    ) -> Result<Arc<Self>, String> {
        let (reader, wake) = UnixStream::pair().map_err(|error| error.to_string())?;
        reader
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        wake.set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        Ok(Arc::new(Self {
            socket,
            legacy_only,
            stopped: AtomicBool::new(false),
            started: AtomicBool::new(false),
            ready: AtomicBool::new(false),
            enabled: AtomicBool::new(enabled),
            limit: AtomicUsize::new(limit.clamp(1, 100)),
            next_id: std::sync::atomic::AtomicU64::new(0),
            capture_epoch: std::sync::atomic::AtomicU64::new(0),
            queue: Mutex::new(VecDeque::new()),
            wake: Mutex::new(wake),
            reader: Mutex::new(reader),
            history: Mutex::new(Vec::new()),
            mailbox: Mutex::new(Snapshot::default()),
            notifier: Mutex::new(None),
        }))
    }
    fn wake(&self) {
        let _ = self.wake.lock().unwrap().write(&[1]);
    }
    #[cfg(test)]
    fn latest(&self) -> Snapshot {
        self.mailbox.lock().unwrap().clone()
    }
    pub(crate) fn configure(&self, enabled: bool, limit: usize) {
        {
            let mut history = self.history.lock().unwrap();
            self.capture_epoch.fetch_add(1, Ordering::AcqRel);
            self.enabled.store(enabled, Ordering::Release);
            self.limit.store(limit.clamp(1, 100), Ordering::Release);
            if enabled {
                trim(&mut history, limit);
            } else {
                history.clear();
            }
        }
        let error = self.mailbox.lock().unwrap().error.clone();
        self.publish(self.ready.load(Ordering::Acquire), error);
        self.wake();
    }
    pub(crate) fn copy(
        &self,
        text: String,
    ) -> iced::futures::future::BoxFuture<'static, Result<(), String>> {
        let (sender, receiver) = oneshot::channel();
        let error = if text.len() > TEXT_LIMIT || text.contains('\0') {
            Some("clipboard text exceeds 16 KiB or contains NUL")
        } else if !self.ready.load(Ordering::Acquire) {
            Some("native clipboard service is unavailable")
        } else {
            None
        };
        let mut queue = self.queue.lock().unwrap();
        if let Some(error) = error {
            let _ = sender.send(Err(error.into()));
        } else if queue.len() >= 64 {
            let _ = sender.send(Err("clipboard command queue is full".into()));
        } else {
            queue.push_back(CopyRequest {
                text,
                queued: Instant::now(),
                reply: sender,
            });
            self.wake();
        }
        Box::pin(async move {
            receiver
                .await
                .unwrap_or_else(|_| Err("clipboard owner stopped".into()))
        })
    }
    fn publish(&self, available: bool, error: Option<String>) {
        let snapshot = Snapshot {
            available,
            enabled: self.enabled.load(Ordering::Acquire),
            entries: Arc::new(self.history.lock().unwrap().clone()),
            error,
        };
        let mut current = self.mailbox.lock().unwrap();
        if *current != snapshot {
            *current = snapshot;
            if let Some(sender) = self.notifier.lock().unwrap().as_mut() {
                let _ = sender.try_send(());
            }
        }
    }
    fn reject(&self) {
        while let Some(request) = self.queue.lock().unwrap().pop_front() {
            let _ = request
                .reply
                .send(Err("clipboard service disconnected".into()));
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
        self.client.ready.store(false, Ordering::Release);
        self.client.wake();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        self.client.reject();
        self.client.publish(false, None);
        self.client.started.store(false, Ordering::Release);
    }
}
pub(crate) fn events(client: Arc<Client>) -> BoxStream<'static, Snapshot> {
    if client.started.swap(true, Ordering::AcqRel) {
        return iced::futures::stream::once(async move {
            Snapshot {
                enabled: client.enabled.load(Ordering::Acquire),
                error: Some("clipboard subscription is already active".into()),
                ..Snapshot::default()
            }
        })
        .boxed();
    }
    let (sender, receiver) = mpsc::channel(1);
    *client.notifier.lock().unwrap() = Some(sender);
    let thread = {
        client.stopped.store(false, Ordering::Release);
        let owner = client.clone();
        std::thread::Builder::new()
            .name("oxibar-clipboard".into())
            .spawn(move || {
                while !owner.stopped.load(Ordering::Acquire) {
                    if let Err(error) = session(owner.clone()) {
                        owner.ready.store(false, Ordering::Release);
                        owner.publish(false, Some(error));
                        owner.reject();
                    }
                    if !owner.stopped.load(Ordering::Acquire) {
                        let mut reader = owner.reader.lock().unwrap();
                        {
                            let mut fds = [rustix::event::PollFd::new(
                                &*reader,
                                rustix::event::PollFlags::IN,
                            )];
                            let _ = rustix::event::poll(
                                &mut fds,
                                Some(&rustix::event::Timespec {
                                    tv_sec: 2,
                                    tv_nsec: 0,
                                }),
                            );
                        }
                        let mut bytes = [0; 128];
                        while reader.read(&mut bytes).is_ok_and(|count| count > 0) {}
                    }
                }
            })
            .ok()
    };
    if thread.is_none() {
        client.publish(
            false,
            Some("could not start a unique clipboard owner".into()),
        );
    }
    let worker = Worker {
        client: client.clone(),
        thread,
    };
    iced::futures::stream::unfold(
        (receiver, worker),
        move |(mut receiver, worker)| async move {
            receiver.next().await?;
            let snapshot = worker.client.mailbox.lock().unwrap().clone();
            Some((snapshot, (receiver, worker)))
        },
    )
    .boxed()
}
#[derive(Clone)]
enum Offer {
    Modern(eo::ExtDataControlOfferV1),
    Wlr(wo::ZwlrDataControlOfferV1),
}
impl Offer {
    fn id(&self) -> u32 {
        match self {
            Self::Modern(proxy) => proxy.id().protocol_id(),
            Self::Wlr(proxy) => proxy.id().protocol_id(),
        }
    }
    fn receive(&self, mime: &str, fd: std::os::fd::BorrowedFd<'_>) {
        match self {
            Self::Modern(proxy) => proxy.receive(mime.into(), fd),
            Self::Wlr(proxy) => proxy.receive(mime.into(), fd),
        }
    }
    fn destroy(&self) {
        match self {
            Self::Modern(proxy) => proxy.destroy(),
            Self::Wlr(proxy) => proxy.destroy(),
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
    fn id(&self) -> u32 {
        match self {
            Self::Modern(proxy) => proxy.id().protocol_id(),
            Self::Wlr(proxy) => proxy.id().protocol_id(),
        }
    }
    fn destroy(&self) {
        match self {
            Self::Modern(proxy) => proxy.destroy(),
            Self::Wlr(proxy) => proxy.destroy(),
        }
    }
}
struct Io {
    file: File,
    bytes: Vec<u8>,
    offset: usize,
    reading: bool,
    offer: u32,
    selection_epoch: u64,
    capture_epoch: u64,
    deadline: Instant,
}
struct State {
    client: Arc<Client>,
    modern: Option<em::ExtDataControlManagerV1>,
    wlr: Option<wm::ZwlrDataControlManagerV1>,
    seat: Option<wl_seat::WlSeat>,
    device: Option<Device>,
    offers: BTreeMap<u32, (Offer, Vec<String>)>,
    selected: Option<u32>,
    selection_epoch: u64,
    sources: BTreeMap<u32, Source>,
    acknowledgements: BTreeMap<u32, (oneshot::Sender<Result<(), String>>, Instant)>,
    io: Vec<Io>,
    synced: u64,
    finished: bool,
    enabled: bool,
    capture_epoch: u64,
}
impl State {
    fn offer(&mut self, offer: Offer) {
        self.offers.insert(offer.id(), (offer, Vec::new()));
    }
    fn mime(&mut self, id: u32, mime: String) {
        if let Some((_, types)) = self.offers.get_mut(&id)
            && types.len() < 32
            && mime.len() <= 256
        {
            types.push(mime);
        }
    }
    fn selection(&mut self, selected: Option<u32>) {
        self.selection_epoch = self.selection_epoch.saturating_add(1);
        if let Some(old) = self.selected.take()
            && Some(old) != selected
            && let Some((offer, _)) = self.offers.remove(&old)
        {
            offer.destroy();
        }
        self.selected = selected;
        self.capture();
    }
    fn capture(&mut self) {
        if !self.enabled || self.io.len() >= 64 {
            return;
        }
        let Some((offer, types)) = self.selected.and_then(|id| self.offers.get(&id)) else {
            return;
        };
        let Some(mime) = [
            MIME,
            "text/plain;charset=UTF-8",
            "text/plain",
            "UTF8_STRING",
        ]
        .into_iter()
        .find(|mime| types.iter().any(|value| value == mime)) else {
            return;
        };
        if let Ok((read, write)) = rustix::pipe::pipe_with(
            rustix::pipe::PipeFlags::CLOEXEC | rustix::pipe::PipeFlags::NONBLOCK,
        ) {
            offer.receive(mime, write.as_fd());
            self.io.push(Io {
                file: File::from(read),
                bytes: Vec::new(),
                offset: 0,
                reading: true,
                offer: offer.id(),
                selection_epoch: self.selection_epoch,
                capture_epoch: self.capture_epoch,
                deadline: Instant::now() + Duration::from_secs(3),
            });
        }
    }
    fn primary(&mut self, id: Option<u32>) {
        if let Some(id) = id
            && let Some((offer, _)) = self.offers.remove(&id)
        {
            offer.destroy();
        }
    }
    fn send(&mut self, fd: std::os::fd::OwnedFd, text: &str, mime: &str) {
        if self.io.len() >= 64 || ![MIME, "text/plain", "UTF8_STRING"].contains(&mime) {
            return;
        }
        let Ok(flags) = rustix::fs::fcntl_getfl(&fd) else {
            return;
        };
        if rustix::fs::fcntl_setfl(&fd, flags | rustix::fs::OFlags::NONBLOCK).is_err() {
            return;
        }
        self.io.push(Io {
            file: File::from(fd),
            bytes: text.as_bytes().to_vec(),
            offset: 0,
            reading: false,
            offer: 0,
            selection_epoch: 0,
            capture_epoch: 0,
            deadline: Instant::now() + Duration::from_secs(3),
        });
    }
    fn tick(&mut self) {
        let enabled = self.client.enabled.load(Ordering::Acquire);
        let capture_epoch = self.client.capture_epoch.load(Ordering::Acquire);
        if enabled != self.enabled || capture_epoch != self.capture_epoch {
            self.enabled = enabled;
            self.capture_epoch = capture_epoch;
            self.io.retain(|io| !io.reading);
            if !enabled {
                self.client.history.lock().unwrap().clear();
                self.io.retain(|io| !io.reading);
                self.client.publish(true, None);
            } else {
                self.capture();
            }
        }
        let mut accepted = Vec::new();
        self.io.retain_mut(|io| {
            if Instant::now() > io.deadline {
                return false;
            }
            if io.reading {
                let mut bytes = [0; 8192];
                loop {
                    match io.file.read(&mut bytes) {
                        Ok(0) => {
                            if self.enabled
                                && self.selected == Some(io.offer)
                                && self.selection_epoch == io.selection_epoch
                                && self.capture_epoch == io.capture_epoch
                                && let Ok(text) = String::from_utf8(std::mem::take(&mut io.bytes))
                                && !text.is_empty()
                                && !text.contains('\0')
                            {
                                accepted.push((io.capture_epoch, text));
                            }
                            return false;
                        }
                        Ok(count) => {
                            if io.bytes.len() + count > TEXT_LIMIT {
                                return false;
                            }
                            io.bytes.extend_from_slice(&bytes[..count]);
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            return true;
                        }
                        Err(_) => return false,
                    }
                }
            } else {
                match io.file.write(&io.bytes[io.offset..]) {
                    Ok(0) => false,
                    Ok(count) => {
                        io.offset += count;
                        io.offset < io.bytes.len()
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => true,
                    Err(_) => false,
                }
            }
        });
        if !accepted.is_empty() {
            let mut history = self.client.history.lock().unwrap();
            for (epoch, text) in accepted {
                if !self.client.enabled.load(Ordering::Acquire)
                    || epoch != self.client.capture_epoch.load(Ordering::Acquire)
                {
                    continue;
                }
                let old = history
                    .iter()
                    .position(|entry| entry.text == text)
                    .map(|position| history.remove(position));
                let id = old.map_or_else(
                    || {
                        self.client
                            .next_id
                            .fetch_add(1, Ordering::AcqRel)
                            .saturating_add(1)
                    },
                    |entry| entry.id,
                );
                history.insert(0, Entry { id, text });
            }
            trim(&mut history, self.client.limit.load(Ordering::Acquire));
            drop(history);
            self.client.publish(true, None);
        } else {
            let mut history = self.client.history.lock().unwrap();
            let previous = history.len();
            trim(&mut history, self.client.limit.load(Ordering::Acquire));
            let changed = previous != history.len();
            drop(history);
            if changed {
                self.client.publish(true, None);
            }
        }
        let expired: Vec<_> = self
            .acknowledgements
            .iter()
            .filter(|(_, (_, deadline))| Instant::now() > *deadline)
            .map(|(id, _)| *id)
            .collect();
        for id in expired {
            if let Some((reply, _)) = self.acknowledgements.remove(&id) {
                let _ = reply.send(Err("clipboard ownership confirmation timed out".into()));
            }
        }
    }
    fn commands(&mut self, connection: &Connection, qh: &QueueHandle<Self>) {
        loop {
            let request = self.client.queue.lock().unwrap().pop_front();
            let Some(request) = request else { break };
            if request.queued.elapsed() > Duration::from_secs(3) {
                let _ = request.reply.send(Err("clipboard command expired".into()));
                continue;
            }
            let text = Arc::new(request.text);
            let source = match self.device.as_ref() {
                Some(Device::Modern(device)) => {
                    let source = self
                        .modern
                        .as_ref()
                        .unwrap()
                        .create_data_source(qh, text.clone());
                    for mime in [MIME, "text/plain", "UTF8_STRING"] {
                        source.offer(mime.into());
                    }
                    device.set_selection(Some(&source));
                    Source::Modern(source)
                }
                Some(Device::Wlr(device)) => {
                    let source = self
                        .wlr
                        .as_ref()
                        .unwrap()
                        .create_data_source(qh, text.clone());
                    for mime in [MIME, "text/plain", "UTF8_STRING"] {
                        source.offer(mime.into());
                    }
                    device.set_selection(Some(&source));
                    Source::Wlr(source)
                }
                None => {
                    let _ = request
                        .reply
                        .send(Err("clipboard service unavailable".into()));
                    continue;
                }
            };
            self.sources.insert(source.id(), source);
            let callback = connection.display().sync(qh, ());
            self.acknowledgements.insert(
                callback.id().protocol_id(),
                (request.reply, Instant::now() + Duration::from_secs(3)),
            );
        }
    }
}
fn trim(history: &mut Vec<Entry>, limit: usize) {
    history.truncate(limit.clamp(1, 100));
    while history.iter().map(|entry| entry.text.len()).sum::<usize>() > HISTORY_BYTES {
        history.pop();
    }
}
fn poll(
    connection: &Connection,
    queue: &mut EventQueue<State>,
    state: &mut State,
    reader: &mut UnixStream,
    timeout: Option<Duration>,
) -> Result<(), String> {
    if state.client.stopped.load(Ordering::Acquire) {
        return Err("clipboard stopped".into());
    }
    queue
        .dispatch_pending(state)
        .map_err(|error| error.to_string())?;
    connection.flush().map_err(|error| error.to_string())?;
    let guard = queue.prepare_read();
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
                if io.reading {
                    rustix::event::PollFlags::IN
                } else {
                    rustix::event::PollFlags::OUT
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
        if let Some(guard) = guard {
            guard.read().map_err(|error| error.to_string())?;
        }
    } else {
        drop(guard);
    }
    let mut byte = [0; 128];
    while reader.read(&mut byte).is_ok_and(|count| count > 0) {}
    queue
        .dispatch_pending(state)
        .map_err(|error| error.to_string())?;
    state.tick();
    if state.finished {
        Err("clipboard protocol owner finished".into())
    } else {
        Ok(())
    }
}
fn session(client: Arc<Client>) -> Result<(), String> {
    let connection = if let Some(path) = &client.socket {
        let socket = UnixStream::connect(path).map_err(|error| error.to_string())?;
        Connection::from_socket(socket).map_err(|error| error.to_string())?
    } else {
        Connection::connect_to_env().map_err(|error| error.to_string())?
    };
    let mut queue = connection.new_event_queue();
    let qh = queue.handle();
    let mut state = State {
        client: client.clone(),
        modern: None,
        wlr: None,
        seat: None,
        device: None,
        offers: BTreeMap::new(),
        selected: None,
        selection_epoch: 0,
        sources: BTreeMap::new(),
        acknowledgements: BTreeMap::new(),
        io: Vec::new(),
        synced: 0,
        finished: false,
        enabled: client.enabled.load(Ordering::Acquire),
        capture_epoch: client.capture_epoch.load(Ordering::Acquire),
    };
    let mut reader = client
        .reader
        .lock()
        .unwrap()
        .try_clone()
        .map_err(|error| error.to_string())?;
    connection.display().get_registry(&qh, ());
    for _ in 0..2 {
        let before = state.synced;
        connection.display().sync(&qh, ());
        let deadline = Instant::now() + Duration::from_secs(3);
        while state.synced == before {
            if Instant::now() > deadline {
                return Err("clipboard initialization timed out".into());
            }
            poll(
                &connection,
                &mut queue,
                &mut state,
                &mut reader,
                Some(Duration::from_millis(100)),
            )?;
        }
    }
    let seat = state.seat.as_ref().ok_or("clipboard seat unavailable")?;
    state.device = if let Some(manager) = state.modern.as_ref().filter(|_| !client.legacy_only) {
        Some(Device::Modern(manager.get_data_device(seat, &qh, ())))
    } else if let Some(manager) = &state.wlr {
        Some(Device::Wlr(manager.get_data_device(seat, &qh, ())))
    } else {
        return Err("compositor exposes no native data-control protocol".into());
    };
    client.ready.store(true, Ordering::Release);
    client.publish(true, None);
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
                    .map(|(_, deadline)| *deadline),
            )
            .min()
            .map(|deadline| deadline.saturating_duration_since(Instant::now()));
        poll(&connection, &mut queue, &mut state, &mut reader, timeout)?;
    }
    Ok(())
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
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match interface.as_str() {
                "ext_data_control_manager_v1" => {
                    state.modern = Some(registry.bind(name, version.min(1), qh, ()))
                }
                "zwlr_data_control_manager_v1" => {
                    state.wlr = Some(registry.bind(name, version.min(2), qh, ()))
                }
                "wl_seat" if state.seat.is_none() => {
                    state.seat = Some(registry.bind(name, version.min(2), qh, ()))
                }
                _ => {}
            }
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
        if let Some((reply, _)) = state.acknowledgements.remove(&proxy.id().protocol_id()) {
            let _ = reply.send(Ok(()));
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
            ed::Event::Selection { id } => {
                state.selection(id.map(|proxy| proxy.id().protocol_id()))
            }
            ed::Event::PrimarySelection { id } => {
                state.primary(id.map(|proxy| proxy.id().protocol_id()))
            }
            ed::Event::Finished => state.finished = true,
            _ => {}
        }
    }
    wayland_client::event_created_child!(State,ed::ExtDataControlDeviceV1,[0=>(eo::ExtDataControlOfferV1,())]);
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
            wd::Event::Selection { id } => {
                state.selection(id.map(|proxy| proxy.id().protocol_id()))
            }
            wd::Event::PrimarySelection { id } => {
                state.primary(id.map(|proxy| proxy.id().protocol_id()))
            }
            wd::Event::Finished => state.finished = true,
            _ => {}
        }
    }
    wayland_client::event_created_child!(State,wd::ZwlrDataControlDeviceV1,[0=>(wo::ZwlrDataControlOfferV1,())]);
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
impl Dispatch<es::ExtDataControlSourceV1, Arc<String>> for State {
    fn event(
        state: &mut Self,
        proxy: &es::ExtDataControlSourceV1,
        event: es::Event,
        text: &Arc<String>,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            es::Event::Send { mime_type, fd } => state.send(fd, text, &mime_type),
            es::Event::Cancelled => {
                if let Some(source) = state.sources.remove(&proxy.id().protocol_id()) {
                    source.destroy();
                }
            }
            _ => {}
        }
    }
}
impl Dispatch<ws::ZwlrDataControlSourceV1, Arc<String>> for State {
    fn event(
        state: &mut Self,
        proxy: &ws::ZwlrDataControlSourceV1,
        event: ws::Event,
        text: &Arc<String>,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ws::Event::Send { mime_type, fd } => state.send(fd, text, &mime_type),
            ws::Event::Cancelled => {
                if let Some(source) = state.sources.remove(&proxy.id().protocol_id()) {
                    source.destroy();
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ephemeral_history_has_count_and_byte_bounds() {
        let mut history = (0..100)
            .map(|id| Entry {
                id,
                text: "x".repeat(TEXT_LIMIT),
            })
            .collect();
        trim(&mut history, 100);
        assert!(history.iter().map(|entry| entry.text.len()).sum::<usize>() <= HISTORY_BYTES);
        trim(&mut history, 3);
        assert_eq!(history.len(), 3);
    }

    #[test]
    fn offline_opt_out_clears_history_and_never_reuses_identity_or_blocks_copy() {
        let client = Client::new(true, 2).unwrap();
        client.history.lock().unwrap().push(Entry {
            id: 8,
            text: "private fixture".into(),
        });
        client.next_id.store(9, Ordering::Release);
        client.configure(false, 2);
        assert!(client.latest().entries.is_empty());
        client.configure(true, 2);
        assert!(client.latest().entries.is_empty());
        assert_eq!(client.next_id.load(Ordering::Acquire), 9);
        assert!(iced::futures::executor::block_on(client.copy("fixture".into())).is_err());
    }

    #[test]
    #[ignore = "requires an explicitly private compositor: OXIBAR_TEST_PRIVATE_WAYLAND=1"]
    fn private_wayland_copy_history_opt_out_restart_and_owned_cleanup() {
        assert_eq!(
            std::env::var("OXIBAR_TEST_PRIVATE_WAYLAND").as_deref(),
            Ok("1")
        );
        let runtime = std::path::PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap());
        let display = std::env::var("OXIBAR_TEST_PARENT_WAYLAND")
            .unwrap_or_else(|_| std::env::var("WAYLAND_DISPLAY").unwrap());
        let socket = runtime.join(display);
        let producer = Client::configured(false, 10, Some(socket.clone()), false).unwrap();
        let consumer = Client::configured(true, 2, Some(socket), true).unwrap();
        let mut producer_events = events(producer.clone());
        let mut consumer_events = events(consumer.clone());
        wait(&mut producer_events, |snapshot| snapshot.available);
        wait(&mut consumer_events, |snapshot| snapshot.available);
        let copy =
            |text: &str| iced::futures::executor::block_on(producer.copy(text.into())).unwrap();
        copy("first private fixture");
        let first = wait(&mut consumer_events, |snapshot| {
            snapshot
                .entries
                .first()
                .is_some_and(|entry| entry.text == "first private fixture")
        });
        let first_id = first.entries[0].id;
        drop(events(consumer.clone()));
        assert!(consumer.started.load(Ordering::Acquire));
        copy("second private fixture");
        wait(&mut consumer_events, |snapshot| {
            snapshot
                .entries
                .first()
                .is_some_and(|entry| entry.text == "second private fixture")
        });
        copy("third private fixture");
        let third = wait(&mut consumer_events, |snapshot| {
            snapshot
                .entries
                .first()
                .is_some_and(|entry| entry.text == "third private fixture")
        });
        assert_eq!(third.entries.len(), 2);
        assert!(third.entries[0].id > first_id);
        let third_id = third.entries[0].id;
        consumer.configure(false, 2);
        assert!(consumer.latest().entries.is_empty());
        copy("fourth private fixture");
        wait(&mut consumer_events, |snapshot| {
            !snapshot.enabled && snapshot.entries.is_empty()
        });
        consumer.configure(true, 2);
        let fourth = wait(&mut consumer_events, |snapshot| {
            snapshot
                .entries
                .first()
                .is_some_and(|entry| entry.text == "fourth private fixture")
        });
        assert!(fourth.entries[0].id > third_id);
        iced::futures::executor::block_on(consumer.copy("owned source fixture".into())).unwrap();
        let before = Instant::now();
        drop(consumer_events);
        assert!(before.elapsed() < Duration::from_secs(1));
        assert!(!consumer.started.load(Ordering::Acquire));
        let mut consumer_events = events(consumer.clone());
        wait(&mut consumer_events, |snapshot| snapshot.available);
        copy("after reconnect private fixture");
        let restarted = wait(&mut consumer_events, |snapshot| {
            snapshot
                .entries
                .first()
                .is_some_and(|entry| entry.text == "after reconnect private fixture")
        });
        assert!(restarted.entries[0].id > fourth.entries[0].id);
        assert!(producer.latest().entries.is_empty());
        drop(consumer_events);
        drop(producer_events);
    }

    fn wait(
        stream: &mut BoxStream<'static, Snapshot>,
        predicate: impl Fn(&Snapshot) -> bool,
    ) -> Snapshot {
        iced::futures::executor::block_on(async {
            let deadline = async_io::Timer::after(Duration::from_secs(6));
            iced::futures::pin_mut!(deadline);
            loop {
                let next = stream.next();
                iced::futures::pin_mut!(next);
                match iced::futures::future::select(next, deadline.as_mut()).await {
                    iced::futures::future::Either::Left((Some(snapshot), _))
                        if predicate(&snapshot) =>
                    {
                        return snapshot;
                    }
                    iced::futures::future::Either::Left((Some(_), _)) => (),
                    _ => panic!("private clipboard snapshot timed out"),
                }
            }
        })
    }
}
