use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, VecDeque},
    io::{Read, Write},
    os::{fd::AsRawFd, unix::net::UnixStream},
    rc::Rc,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use iced::futures::{
    StreamExt,
    channel::{mpsc, oneshot},
    stream::{self, BoxStream},
};
use libpulse_binding as pulse;
use pulse::{
    callbacks::ListResult,
    context::{Context, FlagSet, State, subscribe::InterestMaskSet},
    mainloop::{
        api::{Mainloop as _, MainloopInner},
        events::io::{FlagSet as IoFlags, IoEvent},
        standard::{IterateResult, Mainloop, MainloopInternal},
    },
    operation::Operation,
    time::{MicroSeconds, MonotonicTs},
    volume::{ChannelVolumes, Volume},
};

use super::system::{AudioDevice, DeviceAction, DeviceKind, DeviceSnapshot};

const QUEUE_LIMIT: usize = 64;
const OPERATION_TIMEOUT: Duration = Duration::from_secs(3);
const RECONNECT_DELAY: Duration = Duration::from_secs(2);

struct Request {
    action: DeviceAction,
    issued: Instant,
    reply: oneshot::Sender<Result<(), String>>,
}

pub(crate) struct Client {
    requests: Mutex<VecDeque<Request>>,
    wake_read: UnixStream,
    wake_write: UnixStream,
    stopped: AtomicBool,
    started: AtomicBool,
    ready: AtomicBool,
    refresh: AtomicBool,
    retry_lock: Mutex<()>,
    retry: Condvar,
    server: Option<String>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioClient")
            .field("ready", &self.ready.load(Ordering::Acquire))
            .finish()
    }
}

impl Client {
    pub(crate) fn new() -> Result<Arc<Self>, String> {
        Self::for_server(None)
    }

    pub(super) fn for_server(server: Option<String>) -> Result<Arc<Self>, String> {
        let (wake_read, wake_write) = UnixStream::pair().map_err(|e| e.to_string())?;
        wake_read.set_nonblocking(true).map_err(|e| e.to_string())?;
        wake_write
            .set_nonblocking(true)
            .map_err(|e| e.to_string())?;
        Ok(Arc::new(Self {
            requests: Mutex::new(VecDeque::new()),
            wake_read,
            wake_write,
            stopped: AtomicBool::new(false),
            started: AtomicBool::new(false),
            ready: AtomicBool::new(false),
            refresh: AtomicBool::new(false),
            retry_lock: Mutex::new(()),
            retry: Condvar::new(),
            server,
        }))
    }

    pub(crate) fn submit(&self, action: DeviceAction) -> oneshot::Receiver<Result<(), String>> {
        let (reply, receiver) = oneshot::channel();
        let mut requests = self.requests.lock().unwrap_or_else(|e| e.into_inner());
        let error = action.validate().err().or_else(|| {
            (!self.ready.load(Ordering::Acquire))
                .then(|| "audio: server is disconnected".to_owned())
        });
        if let Some(error) = error {
            let _ = reply.send(Err(error));
            return receiver;
        }
        // Superseded slider/toggle writes never accumulate behind a slow server.
        if let Some(index) = requests
            .iter()
            .position(|request| action.supersedes(&request.action))
            && let Some(request) = requests.remove(index)
        {
            let _ = request.reply.send(Ok(()));
        }
        if requests.len() >= QUEUE_LIMIT {
            let _ = reply.send(Err("audio: command queue is full".to_owned()));
        } else {
            requests.push_back(Request {
                action,
                issued: Instant::now(),
                reply,
            });
            self.wake();
        }
        receiver
    }

    pub(crate) fn refresh(&self) {
        self.refresh.store(true, Ordering::Release);
        self.wake();
    }

    fn wake(&self) {
        let _ = (&self.wake_write).write(&[1]);
        self.retry.notify_all();
    }

    fn reject_pending(&self) {
        self.ready.store(false, Ordering::Release);
        let mut requests = self.requests.lock().unwrap_or_else(|e| e.into_inner());
        for request in requests.drain(..) {
            let _ = request.reply.send(Err(
                "audio: server disconnected before command completed".to_owned()
            ));
        }
    }
}

