use std::{
    collections::{HashMap, HashSet},
    io::{BufRead, BufReader},
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use iced::futures::{
    StreamExt,
    channel::oneshot,
    future::{Either, select},
    stream::BoxStream,
};
use zbus::{
    Connection, DBusError, Proxy,
    message::Header,
    zvariant::{OwnedObjectPath, OwnedValue, Str},
};

use super::{
    agent::{AGENT_PATH, PairEvent, Prompt, PromptKind, Reply},
    native::{self, Client, Limits},
    system::{ADAPTER, Action, BLUEZ, DEVICE, Objects, Snapshot},
    worker,
};

const RADIO: &str = "/org/bluez/hci0";
const KEYBOARD: &str = "/org/bluez/hci0/dev_AA_BB_CC_DD_EE_01";
const SPEAKER: &str = "/org/bluez/hci0/dev_AA_BB_CC_DD_EE_02";

struct Bus {
    directory: PathBuf,
    child: Child,
    address: String,
}

impl Bus {
    fn start() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory =
            std::env::temp_dir().join(format!("oxibar-bluez-{}-{nonce}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        let config = directory.join("bus.conf");
        std::fs::write(&config, format!(
            "<busconfig><type>session</type><listen>unix:tmpdir={}</listen><auth>EXTERNAL</auth><policy context=\"default\"><allow own=\"*\"/><allow send_destination=\"*\"/><allow receive_sender=\"*\"/></policy></busconfig>",
            directory.display()
        )).unwrap();
        let child = Command::new("dbus-daemon")
            .arg("--nofork")
            .arg("--nosyslog")
            .arg("--print-address=1")
            .arg(format!("--config-file={}", config.display()))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("install dbus-daemon for the private BlueZ integration test");
        let mut bus = Self {
            directory,
            child,
            address: String::new(),
        };
        let stdout = bus.child.stdout.take().unwrap();
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let mut address = String::new();
            let result = BufReader::new(stdout)
                .read_line(&mut address)
                .map(|_| address);
            let _ = sender.send(result);
        });
        bus.address = receiver
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .unwrap()
            .trim()
            .to_owned();
        assert!(bus.address.starts_with("unix:"));
        bus
    }
}

impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[derive(Clone, Copy, Debug)]
enum Mode {
    Pin,
    Passkey,
    Confirmation,
    Authorization,
    Service,
    DisplayPin,
    DisplayPasskey,
}

#[derive(Clone)]
struct DeviceState {
    name: String,
    paired: bool,
    trusted: bool,
    connected: bool,
}

struct Data {
    radios: HashSet<String>,
    powered: bool,
    discovery: HashSet<String>,
    devices: HashMap<String, DeviceState>,
    agent: Option<(String, OwnedObjectPath)>,
    mode: Mode,
    cancel: Option<oneshot::Sender<()>>,
    delay_connect: bool,
    connect_cancel: Option<oneshot::Sender<()>>,
    calls: Vec<String>,
    reads: usize,
    replies: Vec<String>,
}

type Shared = Arc<Mutex<Data>>;

fn path(value: &str) -> OwnedObjectPath {
    OwnedObjectPath::try_from(value).unwrap()
}
fn text(value: &str) -> OwnedValue {
    OwnedValue::from(Str::from(value))
}

