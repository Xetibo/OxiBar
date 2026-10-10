use std::{
    collections::BTreeMap,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use iced::futures::{
    FutureExt, Stream, StreamExt,
    channel::{mpsc, oneshot},
    future::{BoxFuture, Either, select},
    stream::{self, BoxStream, FuturesUnordered},
};
use zbus::{
    Connection, MatchRule, MessageStream, Proxy,
    message::{Sequence, Type},
    zvariant::{OwnedObjectPath, Value},
};

use super::{
    agent::{AGENT_PATH, Agent, Hub, PairEvent, Reply},
    system::{self, ADAPTER, Action, BLUEZ, DEVICE, OBJECT_MANAGER, Objects, PROPERTIES, Snapshot},
    worker::{self, RECONNECT_DELAY, REQUEST_TIMEOUT},
};

#[derive(Clone, Copy, Debug)]
pub(crate) struct Limits {
    pub(crate) discovery: Duration,
    pub(crate) pairing: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            discovery: Duration::from_secs(20),
            pairing: Duration::from_secs(90),
        }
    }
}

enum Command {
    Refresh,
    Action {
        action: Action,
        queued: Instant,
        reply: oneshot::Sender<Result<(), String>>,
    },
    Pair {
        id: u64,
        device: String,
        cancel: oneshot::Receiver<()>,
    },
}

pub(crate) struct Client {
    commands: Mutex<mpsc::Sender<Command>>,
    receiver: Mutex<Option<mpsc::Receiver<Command>>>,
    stop: Mutex<Option<oneshot::Sender<()>>>,
    ready: AtomicBool,
    started: AtomicBool,
    pub(crate) hub: Arc<Hub>,
    limits: Limits,
    address: Option<String>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlueZClient")
            .field("ready", &self.ready.load(Ordering::Acquire))
            .finish_non_exhaustive()
    }
}

impl Client {
    pub(crate) fn new(limits: Limits) -> Arc<Self> {
        Self::for_address(None, limits)
    }

    pub(crate) fn for_address(address: Option<String>, limits: Limits) -> Arc<Self> {
        let (sender, receiver) = mpsc::channel(64);
        Arc::new(Self {
            commands: Mutex::new(sender),
            receiver: Mutex::new(Some(receiver)),
            stop: Mutex::new(None),
            ready: AtomicBool::new(false),
            started: AtomicBool::new(false),
            hub: Hub::new(),
            limits,
            address,
        })
    }

    pub(crate) fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }

    pub(crate) fn refresh(&self) {
        let _ = self.commands.lock().unwrap().try_send(Command::Refresh);
    }

    pub(crate) fn submit(&self, action: Action) -> oneshot::Receiver<Result<(), String>> {
        let (reply, receiver) = oneshot::channel();
        let valid_path = action.path().starts_with("/org/bluez/")
            && OwnedObjectPath::try_from(action.path()).is_ok();
        let command = Command::Action {
            action,
            queued: Instant::now(),
            reply,
        };
        if !valid_path {
            reject(command, "bluetooth: invalid BlueZ object path");
        } else if !self.is_ready() {
            reject(command, "bluetooth: BlueZ is unavailable");
        } else if let Err(error) = self.commands.lock().unwrap().try_send(command) {
            reject(error.into_inner(), "bluetooth: command queue is full");
        }
        receiver
    }

    pub(crate) fn pair(
        self: &Arc<Self>,
        device: String,
    ) -> Result<(u64, BoxStream<'static, PairEvent>), String> {
        if !self.is_ready() {
            return Err("bluetooth: BlueZ is unavailable".to_owned());
        }
        let (id, receiver, cancel, completion) =
            self.hub.start(device.clone(), self.limits.pairing)?;
        if let Err(error) =
            self.commands
                .lock()
                .unwrap()
                .try_send(Command::Pair { id, device, cancel })
        {
            drop(error);
            self.hub
                .finish(id, Err("bluetooth: command queue is full".to_owned()));
        }
        Ok((
            id,
            Box::pin(PairStream {
                receiver,
                completion,
                hub: self.hub.clone(),
                id,
                finished: false,
            }),
        ))
    }

    pub(crate) fn respond(&self, id: u64, reply: Reply) -> Result<(), String> {
        self.hub.respond(id, reply)
    }
    pub(crate) fn cancel_pair(&self, id: Option<u64>) {
        self.hub.cancel(id);
    }
}

