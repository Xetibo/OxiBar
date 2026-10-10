use std::{
    collections::{BTreeMap, BTreeSet, HashMap, VecDeque},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use iced::futures::{
    FutureExt, StreamExt,
    channel::{mpsc, oneshot},
    future::{LocalBoxFuture, pending},
    stream::{BoxStream, FuturesUnordered},
};
use notify::Watcher;
use zbus::{
    Connection, MessageStream,
    message::Type,
    zvariant::{OwnedObjectPath, OwnedValue},
};

use crate::{
    system::{self, Action, Capability, Domain, Limits, Session, SessionAction, Snapshot},
    worker,
};

const LOGIN: &str = "org.freedesktop.login1";
const LOGIN_PATH: &str = "/org/freedesktop/login1";
const MANAGER: &str = "org.freedesktop.login1.Manager";
const SESSION: &str = "org.freedesktop.login1.Session";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const DEADLINE: Duration = Duration::from_secs(3);
type Properties = HashMap<String, OwnedValue>;
type Response = oneshot::Sender<Result<Snapshot, String>>;

#[derive(Debug)]
struct Request {
    action: Action,
    replies: Vec<Response>,
    queued: Instant,
    epoch: u64,
}

#[derive(Debug)]
pub struct Client {
    limits: Limits,
    address: Option<String>,
    queue: Mutex<VecDeque<Request>>,
    wake: Mutex<mpsc::Sender<()>>,
    receiver: Mutex<Option<mpsc::Receiver<()>>>,
    stop: Mutex<Option<oneshot::Sender<()>>>,
    pub ready: AtomicBool,
    started: AtomicBool,
    refresh: AtomicBool,
    epoch: AtomicU64,
    profile_epoch: AtomicU64,
    session_epoch: AtomicU64,
}

impl Client {
    pub fn new(limits: Limits) -> Arc<Self> {
        Self::for_address(limits, None)
    }
    pub(super) fn for_address(limits: Limits, address: Option<String>) -> Arc<Self> {
        let (wake, receiver) = mpsc::channel(1);
        Arc::new(Self {
            limits,
            address,
            queue: Mutex::new(VecDeque::new()),
            wake: Mutex::new(wake),
            receiver: Mutex::new(Some(receiver)),
            stop: Mutex::new(None),
            ready: AtomicBool::new(false),
            started: AtomicBool::new(false),
            refresh: AtomicBool::new(false),
            epoch: AtomicU64::new(0),
            profile_epoch: AtomicU64::new(0),
            session_epoch: AtomicU64::new(0),
        })
    }
    pub fn submit(&self, action: Action) -> oneshot::Receiver<Result<Snapshot, String>> {
        let (sender, receiver) = oneshot::channel();
        if let Err(error) = action.validate() {
            let _ = sender.send(Err(error));
            return receiver;
        }
        if !self.ready.load(Ordering::Acquire) {
            let _ = sender.send(Err("System backend is not ready".to_owned()));
            return receiver;
        }
        let mut queue = self.queue.lock().unwrap_or_else(|error| error.into_inner());
        if queue.iter().map(|item| item.replies.len()).sum::<usize>() >= 64 {
            let _ = sender.send(Err("System request queue is full".to_owned()));
            return receiver;
        }
        if let Action::Brightness(name, _) = &action
            && let Some(request) = queue
                .iter_mut()
                .rev()
                .find(|item| matches!(&item.action, Action::Brightness(old, _) if old == name))
        {
            request.action = action;
            request.replies.push(sender);
            request.queued = Instant::now();
        } else {
            let epoch = self.domain_epoch(action.domain());
            queue.push_back(Request {
                action,
                replies: vec![sender],
                queued: Instant::now(),
                epoch,
            });
        }
        drop(queue);
        self.wake();
        receiver
    }
    pub fn refresh(&self) {
        self.refresh.store(true, Ordering::Release);
        self.wake();
    }
    fn domain_epoch(&self, domain: Domain) -> u64 {
        match domain {
            Domain::Brightness => 0,
            Domain::Profile => self.profile_epoch.load(Ordering::Acquire),
            Domain::Session => self.session_epoch.load(Ordering::Acquire),
        }
    }
    fn bus_changed(&self) {
        self.epoch.fetch_add(1, Ordering::AcqRel);
        self.profile_epoch.fetch_add(1, Ordering::AcqRel);
        self.session_epoch.fetch_add(1, Ordering::AcqRel);
    }
    fn wake(&self) {
        let _ = self
            .wake
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .try_send(());
    }
    fn reject_pending(&self, error: &str) {
        for request in self
            .queue
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .drain(..)
        {
            for reply in request.replies {
                let _ = reply.send(Err(error.to_owned()));
            }
        }
    }
}

#[derive(Debug)]
struct Worker {
    client: Arc<Client>,
    join: Option<std::thread::JoinHandle<()>>,
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.client.ready.store(false, Ordering::Release);
        if let Some(stop) = self
            .client
            .stop
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
        {
            let _ = stop.send(());
        }
        self.client.wake();
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
        self.client.reject_pending("System backend stopped");
        self.client.started.store(false, Ordering::Release);
    }
}

