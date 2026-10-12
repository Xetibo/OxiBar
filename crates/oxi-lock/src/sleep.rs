use std::{
    fs::File,
    io::{self, Read},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::process::CommandExt,
    },
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use zbus::blocking::{Connection, Proxy};

pub struct LockerProcess {
    child: Option<Child>,
    reader: Option<File>,
    bytes: Vec<u8>,
    ready: bool,
}
impl Default for LockerProcess {
    fn default() -> Self {
        Self {
            child: None,
            reader: None,
            bytes: Vec::with_capacity(16),
            ready: false,
        }
    }
}
impl LockerProcess {
    fn alive(&mut self) -> io::Result<bool> {
        if self
            .child
            .as_mut()
            .map(Child::try_wait)
            .transpose()?
            .flatten()
            .is_some()
        {
            self.child = None;
            self.reader = None;
            self.ready = false;
        }
        Ok(self.child.is_some())
    }
    fn start(&mut self) -> io::Result<()> {
        if self.alive()? {
            return Ok(());
        }
        let path = std::env::current_exe()?.with_file_name("oxilock");
        let mut command = Command::new(path);
        command.args(["--ready-fd", "3"]);
        self.spawn(command)
    }
    fn spawn(&mut self, mut command: Command) -> io::Result<()> {
        let mut descriptors = [0; 2];
        // Both ends remain private; only the child's readiness writer survives exec.
        if unsafe { libc::pipe2(descriptors.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) } != 0
        {
            return Err(io::Error::last_os_error());
        }
        let reader = unsafe { File::from_raw_fd(descriptors[0]) };
        let writer = unsafe { File::from_raw_fd(descriptors[1]) };
        let fd = writer.as_raw_fd();
        command.stdin(Stdio::null()).stdout(Stdio::null());
        unsafe {
            command.pre_exec(move || {
                if libc::dup2(fd, 3) < 0 || libc::fcntl(3, libc::F_SETFD, 0) < 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        self.child = Some(command.spawn()?);
        self.reader = Some(reader);
        self.bytes.clear();
        self.ready = false;
        Ok(())
    }
    fn poll(&mut self) -> io::Result<bool> {
        if !self.alive()? {
            return Err(io::Error::other("Locker exited before sleep readiness"));
        }
        if self.ready {
            return Ok(true);
        }
        let mut bytes = [0; 16];
        match self
            .reader
            .as_mut()
            .ok_or_else(|| io::Error::other("Missing readiness channel"))?
            .read(&mut bytes)
        {
            Ok(0) => return Err(io::Error::other("Locker closed readiness channel")),
            Ok(count) => {
                self.bytes.extend_from_slice(&bytes[..count]);
                if self.bytes.len() > 16 {
                    return Err(io::Error::other("Invalid readiness response"));
                }
            }
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock
                    || error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
        if self.bytes.contains(&b'\n') {
            if self.bytes != b"locked\n" {
                return Err(io::Error::other(
                    "Locker did not confirm protocol readiness",
                ));
            }
            self.ready = true;
            self.reader = None;
        }
        Ok(self.ready)
    }
    fn wait(&mut self, timeout: Duration) -> io::Result<()> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.poll()? {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(10));
        }
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "Locker did not reach protocol readiness before sleep deadline",
        ))
    }
}

fn inhibitor(proxy: &Proxy<'_>, mode: &str) -> Result<OwnedFd, Box<dyn std::error::Error>> {
    let fd: zbus::zvariant::OwnedFd = proxy.call(
        "Inhibit",
        &("sleep", "OxiLock", "Wait for protocol lock readiness", mode),
    )?;
    Ok(fd.into())
}
fn delay_window(microseconds: u64) -> io::Result<Duration> {
    Duration::from_micros(microseconds)
        .checked_sub(Duration::from_millis(750))
        .filter(|window| *window >= Duration::from_millis(100))
        .map(|window| window.min(Duration::from_secs(10)))
        .ok_or_else(|| io::Error::other("Logind sleep-delay window is too short for readiness"))
}