struct PairStream {
    receiver: mpsc::Receiver<PairEvent>,
    completion: oneshot::Receiver<Result<(), String>>,
    hub: Arc<Hub>,
    id: u64,
    finished: bool,
}

impl Stream for PairStream {
    type Item = PairEvent;
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        if this.finished {
            return Poll::Ready(None);
        }
        if let Poll::Ready(result) = Pin::new(&mut this.completion).poll(cx) {
            this.finished = true;
            return Poll::Ready(Some(PairEvent::Finished(
                result.unwrap_or_else(|_| Err("bluetooth: pairing owner stopped".to_owned())),
            )));
        }
        let event = Pin::new(&mut this.receiver).poll_next(cx);
        if matches!(event, Poll::Ready(None)) {
            this.finished = true;
            return Poll::Ready(Some(PairEvent::Finished(Err(
                "bluetooth: pairing owner stopped".to_owned(),
            ))));
        }
        event
    }
}

impl Drop for PairStream {
    fn drop(&mut self) {
        if !self.finished {
            self.hub.cancel(Some(self.id));
        }
    }
}

#[derive(Default)]
struct Mailbox {
    latest: Mutex<Option<Result<Snapshot, String>>>,
    last: Mutex<Option<Result<Snapshot, String>>>,
    wake: Mutex<Option<mpsc::Sender<()>>>,
}

impl Mailbox {
    fn publish(&self, state: Result<Snapshot, String>) {
        let mut last = self.last.lock().unwrap();
        if last.as_ref() == Some(&state) {
            return;
        }
        *last = Some(state.clone());
        *self.latest.lock().unwrap() = Some(state);
        if let Some(sender) = self.wake.lock().unwrap().as_mut() {
            let _ = sender.try_send(());
        }
    }
}

struct Worker {
    client: Arc<Client>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.client.ready.store(false, Ordering::Release);
        self.client.hub.cancel(None);
        if let Some(stop) = self.client.stop.lock().unwrap().take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        self.client.started.store(false, Ordering::Release);
    }
}

pub(crate) fn events(client: Arc<Client>) -> BoxStream<'static, Result<Snapshot, String>> {
    if client.started.swap(true, Ordering::AcqRel) {
        return stream::once(async { Err("bluetooth: native owner is already active".to_owned()) })
            .boxed();
    }
    let receiver = client.receiver.lock().unwrap().take().unwrap_or_else(|| {
        let (sender, receiver) = mpsc::channel(64);
        *client.commands.lock().unwrap() = sender;
        receiver
    });
    let (stop, stopped) = oneshot::channel();
    *client.stop.lock().unwrap() = Some(stop);
    let (wake, receiver_wake) = mpsc::channel(1);
    let mailbox = Arc::new(Mailbox::default());
    *mailbox.wake.lock().unwrap() = Some(wake);
    let worker_client = client.clone();
    let worker_mailbox = mailbox.clone();
    let worker = thread::Builder::new()
        .name("oxibar-bluez".to_owned())
        .spawn(move || {
            iced::futures::executor::block_on(run(
                worker_client,
                receiver,
                stopped,
                worker_mailbox,
            ));
        });
    let guard = match worker {
        Ok(thread) => Worker {
            client,
            thread: Some(thread),
        },
        Err(error) => {
            client.started.store(false, Ordering::Release);
            return stream::once(async move {
                Err(format!("bluetooth: failed to start native owner: {error}"))
            })
            .boxed();
        }
    };
    stream::unfold(
        (receiver_wake, mailbox, guard),
        |(mut receiver, mailbox, guard)| async move {
            loop {
                receiver.next().await?;
                let latest = mailbox.latest.lock().unwrap().take();
                if let Some(latest) = latest {
                    return Some((latest, (receiver, mailbox, guard)));
                }
            }
        },
    )
    .boxed()
}

