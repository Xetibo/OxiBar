use super::{
    pixels::{CACHE_LIMIT, STAGING_LIMIT, Shape},
    protocols::{
        export::{
            hyprland_toplevel_export_frame_v1 as export_frame,
            hyprland_toplevel_export_manager_v1 as export_manager,
        },
        mapping::{
            hyprland_toplevel_mapping_manager_v1 as mapping_manager,
            hyprland_toplevel_window_mapping_handle_v1 as mapping_handle,
        },
    },
};
use iced::{
    futures::{
        StreamExt,
        channel::{mpsc, oneshot},
        stream::{self, BoxStream},
    },
    widget::image,
};
use memmap2::MmapMut;
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    fs::File,
    io::{Read, Write},
    os::unix::net::UnixStream,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum,
    backend::ObjectId,
    protocol::{wl_buffer, wl_callback, wl_output, wl_registry, wl_seat, wl_shm, wl_shm_pool},
};
use wayland_protocols::ext::{
    foreign_toplevel_list::v1::client::{
        ext_foreign_toplevel_handle_v1 as ext_handle, ext_foreign_toplevel_list_v1 as ext_list,
    },
    image_capture_source::v1::client::{
        ext_foreign_toplevel_image_capture_source_manager_v1 as source_manager,
        ext_image_capture_source_v1 as source,
    },
    image_copy_capture::v1::client::{
        ext_image_copy_capture_frame_v1 as copy_frame,
        ext_image_copy_capture_manager_v1 as copy_manager,
        ext_image_copy_capture_session_v1 as copy_session,
    },
};
use wayland_protocols_wlr::foreign_toplevel::v1::client::{
    zwlr_foreign_toplevel_handle_v1 as wlr_handle, zwlr_foreign_toplevel_manager_v1 as wlr_manager,
};