struct Mailbox {
    latest: Mutex<Option<Snapshot>>,
    sender: Mutex<mpsc::Sender<()>>,
}
impl Mailbox {
    fn publish(&self, snapshot: Snapshot) {
        *self
            .latest
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(snapshot);
        let _ = self
            .sender
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .try_send(());
    }
}

pub fn events(client: Arc<Client>) -> BoxStream<'static, Snapshot> {
    if client.started.swap(true, Ordering::AcqRel) {
        return iced::futures::stream::empty().boxed();
    }
    let (sender, receiver) = mpsc::channel(1);
    let mailbox = Arc::new(Mailbox {
        latest: Mutex::new(None),
        sender: Mutex::new(sender),
    });
    let (stop, stop_receiver) = oneshot::channel();
    *client
        .stop
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = Some(stop);
    let wake = client
        .receiver
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .take()
        .unwrap_or_else(|| {
            let (sender, receiver) = mpsc::channel(1);
            *client
                .wake
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = sender;
            receiver
        });
    let owner = client.clone();
    let updates = mailbox.clone();
    let join = std::thread::Builder::new()
        .name("oxibar-system".to_owned())
        .spawn(move || {
            iced::futures::executor::block_on(run(owner, updates, wake, stop_receiver));
        });
    let join = match join {
        Ok(join) => Some(join),
        Err(error) => {
            let error = format!("System worker could not start: {error}");
            mailbox.publish(Snapshot {
                brightness_error: Some(error.clone()),
                profile_error: Some(error.clone()),
                session_error: Some(error),
                ..Snapshot::default()
            });
            None
        }
    };
    let worker = Worker { client, join };
    iced::futures::stream::unfold(
        (worker, receiver, mailbox),
        |(worker, mut receiver, mailbox)| async move {
            loop {
                receiver.next().await?;
                if let Some(snapshot) = mailbox
                    .latest
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .take()
                {
                    return Some((snapshot, (worker, receiver, mailbox.clone())));
                }
            }
        },
    )
    .boxed()
}

#[derive(Debug, Clone)]
struct Target {
    owner: String,
    path: String,
    interface: String,
}
#[derive(Debug, Clone)]
struct Remote {
    profile: Option<Target>,
    login: Option<Target>,
    profiles: Vec<String>,
    active: Option<String>,
    degraded: Option<String>,
    profile_error: Option<String>,
    session: Option<Session>,
    capabilities: BTreeMap<SessionAction, Capability>,
    session_error: Option<String>,
}
struct Bus {
    connection: Connection,
    signals: MessageStream,
    remote: Option<Remote>,
}

async fn connect(address: Option<&str>) -> Result<Bus, String> {
    let builder = match address {
        Some(address) => zbus::connection::Builder::address(address),
        None => zbus::connection::Builder::system(),
    }
    .map_err(|error| error.to_string())?;
    let connection = builder
        .method_timeout(Duration::from_secs(30))
        .build()
        .await
        .map_err(|error| error.to_string())?;
    let rule = zbus::MatchRule::builder().msg_type(Type::Signal).build();
    let signals = MessageStream::for_match_rule(rule, &connection, Some(256))
        .await
        .map_err(|error| error.to_string())?;
    Ok(Bus {
        connection,
        signals,
        remote: None,
    })
}

async fn properties(connection: &Connection, target: &Target) -> Result<Properties, String> {
    connection
        .call_method(
            Some(target.owner.as_str()),
            target.path.as_str(),
            Some(PROPERTIES),
            "GetAll",
            &(target.interface.as_str()),
        )
        .await
        .map_err(|error| error.to_string())?
        .body()
        .deserialize()
        .map_err(|error| error.to_string())
}
fn text(properties: &Properties, name: &str) -> Option<String> {
    properties
        .get(name)
        .and_then(|value| <&str>::try_from(value).ok())
        .map(str::to_owned)
}

