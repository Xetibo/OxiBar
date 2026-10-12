mod codec;
mod pam;

use oxi_auth_ui::{Attempt, Prompt, Secret};
pub use pam::current_user;

use std::{
    io,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{net::UnixStream, process::CommandExt},
    },
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

const AUTHENTICATION_TIMEOUT: Duration = Duration::from_secs(300);

pub fn ensure_service() -> io::Result<()> {
    use std::os::unix::fs::MetadataExt;
    for path in ["/etc/pam.d", "/etc/pam.d/oxilock"] {
        let metadata = std::fs::metadata(path)?;
        if metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "oxilock PAM configuration must be administrator-owned and read-only to other users",
            ));
        }
    }
    if !std::fs::metadata("/etc/pam.d/oxilock")?.is_file() {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(())
}

#[derive(Debug)]
pub enum Event {
    Prompt(Attempt, Prompt),
    Finished(Attempt, bool),
}

pub struct Worker {
    child: Child,
    stream: UnixStream,
    reader: codec::Reader,
    attempt: Attempt,
    finished: bool,
    started: Instant,
}

impl Worker {
    pub fn start(attempt: Attempt) -> io::Result<Self> {
        let mut command = Command::new("/proc/self/exe");
        command.arg("--pam-worker").arg("3").env_clear();
        for key in ["LANG", "LC_ALL", "LD_LIBRARY_PATH"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        Self::spawn(command, attempt)
    }
    fn spawn(mut command: Command, attempt: Attempt) -> io::Result<Self> {
        if attempt.0 == 0 {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let (stream, worker) = UnixStream::pair()?;
        let fd = worker.as_raw_fd();
        let parent = std::process::id() as libc::pid_t;
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0);
        // Only async-signal-safe descriptor and parent-death operations run before exec.
        unsafe {
            command.pre_exec(move || {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL, 0, 0, 0) != 0 {
                    return Err(io::Error::last_os_error());
                }
                if libc::getppid() != parent {
                    return Err(io::Error::from_raw_os_error(libc::ECHILD));
                }
                if fd != 3 && libc::dup2(fd, 3) < 0 {
                    return Err(io::Error::last_os_error());
                }
                if libc::fcntl(3, libc::F_SETFD, 0) < 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command.spawn()?;
        drop(worker);
        let mut worker = Self {
            child,
            stream,
            reader: codec::Reader::default(),
            attempt,
            finished: false,
            started: Instant::now(),
        };
        codec::write(&mut worker.stream, codec::Frame::Begin(attempt))?;
        worker.stream.set_nonblocking(true)?;
        Ok(worker)
    }
    pub fn poll(&mut self) -> io::Result<Option<Event>> {
        if self.finished {
            return Ok(None);
        }
        if self.started.elapsed() >= AUTHENTICATION_TIMEOUT {
            return Err(io::ErrorKind::TimedOut.into());
        }
        match self.reader.poll(&mut self.stream)? {
            Some(codec::Frame::Prompt(attempt, prompt)) if attempt == self.attempt => {
                Ok(Some(Event::Prompt(attempt, prompt)))
            }
            Some(codec::Frame::Finished(attempt, success)) if attempt == self.attempt => {
                self.finished = true;
                Ok(Some(Event::Finished(attempt, success)))
            }
            None => Ok(None),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unexpected authentication worker frame",
            )),
        }
    }
    pub fn respond(&mut self, response: Secret) -> io::Result<()> {
        codec::write(
            &mut self.stream,
            codec::Frame::Response(self.attempt, response),
        )
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        // The worker owns a private process group; cancellation also stops PAM helpers.
        unsafe {
            libc::kill(-(self.child.id() as i32), libc::SIGKILL);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn harden_process() -> io::Result<()> {
    let limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // Authentication input must not enter core dumps or same-UID debugger attachments.
    if unsafe { libc::setrlimit(libc::RLIMIT_CORE, &limit) } != 0
        || unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) } != 0
    {
        return Err(io::Error::last_os_error());
    }
    if unsafe { libc::getuid() != libc::geteuid() || libc::getgid() != libc::getegid() } {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "authentication applications must not be setuid or setgid",
        ));
    }
    Ok(())
}

pub fn run_worker(fd: i32) -> io::Result<()> {
    harden_process()?;
    worker(&mut private_stream(fd)?, None)
}

fn private_stream(fd: i32) -> io::Result<UnixStream> {
    if fd != 3 {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
    let mut size = std::mem::size_of_val(&credentials) as libc::socklen_t;
    // FD 3 is transferred by the parent; verify its socket peer before taking ownership.
    if unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut credentials as *mut libc::ucred).cast(),
            &mut size,
        )
    } != 0
        || credentials.uid != unsafe { libc::getuid() }
    {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
        return Err(io::Error::last_os_error());
    }
    let stream = unsafe { UnixStream::from_raw_fd(fd) };
    stream.set_read_timeout(Some(AUTHENTICATION_TIMEOUT))?;
    Ok(stream)
}

fn worker(stream: &mut UnixStream, configuration: Option<&std::ffi::CStr>) -> io::Result<()> {
    if configuration.is_none() {
        ensure_service()?;
    }
    let codec::Frame::Begin(attempt) = codec::read(stream)? else {
        return Err(io::ErrorKind::InvalidData.into());
    };
    let user = current_user()?;
    let success = pam::authenticate(stream, attempt, &user, configuration);
    codec::write(stream, codec::Frame::Finished(attempt, success))
}

#[cfg(test)]
mod tests;