#[derive(Debug, Clone)]
pub struct Window {
    pub id: u64,
    pub title: String,
    pub app_id: String,
    pub outputs: Vec<String>,
    pub focused: bool,
    pub minimized: bool,
    pub activatable: bool,
    pub image: Option<image::Handle>,
    pub error: Option<String>,
    pub frames: u64,
}
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub backend: String,
    pub windows: Vec<Window>,
    pub error: Option<String>,
    pub staging_bytes: usize,
    pub thumbnail_bytes: usize,
    pub in_flight: usize,
    pub captures: u64,
}
enum Command {
    Metadata(oxibar_plugin_api::CompositorSnapshot),
    Priority(u64),
    Activate(u64, oneshot::Sender<Result<(), String>>),
}
pub struct Client {
    queue: Mutex<VecDeque<Command>>,
    wake: Mutex<UnixStream>,
    reader: UnixStream,
    stop: AtomicBool,
    started: AtomicBool,
    ready: AtomicBool,
    next: AtomicU64,
    fps: u32,
    size: u32,
    metadata: Mutex<oxibar_plugin_api::CompositorSnapshot>,
    #[cfg(test)]
    socket: Option<std::path::PathBuf>,
}
impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Overview capture owner")
    }
}
impl PartialEq for Client {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}
impl Eq for Client {}
impl std::hash::Hash for Client {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::ptr::hash(self, state)
    }
}
impl Client {
    pub(crate) fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }
    pub fn new(
        fps: u32,
        size: u32,
        metadata: oxibar_plugin_api::CompositorSnapshot,
    ) -> Result<Arc<Self>, String> {
        let (reader, writer) = UnixStream::pair().map_err(|e| e.to_string())?;
        reader.set_nonblocking(true).map_err(|e| e.to_string())?;
        writer.set_nonblocking(true).map_err(|e| e.to_string())?;
        Ok(Arc::new(Self {
            queue: Mutex::new(VecDeque::new()),
            wake: Mutex::new(writer),
            reader,
            stop: AtomicBool::new(false),
            started: AtomicBool::new(false),
            ready: AtomicBool::new(false),
            next: AtomicU64::new(1),
            fps: fps.clamp(1, 8),
            size: size.clamp(128, 640),
            metadata: Mutex::new(metadata),
            #[cfg(test)]
            socket: None,
        }))
    }
    #[cfg(test)]
    pub(super) fn for_socket(socket: std::path::PathBuf) -> Arc<Self> {
        let mut client = Self::new(8, 256, Default::default()).unwrap();
        Arc::get_mut(&mut client).unwrap().socket = Some(socket);
        client
    }
    fn wake(&self) {
        if let Ok(mut stream) = self.wake.lock() {
            let _ = stream.write(&[1]);
        }
    }
    fn send(&self, command: Command) -> Result<(), String> {
        if self.stop.load(Ordering::Acquire) {
            return Err("Overview capture stopped".into());
        }
        let mut queue = self
            .queue
            .lock()
            .map_err(|_| "Overview queue unavailable")?;
        if queue.len() >= 32 {
            return Err("Overview command queue is full".into());
        }
        if matches!(command, Command::Metadata(_)) {
            queue.retain(|c| !matches!(c, Command::Metadata(_)));
        }
        if matches!(command, Command::Priority(_)) {
            queue.retain(|c| !matches!(c, Command::Priority(_)));
        }
        queue.push_back(command);
        drop(queue);
        self.wake();
        Ok(())
    }
    pub fn metadata(&self, value: oxibar_plugin_api::CompositorSnapshot) {
        let _ = self.send(Command::Metadata(value));
    }
    pub fn priority(&self, id: u64) {
        let _ = self.send(Command::Priority(id));
    }
    pub async fn activate(&self, id: u64) -> Result<(), String> {
        if !self.ready.load(Ordering::Acquire) {
            return Err("Native window activation is unavailable".into());
        }
        let (tx, rx) = oneshot::channel();
        self.send(Command::Activate(id, tx))?;
        rx.await
            .map_err(|_| "Capture owner ended before activation".to_owned())?
    }
    fn reject_pending(&self) {
        let commands = std::mem::take(&mut *self.queue.lock().unwrap());
        for command in commands {
            if let Command::Activate(_, reply) = command {
                let _ = reply.send(Err("Native window service ended".into()));
            }
        }
    }
}
#[derive(Default)]
struct Mailbox {
    latest: Mutex<Option<Snapshot>>,
    wake: Mutex<Option<mpsc::Sender<()>>>,
}
impl Mailbox {
    fn publish(&self, snapshot: Snapshot) {
        *self.latest.lock().unwrap() = Some(snapshot);
        if let Some(tx) = self.wake.lock().unwrap().as_mut() {
            let _ = tx.try_send(());
        }
    }
}
struct Worker {
    client: Arc<Client>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.client.ready.store(false, Ordering::Release);
        self.client.stop.store(true, Ordering::Release);
        self.client.wake();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        self.client.started.store(false, Ordering::Release);
        self.client.reject_pending();
    }
}
pub fn events(client: Arc<Client>) -> BoxStream<'static, Snapshot> {
    if client.started.swap(true, Ordering::AcqRel) {
        return stream::once(async {
            Snapshot {
                error: Some("Overview capture already subscribed".into()),
                ..Default::default()
            }
        })
        .boxed();
    }
    let mailbox = Arc::new(Mailbox::default());
    let (tx, rx) = mpsc::channel(1);
    *mailbox.wake.lock().unwrap() = Some(tx);
    let worker_client = client.clone();
    let output = mailbox.clone();
    let thread = std::thread::Builder::new()
        .name("oxibar-overview".into())
        .spawn(move || {
            let mut failures = 0;
            while !worker_client.stop.load(Ordering::Acquire) {
                let result = Session::new(worker_client.clone()).and_then(|mut session| {
                    worker_client.ready.store(true, Ordering::Release);
                    session.run(&output)
                });
                worker_client.ready.store(false, Ordering::Release);
                worker_client.reject_pending();
                if worker_client.stop.load(Ordering::Acquire) {
                    break;
                }
                failures += 1;
                output.publish(Snapshot {
                    error: Some(
                        result
                            .err()
                            .unwrap_or_else(|| "Window capture service ended".into()),
                    ),
                    ..Default::default()
                });
                let deadline = Instant::now() + Duration::from_secs(2);
                while Instant::now() < deadline && !worker_client.stop.load(Ordering::Acquire) {
                    std::thread::sleep(Duration::from_millis(20));
                }
                if failures > 100 {
                    break;
                }
            }
            worker_client.ready.store(false, Ordering::Release);
            worker_client.reject_pending();
        });
    let thread = match thread {
        Ok(t) => Some(t),
        Err(e) => {
            mailbox.publish(Snapshot {
                error: Some(e.to_string()),
                ..Default::default()
            });
            None
        }
    };
    let worker = Worker { client, thread };
    snapshot_stream(rx, mailbox, worker)
}
fn snapshot_stream(
    rx: mpsc::Receiver<()>,
    mailbox: Arc<Mailbox>,
    worker: Worker,
) -> BoxStream<'static, Snapshot> {
    stream::unfold(
        (rx, mailbox, worker),
        |(mut rx, mailbox, worker)| async move {
            while rx.next().await.is_some() {
                let value = mailbox.latest.lock().unwrap().take();
                if let Some(value) = value {
                    return Some((value, (rx, mailbox, worker)));
                }
            }
            None
        },
    )
    .boxed()
}
enum Handle {
    Wlr(wlr_handle::ZwlrForeignToplevelHandleV1),
    Ext(ext_handle::ExtForeignToplevelHandleV1),
}
struct Toplevel {
    handle: Handle,
    window: Window,
    output_ids: Vec<ObjectId>,
    address: Option<String>,
    activation: Option<u64>,
    source: Option<source::ExtImageCaptureSourceV1>,
    session: Option<copy_session::ExtImageCopyCaptureSessionV1>,
    constraints: Option<Shape>,
    pending_size: (u32, u32),
    formats: Vec<wl_shm::Format>,
    job: Option<Job>,
    next: Instant,
    bytes: usize,
    done: bool,
}
enum Frame {
    Hypr(export_frame::HyprlandToplevelExportFrameV1),
    Ext(copy_frame::ExtImageCopyCaptureFrameV1),
}
impl Frame {
    fn id(&self) -> ObjectId {
        match self {
            Self::Hypr(p) => p.id(),
            Self::Ext(p) => p.id(),
        }
    }
    fn destroy(&self) {
        match self {
            Self::Hypr(p) => p.destroy(),
            Self::Ext(p) => p.destroy(),
        }
    }
}
struct Buffer {
    proxy: wl_buffer::WlBuffer,
    memory: MmapMut,
    shape: Shape,
}
impl Drop for Buffer {
    fn drop(&mut self) {
        self.proxy.destroy()
    }
}
struct Job {
    frame: Frame,
    shape: Option<Shape>,
    buffer: Option<Buffer>,
    inverted: bool,
    transform: u32,
    deadline: Instant,
}
impl Drop for Job {
    fn drop(&mut self) {
        self.frame.destroy()
    }
}
struct Output {
    name: String,
    global: u32,
    proxy: wl_output::WlOutput,
}
struct NativeActivation {
    handle: wlr_handle::ZwlrForeignToplevelHandleV1,
    address: Option<String>,
    output_ids: Vec<ObjectId>,
    focused: bool,
    minimized: bool,
}
#[derive(Debug, Clone, Copy)]
enum MappingTarget {
    Window(u64),
    Activation(u64),
}
type ActivationReply = (Instant, oneshot::Sender<Result<(), String>>, ObjectId);