struct Worker {
    client: Arc<Client>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Worker {
    fn start(client: Arc<Client>, publisher: Publisher) -> Result<Self, String> {
        if client.started.swap(true, Ordering::AcqRel) {
            return Err("audio: native subscription already active".to_owned());
        }
        client.stopped.store(false, Ordering::Release);
        let owner = client.clone();
        match thread::Builder::new()
            .name("oxibar-pulse".to_owned())
            .spawn(move || serve(owner, publisher))
        {
            Ok(thread) => Ok(Self {
                client,
                thread: Some(thread),
            }),
            Err(error) => {
                client.started.store(false, Ordering::Release);
                Err(format!("audio: native worker failed: {error}"))
            }
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.client.stopped.store(true, Ordering::Release);
        self.client.wake();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        self.client.reject_pending();
        self.client.started.store(false, Ordering::Release);
    }
}

type Update = Result<DeviceSnapshot, String>;
type Mailbox = Arc<Mutex<Option<Update>>>;

struct Publisher {
    latest: Mailbox,
    wake: mpsc::Sender<()>,
    previous: Option<Update>,
}

impl Publisher {
    fn send(&mut self, update: Update) {
        if self.previous.as_ref() == Some(&update) {
            return;
        }
        self.previous = Some(update.clone());
        *self.latest.lock().unwrap_or_else(|e| e.into_inner()) = Some(update);
        // A full wake queue already guarantees delivery of the newest cached state.
        let _ = self.wake.try_send(());
    }
}

pub(crate) fn events(client: Arc<Client>) -> BoxStream<'static, Update> {
    let (wake, receiver) = mpsc::channel(1);
    let latest: Mailbox = Arc::new(Mutex::new(None));
    let publisher = Publisher {
        latest: latest.clone(),
        wake,
        previous: None,
    };
    match Worker::start(client, publisher) {
        Ok(worker) => stream::unfold(
            (worker, receiver, latest),
            |(worker, mut receiver, latest)| async move {
                loop {
                    receiver.next().await?;
                    let update = latest.lock().unwrap_or_else(|e| e.into_inner()).take();
                    if let Some(update) = update {
                        return Some((update, (worker, receiver, latest)));
                    }
                }
            },
        )
        .boxed(),
        Err(error) => stream::once(async { Err(error) }).boxed(),
    }
}

fn serve(client: Arc<Client>, mut publisher: Publisher) {
    while !client.stopped.load(Ordering::Acquire) {
        let result = Session::connect(client.clone()).and_then(|mut session| {
            session.subscribe()?;
            let mut cache = session.snapshot()?;
            client.ready.store(true, Ordering::Release);
            publisher.send(Ok(cache.devices.clone()));
            loop {
                if client.stopped.load(Ordering::Acquire) {
                    return Ok(());
                }
                session.check_state()?;
                if client.refresh.swap(false, Ordering::AcqRel) || session.dirty.replace(false) {
                    cache = session.snapshot()?;
                    publisher.send(Ok(cache.devices.clone()));
                    if session.dirty.get() || client.refresh.load(Ordering::Acquire) {
                        continue;
                    }
                }
                let request = client
                    .requests
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .pop_front();
                if let Some(request) = request {
                    let result = if request.issued.elapsed() > OPERATION_TIMEOUT {
                        Err("audio: queued command expired".to_owned())
                    } else {
                        session.action(&cache, &request.action)
                    };
                    let _ = request.reply.send(result);
                    session.dirty.set(true);
                } else {
                    session.iterate()?;
                }
            }
        });
        client.reject_pending();
        if client.stopped.load(Ordering::Acquire) {
            break;
        }
        if let Err(error) = result {
            publisher.send(Err(error));
        }
        let guard = client.retry_lock.lock().unwrap_or_else(|e| e.into_inner());
        let _ = client
            .retry
            .wait_timeout_while(guard, RECONNECT_DELAY, |_| {
                !client.stopped.load(Ordering::Acquire)
            });
    }
}

struct NativeDevice {
    index: u32,
    volume: ChannelVolumes,
}

#[derive(Default)]
struct Cache {
    devices: DeviceSnapshot,
    raw: BTreeMap<(DeviceKind, String), NativeDevice>,
}

struct Session {
    context: Context,
    _wake: IoEvent<MainloopInner<MainloopInternal>>,
    mainloop: Mainloop,
    client: Arc<Client>,
    dirty: Rc<Cell<bool>>,
}

impl Session {
    fn connect(client: Arc<Client>) -> Result<Self, String> {
        let mut mainloop = Mainloop::new().ok_or("audio: PulseAudio mainloop allocation failed")?;
        let mut read = client.wake_read.try_clone().map_err(|e| e.to_string())?;
        let wake = mainloop
            .new_io_event(
                read.as_raw_fd(),
                IoFlags::INPUT,
                Box::new(move |_, _, _| {
                    let mut bytes = [0; 128];
                    while matches!(read.read(&mut bytes), Ok(n) if n > 0) {}
                }),
            )
            .ok_or("audio: PulseAudio command wakeup allocation failed")?;
        let mut context = Context::new(&mainloop, "Oxibar Audio")
            .ok_or("audio: PulseAudio context allocation failed")?;
        context
            .connect(client.server.as_deref(), FlagSet::NOAUTOSPAWN, None)
            .map_err(|e| format!("audio: PulseAudio connect failed: {e}"))?;
        let mut session = Self {
            context,
            _wake: wake,
            mainloop,
            client,
            dirty: Rc::new(Cell::new(false)),
        };
        session.wait(|context| context.get_state() == State::Ready)?;
        Ok(session)
    }