fn objects(data: &Data) -> Objects {
    let mut objects = HashMap::new();
    for radio in &data.radios {
        objects.insert(
            path(radio),
            HashMap::from([(
                ADAPTER.to_owned(),
                HashMap::from([
                    ("Alias".to_owned(), text("Private radio")),
                    ("Powered".to_owned(), data.powered.into()),
                    (
                        "Discovering".to_owned(),
                        (!data.discovery.is_empty()).into(),
                    ),
                ]),
            )]),
        );
    }
    for (device, state) in &data.devices {
        objects.insert(
            path(device),
            HashMap::from([(
                DEVICE.to_owned(),
                HashMap::from([
                    ("Alias".to_owned(), text(&state.name)),
                    (
                        "Address".to_owned(),
                        text(&device.rsplit("dev_").next().unwrap().replace('_', ":")),
                    ),
                    (
                        "Adapter".to_owned(),
                        zbus::zvariant::ObjectPath::try_from(RADIO).unwrap().into(),
                    ),
                    ("Paired".to_owned(), state.paired.into()),
                    ("Trusted".to_owned(), state.trusted.into()),
                    ("Connected".to_owned(), state.connected.into()),
                ]),
            )]),
        );
    }
    objects
}

async fn changed(
    connection: &Connection,
    object: &str,
    interface: &str,
    values: HashMap<String, OwnedValue>,
) {
    connection
        .emit_signal(
            None::<&str>,
            object,
            "org.freedesktop.DBus.Properties",
            "PropertiesChanged",
            &(interface, values, Vec::<String>::new()),
        )
        .await
        .unwrap();
}

struct ObjectManager(Shared);
#[zbus::interface(name = "org.freedesktop.DBus.ObjectManager")]
impl ObjectManager {
    fn get_managed_objects(&self) -> Objects {
        let mut data = self.0.lock().unwrap();
        data.reads += 1;
        objects(&data)
    }
}

#[derive(Debug, DBusError)]
#[zbus(prefix = "org.bluez.Error")]
enum MockError {
    Failed(String),
    Canceled(String),
    #[zbus(error)]
    ZBus(zbus::Error),
}

struct AgentManager(Shared);
#[zbus::interface(name = "org.bluez.AgentManager1")]
impl AgentManager {
    fn register_agent(
        &self,
        agent: OwnedObjectPath,
        capability: String,
        #[zbus(header)] header: Header<'_>,
    ) {
        assert_eq!(capability, "KeyboardDisplay");
        assert_eq!(agent.as_str(), AGENT_PATH);
        let mut data = self.0.lock().unwrap();
        data.calls.push("RegisterAgent".to_owned());
        data.agent = Some((header.sender().unwrap().to_string(), agent));
    }
    fn unregister_agent(&self, _agent: OwnedObjectPath) {
        let mut data = self.0.lock().unwrap();
        data.calls.push("UnregisterAgent".to_owned());
        data.agent = None;
    }
    fn request_default_agent(&self, _agent: OwnedObjectPath) {
        self.0
            .lock()
            .unwrap()
            .calls
            .push("UNEXPECTED default agent".to_owned());
    }
}

struct Radio(Shared);
#[zbus::interface(name = "org.bluez.Adapter1")]
impl Radio {
    #[zbus(property)]
    fn alias(&self) -> &str {
        "Private radio"
    }
    #[zbus(property)]
    fn powered(&self) -> bool {
        self.0.lock().unwrap().powered
    }
    #[zbus(property)]
    fn set_powered(&self, enabled: bool) {
        self.0.lock().unwrap().powered = enabled;
    }
    #[zbus(property)]
    fn discovering(&self) -> bool {
        !self.0.lock().unwrap().discovery.is_empty()
    }
    async fn start_discovery(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &Connection,
    ) {
        {
            let mut data = self.0.lock().unwrap();
            data.discovery.insert(header.sender().unwrap().to_string());
            data.calls.push("StartDiscovery".to_owned());
        }
        changed(
            connection,
            RADIO,
            ADAPTER,
            HashMap::from([("Discovering".to_owned(), true.into())]),
        )
        .await;
    }
    async fn stop_discovery(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &Connection,
    ) {
        let discovering = {
            let mut data = self.0.lock().unwrap();
            data.discovery.remove(header.sender().unwrap().as_str());
            data.calls.push("StopDiscovery".to_owned());
            !data.discovery.is_empty()
        };
        changed(
            connection,
            RADIO,
            ADAPTER,
            HashMap::from([("Discovering".to_owned(), discovering.into())]),
        )
        .await;
    }
    async fn remove_device(
        &self,
        device: OwnedObjectPath,
        #[zbus(connection)] connection: &Connection,
    ) {
        self.0.lock().unwrap().devices.remove(device.as_str());
        connection
            .object_server()
            .remove::<Device, _>(device.clone())
            .await
            .unwrap();
    }
}

