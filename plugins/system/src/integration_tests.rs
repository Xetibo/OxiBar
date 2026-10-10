use std::{
    collections::HashMap,
    io::{BufRead, BufReader},
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, AtomicUsize, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use iced::futures::{StreamExt, stream::BoxStream};
use zbus::{
    Connection,
    zvariant::{OwnedObjectPath, OwnedValue, Str},
};

use crate::{
    native::{self, Client},
    system::{self, Action, Limits, SessionAction, Snapshot},
    worker,
};

const LOGIN: &str = "org.freedesktop.login1";
const LOGIN_PATH: &str = "/org/freedesktop/login1";
const SESSION_PATH: &str = "/org/freedesktop/login1/session/fixture";
const PROFILE: &str = "org.freedesktop.UPower.PowerProfiles";
const PROFILE_PATH: &str = "/org/freedesktop/UPower/PowerProfiles";

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
            std::env::temp_dir().join(format!("oxibar-system-{}-{nonce}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        let config = directory.join("bus.conf");
        std::fs::write(&config, format!("<busconfig><type>session</type><listen>unix:tmpdir={}</listen><auth>EXTERNAL</auth><policy context=\"default\"><allow own=\"*\"/><allow send_destination=\"*\"/><allow receive_sender=\"*\"/></policy></busconfig>", directory.display())).unwrap();
        let mut child = Command::new("dbus-daemon")
            .args(["--nofork", "--nosyslog", "--print-address=1"])
            .arg(format!("--config-file={}", config.display()))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("dbus-daemon is required for private System tests");
        let stdout = child.stdout.take().unwrap();
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let mut address = String::new();
            let _ = sender.send(
                BufReader::new(stdout)
                    .read_line(&mut address)
                    .map(|_| address),
            );
        });
        let mut bus = Self {
            directory,
            child,
            address: String::new(),
        };
        bus.address = receiver
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .unwrap()
            .trim()
            .to_owned();
        assert!(bus.address.starts_with("unix:"));
        bus
    }
    fn backlights(&self) -> PathBuf {
        let root = self.directory.join("backlights");
        std::fs::create_dir_all(root.join("panel")).unwrap();
        std::fs::write(root.join("panel/max_brightness"), "1000").unwrap();
        std::fs::write(root.join("panel/brightness"), "500").unwrap();
        root
    }
}
impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

struct Data {
    root: PathBuf,
    uid: AtomicU32,
    profile: Mutex<String>,
    calls: Mutex<Vec<String>>,
    profile_reads: AtomicUsize,
    profile_delay: AtomicUsize,
}
impl Data {
    fn new(root: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            root,
            uid: AtomicU32::new(rustix::process::getuid().as_raw()),
            profile: Mutex::new("balanced".to_owned()),
            calls: Mutex::new(Vec::new()),
            profile_reads: AtomicUsize::new(0),
            profile_delay: AtomicUsize::new(0),
        })
    }
    fn record(&self, value: impl Into<String>) {
        self.calls.lock().unwrap().push(value.into());
    }
}

struct Manager(Arc<Data>);
#[zbus::interface(name = "org.freedesktop.login1.Manager")]
impl Manager {
    fn get_session(&self, id: &str) -> zbus::fdo::Result<OwnedObjectPath> {
        if id != "fixture" {
            return Err(zbus::fdo::Error::UnknownObject(
                "unknown session".to_owned(),
            ));
        }
        Ok(OwnedObjectPath::try_from(SESSION_PATH).unwrap())
    }
    #[zbus(name = "GetSessionByPID")]
    fn get_session_by_pid(&self, pid: u32) -> OwnedObjectPath {
        assert_eq!(pid, std::process::id());
        OwnedObjectPath::try_from(SESSION_PATH).unwrap()
    }
    fn can_suspend(&self) -> &str {
        "challenge"
    }
    fn can_hibernate(&self) -> &str {
        "no"
    }
    fn can_reboot(&self) -> &str {
        "yes"
    }
    fn can_power_off(&self) -> &str {
        "yes"
    }
    fn suspend(&self, interactive: bool) {
        assert!(interactive);
        self.0.record("suspend");
    }
    fn hibernate(&self, interactive: bool) {
        assert!(interactive);
        self.0.record("hibernate");
    }
    fn reboot(&self, interactive: bool) {
        assert!(interactive);
        self.0.record("reboot");
    }
    fn power_off(&self, interactive: bool) {
        assert!(interactive);
        self.0.record("power-off");
    }
}