struct State {
    client: Arc<Client>,
    globals: HashMap<String, (u32, u32)>,
    shm: Option<wl_shm::WlShm>,
    seat: Option<wl_seat::WlSeat>,
    exporter: Option<export_manager::HyprlandToplevelExportManagerV1>,
    mapping: Option<mapping_manager::HyprlandToplevelMappingManagerV1>,
    source_manager: Option<source_manager::ExtForeignToplevelImageCaptureSourceManagerV1>,
    copy_manager: Option<copy_manager::ExtImageCopyCaptureManagerV1>,
    wlr: Option<wlr_manager::ZwlrForeignToplevelManagerV1>,
    ext: Option<ext_list::ExtForeignToplevelListV1>,
    windows: BTreeMap<u64, Toplevel>,
    activations: BTreeMap<u64, NativeActivation>,
    outputs: HashMap<ObjectId, Output>,
    synchronized: u64,
    modern: bool,
    ended: bool,
    dirty: bool,
    priority: Option<u64>,
    captures: u64,
    reply: Option<ActivationReply>,
}
impl State {
    fn current_frame(&self, id: u64, proxy: &ObjectId) -> bool {
        self.windows
            .get(&id)
            .and_then(|w| w.job.as_ref())
            .is_some_and(|job| job.frame.id() == *proxy)
    }
    fn bytes(&self) -> usize {
        self.windows
            .values()
            .filter_map(|w| w.job.as_ref()?.buffer.as_ref())
            .map(|b| b.memory.len())
            .sum()
    }
    fn close(&mut self, id: u64) {
        if let Some(window) = self.windows.remove(&id) {
            drop(window.job);
            if let Some(p) = window.session {
                p.destroy()
            }
            if let Some(p) = window.source {
                p.destroy()
            }
            match window.handle {
                Handle::Ext(p) => p.destroy(),
                Handle::Wlr(p) => p.destroy(),
            }
        }
        self.dirty = true;
    }
    fn snapshot(&mut self) -> Snapshot {
        let metadata = self.client.metadata.lock().unwrap();
        for window in self.windows.values_mut() {
            let mapped = window.address.as_deref().and_then(|address| {
                metadata
                    .windows
                    .iter()
                    .find(|w| crate::compositor::window_address(&w.id) == Some(address))
            });
            let activation = window.activation.and_then(|id| self.activations.get(&id));
            let outputs = activation.map_or(&window.output_ids, |native| &native.output_ids);
            window.window.outputs = outputs
                .iter()
                .filter_map(|id| self.outputs.get(id))
                .map(|o| o.name.clone())
                .filter(|n| !n.is_empty())
                .collect();
            if let Some(mapped) = mapped {
                if window.window.outputs.is_empty() {
                    window.window.outputs.extend(mapped.output.clone())
                }
                window.window.focused = mapped.focused;
            }
            if let Some(native) = activation {
                window.window.focused = native.focused;
                window.window.minimized = native.minimized;
            }
            window.window.activatable = self.seat.is_some()
                && (matches!(window.handle, Handle::Wlr(_)) || activation.is_some());
        }
        Snapshot {
            backend: if self.modern {
                "ext-image-copy-capture"
            } else if self.exporter.is_none() {
                "Foreign toplevel list (capture unsupported)"
            } else {
                "Hyprland toplevel export v2"
            }
            .into(),
            windows: self
                .windows
                .values()
                .filter(|w| w.done)
                .map(|w| w.window.clone())
                .collect(),
            error: if !self.modern && self.exporter.is_none() {
                Some("This compositor cannot capture individual window contents".into())
            } else {
                None
            },
            staging_bytes: self.bytes(),
            thumbnail_bytes: self.windows.values().map(|w| w.bytes).sum(),
            in_flight: self.windows.values().filter(|w| w.job.is_some()).count(),
            captures: self.captures,
        }
    }
    fn fail(&mut self, id: u64, error: String) {
        if let Some(w) = self.windows.get_mut(&id) {
            w.job = None;
            w.window.error = Some(error);
            w.next = Instant::now() + Duration::from_secs(2);
        }
        self.dirty = true;
    }
    fn unavailable(&mut self, id: u64, error: String) {
        if let Some(window) = self.windows.get_mut(&id) {
            window.window.image = None;
            window.bytes = 0;
        }
        self.fail(id, error);
    }
    fn buffer(&self, shape: Shape, qh: &QueueHandle<Self>) -> Result<Buffer, String> {
        let size = shape.bytes()?;
        if self.bytes() + size > STAGING_LIMIT {
            return Err("Capture staging budget is occupied".into());
        }
        let fd =
            rustix::fs::memfd_create(c"oxibar-window-preview", rustix::fs::MemfdFlags::CLOEXEC)
                .map_err(|e| e.to_string())?;
        let file = File::from(fd);
        file.set_len(size as u64).map_err(|e| e.to_string())?;
        // Private memfd length is fixed. Pixels are read only after the compositor's ready event.
        let memory = unsafe { memmap2::MmapOptions::new().len(size).map_mut(&file) }
            .map_err(|e| e.to_string())?;
        use std::os::fd::AsFd;
        let pool = self
            .shm
            .as_ref()
            .ok_or("Shared-memory capture unavailable")?
            .create_pool(file.as_fd(), size as i32, qh, ());
        let proxy = pool.create_buffer(
            0,
            shape.width as i32,
            shape.height as i32,
            shape.stride as i32,
            shape.format,
            qh,
            (),
        );
        pool.destroy();
        Ok(Buffer {
            proxy,
            memory,
            shape,
        })
    }
    fn complete(&mut self, id: u64) {
        let limit = self
            .client
            .size
            .min(
                ((CACHE_LIMIT / (self.windows.len().max(1) * 4)) as f64)
                    .sqrt()
                    .floor() as u32,
            )
            .max(64);
        let Some(window) = self.windows.get_mut(&id) else {
            return;
        };
        let Some(job) = window.job.take() else { return };
        let result = job
            .buffer
            .as_ref()
            .ok_or("Capture completed without a buffer".to_owned())
            .and_then(|buffer| {
                buffer
                    .shape
                    .thumbnail(&buffer.memory, limit, job.inverted, job.transform)
            });
        match result {
            Ok((width, height, rgba)) => {
                let size = rgba.len();
                let mut used: usize = self
                    .windows
                    .values()
                    .filter(|w| w.window.id != id)
                    .map(|w| w.bytes)
                    .sum();
                if used + size > CACHE_LIMIT {
                    let mut stale: Vec<_> = self
                        .windows
                        .iter()
                        .filter(|(key, w)| **key != id && w.bytes > 0)
                        .map(|(key, w)| (*key, w.window.frames))
                        .collect();
                    stale.sort_by_key(|(key, frames)| (self.priority == Some(*key), *frames));
                    for (key, _) in stale {
                        if used + size <= CACHE_LIMIT {
                            break;
                        }
                        let w = self.windows.get_mut(&key).unwrap();
                        used = used.saturating_sub(w.bytes);
                        w.bytes = 0;
                        w.window.image = None;
                    }
                }
                let w = self.windows.get_mut(&id).unwrap();
                if used + size <= CACHE_LIMIT {
                    w.window.image = Some(image::Handle::from_rgba(width, height, rgba));
                    w.bytes = size;
                    w.window.frames += 1;
                    w.window.error = None;
                    self.captures += 1;
                } else {
                    w.window.error = Some("Thumbnail cache budget reached".into());
                }
                w.next = Instant::now() + Duration::from_millis(1000 / self.client.fps as u64);
            }
            Err(error) => self.fail(id, error),
        }
        self.dirty = true;
    }
    fn schedule(&mut self, qh: &QueueHandle<Self>) {
        let now = Instant::now();
        let expired: Vec<_> = self
            .windows
            .iter()
            .filter(|(_, w)| w.job.as_ref().is_some_and(|j| now >= j.deadline))
            .map(|(id, _)| *id)
            .collect();
        for id in expired {
            self.fail(
                id,
                "Window has not supplied a capture frame (possibly minimized)".into(),
            );
        }
        let mut active = self.windows.values().filter(|w| w.job.is_some()).count();
        let mut ready: Vec<_> = self
            .windows
            .iter()
            .filter(|(_, w)| w.done && w.job.is_none() && w.next <= now)
            .map(|(id, w)| (*id, w.next))
            .collect();
        ready.sort_by_key(|(id, next)| (self.priority != Some(*id), *next, *id));
        for (id, _) in ready {
            if active >= 2 {
                break;
            }
            if self.modern {
                let shape = self.windows[&id].constraints;
                let Some(shape) = shape else { continue };
                let buffer = match self.buffer(shape, qh) {
                    Ok(b) => b,
                    Err(e) => {
                        self.fail(id, e);
                        continue;
                    }
                };
                let w = self.windows.get_mut(&id).unwrap();
                let Some(session) = w.session.as_ref() else {
                    continue;
                };
                let frame = session.create_frame(qh, id);
                frame.attach_buffer(&buffer.proxy);
                frame.damage_buffer(0, 0, shape.width as i32, shape.height as i32);
                frame.capture();
                w.job = Some(Job {
                    frame: Frame::Ext(frame),
                    shape: Some(shape),
                    buffer: Some(buffer),
                    inverted: false,
                    transform: 0,
                    deadline: now + Duration::from_secs(3),
                });
            } else if let (Some(manager), Handle::Wlr(handle)) =
                (&self.exporter, &self.windows[&id].handle)
            {
                let frame = manager.capture_toplevel_with_wlr_toplevel_handle(0, handle, qh, id);
                self.windows.get_mut(&id).unwrap().job = Some(Job {
                    frame: Frame::Hypr(frame),
                    shape: None,
                    buffer: None,
                    inverted: false,
                    transform: 0,
                    deadline: now + Duration::from_secs(3),
                });
            } else {
                continue;
            }
            active += 1;
        }
    }
}
struct Session {
    connection: Connection,
    queue: EventQueue<State>,
    state: State,
}
impl Session {
    fn new(client: Arc<Client>) -> Result<Self, String> {
        #[cfg(test)]
        let injected = client
            .socket
            .as_ref()
            .map(|path| {
                Connection::from_socket(UnixStream::connect(path).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())
            })
            .transpose()?;
        #[cfg(not(test))]
        let injected: Option<Connection> = None;
        let connection = injected
            .or_else(crate::app::connect_wayland)
            .ok_or("Unable to connect to Wayland for window previews")?;
        let queue = connection.new_event_queue();
        let registry = connection.display().get_registry(&queue.handle(), ());
        let state = State {
            client,
            globals: HashMap::new(),
            shm: None,
            seat: None,
            exporter: None,
            mapping: None,
            source_manager: None,
            copy_manager: None,
            wlr: None,
            ext: None,
            windows: BTreeMap::new(),
            activations: BTreeMap::new(),
            outputs: HashMap::new(),
            synchronized: 0,
            modern: false,
            ended: false,
            dirty: true,
            priority: None,
            captures: 0,
            reply: None,
        };
        let mut this = Self {
            connection,
            queue,
            state,
        };
        this.sync()?;
        let qh = this.queue.handle();
        this.state.modern = this.state.mapping.is_some()
            && this.state.source_manager.is_some()
            && this.state.copy_manager.is_some()
            && this
                .state
                .globals
                .contains_key("ext_foreign_toplevel_list_v1");
        if this.state.modern {
            let (name, _) = this.state.globals["ext_foreign_toplevel_list_v1"];
            this.state.ext = Some(registry.bind(name, 1, &qh, ()));
        }
        if let Some((name, version)) = this
            .state
            .globals
            .get("zwlr_foreign_toplevel_manager_v1")
            .copied()
        {
            this.state.wlr = Some(registry.bind(name, version.min(3), &qh, ()));
        }
        this.sync()?;
        Ok(this)
    }
    fn sync(&mut self) -> Result<(), String> {
        let value = self.state.synchronized;
        self.connection.display().sync(&self.queue.handle(), false);
        let deadline = Instant::now() + Duration::from_secs(3);
        while self.state.synchronized == value {
            if Instant::now() > deadline {
                return Err("Window protocol discovery timed out".into());
            }
            self.poll(Some(Duration::from_millis(100)))?;
        }
        Ok(())
    }
    fn poll(&mut self, timeout: Option<Duration>) -> Result<(), String> {
        self.queue
            .dispatch_pending(&mut self.state)
            .map_err(|e| e.to_string())?;
        if self.state.client.stop.load(Ordering::Acquire) {
            return Err("Overview stopped".into());
        }
        self.connection.flush().map_err(|e| e.to_string())?;
        let Some(guard) = self.queue.prepare_read() else {
            return Ok(());
        };
        let backend = self.connection.backend();
        let fd = backend.poll_fd();
        let flags = {
            let mut fds = [
                rustix::event::PollFd::new(&fd, rustix::event::PollFlags::IN),
                rustix::event::PollFd::new(&self.state.client.reader, rustix::event::PollFlags::IN),
            ];
            let timeout = timeout.map(|d| rustix::event::Timespec {
                tv_sec: d.as_secs() as i64,
                tv_nsec: d.subsec_nanos() as i64,
            });
            rustix::event::poll(&mut fds, timeout.as_ref()).map_err(|e| e.to_string())?;
            (fds[0].revents(), fds[1].revents())
        };
        if flags.1.contains(rustix::event::PollFlags::IN) {
            let mut reader = &self.state.client.reader;
            let mut bytes = [0; 128];
            while reader.read(&mut bytes).is_ok_and(|n| n > 0) {}
        }
        if flags.0.intersects(
            rustix::event::PollFlags::IN
                | rustix::event::PollFlags::HUP
                | rustix::event::PollFlags::ERR,
        ) {
            guard.read().map_err(|e| e.to_string())?;
        } else {
            drop(guard)
        }
        self.queue
            .dispatch_pending(&mut self.state)
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    fn run(&mut self, mailbox: &Mailbox) -> Result<(), String> {
        loop {
            if self.state.client.stop.load(Ordering::Acquire) {
                return Ok(());
            }
            if self.state.ended {
                return Err("Native toplevel service ended".into());
            }
            let commands = std::mem::take(&mut *self.state.client.queue.lock().unwrap());
            for command in commands {
                match command {
                    Command::Metadata(value) => {
                        *self.state.client.metadata.lock().unwrap() = value;
                        self.state.dirty = true
                    }
                    Command::Priority(id) => self.state.priority = Some(id),
                    Command::Activate(id, reply) => {
                        let result = self.activate(id);
                        if let Err(error) = result {
                            let _ = reply.send(Err(error));
                        } else {
                            if let Some((_, previous, _)) = self.state.reply.take() {
                                let _ = previous.send(Err("Activation superseded".into()));
                            }
                            let callback =
                                self.connection.display().sync(&self.queue.handle(), true);
                            self.state.reply = Some((
                                Instant::now() + Duration::from_secs(3),
                                reply,
                                callback.id(),
                            ));
                        }
                    }
                }
            }
            self.state.schedule(&self.queue.handle());
            if self
                .state
                .reply
                .as_ref()
                .is_some_and(|(deadline, _, _)| Instant::now() >= *deadline)
                && let Some((_, reply, _)) = self.state.reply.take()
            {
                let _ = reply.send(Err("Native window activation timed out".into()));
            }
            if self.state.dirty {
                self.state.dirty = false;
                mailbox.publish(self.state.snapshot());
            }
            let available = self.state.modern || self.state.exporter.is_some();
            let wait = if available && !self.state.windows.is_empty() || self.state.reply.is_some()
            {
                Some(Duration::from_millis(1000 / self.state.client.fps as u64))
            } else {
                None
            };
            self.poll(wait)?;
        }
    }
    fn activate(&mut self, id: u64) -> Result<(), String> {
        let window = self
            .state
            .windows
            .get(&id)
            .ok_or("The selected window has closed")?;
        match &window.handle {
            Handle::Wlr(handle) => {
                let seat = self
                    .state
                    .seat
                    .as_ref()
                    .ok_or("No seat for window activation")?;
                if window.window.minimized {
                    handle.unset_minimized();
                }
                handle.activate(seat);
                Ok(())
            }
            Handle::Ext(_) => {
                let target = window
                    .activation
                    .and_then(|id| self.state.activations.get(&id))
                    .ok_or("Exact native activation handle is unavailable")?;
                let seat = self
                    .state
                    .seat
                    .as_ref()
                    .ok_or("No seat for window activation")?;
                if target.minimized {
                    target.handle.unset_minimized();
                }
                target.handle.activate(seat);
                Ok(())
            }
        }
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        let ids: Vec<_> = self.state.windows.keys().copied().collect();
        for id in ids {
            self.state.close(id);
        }
        for native in std::mem::take(&mut self.state.activations).into_values() {
            native.handle.destroy();
        }
        if let Some((_, reply, _)) = self.state.reply.take() {
            let _ = reply.send(Err("Overview capture ended".into()));
        }
        let _ = self.connection.flush();
    }
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
                state.globals.insert(interface.clone(), (name, version));
                match interface.as_str() {
                    "wl_shm" if state.shm.is_none() => {
                        state.shm = Some(registry.bind(name, 1, qh, ()))
                    }
                    "wl_seat" if state.seat.is_none() => {
                        state.seat = Some(registry.bind(name, version.min(9), qh, ()))
                    }
                    "wl_output" => {
                        let p: wl_output::WlOutput = registry.bind(name, version.min(4), qh, name);
                        state.outputs.insert(
                            p.id(),
                            Output {
                                name: String::new(),
                                global: name,
                                proxy: p,
                            },
                        );
                    }
                    "hyprland_toplevel_export_manager_v1" if version >= 2 => {
                        state.exporter = Some(registry.bind(name, 2, qh, ()))
                    }
                    "hyprland_toplevel_mapping_manager_v1" => {
                        state.mapping = Some(registry.bind(name, 1, qh, ()))
                    }
                    "ext_foreign_toplevel_image_capture_source_manager_v1" => {
                        state.source_manager = Some(registry.bind(name, 1, qh, ()))
                    }
                    "ext_image_copy_capture_manager_v1" => {
                        state.copy_manager = Some(registry.bind(name, 1, qh, ()))
                    }
                    _ => {}
                }
            }
            wl_registry::Event::GlobalRemove { name } => {
                state.outputs.retain(|_, output| {
                    if output.global == name {
                        if output.proxy.version() >= 3 {
                            output.proxy.release();
                        }
                        false
                    } else {
                        true
                    }
                });
                for window in state.windows.values_mut() {
                    window
                        .output_ids
                        .retain(|id| state.outputs.contains_key(id));
                }
                for native in state.activations.values_mut() {
                    native
                        .output_ids
                        .retain(|id| state.outputs.contains_key(id));
                }
                state.dirty = true;
                if state.globals.iter().any(|(interface, (n, _))| {
                    *n == name
                        && matches!(
                            interface.as_str(),
                            "wl_shm"
                                | "wl_seat"
                                | "hyprland_toplevel_export_manager_v1"
                                | "hyprland_toplevel_mapping_manager_v1"
                                | "ext_foreign_toplevel_image_capture_source_manager_v1"
                                | "ext_image_copy_capture_manager_v1"
                                | "zwlr_foreign_toplevel_manager_v1"
                                | "ext_foreign_toplevel_list_v1"
                        )
                }) {
                    state.ended = true
                }
            }
            _ => {}
        }
    }
}
impl Dispatch<wl_callback::WlCallback, bool> for State {
    fn event(
        state: &mut Self,
        proxy: &wl_callback::WlCallback,
        _: wl_callback::Event,
        activation: &bool,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        state.synchronized += 1;
        if *activation
            && state
                .reply
                .as_ref()
                .is_some_and(|(_, _, id)| *id == proxy.id())
            && let Some((_, reply, _)) = state.reply.take()
        {
            let _ = reply.send(Ok(()));
        }
    }
}
impl Dispatch<wl_output::WlOutput, u32> for State {
    fn event(
        state: &mut Self,
        proxy: &wl_output::WlOutput,
        event: wl_output::Event,
        _: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_output::Event::Name { name } = event
            && let Some(output) = state.outputs.get_mut(&proxy.id())
        {
            output.name = name;
            state.dirty = true;
        }
    }
}