    fn check_state(&self) -> Result<(), String> {
        if self.client.stopped.load(Ordering::Acquire) {
            Err("audio: subscription stopped".to_owned())
        } else if matches!(self.context.get_state(), State::Failed | State::Terminated) {
            Err(format!(
                "audio: PulseAudio disconnected: {}",
                self.context.errno()
            ))
        } else {
            Ok(())
        }
    }

    fn iterate(&mut self) -> Result<(), String> {
        match self.mainloop.iterate(true) {
            IterateResult::Success(_) => Ok(()),
            result => Err(format!("audio: PulseAudio mainloop failed: {result:?}")),
        }
    }

    fn wait(&mut self, done: impl Fn(&Context) -> bool) -> Result<(), String> {
        let expired = Rc::new(Cell::new(false));
        let deadline = expired.clone();
        let _timer = self
            .mainloop
            .new_timer_event_rt(
                MonotonicTs::now() + MicroSeconds(OPERATION_TIMEOUT.as_micros() as u64),
                Box::new(move |_| deadline.set(true)),
            )
            .ok_or("audio: PulseAudio deadline allocation failed")?;
        loop {
            self.check_state()?;
            if done(&self.context) {
                return Ok(());
            }
            if expired.get() {
                return Err("audio: PulseAudio operation timed out".to_owned());
            }
            self.iterate()?;
        }
    }

    fn finish(
        &mut self,
        mut operation: Operation<dyn FnMut(bool)>,
        result: Rc<Cell<Option<bool>>>,
    ) -> Result<(), String> {
        if let Err(error) = self.wait(|_| result.get().is_some()) {
            cancel_unfinished(&mut operation);
            return Err(error);
        }
        if result.get() == Some(true) {
            Ok(())
        } else {
            Err("audio: server rejected command".to_owned())
        }
    }

    fn subscribe(&mut self) -> Result<(), String> {
        let dirty = self.dirty.clone();
        self.context
            .set_subscribe_callback(Some(Box::new(move |facility, _, _| {
                if facility.is_some() {
                    dirty.set(true);
                }
            })));
        let result = Rc::new(Cell::new(None));
        let callback = result.clone();
        let operation = self.context.subscribe(
            InterestMaskSet::SINK | InterestMaskSet::SOURCE | InterestMaskSet::SERVER,
            move |success| callback.set(Some(success)),
        );
        self.finish(operation, result)
    }