struct ProfileState {
    target: Target,
    profiles: Vec<String>,
    active: Option<String>,
    degraded: Option<String>,
}
struct LoginState {
    target: Target,
    session: Option<Session>,
    capabilities: BTreeMap<SessionAction, Capability>,
    error: Option<String>,
}

async fn query_profiles(connection: &Connection) -> Result<ProfileState, String> {
    let dbus = zbus::fdo::DBusProxy::new(connection)
        .await
        .map_err(|error| error.to_string())?;
    let mut error = "Power-profile service is unavailable".to_owned();
    for (name, path) in [
        (
            "org.freedesktop.UPower.PowerProfiles",
            "/org/freedesktop/UPower/PowerProfiles",
        ),
        ("net.hadess.PowerProfiles", "/net/hadess/PowerProfiles"),
    ] {
        let Ok(owner) = dbus
            .get_name_owner(
                name.try_into()
                    .map_err(|error: zbus::names::Error| error.to_string())?,
            )
            .await
        else {
            continue;
        };
        let target = Target {
            owner: owner.to_string(),
            path: path.to_owned(),
            interface: name.to_owned(),
        };
        match properties(connection, &target).await {
            Ok(values) => {
                let profiles = values
                    .get("Profiles")
                    .and_then(|value| value.try_clone().ok())
                    .and_then(|value| Vec::<Properties>::try_from(value).ok())
                    .unwrap_or_default();
                let mut profiles: Vec<_> = profiles
                    .iter()
                    .filter_map(|value| text(value, "Profile"))
                    .filter(|value| !value.is_empty() && value.len() <= 128)
                    .collect();
                profiles.sort();
                profiles.dedup();
                return Ok(ProfileState {
                    target,
                    profiles,
                    active: text(&values, "ActiveProfile"),
                    degraded: text(&values, "PerformanceDegraded")
                        .filter(|value| !value.is_empty()),
                });
            }
            Err(failed) => error = failed,
        }
    }
    Err(error)
}

async fn query_login(connection: &Connection, limits: &Limits) -> Result<LoginState, String> {
    let dbus = zbus::fdo::DBusProxy::new(connection)
        .await
        .map_err(|error| error.to_string())?;
    let owner = dbus
        .get_name_owner(
            LOGIN
                .try_into()
                .map_err(|error: zbus::names::Error| error.to_string())?,
        )
        .await
        .map_err(|error| error.to_string())?;
    let mut capabilities = BTreeMap::new();
    let mut session = None;
    let mut error = None;
    let target = Target {
        owner: owner.to_string(),
        path: LOGIN_PATH.to_owned(),
        interface: MANAGER.to_owned(),
    };
    let proxy = zbus::Proxy::new(connection, target.owner.as_str(), LOGIN_PATH, MANAGER)
        .await
        .map_err(|error| error.to_string())?;
    for action in [
        SessionAction::Suspend,
        SessionAction::Hibernate,
        SessionAction::Reboot,
        SessionAction::PowerOff,
    ] {
        let method = format!("Can{}", action.method());
        let value: Result<String, _> = proxy.call(method.as_str(), &()).await;
        capabilities.insert(
            action,
            value
                .as_deref()
                .map(Capability::parse)
                .unwrap_or(Capability::Unavailable),
        );
    }
    let path: Result<OwnedObjectPath, _> = match &limits.session_id {
        Some(id) => proxy.call("GetSession", &(id.as_str())).await,
        None => proxy.call("GetSessionByPID", &(std::process::id())).await,
    };
    match path {
        Ok(path) => {
            let session_target = Target {
                owner: target.owner.clone(),
                path: path.to_string(),
                interface: SESSION.to_owned(),
            };
            match owned_session(connection, &session_target, None).await {
                Ok(owned) => {
                    session = Some(owned);
                    capabilities.insert(SessionAction::Logout, Capability::Yes);
                }
                Err(failed) => error = Some(failed),
            }
        }
        Err(failed) => error = Some(failed.to_string()),
    }
    Ok(LoginState {
        target,
        session,
        capabilities,
        error,
    })
}