impl State {
    fn toplevel(&mut self, handle: Handle) -> u64 {
        let id = self.client.next.fetch_add(1, Ordering::Relaxed);
        if self.windows.len() < 256 {
            self.windows.insert(
                id,
                Toplevel {
                    handle,
                    window: Window {
                        id,
                        title: String::new(),
                        app_id: String::new(),
                        outputs: Vec::new(),
                        focused: false,
                        minimized: false,
                        activatable: false,
                        image: None,
                        error: None,
                        frames: 0,
                    },
                    output_ids: Vec::new(),
                    address: None,
                    activation: None,
                    source: None,
                    session: None,
                    constraints: None,
                    pending_size: (0, 0),
                    formats: Vec::new(),
                    job: None,
                    next: Instant::now(),
                    bytes: 0,
                    done: false,
                },
            );
        } else {
            match handle {
                Handle::Wlr(proxy) => proxy.destroy(),
                Handle::Ext(proxy) => proxy.destroy(),
            };
            return 0;
        }
        self.dirty = true;
        id
    }
    fn id(&self, proxy: &ObjectId) -> Option<u64> {
        self.windows.iter().find_map(|(id, w)| {
            let candidate = match &w.handle {
                Handle::Wlr(p) => p.id(),
                Handle::Ext(p) => p.id(),
            };
            (candidate == *proxy).then_some(*id)
        })
    }
    fn link_activations(&mut self) {
        for window in self.windows.values_mut() {
            if !matches!(window.handle, Handle::Ext(_)) || window.activation.is_some() {
                continue;
            }
            let Some(address) = window.address.as_ref() else {
                continue;
            };
            let mut matches = self
                .activations
                .iter()
                .filter(|(_, native)| native.address.as_ref() == Some(address));
            if let Some((id, _)) = matches.next()
                && matches.next().is_none()
            {
                // Bind once to this native incarnation. A recycled address cannot replace it.
                window.activation = Some(*id);
            }
        }
    }
}
impl Dispatch<wlr_manager::ZwlrForeignToplevelManagerV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &wlr_manager::ZwlrForeignToplevelManagerV1,
        event: wlr_manager::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wlr_manager::Event::Toplevel { toplevel } => {
                if state.modern {
                    if state.activations.len() >= 256 {
                        toplevel.destroy();
                        return;
                    }
                    let id = state.client.next.fetch_add(1, Ordering::Relaxed);
                    state.activations.insert(
                        id,
                        NativeActivation {
                            handle: toplevel.clone(),
                            address: None,
                            output_ids: Vec::new(),
                            focused: false,
                            minimized: false,
                        },
                    );
                    if let Some(manager) = &state.mapping {
                        manager.get_window_for_toplevel_wlr(
                            &toplevel,
                            qh,
                            MappingTarget::Activation(id),
                        );
                    }
                    return;
                }
                let id = state.toplevel(Handle::Wlr(toplevel.clone()));
                if id == 0 {
                    return;
                }
                if let Some(manager) = &state.mapping {
                    manager.get_window_for_toplevel_wlr(&toplevel, qh, MappingTarget::Window(id));
                }
            }
            wlr_manager::Event::Finished => state.ended = true,
            _ => {}
        }
    }
    wayland_client::event_created_child!(State,wlr_manager::ZwlrForeignToplevelManagerV1,[0=>(wlr_handle::ZwlrForeignToplevelHandleV1,())]);
}
impl Dispatch<wlr_handle::ZwlrForeignToplevelHandleV1, ()> for State {
    fn event(
        state: &mut Self,
        proxy: &wlr_handle::ZwlrForeignToplevelHandleV1,
        event: wlr_handle::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if state.modern {
            let Some(id) = state
                .activations
                .iter()
                .find_map(|(id, native)| (native.handle.id() == proxy.id()).then_some(*id))
            else {
                return;
            };
            let native = state.activations.get_mut(&id).unwrap();
            match event {
                wlr_handle::Event::OutputEnter { output } => {
                    if !native.output_ids.contains(&output.id()) {
                        native.output_ids.push(output.id());
                    }
                }
                wlr_handle::Event::OutputLeave { output } => {
                    native.output_ids.retain(|p| *p != output.id())
                }
                wlr_handle::Event::State { state: values } => {
                    native.focused = values
                        .chunks_exact(4)
                        .any(|v| u32::from_ne_bytes(v.try_into().unwrap()) == 2);
                    native.minimized = values
                        .chunks_exact(4)
                        .any(|v| u32::from_ne_bytes(v.try_into().unwrap()) == 1);
                }
                wlr_handle::Event::Closed => {
                    state.activations.remove(&id);
                    proxy.destroy();
                }
                _ => {}
            }
            state.dirty = true;
            return;
        }
        let Some(id) = state.id(&proxy.id()) else {
            return;
        };
        let w = state.windows.get_mut(&id).unwrap();
        match event {
            wlr_handle::Event::Title { title } => {
                w.window.title = title.chars().take(512).collect()
            }
            wlr_handle::Event::AppId { app_id } => {
                w.window.app_id = app_id.chars().take(256).collect()
            }
            wlr_handle::Event::OutputEnter { output } => {
                if !w.output_ids.contains(&output.id()) {
                    w.output_ids.push(output.id())
                }
            }
            wlr_handle::Event::OutputLeave { output } => w.output_ids.retain(|p| *p != output.id()),
            wlr_handle::Event::State { state: values } => {
                w.window.focused = values
                    .chunks_exact(4)
                    .any(|v| u32::from_ne_bytes(v.try_into().unwrap()) == 2);
                w.window.minimized = values
                    .chunks_exact(4)
                    .any(|v| u32::from_ne_bytes(v.try_into().unwrap()) == 1);
            }
            wlr_handle::Event::Done => {
                w.done = true;
                state.dirty = true
            }
            wlr_handle::Event::Closed => state.close(id),
            _ => {}
        }
    }
}
impl Dispatch<ext_list::ExtForeignToplevelListV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ext_list::ExtForeignToplevelListV1,
        event: ext_list::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            ext_list::Event::Toplevel { toplevel } => {
                let id = state.toplevel(Handle::Ext(toplevel.clone()));
                if id == 0 {
                    return;
                }
                if let Some(manager) = &state.mapping {
                    manager.get_window_for_toplevel(&toplevel, qh, MappingTarget::Window(id));
                }
                if let (Some(sources), Some(capture)) = (&state.source_manager, &state.copy_manager)
                {
                    let source = sources.create_source(&toplevel, qh, ());
                    let session =
                        capture.create_session(&source, copy_manager::Options::empty(), qh, id);
                    if let Some(w) = state.windows.get_mut(&id) {
                        w.source = Some(source);
                        w.session = Some(session);
                    }
                }
            }
            ext_list::Event::Finished => state.ended = true,
            _ => {}
        }
    }
    wayland_client::event_created_child!(State,ext_list::ExtForeignToplevelListV1,[0=>(ext_handle::ExtForeignToplevelHandleV1,())]);
}
impl Dispatch<ext_handle::ExtForeignToplevelHandleV1, ()> for State {
    fn event(
        state: &mut Self,
        proxy: &ext_handle::ExtForeignToplevelHandleV1,
        event: ext_handle::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(id) = state.id(&proxy.id()) else {
            return;
        };
        let w = state.windows.get_mut(&id).unwrap();
        match event {
            ext_handle::Event::Title { title } => {
                w.window.title = title.chars().take(512).collect()
            }
            ext_handle::Event::AppId { app_id } => {
                w.window.app_id = app_id.chars().take(256).collect()
            }
            ext_handle::Event::Done => {
                w.done = true;
                state.dirty = true
            }
            ext_handle::Event::Closed => state.close(id),
            _ => {}
        }
    }
}
impl Dispatch<mapping_handle::HyprlandToplevelWindowMappingHandleV1, MappingTarget> for State {
    fn event(
        state: &mut Self,
        proxy: &mapping_handle::HyprlandToplevelWindowMappingHandleV1,
        event: mapping_handle::Event,
        target: &MappingTarget,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let mapping_handle::Event::WindowAddress {
            address_hi,
            address,
        } = event
        {
            let address = Some(format!(
                "0x{:x}",
                ((address_hi as u64) << 32) | address as u64
            ));
            match target {
                MappingTarget::Window(id) => {
                    if let Some(w) = state.windows.get_mut(id) {
                        w.address = address;
                    }
                }
                MappingTarget::Activation(id) => {
                    if let Some(w) = state.activations.get_mut(id) {
                        w.address = address;
                    }
                }
            }
            state.link_activations();
            state.dirty = true;
        }
        proxy.destroy();
    }
}
impl Dispatch<copy_session::ExtImageCopyCaptureSessionV1, u64> for State {
    fn event(
        state: &mut Self,
        _: &copy_session::ExtImageCopyCaptureSessionV1,
        event: copy_session::Event,
        id: &u64,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(w) = state.windows.get_mut(id) else {
            return;
        };
        match event {
            copy_session::Event::BufferSize { width, height } => {
                w.pending_size = (width, height);
            }
            copy_session::Event::ShmFormat {
                format: WEnum::Value(format),
            } => w.formats.push(format),
            copy_session::Event::Done => {
                let (width, height) = w.pending_size;
                let value = w.formats.iter().find_map(|format| {
                    Shape::new(
                        width,
                        height,
                        width.saturating_mul(4),
                        WEnum::Value(*format),
                    )
                    .ok()
                });
                w.formats.clear();
                if value != w.constraints {
                    w.job = None;
                    w.constraints = value;
                    w.next = Instant::now();
                }
                if value.is_none() {
                    w.window.image = None;
                    w.bytes = 0;
                    w.window.error = Some("Unsupported window capture buffer constraints".into());
                }
                state.dirty = true;
            }
            copy_session::Event::Stopped => {
                w.job = None;
                if let Some(session) = w.session.take() {
                    session.destroy();
                }
                w.constraints = None;
                w.window.image = None;
                w.bytes = 0;
                w.window.error = Some("Window capture stopped or was denied".into());
                state.dirty = true;
            }
            _ => {}
        }
    }
}
impl Dispatch<copy_frame::ExtImageCopyCaptureFrameV1, u64> for State {
    fn event(
        state: &mut Self,
        proxy: &copy_frame::ExtImageCopyCaptureFrameV1,
        event: copy_frame::Event,
        id: &u64,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if !state.current_frame(*id, &proxy.id()) {
            return;
        }
        match event {
            copy_frame::Event::Transform {
                transform: WEnum::Value(t),
            } => {
                if let Some(j) = state.windows.get_mut(id).and_then(|w| w.job.as_mut()) {
                    j.transform = t as u32;
                }
            }
            copy_frame::Event::Ready => state.complete(*id),
            copy_frame::Event::Failed { reason } => {
                state.unavailable(*id, format!("Window capture unavailable: {reason:?}"))
            }
            _ => {}
        }
    }
}
impl Dispatch<export_frame::HyprlandToplevelExportFrameV1, u64> for State {
    fn event(
        state: &mut Self,
        proxy: &export_frame::HyprlandToplevelExportFrameV1,
        event: export_frame::Event,
        id: &u64,
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if !state.current_frame(*id, &proxy.id()) {
            return;
        }
        match event {
            export_frame::Event::Buffer {
                format,
                width,
                height,
                stride,
            } => match Shape::new(width, height, stride, format) {
                Ok(shape) => {
                    if let Some(job) = state.windows.get_mut(id).and_then(|w| w.job.as_mut()) {
                        job.shape = Some(shape)
                    }
                }
                Err(e) => state.fail(*id, e),
            },
            export_frame::Event::BufferDone => {
                if let Some(shape) = state
                    .windows
                    .get(id)
                    .and_then(|w| w.job.as_ref())
                    .and_then(|j| j.shape)
                {
                    match state.buffer(shape, qh) {
                        Ok(buffer) => {
                            proxy.copy(&buffer.proxy, 1);
                            if let Some(job) =
                                state.windows.get_mut(id).and_then(|w| w.job.as_mut())
                            {
                                job.buffer = Some(buffer)
                            }
                        }
                        Err(e) => state.fail(*id, e),
                    }
                }
            }
            export_frame::Event::Flags {
                flags: WEnum::Value(flags),
            } => {
                if let Some(job) = state.windows.get_mut(id).and_then(|w| w.job.as_mut()) {
                    job.inverted = flags.contains(export_frame::Flags::YInvert)
                }
            }
            export_frame::Event::Ready { .. } => state.complete(*id),
            export_frame::Event::Failed => state.unavailable(
                *id,
                "Window content capture was denied or is unavailable".into(),
            ),
            _ => {}
        }
    }
}
wayland_client::delegate_noop!(State:ignore wl_shm::WlShm);
wayland_client::delegate_noop!(State:ignore wl_shm_pool::WlShmPool);
wayland_client::delegate_noop!(State:ignore wl_buffer::WlBuffer);
wayland_client::delegate_noop!(State:ignore wl_seat::WlSeat);
wayland_client::delegate_noop!(State:ignore export_manager::HyprlandToplevelExportManagerV1);
wayland_client::delegate_noop!(State:ignore mapping_manager::HyprlandToplevelMappingManagerV1);
wayland_client::delegate_noop!(State:ignore source_manager::ExtForeignToplevelImageCaptureSourceManagerV1);
wayland_client::delegate_noop!(State:ignore copy_manager::ExtImageCopyCaptureManagerV1);
wayland_client::delegate_noop!(State:ignore source::ExtImageCaptureSourceV1);

#[cfg(test)]
mod tests {
    use super::*;
    use iced::futures::{FutureExt, executor::block_on};
    #[test]
    fn coalesced_burst_never_ends_capture_subscription_on_an_empty_wake() {
        let client = Client::new(8, 512, Default::default()).unwrap();
        let mailbox = Arc::new(Mailbox::default());
        let (tx, rx) = mpsc::channel(1);
        *mailbox.wake.lock().unwrap() = Some(tx);
        mailbox.publish(Snapshot {
            captures: 1,
            ..Default::default()
        });
        mailbox.publish(Snapshot {
            captures: 2,
            ..Default::default()
        });
        let mut stream = snapshot_stream(
            rx,
            mailbox,
            Worker {
                client: client.clone(),
                thread: None,
            },
        );
        assert_eq!(block_on(stream.next()).unwrap().captures, 2);
        assert!(stream.next().now_or_never().is_none());
        assert!(block_on(client.activate(1)).is_err());
        drop(stream);
    }
}