struct Device {
    shared: Shared,
    path: String,
}
#[zbus::interface(name = "org.bluez.Device1")]
impl Device {
    #[zbus(property)]
    fn alias(&self) -> String {
        self.shared.lock().unwrap().devices[&self.path].name.clone()
    }
    #[zbus(property)]
    fn address(&self) -> String {
        self.path.rsplit("dev_").next().unwrap().replace('_', ":")
    }
    #[zbus(property)]
    fn adapter(&self) -> OwnedObjectPath {
        path(RADIO)
    }
    #[zbus(property)]
    fn paired(&self) -> bool {
        self.shared.lock().unwrap().devices[&self.path].paired
    }
    #[zbus(property)]
    fn connected(&self) -> bool {
        self.shared.lock().unwrap().devices[&self.path].connected
    }
    #[zbus(property)]
    fn trusted(&self) -> bool {
        self.shared.lock().unwrap().devices[&self.path].trusted
    }
    #[zbus(property)]
    fn set_trusted(&self, enabled: bool) {
        self.shared
            .lock()
            .unwrap()
            .devices
            .get_mut(&self.path)
            .unwrap()
            .trusted = enabled;
    }
    async fn connect(&self, #[zbus(connection)] connection: &Connection) -> Result<(), MockError> {
        let canceled = {
            let mut data = self.shared.lock().unwrap();
            data.calls.push("Connect".to_owned());
            if data.delay_connect {
                let (sender, receiver) = oneshot::channel();
                data.connect_cancel = Some(sender);
                Some(receiver)
            } else {
                None
            }
        };
        if let Some(canceled) = canceled {
            match select(
                Box::pin(async_io::Timer::after(Duration::from_secs(30))),
                canceled,
            )
            .await
            {
                Either::Left(_) => {}
                Either::Right(_) => {
                    return Err(MockError::Canceled("Connection canceled".to_owned()));
                }
            }
        }
        self.shared
            .lock()
            .unwrap()
            .devices
            .get_mut(&self.path)
            .unwrap()
            .connected = true;
        changed(
            connection,
            &self.path,
            DEVICE,
            HashMap::from([("Connected".to_owned(), true.into())]),
        )
        .await;
        Ok(())
    }
    async fn disconnect(&self, #[zbus(connection)] connection: &Connection) {
        {
            let mut data = self.shared.lock().unwrap();
            data.devices.get_mut(&self.path).unwrap().connected = false;
            data.calls.push("Disconnect".to_owned());
            if let Some(cancel) = data.connect_cancel.take() {
                let _ = cancel.send(());
            }
        }
        changed(
            connection,
            &self.path,
            DEVICE,
            HashMap::from([("Connected".to_owned(), false.into())]),
        )
        .await;
    }
    async fn pair(&self, #[zbus(connection)] connection: &Connection) -> Result<(), MockError> {
        let (destination, agent, mode, canceled) = {
            let mut data = self.shared.lock().unwrap();
            data.calls.push("Pair".to_owned());
            let (sender, receiver) = oneshot::channel();
            data.cancel = Some(sender);
            let (destination, agent) = data.agent.clone().unwrap();
            (destination, agent, data.mode, receiver)
        };
        let device = path(&self.path);
        let exchange = async {
            let agent = Proxy::new(
                connection,
                destination.as_str(),
                agent.as_str(),
                "org.bluez.Agent1",
            )
            .await?;
            match mode {
                Mode::Pin => {
                    let reply: String = agent.call("RequestPinCode", &(device.clone(),)).await?;
                    self.shared
                        .lock()
                        .unwrap()
                        .replies
                        .push(format!("pin:{reply}"));
                }
                Mode::Passkey => {
                    let reply: u32 = agent.call("RequestPasskey", &(device.clone(),)).await?;
                    self.shared
                        .lock()
                        .unwrap()
                        .replies
                        .push(format!("passkey:{reply}"));
                }
                Mode::Confirmation => {
                    agent
                        .call::<_, _, ()>("RequestConfirmation", &(device.clone(), 123_u32))
                        .await?;
                }
                Mode::Authorization => {
                    agent
                        .call::<_, _, ()>("RequestAuthorization", &(device.clone(),))
                        .await?;
                }
                Mode::Service => {
                    agent
                        .call::<_, _, ()>(
                            "AuthorizeService",
                            &(device.clone(), "00001124-0000-1000-8000-00805f9b34fb"),
                        )
                        .await?;
                }
                Mode::DisplayPin => {
                    agent
                        .call::<_, _, ()>("DisplayPinCode", &(device.clone(), "ABCD-1234"))
                        .await?;
                    async_io::Timer::after(Duration::from_millis(250)).await;
                }
                Mode::DisplayPasskey => {
                    agent
                        .call::<_, _, ()>("DisplayPasskey", &(device.clone(), 123_u32, 3_u16))
                        .await?;
                    async_io::Timer::after(Duration::from_secs(30)).await;
                }
            }
            Ok::<(), MockError>(())
        };
        match select(Box::pin(exchange), canceled).await {
            Either::Left((result, _)) => result?,
            Either::Right(_) => return Err(MockError::Canceled("Canceled by client".to_owned())),
        }
        self.shared
            .lock()
            .unwrap()
            .devices
            .get_mut(&self.path)
            .unwrap()
            .paired = true;
        changed(
            connection,
            &self.path,
            DEVICE,
            HashMap::from([("Paired".to_owned(), true.into())]),
        )
        .await;
        Ok(())
    }
    fn cancel_pairing(&self) {
        let mut data = self.shared.lock().unwrap();
        data.calls.push("CancelPairing".to_owned());
        if let Some(cancel) = data.cancel.take() {
            let _ = cancel.send(());
        }
    }
}