struct Session(Arc<Data>);
#[zbus::interface(name = "org.freedesktop.login1.Session")]
impl Session {
    #[zbus(property)]
    fn id(&self) -> &str {
        "fixture"
    }
    #[zbus(property)]
    fn user(&self) -> (u32, OwnedObjectPath) {
        (
            self.0.uid.load(Ordering::Acquire),
            OwnedObjectPath::try_from("/org/freedesktop/login1/user/fixture").unwrap(),
        )
    }
    fn set_brightness(&self, subsystem: &str, name: &str, value: u32) -> zbus::fdo::Result<()> {
        assert_eq!(subsystem, "backlight");
        system::write_brightness(&self.0.root, name, value).map_err(zbus::fdo::Error::Failed)?;
        self.0.record(format!("brightness:{name}:{value}"));
        Ok(())
    }
    fn terminate(&self) {
        self.0.record("logout");
    }
}

struct Profiles(Arc<Data>);
#[zbus::interface(name = "org.freedesktop.UPower.PowerProfiles")]
impl Profiles {
    #[zbus(property)]
    async fn profiles(&self) -> Vec<HashMap<String, OwnedValue>> {
        self.0.profile_reads.fetch_add(1, Ordering::AcqRel);
        async_io::Timer::after(Duration::from_millis(
            self.0.profile_delay.load(Ordering::Acquire) as u64,
        ))
        .await;
        ["power-saver", "balanced", "performance"]
            .into_iter()
            .map(|value| {
                HashMap::from([("Profile".to_owned(), OwnedValue::from(Str::from(value)))])
            })
            .collect()
    }
    #[zbus(property)]
    fn active_profile(&self) -> String {
        self.0.profile.lock().unwrap().clone()
    }
    #[zbus(property)]
    fn set_active_profile(&mut self, value: String) -> zbus::fdo::Result<()> {
        if !["power-saver", "balanced", "performance"].contains(&value.as_str()) {
            return Err(zbus::fdo::Error::InvalidArgs("invalid profile".to_owned()));
        }
        self.0.record(format!("profile:{value}"));
        *self.0.profile.lock().unwrap() = value;
        Ok(())
    }
    #[zbus(property)]
    fn performance_degraded(&self) -> &str {
        "lap-detected"
    }
}

async fn login(bus: &Bus, data: Arc<Data>) -> Connection {
    zbus::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .name(LOGIN)
        .unwrap()
        .serve_at(LOGIN_PATH, Manager(data.clone()))
        .unwrap()
        .serve_at(SESSION_PATH, Session(data))
        .unwrap()
        .build()
        .await
        .unwrap()
}
async fn profiles(bus: &Bus, data: Arc<Data>) -> Connection {
    zbus::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .name(PROFILE)
        .unwrap()
        .serve_at(PROFILE_PATH, Profiles(data))
        .unwrap()
        .build()
        .await
        .unwrap()
}
fn client(bus: &Bus, root: PathBuf) -> Arc<Client> {
    Client::for_address(
        Limits {
            root,
            poll: Duration::from_millis(100),
            session_id: None,
        },
        Some(bus.address.clone()),
    )
}
async fn snapshot(
    stream: &mut BoxStream<'static, Snapshot>,
    predicate: impl Fn(&Snapshot) -> bool,
) -> Snapshot {
    let mut last = None;
    worker::bounded(
        async {
            loop {
                let value = stream.next().await.ok_or("System stream ended")?;
                if predicate(&value) {
                    return Ok(value);
                }
                last = Some(value);
            }
        },
        Duration::from_secs(6),
    )
    .await
    .unwrap_or_else(|error| panic!("{error}; last System snapshot: {last:?}"))
}
async fn action(client: &Client, value: Action) -> Result<Snapshot, String> {
    worker::bounded(
        async {
            client
                .submit(value)
                .await
                .map_err(|_| "System action dropped")?
        },
        Duration::from_secs(4),
    )
    .await
}