    fn snapshot(&mut self) -> Result<Cache, String> {
        let cache = Rc::new(RefCell::new(Cache::default()));
        let done = Rc::new(Cell::new(0u8));
        let failed = Rc::new(Cell::new(false));
        let server_cache = cache.clone();
        let server_done = done.clone();
        let mut server = self.context.introspect().get_server_info(move |info| {
            let mut cache = server_cache.borrow_mut();
            cache.devices.default_output = info.default_sink_name.as_ref().map(|n| n.to_string());
            cache.devices.default_input = info.default_source_name.as_ref().map(|n| n.to_string());
            server_done.set(server_done.get() + 1);
        });
        let sink_cache = cache.clone();
        let sink_done = done.clone();
        let sink_failed = failed.clone();
        let mut sinks = self
            .context
            .introspect()
            .get_sink_info_list(move |info| match info {
                ListResult::Item(info) => {
                    if let Some(name) = info.name.as_deref() {
                        let mut cache = sink_cache.borrow_mut();
                        cache.devices.outputs.push(device(
                            name,
                            info.description.as_deref(),
                            &info.volume,
                            info.mute,
                        ));
                        cache.raw.insert(
                            (DeviceKind::Output, name.to_owned()),
                            NativeDevice {
                                index: info.index,
                                volume: info.volume,
                            },
                        );
                    }
                }
                ListResult::End => sink_done.set(sink_done.get() + 1),
                ListResult::Error => {
                    sink_failed.set(true);
                    sink_done.set(sink_done.get() + 1);
                }
            });
        let source_cache = cache.clone();
        let source_done = done.clone();
        let source_failed = failed.clone();
        let mut sources = self
            .context
            .introspect()
            .get_source_info_list(move |info| match info {
                ListResult::Item(info) if info.monitor_of_sink.is_none() => {
                    if let Some(name) = info.name.as_deref() {
                        let mut cache = source_cache.borrow_mut();
                        cache.devices.inputs.push(device(
                            name,
                            info.description.as_deref(),
                            &info.volume,
                            info.mute,
                        ));
                        cache.raw.insert(
                            (DeviceKind::Input, name.to_owned()),
                            NativeDevice {
                                index: info.index,
                                volume: info.volume,
                            },
                        );
                    }
                }
                ListResult::Item(_) => {}
                ListResult::End => source_done.set(source_done.get() + 1),
                ListResult::Error => {
                    source_failed.set(true);
                    source_done.set(source_done.get() + 1);
                }
            });
        let result = self.wait(|_| done.get() == 3);
        if result.is_err() {
            cancel_unfinished(&mut server);
            cancel_unfinished(&mut sinks);
            cancel_unfinished(&mut sources);
        }
        result?;
        if failed.get() {
            return Err("audio: PulseAudio device query failed".to_owned());
        }
        let mut cache = std::mem::take(&mut *cache.borrow_mut());
        sort_devices(
            &mut cache.devices.outputs,
            cache.devices.default_output.as_deref(),
        );
        sort_devices(
            &mut cache.devices.inputs,
            cache.devices.default_input.as_deref(),
        );
        Ok(cache)
    }