async fn run(
    client: Arc<Client>,
    mut commands: mpsc::Receiver<Command>,
    stop: oneshot::Receiver<()>,
    mailbox: Arc<Mailbox>,
) {
    let mut stop = stop.fuse();
    loop {
        client.ready.store(false, Ordering::Release);
        while let Ok(command) = commands.try_recv() {
            reject(command, "bluetooth: BlueZ disconnected");
        }
        let connect = connect(&client).fuse();
        iced::futures::pin_mut!(connect);
        let connection = iced::futures::select! {
            result = connect => match result { Ok(connection) => connection, Err(error) => {
                mailbox.publish(Err(error));
                let delay = FutureExt::fuse(async_io::Timer::after(RECONNECT_DELAY));
                iced::futures::pin_mut!(delay);
                iced::futures::select! { _ = stop => break, _ = delay => continue }
            } },
            _ = stop => break,
        };
        let released = client.hub.listen_release();
        let session = {
            let initialization =
                Session::new(connection.clone(), client.clone(), mailbox.clone()).fuse();
            iced::futures::pin_mut!(initialization);
            iced::futures::select! { result = initialization => result, _ = stop => {
                client.hub.set_owner(None);
                let _ = connection.close().await;
                break;
            } }
        };
        match session {
            Ok(mut session) => {
                let result = {
                    let work = session.run(&mut commands, released).fuse();
                    iced::futures::pin_mut!(work);
                    iced::futures::select! { result = work => Some(result), _ = stop => None }
                };
                client.ready.store(false, Ordering::Release);
                session.cleanup().await;
                if let Some(result) = result {
                    mailbox.publish(Err(result
                        .err()
                        .unwrap_or_else(|| "bluetooth: native owner stopped".to_owned())));
                } else {
                    let _ = connection.close().await;
                    break;
                }
            }
            Err(error) => mailbox.publish(Err(error)),
        }
        client.hub.set_owner(None);
        let _ = connection.close().await;
        while let Ok(command) = commands.try_recv() {
            reject(command, "bluetooth: BlueZ disconnected");
        }
        let delay = FutureExt::fuse(async_io::Timer::after(RECONNECT_DELAY));
        iced::futures::pin_mut!(delay);
        iced::futures::select! { _ = stop => break, _ = delay => {} }
    }
    client.ready.store(false, Ordering::Release);
    client.hub.set_owner(None);
    if let Some((id, _)) = client.hub.active() {
        client
            .hub
            .finish(id, Err("bluetooth: pairing canceled".to_owned()));
    }
    while let Ok(command) = commands.try_recv() {
        reject(command, "bluetooth: native owner stopped");
    }
}

async fn connect(client: &Client) -> Result<Connection, String> {
    worker::deadline(
        async {
            let builder = match &client.address {
                Some(address) => zbus::connection::Builder::address(address.as_str()),
                None => zbus::connection::Builder::system(),
            }
            .map_err(|error| format!("bluetooth: system bus: {error}"))?;
            builder
                .method_timeout(client.limits.pairing)
                .serve_at(AGENT_PATH, Agent(client.hub.clone()))
                .map_err(|error| error.to_string())?
                .build()
                .await
                .map_err(|error| format!("bluetooth: system bus: {error}"))
        },
        REQUEST_TIMEOUT,
        "system bus connection",
    )
    .await
}

enum Completion {
    Action(
        Action,
        Result<(), String>,
        oneshot::Sender<Result<(), String>>,
    ),
    Pair(u64, Result<(), String>),
}

struct Session {
    connection: Connection,
    owner: String,
    client: Arc<Client>,
    mailbox: Arc<Mailbox>,
    signals: MessageStream,
    owners: MessageStream,
    objects: Objects,
    cursor: Sequence,
    scans: BTreeMap<String, Instant>,
}