struct BlueZ {
    connection: Connection,
    shared: Shared,
}

impl BlueZ {
    async fn start(bus: &Bus) -> Self {
        let shared = Arc::new(Mutex::new(Data {
            radios: HashSet::from([RADIO.to_owned()]),
            powered: true,
            discovery: HashSet::from(["external discovery client".to_owned()]),
            devices: HashMap::from([
                (
                    KEYBOARD.to_owned(),
                    DeviceState {
                        name: "Private keyboard".to_owned(),
                        paired: false,
                        trusted: false,
                        connected: false,
                    },
                ),
                (
                    SPEAKER.to_owned(),
                    DeviceState {
                        name: "Private speaker".to_owned(),
                        paired: true,
                        trusted: true,
                        connected: true,
                    },
                ),
            ]),
            agent: None,
            mode: Mode::Pin,
            cancel: None,
            delay_connect: false,
            connect_cancel: None,
            calls: Vec::new(),
            reads: 0,
            replies: Vec::new(),
        }));
        let connection = zbus::connection::Builder::address(bus.address.as_str())
            .unwrap()
            .name(BLUEZ)
            .unwrap()
            .method_timeout(Duration::from_secs(3))
            .serve_at("/", ObjectManager(shared.clone()))
            .unwrap()
            .serve_at("/org/bluez", AgentManager(shared.clone()))
            .unwrap()
            .serve_at(RADIO, Radio(shared.clone()))
            .unwrap()
            .serve_at(
                KEYBOARD,
                Device {
                    shared: shared.clone(),
                    path: KEYBOARD.to_owned(),
                },
            )
            .unwrap()
            .serve_at(
                SPEAKER,
                Device {
                    shared: shared.clone(),
                    path: SPEAKER.to_owned(),
                },
            )
            .unwrap()
            .build()
            .await
            .unwrap();
        Self { connection, shared }
    }
    async fn reset_pair(&self, mode: Mode) {
        {
            let mut data = self.shared.lock().unwrap();
            data.mode = mode;
            let device = data.devices.get_mut(KEYBOARD).unwrap();
            device.paired = false;
            device.trusted = false;
            device.connected = false;
        }
        changed(
            &self.connection,
            KEYBOARD,
            DEVICE,
            HashMap::from([
                ("Paired".to_owned(), false.into()),
                ("Trusted".to_owned(), false.into()),
                ("Connected".to_owned(), false.into()),
            ]),
        )
        .await;
    }
    async fn agent_call(&self, method: &str) -> Result<(), zbus::Error> {
        let (destination, agent) = self.shared.lock().unwrap().agent.clone().unwrap();
        let proxy = Proxy::new(
            &self.connection,
            destination.as_str(),
            agent.as_str(),
            "org.bluez.Agent1",
        )
        .await?;
        proxy.call(method, &()).await
    }
}

