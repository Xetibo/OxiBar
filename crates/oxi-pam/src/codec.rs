use oxi_auth_ui::{Attempt, MAX_RESPONSE_BYTES, Prompt, PromptKind, Secret};
use std::io::{self, Read, Write};
use zeroize::{Zeroize, Zeroizing};

const MAX_FRAME: usize = MAX_RESPONSE_BYTES + 16;

#[derive(Debug)]
pub enum Frame {
    Begin(Attempt),
    Prompt(Attempt, Prompt),
    Response(Attempt, Secret),
    Finished(Attempt, bool),
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid authentication frame")
}

pub fn write(stream: &mut impl Write, frame: Frame) -> io::Result<()> {
    let mut bytes = Zeroizing::new(Vec::with_capacity(MAX_FRAME + 4));
    bytes.extend_from_slice(&[0; 4]);
    let (tag, attempt) = match &frame {
        Frame::Begin(id) => (0, id),
        Frame::Prompt(id, _) => (1, id),
        Frame::Response(id, _) => (2, id),
        Frame::Finished(id, _) => (3, id),
    };
    bytes.push(tag);
    bytes.extend_from_slice(&attempt.0.to_le_bytes());
    match frame {
        Frame::Begin(_) => {}
        Frame::Prompt(_, prompt) => {
            if prompt.text.len() > MAX_RESPONSE_BYTES {
                return Err(invalid());
            }
            bytes.push(match prompt.kind {
                PromptKind::Secret => 0,
                PromptKind::Visible => 1,
                PromptKind::Info => 2,
                PromptKind::Error => 3,
            });
            bytes.extend_from_slice(prompt.text.as_bytes());
        }
        Frame::Response(_, response) => bytes.extend_from_slice(response.expose().as_bytes()),
        Frame::Finished(_, success) => bytes.push(u8::from(success)),
    }
    let length = (bytes.len() - 4) as u32;
    bytes[..4].copy_from_slice(&length.to_le_bytes());
    stream.write_all(&bytes)
}

fn decode(bytes: &[u8]) -> io::Result<Frame> {
    if bytes.len() < 9 {
        return Err(invalid());
    }
    let attempt = Attempt(u64::from_le_bytes(
        bytes[1..9].try_into().map_err(|_| invalid())?,
    ));
    if attempt.0 == 0 {
        return Err(invalid());
    }
    let payload = &bytes[9..];
    match bytes[0] {
        0 if payload.is_empty() => Ok(Frame::Begin(attempt)),
        1 if !payload.is_empty() => {
            let kind = match payload[0] {
                0 => PromptKind::Secret,
                1 => PromptKind::Visible,
                2 => PromptKind::Info,
                3 => PromptKind::Error,
                _ => return Err(invalid()),
            };
            if payload.len() - 1 > MAX_RESPONSE_BYTES {
                return Err(invalid());
            }
            Ok(Frame::Prompt(
                attempt,
                Prompt {
                    kind,
                    text: std::str::from_utf8(&payload[1..])
                        .map_err(|_| invalid())?
                        .to_owned(),
                },
            ))
        }
        2 if payload.len() <= MAX_RESPONSE_BYTES => {
            let text = std::str::from_utf8(payload).map_err(|_| invalid())?;
            if text.chars().any(char::is_control) {
                return Err(invalid());
            }
            let mut secret = Secret::default();
            secret.push(text);
            Ok(Frame::Response(attempt, secret))
        }
        3 if payload == [0] || payload == [1] => Ok(Frame::Finished(attempt, payload[0] == 1)),
        _ => Err(invalid()),
    }
}

#[derive(Default)]
pub struct Reader {
    bytes: Zeroizing<Vec<u8>>,
}
impl Reader {
    pub fn poll(&mut self, stream: &mut impl Read) -> io::Result<Option<Frame>> {
        if self.bytes.capacity() < MAX_FRAME + 4 {
            self.bytes.reserve_exact(MAX_FRAME + 4);
        }
        loop {
            if self.bytes.len() >= 4 {
                let length =
                    u32::from_le_bytes(self.bytes[..4].try_into().map_err(|_| invalid())?) as usize;
                if !(9..=MAX_FRAME).contains(&length) {
                    return Err(invalid());
                }
                if self.bytes.len() >= length + 4 {
                    let frame = decode(&self.bytes[4..length + 4]);
                    self.bytes.zeroize();
                    return frame.map(Some);
                }
            }
            let needed = if self.bytes.len() < 4 {
                4 - self.bytes.len()
            } else {
                u32::from_le_bytes(self.bytes[..4].try_into().map_err(|_| invalid())?) as usize + 4
                    - self.bytes.len()
            };
            let mut chunk = Zeroizing::new([0u8; MAX_FRAME]);
            match stream.read(&mut chunk[..needed.min(MAX_FRAME)]) {
                Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(count) => self.bytes.extend_from_slice(&chunk[..count]),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(None),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
    }
}

pub fn read(stream: &mut impl Read) -> io::Result<Frame> {
    Reader::default().poll(stream)?.ok_or_else(invalid)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn partial_io_is_bounded_and_secret_debug_is_redacted() {
        let mut secret = Secret::default();
        secret.push("private-response");
        let frame = Frame::Response(Attempt(9), secret);
        assert!(!format!("{frame:?}").contains("private-response"));
        let mut bytes = Vec::new();
        write(&mut bytes, frame).unwrap();
        let Frame::Response(id, response) = read(&mut bytes.as_slice()).unwrap() else {
            panic!()
        };
        assert_eq!(id, Attempt(9));
        assert_eq!(response.expose(), "private-response");
        assert!(read(&mut u32::MAX.to_le_bytes().as_slice()).is_err());
        bytes[4] = 90;
        assert!(read(&mut bytes.as_slice()).is_err());
    }

    #[test]
    fn nonblocking_partial_frames_preserve_order_without_losing_bytes() {
        struct Partial {
            bytes: std::io::Cursor<Vec<u8>>,
            block: bool,
        }
        impl Read for Partial {
            fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
                self.block = !self.block;
                if self.block {
                    return Err(io::ErrorKind::WouldBlock.into());
                }
                let length = bytes.len().min(3);
                self.bytes.read(&mut bytes[..length])
            }
        }
        let mut bytes = Vec::new();
        write(
            &mut bytes,
            Frame::Prompt(
                Attempt(2),
                Prompt {
                    kind: PromptKind::Secret,
                    text: "Password".into(),
                },
            ),
        )
        .unwrap();
        write(&mut bytes, Frame::Finished(Attempt(2), false)).unwrap();
        let mut stream = Partial {
            bytes: std::io::Cursor::new(bytes),
            block: false,
        };
        let mut reader = Reader::default();
        let mut received = Vec::new();
        while received.len() < 2 {
            if let Some(frame) = reader.poll(&mut stream).unwrap() {
                received.push(frame);
            }
        }
        assert!(
            matches!(&received[0], Frame::Prompt(Attempt(2), prompt) if prompt.text == "Password")
        );
        assert!(matches!(received[1], Frame::Finished(Attempt(2), false)));
    }
}
