use oxi_auth_ui::{Attempt, Secret};
use serde::{Deserialize, Serialize};
use std::{
    env,
    io::{self, Read, Write},
    os::{fd::AsRawFd, unix::net::UnixStream},
    time::{Duration, Instant},
};
use zeroize::{Zeroize, Zeroizing};

pub const MAX_MESSAGE: usize = 65_536;
const TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Create(Attempt),
    Respond(Attempt),
    Cancel,
    Start(Attempt),
}

#[derive(Deserialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Success,
    AuthMessage {
        auth_message_type: String,
        auth_message: String,
    },
    Error {
        error_type: String,
        description: String,
    },
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Request<'a> {
    CreateSession {
        username: &'a str,
    },
    PostAuthMessageResponse {
        response: Option<&'a str>,
    },
    CancelSession,
    StartSession {
        cmd: &'a [String],
        env: &'a [String],
    },
}

pub struct Client {
    stream: UnixStream,
    pending: Option<(Operation, Instant)>,
    outgoing: Zeroizing<Vec<u8>>,
    written: usize,
    incoming: Zeroizing<Vec<u8>>,
    read: usize,
    expected: usize,
}

impl Client {
    pub fn connect() -> io::Result<Self> {
        let path = env::var_os("GREETD_SOCK").ok_or_else(|| invalid("GREETD_SOCK is not set"))?;
        let stream = UnixStream::connect(path)?;
        let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
        let mut size = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        // The socket is owned by this process; SO_PEERCRED authenticates the root daemon.
        if unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&mut credentials as *mut libc::ucred).cast(),
                &mut size,
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        if credentials.uid != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "greetd peer is not root",
            ));
        }
        Self::from_stream(stream)
    }

    fn from_stream(stream: UnixStream) -> io::Result<Self> {
        stream.set_nonblocking(true)?;
        Ok(Self {
            stream,
            pending: None,
            outgoing: Zeroizing::new(Vec::with_capacity(MAX_MESSAGE + 4)),
            written: 0,
            incoming: Zeroizing::new(vec![0; MAX_MESSAGE + 4]),
            read: 0,
            expected: 4,
        })
    }
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }
    pub fn create(&mut self, attempt: Attempt, username: &str) -> io::Result<()> {
        if username.is_empty()
            || username.len() > 256
            || username.chars().any(char::is_control)
            || attempt.0 == 0
        {
            return Err(invalid("invalid username or attempt"));
        }
        self.send(
            Operation::Create(attempt),
            Request::CreateSession { username },
        )
    }
    pub fn respond(&mut self, attempt: Attempt, response: Option<&Secret>) -> io::Result<()> {
        self.send(
            Operation::Respond(attempt),
            Request::PostAuthMessageResponse {
                response: response.map(Secret::expose),
            },
        )
    }
    pub fn cancel(&mut self) -> io::Result<()> {
        self.send(Operation::Cancel, Request::CancelSession)
    }
    pub fn start(&mut self, attempt: Attempt, cmd: &[String], env: &[String]) -> io::Result<()> {
        if cmd.is_empty()
            || cmd.len() > 128
            || env.len() > 32
            || cmd
                .iter()
                .chain(env)
                .any(|value| value.contains('\0') || value.len() > 4096)
        {
            return Err(invalid("invalid session arguments"));
        }
        self.send(
            Operation::Start(attempt),
            Request::StartSession { cmd, env },
        )
    }
    fn send(&mut self, operation: Operation, request: Request<'_>) -> io::Result<()> {
        if self.busy() {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "greetd request already pending",
            ));
        }
        self.outgoing.zeroize();
        self.outgoing.extend_from_slice(&[0; 4]);
        serde_json::to_writer(Bounded(&mut self.outgoing), &request)
            .map_err(|_| invalid("greetd request exceeds frame limit"))?;
        let length = (self.outgoing.len() - 4) as u32;
        self.outgoing[..4].copy_from_slice(&length.to_ne_bytes());
        self.written = 0;
        self.pending = Some((operation, Instant::now()));
        Ok(())
    }
    pub fn poll(&mut self) -> io::Result<Option<(Operation, Response)>> {
        let Some((operation, started)) = self.pending else {
            return Ok(None);
        };
        if started.elapsed() > TIMEOUT {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "greetd request timed out",
            ));
        }
        while self.written < self.outgoing.len() {
            match self.stream.write(&self.outgoing[self.written..]) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "greetd socket closed",
                    ));
                }
                Ok(count) => self.written += count,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(None),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            }
        }
        self.outgoing.zeroize();
        loop {
            match self
                .stream
                .read(&mut self.incoming[self.read..self.expected])
            {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "greetd socket closed",
                    ));
                }
                Ok(count) => self.read += count,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(None),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            }
            if self.read != self.expected {
                continue;
            }
            if self.expected == 4 {
                let length = u32::from_ne_bytes(
                    self.incoming[..4]
                        .try_into()
                        .expect("four-byte frame header"),
                ) as usize;
                if length == 0 || length > MAX_MESSAGE {
                    return Err(invalid("invalid greetd frame length"));
                }
                self.expected += length;
                continue;
            }
            let response: Response = serde_json::from_slice(&self.incoming[4..self.expected])
                .map_err(|_| invalid("invalid greetd response"))?;
            match &response {
                Response::AuthMessage {
                    auth_message_type,
                    auth_message,
                } if auth_message_type.len() > 16 || auth_message.len() > 4096 => {
                    return Err(invalid("oversized greetd prompt"));
                }
                Response::Error {
                    error_type,
                    description,
                } if error_type.len() > 32 || description.len() > 4096 => {
                    return Err(invalid("oversized greetd error"));
                }
                _ => {}
            }
            self.incoming.as_mut_slice().zeroize();
            self.read = 0;
            self.expected = 4;
            self.pending = None;
            return Ok(Some((operation, response)));
        }
    }
}

struct Bounded<'a>(&'a mut Vec<u8>);
impl Write for Bounded<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.0.len() + bytes.len() > MAX_MESSAGE + 4 {
            return Err(invalid("oversized greetd request"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
pub(crate) fn fixture(stream: UnixStream) -> Client {
    Client::from_stream(stream).unwrap()
}