async fn snapshot(
    stream: &mut BoxStream<'static, Result<Snapshot, String>>,
    predicate: impl Fn(&Snapshot) -> bool,
) -> Snapshot {
    worker::deadline(
        async {
            while let Some(state) = stream.next().await {
                if let Ok(state) = state
                    && predicate(&state)
                {
                    return Ok(state);
                }
            }
            Err("stream ended".to_owned())
        },
        Duration::from_secs(5),
        "test snapshot",
    )
    .await
    .unwrap()
}

async fn prompt(stream: &mut BoxStream<'static, PairEvent>) -> Prompt {
    worker::deadline(
        async {
            loop {
                match stream.next().await.unwrap() {
                    PairEvent::Prompt(prompt) => return Ok(prompt),
                    PairEvent::Progress(_) => {}
                    PairEvent::Finished(result) => {
                        return Err(format!("pair ended before prompt: {result:?}"));
                    }
                }
            }
        },
        Duration::from_secs(3),
        "test prompt",
    )
    .await
    .unwrap()
}

async fn finish(stream: &mut BoxStream<'static, PairEvent>) -> Result<(), String> {
    worker::deadline(
        async {
            loop {
                if let Some(PairEvent::Finished(result)) = stream.next().await {
                    return Ok(result);
                }
            }
        },
        Duration::from_secs(4),
        "test pairing completion",
    )
    .await
    .unwrap()
}

async fn action(client: &Client, action: Action) {
    worker::deadline(
        async { client.submit(action).await.unwrap() },
        Duration::from_secs(4),
        "test action",
    )
    .await
    .unwrap();
}