impl Session {
    async fn new(
        connection: Connection,
        client: Arc<Client>,
        mailbox: Arc<Mailbox>,
    ) -> Result<Self, String> {
        worker::deadline(
            async {
                let bus = zbus::fdo::DBusProxy::new(&connection)
                    .await
                    .map_err(|error| error.to_string())?;
                let owner = bus
                    .get_name_owner(BLUEZ.try_into().unwrap())
                    .await
                    .map_err(|error| format!("bluetooth: BlueZ unavailable: {error}"))?
                    .to_string();
                let rule = MatchRule::builder()
                    .msg_type(Type::Signal)
                    .sender(owner.as_str())
                    .map_err(|error| error.to_string())?
                    .build();
                let signals = MessageStream::for_match_rule(rule, &connection, Some(1024))
                    .await
                    .map_err(|error| error.to_string())?;
                let owner_rule = MatchRule::builder()
                    .msg_type(Type::Signal)
                    .sender("org.freedesktop.DBus")
                    .unwrap()
                    .interface("org.freedesktop.DBus")
                    .unwrap()
                    .member("NameOwnerChanged")
                    .unwrap()
                    .build();
                let owners = MessageStream::for_match_rule(owner_rule, &connection, Some(64))
                    .await
                    .map_err(|error| error.to_string())?;
                let (objects, cursor) = read_objects(&connection, &owner).await?;
                client.hub.set_owner(Some(owner.clone()));
                let manager = Proxy::new(
                    &connection,
                    owner.as_str(),
                    "/org/bluez",
                    "org.bluez.AgentManager1",
                )
                .await
                .map_err(|error| error.to_string())?;
                manager
                    .call::<_, _, ()>(
                        "RegisterAgent",
                        &(
                            OwnedObjectPath::try_from(AGENT_PATH).unwrap(),
                            "KeyboardDisplay",
                        ),
                    )
                    .await
                    .map_err(|error| format!("bluetooth: register pairing agent: {error}"))?;
                let session = Self {
                    connection: connection.clone(),
                    owner,
                    client,
                    mailbox,
                    signals,
                    owners,
                    objects,
                    cursor,
                    scans: BTreeMap::new(),
                };
                session.client.ready.store(true, Ordering::Release);
                session.publish();
                Ok(session)
            },
            REQUEST_TIMEOUT,
            "BlueZ initialization",
        )
        .await
    }

    fn publish(&self) {
        let mut state = system::snapshot(&self.objects);
        for adapter in &mut state.adapters {
            adapter.scanning = self.scans.contains_key(&adapter.path);
        }
        self.mailbox.publish(Ok(state));
    }