pub fn suspend() -> Result<(), Box<dyn std::error::Error>> {
    let connection = Connection::system()?;
    let proxy = manager(&connection)?;
    suspend_ready(
        &proxy,
        &mut LockerProcess::default(),
        Duration::from_secs(10),
    )
}
fn suspend_ready(
    proxy: &Proxy<'_>,
    locker: &mut LockerProcess,
    timeout: Duration,
) -> Result<(), Box<dyn std::error::Error>> {
    let lease = inhibitor(proxy, "block")?;
    locker.start()?;
    locker.wait(timeout)?;
    drop(lease);
    proxy.call::<_, _, ()>("Suspend", &(false,))?;
    Ok(())
}
pub fn watch() -> Result<(), Box<dyn std::error::Error>> {
    let connection = Connection::system()?;
    let proxy = manager(&connection)?;
    let signals = proxy.receive_signal("PrepareForSleep")?;
    let timeout = delay_window(proxy.get_property("InhibitDelayMaxUSec")?)?;
    let mut lease = Some(inhibitor(&proxy, "delay")?);
    let mut locker = LockerProcess::default();
    if proxy.get_property::<bool>("PreparingForSleep")? {
        prepare(&mut locker, timeout, &mut lease);
    }
    for message in signals {
        let (sleeping,): (bool,) = message.body().deserialize()?;
        if sleeping {
            prepare(&mut locker, timeout, &mut lease);
        } else {
            locker.alive()?;
            lease = Some(inhibitor(&proxy, "delay")?);
        }
    }
    Err("Logind sleep signal stream ended".into())
}
fn manager(connection: &Connection) -> zbus::Result<Proxy<'_>> {
    Proxy::new(
        connection,
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )
}
fn prepare(locker: &mut LockerProcess, timeout: Duration, lease: &mut Option<OwnedFd>) {
    let ready = locker.start().and_then(|_| locker.wait(timeout));
    if let Err(error) = ready {
        eprintln!("oxilock-sleep: {error}");
    } else {
        eprintln!("oxilock-sleep: protocol lock ready; releasing delay inhibitor");
    }
    // Logind delay inhibitors are finite; a failed lock is reported, never labelled ready.
    lease.take();
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(script: &str) -> LockerProcess {
        let mut command = Command::new("sh");
        command.args(["-c", script]);
        let mut locker = LockerProcess::default();
        locker.spawn(command).unwrap();
        locker
    }
    fn stop(locker: &mut LockerProcess) {
        if let Some(child) = &mut locker.child {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
    #[test]
    fn only_complete_protocol_readiness_opens_gate_and_reuse_requires_live_process() {
        let mut locker = fixture("printf lock >&3; sleep 0.05; printf 'ed\\n' >&3; sleep 2");
        assert!(!locker.poll().unwrap());
        locker.wait(Duration::from_secs(1)).unwrap();
        assert!(locker.poll().unwrap());
        stop(&mut locker);
        assert!(locker.poll().is_err());
    }
    #[test]
    fn refusal_timeout_and_short_logind_window_cannot_claim_ready() {
        let mut denied = fixture("printf 'denied\\n' >&3; sleep 2");
        assert!(denied.wait(Duration::from_secs(1)).is_err());
        stop(&mut denied);
        let mut slow = fixture("sleep 2");
        assert_eq!(
            slow.wait(Duration::from_millis(30)).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        stop(&mut slow);
        assert!(delay_window(500_000).is_err());
        assert_eq!(
            delay_window(5_000_000).unwrap(),
            Duration::from_millis(4250)
        );
    }
    struct Logind {
        readers: std::sync::Arc<std::sync::Mutex<Vec<std::os::unix::net::UnixStream>>>,
        requests: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }
    #[zbus::interface(name = "org.freedesktop.login1.Manager")]
    impl Logind {
        fn suspend(&self, _interactive: bool) -> zbus::fdo::Result<()> {
            if self
                .readers
                .lock()
                .unwrap()
                .iter_mut()
                .any(|reader| reader.read(&mut [0]).ok() != Some(0))
            {
                return Err(zbus::fdo::Error::Failed(
                    "Block lease must close before Suspend".into(),
                ));
            }
            self.requests
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }
        fn inhibit(
            &self,
            _what: &str,
            _who: &str,
            _why: &str,
            _mode: &str,
        ) -> zbus::zvariant::OwnedFd {
            let (reader, writer) = std::os::unix::net::UnixStream::pair().unwrap();
            reader.set_nonblocking(true).unwrap();
            self.readers.lock().unwrap().push(reader);
            let fd: OwnedFd = writer.into();
            fd.into()
        }
    }
    #[test]
    fn private_logind_fd_lease_is_held_through_readiness_and_released_after_gate() {
        let readers = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let requests = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (left, right) = std::os::unix::net::UnixStream::pair().unwrap();
        let service = Logind {
            readers: readers.clone(),
            requests: requests.clone(),
        };
        let server = thread::spawn(move || {
            zbus::blocking::connection::Builder::unix_stream(left)
                .p2p()
                .server(zbus::Guid::generate())
                .unwrap()
                .serve_at("/org/freedesktop/login1", service)
                .unwrap()
                .build()
                .unwrap()
        });
        let client = zbus::blocking::connection::Builder::unix_stream(right)
            .p2p()
            .build()
            .unwrap();
        let _server = server.join().unwrap();
        let proxy = manager(&client).unwrap();
        let mut lease = Some(inhibitor(&proxy, "delay").unwrap());
        assert_eq!(
            readers.lock().unwrap()[0]
                .read(&mut [0])
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock
        );
        let mut locker = fixture("sleep 0.05; printf 'locked\\n' >&3; sleep 2");
        prepare(&mut locker, Duration::from_secs(1), &mut lease);
        assert!(locker.ready);
        assert!(lease.is_none());
        assert_eq!(readers.lock().unwrap()[0].read(&mut [0]).unwrap(), 0);
        assert!(locker.poll().unwrap());
        stop(&mut locker);
        let mut denied = fixture("printf 'denied\n' >&3; sleep 2");
        assert!(suspend_ready(&proxy, &mut denied, Duration::from_secs(1)).is_err());
        assert_eq!(requests.load(std::sync::atomic::Ordering::SeqCst), 0);
        stop(&mut denied);
        let mut accepted = fixture("printf 'locked\n' >&3; sleep 2");
        suspend_ready(&proxy, &mut accepted, Duration::from_secs(1)).unwrap();
        assert_eq!(requests.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(accepted.poll().unwrap());
        stop(&mut accepted);
    }
}