    fn action(&mut self, cache: &Cache, action: &DeviceAction) -> Result<(), String> {
        self.check_state()?;
        action.validate()?;
        let (kind, name) = action.target();
        let device = cache
            .raw
            .get(&(kind, name.to_owned()))
            .ok_or_else(|| format!("audio: device disappeared: {name}"))?;
        let result = Rc::new(Cell::new(None));
        let status = result.clone();
        let callback: Box<dyn FnMut(bool)> = Box::new(move |success| status.set(Some(success)));
        let operation = match action {
            DeviceAction::Default(DeviceKind::Output, name) => {
                self.context.set_default_sink(name, callback)
            }
            DeviceAction::Default(DeviceKind::Input, name) => {
                self.context.set_default_source(name, callback)
            }
            DeviceAction::Volume(kind, _, percent) => {
                let volume = scaled_volume(device.volume, *percent)?;
                match kind {
                    DeviceKind::Output => self.context.introspect().set_sink_volume_by_index(
                        device.index,
                        &volume,
                        Some(callback),
                    ),
                    DeviceKind::Input => self.context.introspect().set_source_volume_by_index(
                        device.index,
                        &volume,
                        Some(callback),
                    ),
                }
            }
            DeviceAction::Mute(DeviceKind::Output, _, muted) => self
                .context
                .introspect()
                .set_sink_mute_by_index(device.index, *muted, Some(callback)),
            DeviceAction::Mute(DeviceKind::Input, _, muted) => self
                .context
                .introspect()
                .set_source_mute_by_index(device.index, *muted, Some(callback)),
        };
        self.finish(operation, result)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.context.set_subscribe_callback(None);
        self.context.disconnect();
    }
}

fn cancel_unfinished<T: ?Sized>(operation: &mut Operation<T>) {
    if operation.get_state() != pulse::operation::State::Done {
        operation.cancel();
    }
}

fn percent(volume: &ChannelVolumes) -> u8 {
    ((u64::from(volume.max().0) * 100 + u64::from(Volume::NORMAL.0) / 2)
        / u64::from(Volume::NORMAL.0))
    .min(150) as u8
}

fn scaled_volume(mut volume: ChannelVolumes, percent: u8) -> Result<ChannelVolumes, String> {
    if !volume.is_valid() || volume.len() == 0 {
        return Err("audio: invalid channel volume".to_owned());
    }
    let target = Volume((u32::from(percent.min(150)) * Volume::NORMAL.0 + 50) / 100);
    if volume.max() == Volume::MUTED {
        volume.set(volume.len(), target);
    } else {
        volume
            .scale(target)
            .ok_or("audio: could not scale channel volume")?;
    }
    Ok(volume)
}

fn device(name: &str, label: Option<&str>, volume: &ChannelVolumes, muted: bool) -> AudioDevice {
    AudioDevice {
        name: name.to_owned(),
        label: label
            .filter(|label| !label.is_empty())
            .unwrap_or(name)
            .to_owned(),
        volume: percent(volume),
        muted,
    }
}

fn sort_devices(devices: &mut [AudioDevice], default: Option<&str>) {
    devices.sort_by(|a, b| {
        (default == Some(b.name.as_str()))
            .cmp(&(default == Some(a.name.as_str())))
            .then(a.label.cmp(&b.label))
            .then(a.name.cmp(&b.name))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_scaling_preserves_balance_and_handles_silence_and_cap() {
        let mut volume = ChannelVolumes::default();
        volume.set(2, Volume::NORMAL);
        volume.get_mut()[1] = Volume(Volume::NORMAL.0 / 2);
        let scaled = scaled_volume(volume, 150).unwrap();
        assert_eq!(percent(&scaled), 150);
        assert_eq!(scaled.get()[0].0, Volume::NORMAL.0 * 3 / 2);
        assert_eq!(scaled.get()[1].0, Volume::NORMAL.0 * 3 / 4);
        let silent = scaled_volume(volume, 0).unwrap();
        assert_eq!(percent(&silent), 0);
        assert_eq!(
            scaled_volume(silent, 200).unwrap().get()[1].0,
            Volume::NORMAL.0 * 3 / 2
        );
        assert!(scaled_volume(ChannelVolumes::default(), 50).is_err());
    }

    #[test]
    fn queued_slider_writes_are_bounded_and_never_replay_offline() {
        let client = Client::new().unwrap();
        assert!(
            iced::futures::executor::block_on(client.submit(DeviceAction::Volume(
                DeviceKind::Output,
                "sink".to_owned(),
                20
            )))
            .unwrap()
            .is_err()
        );
        client.ready.store(true, Ordering::Release);
        let old = client.submit(DeviceAction::Volume(
            DeviceKind::Output,
            "sink".to_owned(),
            20,
        ));
        let latest = client.submit(DeviceAction::Volume(
            DeviceKind::Output,
            "sink".to_owned(),
            80,
        ));
        assert!(iced::futures::executor::block_on(old).unwrap().is_ok());
        assert_eq!(client.requests.lock().unwrap().len(), 1);
        assert_eq!(
            client.requests.lock().unwrap()[0].action,
            DeviceAction::Volume(DeviceKind::Output, "sink".to_owned(), 80)
        );
        client.reject_pending();
        assert!(iced::futures::executor::block_on(latest).unwrap().is_err());
        assert!(client.requests.lock().unwrap().is_empty());
    }

    #[test]
    fn dropping_subscription_interrupts_reconnect_and_can_restart() {
        let client = Client::for_server(Some(
            "unix:/tmp/opencode/oxibar-no-such-pulse-server".to_owned(),
        ))
        .unwrap();
        let mut stream = events(client.clone());
        assert!(
            iced::futures::executor::block_on(stream.next())
                .unwrap()
                .is_err()
        );
        drop(stream);
        assert!(!client.started.load(Ordering::Acquire));
        let stream = events(client.clone());
        drop(stream);
        assert!(!client.started.load(Ordering::Acquire));
    }
}