async fn query_remote(connection: &Connection, limits: &Limits) -> Result<Remote, String> {
    let (profiles, login) = iced::futures::join!(
        worker::bounded(query_profiles(connection), DEADLINE),
        worker::bounded(query_login(connection, limits), DEADLINE)
    );
    let mut remote = Remote {
        profile: None,
        login: None,
        profiles: Vec::new(),
        active: None,
        degraded: None,
        profile_error: None,
        session: None,
        capabilities: BTreeMap::new(),
        session_error: None,
    };
    match profiles {
        Ok(profiles) => {
            remote.profile = Some(profiles.target);
            remote.profiles = profiles.profiles;
            remote.active = profiles.active;
            remote.degraded = profiles.degraded;
        }
        Err(error) => remote.profile_error = Some(error),
    }
    match login {
        Ok(login) => {
            remote.login = Some(login.target);
            remote.session = login.session;
            remote.capabilities = login.capabilities;
            remote.session_error = login.error;
        }
        Err(error) => remote.session_error = Some(error),
    }
    Ok(remote)
}

async fn owned_session(
    connection: &Connection,
    target: &Target,
    expected: Option<&Session>,
) -> Result<Session, String> {
    let values = properties(connection, target).await?;
    let user = values
        .get("User")
        .and_then(|value| value.try_clone().ok())
        .and_then(|value| <(u32, OwnedObjectPath)>::try_from(value).ok())
        .ok_or("Login session has no valid owner")?;
    let id = text(&values, "Id").ok_or("Login session has no ID")?;
    if user.0 != rustix::process::getuid().as_raw()
        || expected.is_some_and(|session| session.id != id)
    {
        return Err("Login session does not belong to this user/session".to_owned());
    }
    Ok(Session {
        id,
        path: target.path.clone(),
        uid: user.0,
    })
}

enum Delta {
    Brightness(Vec<system::Backlight>),
    Profile(Properties),
    Session,
}
enum Event {
    Connected(Result<Box<Bus>, String>),
    Remote(u64, Result<Box<Remote>, String>),
    Action(Request, Result<Delta, String>),
}

async fn action(
    connection: Option<Connection>,
    remote: Option<Remote>,
    limits: Limits,
    snapshot: Snapshot,
    request: &Action,
) -> Result<Delta, String> {
    match request {
        Action::Brightness(name, percent) => {
            let device = system::read_backlight(&limits.root, name)?;
            let raw = device.raw(*percent);
            let session = remote.as_ref().and_then(|remote| remote.session.as_ref());
            let login = remote.as_ref().and_then(|remote| remote.login.as_ref());
            if let (Some(connection), Some(session), Some(login)) = (&connection, session, login) {
                let target = Target {
                    owner: login.owner.clone(),
                    path: session.path.clone(),
                    interface: SESSION.to_owned(),
                };
                owned_session(connection, &target, Some(session)).await?;
                let result = connection
                    .call_method(
                        Some(target.owner.as_str()),
                        target.path.as_str(),
                        Some(SESSION),
                        "SetBrightness",
                        &("backlight", name.as_str(), raw),
                    )
                    .await;
                if let Err(error) = result {
                    let fallback = matches!(&error, zbus::Error::MethodError(name, _, _) if matches!(name.as_str(), "org.freedesktop.DBus.Error.UnknownMethod" | "org.freedesktop.DBus.Error.ServiceUnknown" | "org.freedesktop.DBus.Error.UnknownObject"));
                    if !fallback {
                        return Err(error.to_string());
                    }
                    system::write_brightness(&limits.root, name, raw)?;
                }
            } else {
                system::write_brightness(&limits.root, name, raw)?;
            }
            Ok(Delta::Brightness(system::read_backlights(&limits.root)?))
        }
        Action::Profile(profile) => {
            let target = remote
                .as_ref()
                .and_then(|remote| remote.profile.as_ref())
                .ok_or("Power-profile service is unavailable")?;
            if !snapshot.profiles.contains(profile) {
                return Err("Power profile is not advertised".to_owned());
            }
            let connection = connection.ok_or("System bus is unavailable")?;
            connection
                .call_method(
                    Some(target.owner.as_str()),
                    target.path.as_str(),
                    Some(PROPERTIES),
                    "Set",
                    &(
                        target.interface.as_str(),
                        "ActiveProfile",
                        zbus::zvariant::Value::from(profile.as_str()),
                    ),
                )
                .await
                .map_err(|error| error.to_string())?;
            Ok(Delta::Profile(properties(&connection, target).await?))
        }
        Action::Session(action) => {
            if !snapshot.can(*action) {
                return Err("Session action is unavailable".to_owned());
            }
            let remote = remote.ok_or("Login service is unavailable")?;
            let login = remote.login.ok_or("Login service is unavailable")?;
            let connection = connection.ok_or("System bus is unavailable")?;
            if *action == SessionAction::Logout {
                let session = remote.session.ok_or("Own login session is unavailable")?;
                let target = Target {
                    owner: login.owner.clone(),
                    path: session.path.clone(),
                    interface: SESSION.to_owned(),
                };
                owned_session(&connection, &target, Some(&session)).await?;
                connection
                    .call_method(
                        Some(login.owner.as_str()),
                        session.path.as_str(),
                        Some(SESSION),
                        "Terminate",
                        &(),
                    )
                    .await
                    .map_err(|error| error.to_string())?;
            } else {
                connection
                    .call_method(
                        Some(login.owner.as_str()),
                        LOGIN_PATH,
                        Some(MANAGER),
                        action.method(),
                        &(true),
                    )
                    .await
                    .map_err(|error| error.to_string())?;
            }
            Ok(Delta::Session)
        }
    }
}