#[test]
#[ignore = "requires dbus-daemon; starts a private bus and mock BlueZ without hardware"]
fn private_bus_events_actions_discovery_reconnect_and_worker_cleanup() {
    iced::futures::executor::block_on(async {
        let bus = Bus::start();
        let service = BlueZ::start(&bus).await;
        let client = Client::for_address(
            Some(bus.address.clone()),
            Limits {
                discovery: Duration::from_millis(250),
                ..Limits::default()
            },
        );
        let mut events = native::events(client.clone());
        let initial = snapshot(&mut events, |_| true).await;
        assert_eq!(initial.adapters.len(), 1);
        assert_eq!(initial.connected[0].name, "Private speaker");
        assert_eq!(initial.available[0].name, "Private keyboard");
        assert!(initial.adapters[0].discovering && !initial.adapters[0].scanning);
        action(&client, Action::Power(RADIO.to_owned(), false)).await;
        snapshot(&mut events, |state| !state.adapters[0].powered).await;
        action(&client, Action::Power(RADIO.to_owned(), true)).await;
        snapshot(&mut events, |state| state.adapters[0].powered).await;
        action(&client, Action::Discovery(RADIO.to_owned(), true)).await;
        snapshot(&mut events, |state| state.adapters[0].scanning).await;
        let stopped = snapshot(&mut events, |state| !state.adapters[0].scanning).await;
        assert!(stopped.adapters[0].discovering);
        assert_eq!(service.shared.lock().unwrap().discovery.len(), 1);
        action(&client, Action::Disconnect(SPEAKER.to_owned())).await;
        snapshot(&mut events, |state| state.connected.is_empty()).await;
        action(&client, Action::Trust(SPEAKER.to_owned(), false)).await;
        snapshot(&mut events, |state| !state.device(SPEAKER).unwrap().trusted).await;
        action(&client, Action::Forget(SPEAKER.to_owned())).await;
        snapshot(&mut events, |state| state.device(SPEAKER).is_none()).await;
        let hotplug = "/org/bluez/hci0/dev_AA_BB_CC_DD_EE_03";
        service.shared.lock().unwrap().devices.insert(
            hotplug.to_owned(),
            DeviceState {
                name: "Hotplug device".to_owned(),
                paired: true,
                trusted: false,
                connected: false,
            },
        );
        service
            .connection
            .object_server()
            .at(
                hotplug,
                Device {
                    shared: service.shared.clone(),
                    path: hotplug.to_owned(),
                },
            )
            .await
            .unwrap();
        snapshot(&mut events, |state| state.device(hotplug).is_some()).await;
        action(&client, Action::Forget(hotplug.to_owned())).await;
        snapshot(&mut events, |state| state.device(hotplug).is_none()).await;
        let second_radio = "/org/bluez/hci1";
        service
            .shared
            .lock()
            .unwrap()
            .radios
            .insert(second_radio.to_owned());
        service
            .connection
            .object_server()
            .at(second_radio, Radio(service.shared.clone()))
            .await
            .unwrap();
        snapshot(&mut events, |state| state.adapters.len() == 2).await;
        service.shared.lock().unwrap().radios.remove(second_radio);
        service
            .connection
            .object_server()
            .remove::<Radio, _>(second_radio)
            .await
            .unwrap();
        snapshot(&mut events, |state| state.adapters.len() == 1).await;
        assert_eq!(
            service.shared.lock().unwrap().reads,
            1,
            "normal updates must not poll inventory"
        );
        service
            .shared
            .lock()
            .unwrap()
            .devices
            .get_mut(KEYBOARD)
            .unwrap()
            .name = "Renamed keyboard".to_owned();
        service
            .connection
            .emit_signal(
                None::<&str>,
                KEYBOARD,
                "org.freedesktop.DBus.Properties",
                "PropertiesChanged",
                &(DEVICE, HashMap::<String, OwnedValue>::new(), vec!["Alias"]),
            )
            .await
            .unwrap();
        snapshot(&mut events, |state| {
            state.device(KEYBOARD).unwrap().name == "Renamed keyboard"
        })
        .await;
        assert_eq!(service.shared.lock().unwrap().reads, 2);
        service.connection.close().await.unwrap();
        worker::deadline(
            async {
                while let Some(state) = events.next().await {
                    if state.is_err() {
                        return Ok(());
                    }
                }
                Err("no disconnect".to_owned())
            },
            Duration::from_secs(3),
            "disconnect",
        )
        .await
        .unwrap();
        assert!(
            client
                .submit(Action::Power(RADIO.to_owned(), false))
                .await
                .unwrap()
                .is_err()
        );
        let restarted = BlueZ::start(&bus).await;
        snapshot(&mut events, |state| state.connected.len() == 1).await;
        assert!(
            restarted.shared.lock().unwrap().powered,
            "offline actions must not be replayed"
        );
        action(&client, Action::Discovery(RADIO.to_owned(), true)).await;
        snapshot(&mut events, |state| state.adapters[0].scanning).await;
        let start = Instant::now();
        drop(events);
        assert!(start.elapsed() < Duration::from_secs(2));
        let data = restarted.shared.lock().unwrap();
        assert_eq!(data.discovery.len(), 1);
        assert!(data.calls.iter().any(|call| call == "UnregisterAgent"));
        assert!(!data.calls.iter().any(|call| call.contains("UNEXPECTED")));
    });
}