    async fn run(
        &mut self,
        commands: &mut mpsc::Receiver<Command>,
        mut released: mpsc::UnboundedReceiver<()>,
    ) -> Result<(), String> {
        let mut operations: FuturesUnordered<BoxFuture<'static, Completion>> =
            FuturesUnordered::new();
        let mut action_active = false;
        loop {
            let scan_deadline = self.scans.values().min().copied();
            let scan_timer = async move {
                if let Some(deadline) = scan_deadline {
                    async_io::Timer::at(deadline).await;
                } else {
                    std::future::pending::<()>().await;
                }
            }
            .fuse();
            let command = async {
                if action_active {
                    std::future::pending::<Option<Command>>().await
                } else {
                    commands.next().await
                }
            }
            .fuse();
            let completion = async {
                if operations.is_empty() {
                    std::future::pending::<Option<Completion>>().await
                } else {
                    operations.next().await
                }
            }
            .fuse();
            iced::futures::pin_mut!(scan_timer, command, completion);
            iced::futures::select! {
                _ = released.next().fuse() => return Err("bluetooth: pairing agent released by BlueZ".to_owned()),
                owner = self.owners.next().fuse() => {
                    let message = owner.ok_or("bluetooth: system bus disconnected")?.map_err(|error| error.to_string())?;
                    let (name, _, current): (String, String, String) = message.body().deserialize().map_err(|error| error.to_string())?;
                    if name == BLUEZ && current != self.owner { return Err("bluetooth: BlueZ restarted or disconnected".to_owned()); }
                },
                signal = self.signals.next().fuse() => {
                    let message = signal.ok_or("bluetooth: system bus disconnected")?.map_err(|error| error.to_string())?;
                    if message.recv_position() <= self.cursor { continue; }
                    self.apply_signal(message).await?;
                    self.publish();
                },
                next = command => {
                    let Some(next) = next else { return Ok(()) };
                    match next {
                        Command::Refresh => {
                            (self.objects, self.cursor) = read_objects(&self.connection, &self.owner).await?;
                            self.publish();
                        }
                        Command::Action { action, queued, reply } => {
                            if queued.elapsed() > REQUEST_TIMEOUT {
                                let _ = reply.send(Err("bluetooth: queued command expired".to_owned()));
                                continue;
                            }
                            let state = system::snapshot(&self.objects);
                            let connection = self.connection.clone();
                            let owner = self.owner.clone();
                            action_active = true;
                            operations.push(async move {
                                let result = worker::deadline(execute(&connection, &owner, &state, &action), REQUEST_TIMEOUT, "BlueZ action").await;
                                Completion::Action(action, result, reply)
                            }.boxed());
                        }
                        Command::Pair { id, device, cancel } => {
                            if self.client.hub.active().is_none_or(|(active, _)| active != id) { continue; }
                            let state = system::snapshot(&self.objects);
                            let connection = self.connection.clone();
                            let owner = self.owner.clone();
                            let hub = self.client.hub.clone();
                            let timeout = self.client.limits.pairing;
                            operations.push(async move {
                                let pairing = worker::deadline(pair_device(&connection, &owner, &state, &device, &hub, id), timeout, "pairing");
                                let result = match select(Box::pin(pairing), cancel).await {
                                    Either::Left((result, _)) => result,
                                    Either::Right(_) => Err("bluetooth: pairing canceled".to_owned()),
                                };
                                if result.is_err() {
                                    let disconnect = hub.needs_disconnect(id);
                                    hub.cancel(Some(id));
                                    cancel_pairing(&connection, &owner, &device, disconnect).await;
                                }
                                Completion::Pair(id, result)
                            }.boxed());
                        }
                    }
                },
                next = completion => if let Some(next) = next {
                    match next {
                        Completion::Pair(id, result) => self.client.hub.finish(id, result),
                        Completion::Action(action, result, reply) => {
                            action_active = false;
                            if result.is_ok() {
                                match &action {
                                    Action::Discovery(path, true) => { self.scans.insert(path.clone(), Instant::now() + self.client.limits.discovery); }
                                    Action::Discovery(path, false) => { self.scans.remove(path); }
                                    Action::Power(path, powered) => {
                                        system::change(&mut self.objects, &OwnedObjectPath::try_from(path.as_str()).unwrap(), ADAPTER.to_owned(), std::collections::HashMap::from([("Powered".to_owned(), (*powered).into())]));
                                        if !powered { self.scans.remove(path); }
                                    }
                                    _ => {}
                                }
                                self.publish();
                            }
                            let _ = reply.send(result);
                        }
                    }
                },
                _ = scan_timer => {
                    let expired: Vec<_> = self.scans.iter().filter(|(_, deadline)| **deadline <= Instant::now()).map(|(path, _)| path.clone()).collect();
                    for path in expired {
                        let result = worker::deadline(call(&self.connection, &self.owner, &path, ADAPTER, "StopDiscovery"), REQUEST_TIMEOUT, "stop discovery").await;
                        self.scans.remove(&path);
                        result?;
                    }
                    self.publish();
                },
            }
        }
    }