fn apply_remote(snapshot: &mut Snapshot, remote: &Remote) {
    snapshot.profile_ready = remote.profile.is_some();
    snapshot.login_ready = remote.login.is_some();
    if remote.profile.is_some() {
        snapshot.profiles = remote.profiles.clone();
        snapshot.active_profile = remote.active.clone();
        snapshot.degraded = remote.degraded.clone();
    }
    snapshot.profile_error = remote.profile_error.clone();
    snapshot.session_error = remote.session_error.clone();
    snapshot.session = remote.session.clone();
    snapshot.capabilities = remote.capabilities.clone();
}
fn refresh_files(snapshot: &mut Snapshot, limits: &Limits) {
    match system::read_backlights(&limits.root) {
        Ok(devices) => {
            snapshot.backlights = devices;
            snapshot.brightness_error = None;
        }
        Err(error) => {
            snapshot.brightness_error = Some(error);
        }
    }
}
fn publish(snapshot: &mut Snapshot, last: &mut Option<Snapshot>, mailbox: &Mailbox) {
    if last.as_ref().is_some_and(|last| last == snapshot) {
        return;
    }
    snapshot.sequence = snapshot.sequence.wrapping_add(1);
    *last = Some(snapshot.clone());
    mailbox.publish(snapshot.clone());
}

fn signal_relevant(message: &zbus::Message, remote: Option<&Remote>) -> bool {
    let header = message.header();
    if header
        .interface()
        .is_some_and(|name| name.as_str() == "org.freedesktop.DBus")
        && header
            .member()
            .is_some_and(|name| name.as_str() == "NameOwnerChanged")
    {
        if header
            .sender()
            .is_none_or(|name| name.as_str() != "org.freedesktop.DBus")
        {
            return false;
        }
        return message
            .body()
            .deserialize::<(String, String, String)>()
            .is_ok_and(|(name, _, _)| {
                matches!(
                    name.as_str(),
                    LOGIN | "org.freedesktop.UPower.PowerProfiles" | "net.hadess.PowerProfiles"
                )
            });
    }
    if let Some(remote) = remote {
        let sender = header.sender().map(|sender| sender.as_str());
        if [remote.login.as_ref(), remote.profile.as_ref()]
            .into_iter()
            .flatten()
            .any(|target| sender == Some(target.owner.as_str()))
        {
            return true;
        }
    }
    false
}