#[test]
#[ignore = "requires dbus-daemon; tests all Agent1 replies on an isolated mock BlueZ bus"]
fn private_bus_pairing_replies_rejection_spoofing_cancel_and_timeout() {
    iced::futures::executor::block_on(async {
        let bus = Bus::start();
        let service = BlueZ::start(&bus).await;
        let client = Client::for_address(
            Some(bus.address.clone()),
            Limits {
                pairing: Duration::from_millis(700),
                ..Limits::default()
            },
        );
        let mut events = native::events(client.clone());
        snapshot(&mut events, |_| true).await;
        let (_, mut pair) = client.pair(KEYBOARD.to_owned()).unwrap();
        let request = prompt(&mut pair).await;
        assert_eq!(request.kind, PromptKind::Pin);
        assert!(
            client
                .respond(request.id, Reply::Text(String::new()))
                .is_err()
        );
        client
            .respond(request.id, Reply::Text("0123-abcd".to_owned()))
            .unwrap();
        assert!(finish(&mut pair).await.is_ok());
        snapshot(&mut events, |state| {
            state.device(KEYBOARD).unwrap().connected
        })
        .await;
        assert!(
            service
                .shared
                .lock()
                .unwrap()
                .replies
                .contains(&"pin:0123-abcd".to_owned())
        );
        for (mode, kind, reply) in [
            (
                Mode::Passkey,
                PromptKind::Passkey,
                Reply::Text("000123".to_owned()),
            ),
            (
                Mode::Confirmation,
                PromptKind::Confirmation(123),
                Reply::Accept,
            ),
            (
                Mode::Authorization,
                PromptKind::Authorization,
                Reply::Accept,
            ),
            (
                Mode::Service,
                PromptKind::Service("00001124-0000-1000-8000-00805f9b34fb".to_owned()),
                Reply::Accept,
            ),
        ] {
            service.reset_pair(mode).await;
            snapshot(&mut events, |state| !state.device(KEYBOARD).unwrap().paired).await;
            let (_, mut pair) = client.pair(KEYBOARD.to_owned()).unwrap();
            let request = prompt(&mut pair).await;
            assert_eq!(request.kind, kind);
            client.respond(request.id, reply).unwrap();
            assert!(finish(&mut pair).await.is_ok());
            assert!(client.respond(request.id, Reply::Accept).is_err());
            snapshot(&mut events, |state| {
                state.device(KEYBOARD).unwrap().connected
            })
            .await;
        }
        assert!(
            service
                .shared
                .lock()
                .unwrap()
                .replies
                .contains(&"passkey:123".to_owned())
        );
        service.reset_pair(Mode::DisplayPin).await;
        snapshot(&mut events, |state| !state.device(KEYBOARD).unwrap().paired).await;
        let (_, mut pair) = client.pair(KEYBOARD.to_owned()).unwrap();
        assert_eq!(
            prompt(&mut pair).await.kind,
            PromptKind::DisplayPin("ABCD-1234".to_owned())
        );
        assert!(finish(&mut pair).await.is_ok());
        snapshot(&mut events, |state| {
            state.device(KEYBOARD).unwrap().connected
        })
        .await;
        service.reset_pair(Mode::Pin).await;
        snapshot(&mut events, |state| !state.device(KEYBOARD).unwrap().paired).await;
        service.shared.lock().unwrap().delay_connect = true;
        let (id, mut pair) = client.pair(KEYBOARD.to_owned()).unwrap();
        let request = prompt(&mut pair).await;
        client
            .respond(request.id, Reply::Text("1234".to_owned()))
            .unwrap();
        worker::deadline(
            async {
                while !client.hub.needs_disconnect(id)
                    || service.shared.lock().unwrap().connect_cancel.is_none()
                {
                    async_io::Timer::after(Duration::from_millis(10)).await;
                }
                Ok(())
            },
            Duration::from_secs(2),
            "connecting phase",
        )
        .await
        .unwrap();
        client.cancel_pair(Some(id));
        assert!(finish(&mut pair).await.is_err());
        {
            let mut data = service.shared.lock().unwrap();
            assert!(!data.devices[KEYBOARD].connected);
            assert!(data.calls.iter().any(|call| call == "Disconnect"));
            data.delay_connect = false;
        }
        service.reset_pair(Mode::Confirmation).await;
        snapshot(&mut events, |state| !state.device(KEYBOARD).unwrap().paired).await;
        let (_, mut pair) = client.pair(KEYBOARD.to_owned()).unwrap();
        let request = prompt(&mut pair).await;
        client.respond(request.id, Reply::Reject).unwrap();
        assert!(finish(&mut pair).await.is_err());
        assert!(!service.shared.lock().unwrap().devices[KEYBOARD].paired);
        service.reset_pair(Mode::Pin).await;
        let (id, mut pair) = client.pair(KEYBOARD.to_owned()).unwrap();
        let request = prompt(&mut pair).await;
        let (destination, _) = service.shared.lock().unwrap().agent.clone().unwrap();
        let outsider = zbus::connection::Builder::address(bus.address.as_str())
            .unwrap()
            .build()
            .await
            .unwrap();
        let spoof = Proxy::new(
            &outsider,
            destination.as_str(),
            AGENT_PATH,
            "org.bluez.Agent1",
        )
        .await
        .unwrap();
        assert!(spoof.call::<_, _, ()>("Cancel", &()).await.is_err());
        assert!(
            spoof
                .call::<_, _, String>("RequestPinCode", &(path(KEYBOARD),))
                .await
                .is_err()
        );
        client.cancel_pair(Some(id));
        assert!(finish(&mut pair).await.is_err());
        assert!(
            client
                .respond(request.id, Reply::Text("1234".to_owned()))
                .is_err()
        );
        service.reset_pair(Mode::DisplayPasskey).await;
        let (_, mut pair) = client.pair(KEYBOARD.to_owned()).unwrap();
        assert_eq!(
            prompt(&mut pair).await.kind,
            PromptKind::DisplayPasskey {
                passkey: 123,
                entered: 3
            }
        );
        drop(pair);
        worker::deadline(
            async {
                while client.hub.active().is_some() {
                    async_io::Timer::after(Duration::from_millis(10)).await;
                }
                Ok(())
            },
            Duration::from_secs(2),
            "drop cancellation",
        )
        .await
        .unwrap();
        service.reset_pair(Mode::Pin).await;
        let (_, mut pair) = client.pair(KEYBOARD.to_owned()).unwrap();
        prompt(&mut pair).await;
        service.agent_call("Cancel").await.unwrap();
        assert!(finish(&mut pair).await.is_err());
        let (_, mut pair) = client.pair(KEYBOARD.to_owned()).unwrap();
        prompt(&mut pair).await;
        assert!(
            finish(&mut pair).await.is_err(),
            "unanswered pairing must time out"
        );
        assert!(!service.shared.lock().unwrap().devices[KEYBOARD].paired);
        let (_, mut pair) = client.pair(KEYBOARD.to_owned()).unwrap();
        prompt(&mut pair).await;
        service.agent_call("Release").await.unwrap();
        assert!(finish(&mut pair).await.is_err());
        drop(events);
    });
}