#[test]
#[ignore = "requires dbus-daemon; uses only a private bus and temporary brightness files"]
fn private_bus_brightness_profiles_session_isolation_reconnect_and_cleanup() {
    iced::futures::executor::block_on(async {
        let bus = Bus::start();
        let root = bus.backlights();
        let data = Data::new(root.clone());
        let login = login(&bus, data.clone()).await;
        let ppd = profiles(&bus, data.clone()).await;
        let client = client(&bus, root.clone());
        let mut stream = native::events(client.clone());
        let initial = snapshot(&mut stream, |value| {
            value.profile_ready && value.session.is_some()
        })
        .await;
        assert_eq!(initial.backlights[0].percent(), 50);
        assert_eq!(initial.active_profile.as_deref(), Some("balanced"));
        assert_eq!(initial.degraded.as_deref(), Some("lap-detected"));
        assert!(initial.can(SessionAction::Suspend));
        assert!(!initial.can(SessionAction::Hibernate));
        let reads = data.profile_reads.load(Ordering::Acquire);
        std::fs::write(root.join("panel/brightness"), "650").unwrap();
        snapshot(&mut stream, |value| value.backlights[0].percent() == 65).await;
        assert_eq!(data.profile_reads.load(Ordering::Acquire), reads);
        let changed = action(&client, Action::Brightness("panel".to_owned(), 37))
            .await
            .unwrap();
        assert_eq!(changed.backlights[0].percent(), 37);
        assert!(
            data.calls
                .lock()
                .unwrap()
                .iter()
                .any(|call| call == "brightness:panel:370")
        );
        let changed = action(&client, Action::Profile("performance".to_owned()))
            .await
            .unwrap();
        assert_eq!(changed.active_profile.as_deref(), Some("performance"));
        assert!(
            action(&client, Action::Profile("unknown".to_owned()))
                .await
                .is_err()
        );
        assert!(
            action(&client, Action::Session(SessionAction::Hibernate))
                .await
                .is_err()
        );
        action(&client, Action::Session(SessionAction::Suspend))
            .await
            .unwrap();
        action(&client, Action::Session(SessionAction::Logout))
            .await
            .unwrap();
        assert!(data.calls.lock().unwrap().contains(&"suspend".to_owned()));
        assert!(data.calls.lock().unwrap().contains(&"logout".to_owned()));
        data.uid.fetch_add(1, Ordering::AcqRel);
        assert!(
            action(&client, Action::Session(SessionAction::Logout))
                .await
                .is_err()
        );
        assert!(
            action(&client, Action::Brightness("panel".to_owned(), 99))
                .await
                .is_err()
        );
        assert_eq!(
            system::read_backlight(&root, "panel").unwrap().percent(),
            37
        );
        data.uid
            .store(rustix::process::getuid().as_raw(), Ordering::Release);
        ppd.close().await.unwrap();
        snapshot(&mut stream, |value| !value.profile_ready).await;
        assert!(
            action(&client, Action::Profile("power-saver".to_owned()))
                .await
                .is_err()
        );
        action(&client, Action::Brightness("panel".to_owned(), 42))
            .await
            .unwrap();
        let ppd = profiles(&bus, data.clone()).await;
        snapshot(&mut stream, |value| value.profile_ready).await;
        let start = Instant::now();
        drop(stream);
        assert!(start.elapsed() < Duration::from_secs(1));
        assert!(
            action(&client, Action::Brightness("panel".to_owned(), 10))
                .await
                .is_err()
        );
        ppd.close().await.unwrap();
        login.close().await.unwrap();
    });
}

#[test]
#[ignore = "requires dbus-daemon; uses only a private bus and temporary brightness files"]
fn private_bus_slow_domain_does_not_block_files_or_worker_cancellation() {
    iced::futures::executor::block_on(async {
        let bus = Bus::start();
        let root = bus.backlights();
        let data = Data::new(root.clone());
        data.profile_delay.store(1500, Ordering::Release);
        let login = login(&bus, data.clone()).await;
        let ppd = profiles(&bus, data.clone()).await;
        let initial_reads = data.profile_reads.load(Ordering::Acquire);
        let client = client(&bus, root);
        let mut stream = native::events(client.clone());
        snapshot(&mut stream, |value| !value.backlights.is_empty()).await;
        worker::bounded(
            async {
                while data.profile_reads.load(Ordering::Acquire) <= initial_reads {
                    async_io::Timer::after(Duration::from_millis(2)).await;
                }
                Ok(())
            },
            Duration::from_secs(1),
        )
        .await
        .unwrap();
        let start = Instant::now();
        action(&client, Action::Brightness("panel".to_owned(), 60))
            .await
            .unwrap();
        assert!(start.elapsed() < Duration::from_millis(500));
        let start = Instant::now();
        drop(stream);
        assert!(start.elapsed() < Duration::from_millis(500));
        ppd.close().await.unwrap();
        login.close().await.unwrap();
    });
}