async fn run(
    client: Arc<Client>,
    mailbox: Arc<Mailbox>,
    mut wake: mpsc::Receiver<()>,
    stop: oneshot::Receiver<()>,
) {
    let mut stop = stop.fuse();
    let mut snapshot = Snapshot::default();
    let mut last = None;
    refresh_files(&mut snapshot, &client.limits);
    publish(&mut snapshot, &mut last, &mailbox);
    client.ready.store(true, Ordering::Release);
    let (file_sender, mut file_receiver) = mpsc::channel(1);
    let file_sender = Arc::new(Mutex::new(file_sender));
    let updates = file_sender.clone();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if event.is_ok_and(|event| !matches!(event.kind, notify::EventKind::Access(_))) {
            let _ = updates
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .try_send(());
        }
    })
    .ok();
    let mut watched = BTreeSet::<PathBuf>::new();
    let mut bus: Option<Bus> = None;
    let mut operations = FuturesUnordered::<LocalBoxFuture<'static, Event>>::new();
    let mut connecting = false;
    let mut refreshing = None;
    let mut remote_dirty = false;
    let mut busy = BTreeSet::new();
    let mut next_poll = Instant::now() + client.limits.poll;
    let mut retry = Instant::now();
    loop {
        if let Some(watcher) = &mut watcher {
            let paths: BTreeSet<_> = std::iter::once(client.limits.root.clone())
                .chain(
                    snapshot
                        .backlights
                        .iter()
                        .map(|device| client.limits.root.join(&device.name)),
                )
                .collect();
            let mut next_watched = watched
                .intersection(&paths)
                .cloned()
                .collect::<BTreeSet<_>>();
            for path in paths.difference(&watched) {
                if watcher
                    .watch(path, notify::RecursiveMode::NonRecursive)
                    .is_ok()
                {
                    next_watched.insert(path.clone());
                }
            }
            for path in watched.difference(&paths) {
                let _ = watcher.unwatch(path);
            }
            watched = next_watched;
        }
        if bus.is_none() && !connecting && Instant::now() >= retry {
            connecting = true;
            let address = client.address.clone();
            operations.push(
                async move {
                    Event::Connected(
                        worker::bounded(connect(address.as_deref()), DEADLINE)
                            .await
                            .map(Box::new),
                    )
                }
                .boxed_local(),
            );
        }
        if remote_dirty
            && refreshing.is_none()
            && let Some(bus) = &bus
        {
            remote_dirty = false;
            let connection = bus.connection.clone();
            let limits = client.limits.clone();
            let epoch = client.epoch.load(Ordering::Acquire);
            refreshing = Some(epoch);
            operations.push(
                async move {
                    Event::Remote(
                        epoch,
                        query_remote(&connection, &limits).await.map(Box::new),
                    )
                }
                .boxed_local(),
            );
        }
        while busy.len() < 3 {
            let request = {
                let mut queue = client
                    .queue
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                queue
                    .iter()
                    .position(|request| !busy.contains(&request.action.domain()))
                    .and_then(|index| queue.remove(index))
            };
            let Some(request) = request else {
                break;
            };
            if request.queued.elapsed() > DEADLINE
                || request.epoch != client.domain_epoch(request.action.domain())
            {
                for reply in request.replies {
                    let _ = reply.send(Err("System request expired or service changed".to_owned()));
                }
                continue;
            }
            busy.insert(request.action.domain());
            let connection = bus.as_ref().map(|bus| bus.connection.clone());
            let remote = bus.as_ref().and_then(|bus| bus.remote.clone());
            let limits = client.limits.clone();
            let state = snapshot.clone();
            let timeout = if matches!(request.action, Action::Session(_)) {
                Duration::from_secs(30)
            } else {
                DEADLINE
            };
            operations.push(
                async move {
                    let result = worker::bounded(
                        action(connection, remote, limits, state, &request.action),
                        timeout,
                    )
                    .await;
                    Event::Action(request, result)
                }
                .boxed_local(),
            );
        }
        enum Selected {
            Stop,
            Signal(Option<Result<zbus::Message, zbus::Error>>),
            Complete(Option<Event>),
            Wake,
            Files,
            Timer,
        }
        let selected = {
            let deadline = if bus.is_none() && !connecting {
                next_poll.min(retry)
            } else {
                next_poll
            };
            let mut signal = match &mut bus {
                Some(bus) => bus.signals.next().boxed_local(),
                None => pending().boxed_local(),
            }
            .fuse();
            let mut complete = if operations.is_empty() {
                pending().boxed_local()
            } else {
                operations.next().boxed_local()
            }
            .fuse();
            let mut command = wake.next().fuse();
            let mut files = file_receiver.next().fuse();
            let mut timer = FutureExt::fuse(async_io::Timer::at(deadline));
            iced::futures::select_biased! { _ = stop => Selected::Stop, value = signal => Selected::Signal(value), value = complete => Selected::Complete(value), _ = command => Selected::Wake, _ = files => Selected::Files, _ = timer => Selected::Timer }
        };
        match selected {
            Selected::Stop => break,
            Selected::Complete(Some(Event::Connected(result))) => {
                connecting = false;
                client.bus_changed();
                match result {
                    Ok(connection) => {
                        bus = Some(*connection);
                        remote_dirty = true;
                    }
                    Err(error) => {
                        snapshot.profile_ready = false;
                        snapshot.login_ready = false;
                        snapshot.profile_error = Some(error.clone());
                        snapshot.session_error = Some(error);
                        retry = Instant::now() + Duration::from_secs(2);
                    }
                }
            }
            Selected::Complete(Some(Event::Remote(epoch, result))) => {
                if refreshing == Some(epoch) {
                    refreshing = None;
                }
                if epoch == client.epoch.load(Ordering::Acquire) {
                    match result {
                        Ok(remote) => {
                            apply_remote(&mut snapshot, &remote);
                            if let Some(bus) = &mut bus {
                                bus.remote = Some(*remote);
                            }
                        }
                        Err(error) => {
                            snapshot.profile_ready = false;
                            snapshot.login_ready = false;
                            snapshot.profile_error = Some(error.clone());
                            snapshot.session_error = Some(error);
                        }
                    }
                } else {
                    remote_dirty = true;
                }
            }
            Selected::Complete(Some(Event::Action(request, result))) => {
                busy.remove(&request.action.domain());
                let result = if request.epoch != client.domain_epoch(request.action.domain()) {
                    Err("System service changed while request was running".to_owned())
                } else {
                    result
                };
                let result = result.map(|delta| {
                    match delta {
                        Delta::Brightness(devices) => {
                            snapshot.backlights = devices;
                            snapshot.brightness_error = None;
                        }
                        Delta::Profile(values) => {
                            client.epoch.fetch_add(1, Ordering::AcqRel);
                            remote_dirty = true;
                            snapshot.active_profile = text(&values, "ActiveProfile");
                            snapshot.degraded = text(&values, "PerformanceDegraded")
                                .filter(|value| !value.is_empty());
                        }
                        Delta::Session => {}
                    }
                    publish(&mut snapshot, &mut last, &mailbox);
                    snapshot.clone()
                });
                for reply in request.replies {
                    let _ = reply.send(result.clone());
                }
            }
            Selected::Signal(Some(Ok(message))) => {
                if signal_relevant(&message, bus.as_ref().and_then(|bus| bus.remote.as_ref())) {
                    client.epoch.fetch_add(1, Ordering::AcqRel);
                    if message
                        .header()
                        .member()
                        .is_some_and(|name| name.as_str() == "NameOwnerChanged")
                    {
                        let login = message
                            .body()
                            .deserialize::<(String, String, String)>()
                            .is_ok_and(|(name, _, _)| name == LOGIN);
                        if login {
                            client.session_epoch.fetch_add(1, Ordering::AcqRel);
                            snapshot.login_ready = false;
                            if let Some(remote) = bus.as_mut().and_then(|bus| bus.remote.as_mut()) {
                                remote.login = None;
                                remote.session = None;
                            }
                        } else {
                            client.profile_epoch.fetch_add(1, Ordering::AcqRel);
                            snapshot.profile_ready = false;
                            if let Some(remote) = bus.as_mut().and_then(|bus| bus.remote.as_mut()) {
                                remote.profile = None;
                            }
                        }
                    }
                    remote_dirty = true;
                }
            }
            Selected::Signal(_) => {
                client.bus_changed();
                bus = None;
                remote_dirty = false;
                refreshing = None;
                snapshot.profile_ready = false;
                snapshot.login_ready = false;
                snapshot.profile_error = Some("System bus disconnected".to_owned());
                snapshot.session_error = snapshot.profile_error.clone();
                retry = Instant::now() + Duration::from_secs(2);
            }
            Selected::Wake => {
                if client.refresh.swap(false, Ordering::AcqRel) {
                    refresh_files(&mut snapshot, &client.limits);
                    remote_dirty = true;
                }
            }
            Selected::Files => {
                refresh_files(&mut snapshot, &client.limits);
            }
            Selected::Timer => {
                refresh_files(&mut snapshot, &client.limits);
                next_poll = Instant::now() + client.limits.poll;
            }
            Selected::Complete(None) => {}
        }
        publish(&mut snapshot, &mut last, &mailbox);
    }
    client.ready.store(false, Ordering::Release);
    drop(operations);
    drop(bus);
    drop(watcher);
    client.reject_pending("System backend stopped");
}