    async fn apply_signal(&mut self, message: zbus::Message) -> Result<(), String> {
        let header = message.header();
        match (
            header.interface().map(|name| name.as_str()),
            header.member().map(|name| name.as_str()),
        ) {
            (Some(OBJECT_MANAGER), Some("InterfacesAdded")) => {
                let (path, interfaces) = message
                    .body()
                    .deserialize()
                    .map_err(|error| error.to_string())?;
                system::add(&mut self.objects, path, interfaces);
            }
            (Some(OBJECT_MANAGER), Some("InterfacesRemoved")) => {
                let (path, interfaces): (OwnedObjectPath, Vec<String>) = message
                    .body()
                    .deserialize()
                    .map_err(|error| error.to_string())?;
                system::remove(&mut self.objects, &path, &interfaces);
                self.scans.retain(|path, _| {
                    self.objects
                        .get(&OwnedObjectPath::try_from(path.as_str()).unwrap())
                        .is_some_and(|interfaces| interfaces.contains_key(ADAPTER))
                });
                if let Some((id, device)) = self.client.hub.active()
                    && system::snapshot(&self.objects).device(&device).is_none()
                {
                    self.client.hub.cancel(Some(id));
                }
            }
            (Some(PROPERTIES), Some("PropertiesChanged")) => {
                let (interface, properties, invalidated): (
                    String,
                    system::Properties,
                    Vec<String>,
                ) = message
                    .body()
                    .deserialize()
                    .map_err(|error| error.to_string())?;
                if ![ADAPTER, DEVICE].contains(&interface.as_str()) {
                    return Ok(());
                }
                if invalidated.is_empty() {
                    if let Some(path) = header.path() {
                        system::change(
                            &mut self.objects,
                            &path.to_owned().into(),
                            interface,
                            properties,
                        );
                    }
                } else {
                    (self.objects, self.cursor) =
                        read_objects(&self.connection, &self.owner).await?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn cleanup(&mut self) {
        self.client.hub.cancel(None);
        let active = self.client.hub.active();
        let jobs: FuturesUnordered<_> = self
            .scans
            .keys()
            .map(|path| {
                call(
                    &self.connection,
                    &self.owner,
                    path,
                    ADAPTER,
                    "StopDiscovery",
                )
            })
            .collect();
        if let Some((_, device)) = &active {
            jobs.push(call(
                &self.connection,
                &self.owner,
                device,
                DEVICE,
                "CancelPairing",
            ));
        }
        if let Some((id, device)) = &active
            && self.client.hub.needs_disconnect(*id)
        {
            jobs.push(call(
                &self.connection,
                &self.owner,
                device,
                DEVICE,
                "Disconnect",
            ));
        }
        let cleanup = async {
            jobs.collect::<Vec<_>>().await;
            let manager = Proxy::new(
                &self.connection,
                self.owner.as_str(),
                "/org/bluez",
                "org.bluez.AgentManager1",
            )
            .await
            .map_err(|error| error.to_string())?;
            manager
                .call::<_, _, ()>(
                    "UnregisterAgent",
                    &(OwnedObjectPath::try_from(AGENT_PATH).unwrap(),),
                )
                .await
                .map_err(|error| error.to_string())
        };
        let _ = worker::deadline(cleanup, Duration::from_secs(1), "BlueZ cleanup").await;
        if let Some((id, _)) = active {
            self.client
                .hub
                .finish(id, Err("bluetooth: pairing canceled".to_owned()));
        }
    }
}

async fn read_objects(connection: &Connection, owner: &str) -> Result<(Objects, Sequence), String> {
    worker::deadline(
        async {
            let reply = connection
                .call_method(
                    Some(owner),
                    "/",
                    Some(OBJECT_MANAGER),
                    "GetManagedObjects",
                    &(),
                )
                .await
                .map_err(|error| format!("bluetooth: device inventory: {error}"))?;
            Ok((
                reply
                    .body()
                    .deserialize()
                    .map_err(|error| error.to_string())?,
                reply.recv_position(),
            ))
        },
        REQUEST_TIMEOUT,
        "device inventory",
    )
    .await
}

async fn call(
    connection: &Connection,
    owner: &str,
    path: &str,
    interface: &str,
    method: &str,
) -> Result<(), String> {
    let proxy = Proxy::new(connection, owner, path, interface)
        .await
        .map_err(|error| error.to_string())?;
    proxy
        .call::<_, _, ()>(method, &())
        .await
        .map_err(|error| format!("bluetooth: {method}: {error}"))
}

async fn property(
    connection: &Connection,
    owner: &str,
    path: &str,
    interface: &str,
    name: &str,
    enabled: bool,
) -> Result<(), String> {
    let proxy = Proxy::new(connection, owner, path, PROPERTIES)
        .await
        .map_err(|error| error.to_string())?;
    proxy
        .call::<_, _, ()>("Set", &(interface, name, Value::from(enabled)))
        .await
        .map_err(|error| format!("bluetooth: {name}: {error}"))
}

async fn execute(
    connection: &Connection,
    owner: &str,
    state: &Snapshot,
    action: &Action,
) -> Result<(), String> {
    match action {
        Action::Power(path, enabled) => {
            state
                .adapter(path)
                .ok_or("bluetooth: adapter was removed")?;
            property(connection, owner, path, ADAPTER, "Powered", *enabled).await
        }
        Action::Discovery(path, enabled) => {
            let adapter = state
                .adapter(path)
                .ok_or("bluetooth: adapter was removed")?;
            if !adapter.powered {
                return Err("bluetooth: adapter is powered off".to_owned());
            }
            call(
                connection,
                owner,
                path,
                ADAPTER,
                if *enabled {
                    "StartDiscovery"
                } else {
                    "StopDiscovery"
                },
            )
            .await
        }
        Action::Disconnect(path) => {
            state.device(path).ok_or("bluetooth: device was removed")?;
            call(connection, owner, path, DEVICE, "Disconnect").await
        }
        Action::Trust(path, enabled) => {
            state.device(path).ok_or("bluetooth: device was removed")?;
            property(connection, owner, path, DEVICE, "Trusted", *enabled).await
        }
        Action::Forget(path) => {
            let device = state.device(path).ok_or("bluetooth: device was removed")?;
            let proxy = Proxy::new(connection, owner, device.adapter.as_str(), ADAPTER)
                .await
                .map_err(|error| error.to_string())?;
            proxy
                .call::<_, _, ()>(
                    "RemoveDevice",
                    &(OwnedObjectPath::try_from(path.as_str())
                        .map_err(|error| error.to_string())?,),
                )
                .await
                .map_err(|error| format!("bluetooth: RemoveDevice: {error}"))
        }
    }
}

async fn pair_device(
    connection: &Connection,
    owner: &str,
    state: &Snapshot,
    path: &str,
    hub: &Hub,
    id: u64,
) -> Result<(), String> {
    let device = state.device(path).ok_or("bluetooth: device was removed")?;
    let adapter = state
        .adapter(&device.adapter)
        .ok_or("bluetooth: adapter was removed")?;
    if !adapter.powered {
        return Err("bluetooth: adapter is powered off".to_owned());
    }
    if !device.paired {
        hub.progress(id, "Pairing");
        call(connection, owner, path, DEVICE, "Pair").await?;
    }
    property(connection, owner, path, DEVICE, "Trusted", true).await?;
    if device.connected {
        return Ok(());
    }
    hub.connecting(id)?;
    hub.progress(id, "Connecting");
    call(connection, owner, path, DEVICE, "Connect").await
}

async fn cancel_pairing(connection: &Connection, owner: &str, device: &str, disconnect: bool) {
    let cleanup = async {
        let cancel = call(connection, owner, device, DEVICE, "CancelPairing");
        let disconnect = async {
            if disconnect {
                let _ = call(connection, owner, device, DEVICE, "Disconnect").await;
            }
        };
        let _ = iced::futures::join!(cancel, disconnect);
        Ok(())
    };
    let _ = worker::deadline(cleanup, Duration::from_secs(1), "cancel pairing").await;
}

fn reject(command: Command, reason: &str) {
    if let Command::Action { reply, .. } = command {
        let _ = reply.send(Err(reason.to_owned()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_commands_are_rejected_and_owner_drop_interrupts_reconnect() {
        let client = Client::for_address(
            Some("unix:path=/tmp/oxibar-bluez-missing-test-bus".to_owned()),
            Limits::default(),
        );
        let result = iced::futures::executor::block_on(
            client.submit(Action::Power("/org/bluez/hci0".to_owned(), true)),
        )
        .unwrap();
        assert!(result.is_err());
        let stream = events(client.clone());
        let start = Instant::now();
        drop(stream);
        assert!(start.elapsed() < Duration::from_secs(1));
        assert!(!client.started.load(Ordering::Acquire));
        drop(events(client));
    }
}
