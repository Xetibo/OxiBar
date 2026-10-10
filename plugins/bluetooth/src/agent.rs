use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use iced::futures::channel::{mpsc, oneshot};
use zbus::{DBusError, message::Header, zvariant::OwnedObjectPath};

pub(crate) const AGENT_PATH: &str = "/org/oxibar/bluetooth_agent";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PromptKind {
    Pin,
    Passkey,
    DisplayPin(String),
    DisplayPasskey { passkey: u32, entered: u16 },
    Confirmation(u32),
    Authorization,
    Service(String),
}

impl PromptKind {
    pub(crate) fn needs_input(&self) -> bool {
        matches!(self, Self::Pin | Self::Passkey)
    }
    pub(crate) fn needs_confirmation(&self) -> bool {
        matches!(
            self,
            Self::Confirmation(_) | Self::Authorization | Self::Service(_)
        )
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Prompt {
    pub(crate) id: u64,
    pub(crate) device: String,
    pub(crate) kind: PromptKind,
}

#[derive(Clone, Debug)]
pub(crate) enum PairEvent {
    Prompt(Prompt),
    Progress(String),
    Finished(Result<(), String>),
}

#[derive(Clone, Debug)]
pub(crate) enum Reply {
    Text(String),
    Accept,
    Reject,
}

#[derive(Debug)]
enum Answer {
    Pin(String),
    Passkey(u32),
    Accept,
    Rejected,
    Canceled,
}

#[derive(Debug, DBusError)]
#[zbus(prefix = "org.bluez.Error")]
pub(crate) enum AgentError {
    Rejected(String),
    Canceled(String),
    #[zbus(error)]
    ZBus(zbus::Error),
}

#[derive(Debug)]
struct Waiting {
    prompt: Prompt,
    reply: Option<oneshot::Sender<Answer>>,
}

#[derive(Debug)]
struct PairSession {
    id: u64,
    device: String,
    deadline: Instant,
    events: mpsc::Sender<PairEvent>,
    cancel: Option<oneshot::Sender<()>>,
    finished: oneshot::Sender<Result<(), String>>,
    waiting: Option<Waiting>,
    connecting: bool,
}

#[derive(Debug, Default)]
struct State {
    owner: Option<String>,
    next_id: u64,
    session: Option<PairSession>,
}

#[derive(Debug)]
pub(crate) struct Hub {
    state: Mutex<State>,
    released: Mutex<Option<mpsc::UnboundedSender<()>>>,
}

type PairChannels = (
    u64,
    mpsc::Receiver<PairEvent>,
    oneshot::Receiver<()>,
    oneshot::Receiver<Result<(), String>>,
);

impl Hub {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State::default()),
            released: Mutex::new(None),
        })
    }

    pub(crate) fn listen_release(&self) -> mpsc::UnboundedReceiver<()> {
        let (sender, receiver) = mpsc::unbounded();
        *self.released.lock().unwrap() = Some(sender);
        receiver
    }

    pub(crate) fn set_owner(&self, owner: Option<String>) {
        self.cancel(None);
        self.state.lock().unwrap().owner = owner;
    }

    pub(crate) fn start(&self, device: String, timeout: Duration) -> Result<PairChannels, String> {
        let mut state = self.state.lock().unwrap();
        if state.owner.is_none() {
            return Err("bluetooth: BlueZ is unavailable".to_owned());
        }
        if state.session.is_some() {
            return Err("bluetooth: another pairing is active".to_owned());
        }
        state.next_id = state.next_id.wrapping_add(1);
        let id = state.next_id;
        let (events, receiver) = mpsc::channel(32);
        let (cancel, cancellation) = oneshot::channel();
        let (finished, completion) = oneshot::channel();
        state.session = Some(PairSession {
            id,
            device,
            deadline: Instant::now() + timeout,
            events,
            cancel: Some(cancel),
            finished,
            waiting: None,
            connecting: false,
        });
        Ok((id, receiver, cancellation, completion))
    }

    pub(crate) fn respond(&self, id: u64, reply: Reply) -> Result<(), String> {
        let mut state = self.state.lock().unwrap();
        let session = state
            .session
            .as_mut()
            .ok_or("bluetooth: pairing is no longer active")?;
        let waiting = session
            .waiting
            .as_mut()
            .filter(|waiting| waiting.prompt.id == id)
            .ok_or("bluetooth: pairing request is no longer active")?;
        let answer = validate_reply(&waiting.prompt.kind, reply)?;
        let sender = waiting
            .reply
            .take()
            .ok_or("bluetooth: this request has already been answered")?;
        sender
            .send(answer)
            .map_err(|_| "bluetooth: pairing request expired".to_owned())
    }

    pub(crate) fn cancel(&self, id: Option<u64>) {
        let mut state = self.state.lock().unwrap();
        if let Some(session) = state
            .session
            .as_mut()
            .filter(|session| id.is_none_or(|id| session.id == id))
        {
            if let Some(reply) = session
                .waiting
                .as_mut()
                .and_then(|waiting| waiting.reply.take())
            {
                let _ = reply.send(Answer::Canceled);
            }
            if let Some(cancel) = session.cancel.take() {
                let _ = cancel.send(());
            }
        }
    }

    pub(crate) fn finish(&self, id: u64, result: Result<(), String>) {
        let mut state = self.state.lock().unwrap();
        if state
            .session
            .as_ref()
            .is_some_and(|session| session.id == id)
        {
            let mut session = state.session.take().unwrap();
            if let Some(reply) = session
                .waiting
                .as_mut()
                .and_then(|waiting| waiting.reply.take())
            {
                let _ = reply.send(Answer::Canceled);
            }
            let _ = session.finished.send(result);
        }
    }

    pub(crate) fn active(&self) -> Option<(u64, String)> {
        self.state
            .lock()
            .unwrap()
            .session
            .as_ref()
            .map(|session| (session.id, session.device.clone()))
    }

    pub(crate) fn connecting(&self, id: u64) -> Result<(), String> {
        let mut state = self.state.lock().unwrap();
        let session = state
            .session
            .as_mut()
            .filter(|session| session.id == id && session.cancel.is_some())
            .ok_or("bluetooth: pairing canceled")?;
        session.connecting = true;
        Ok(())
    }

    pub(crate) fn needs_disconnect(&self, id: u64) -> bool {
        self.state
            .lock()
            .unwrap()
            .session
            .as_ref()
            .is_some_and(|session| session.id == id && session.connecting)
    }

    pub(crate) fn progress(&self, id: u64, label: &str) {
        if let Some(session) = self
            .state
            .lock()
            .unwrap()
            .session
            .as_mut()
            .filter(|session| session.id == id)
        {
            let _ = session
                .events
                .try_send(PairEvent::Progress(label.to_owned()));
        }
    }

    fn check(
        &self,
        header: &Header<'_>,
        device: Option<&OwnedObjectPath>,
    ) -> Result<(), AgentError> {
        let state = self.state.lock().unwrap();
        if state.owner.as_deref() != header.sender().map(|sender| sender.as_str())
            || state.owner.is_none()
        {
            return Err(AgentError::Rejected(
                "Only BlueZ may call this agent".to_owned(),
            ));
        }
        if let Some(device) = device
            && state
                .session
                .as_ref()
                .is_none_or(|session| session.device != device.as_str())
        {
            return Err(AgentError::Rejected(
                "No user-initiated pairing for this device".to_owned(),
            ));
        }
        Ok(())
    }

    fn prompt(
        &self,
        device: OwnedObjectPath,
        kind: PromptKind,
        reply: Option<oneshot::Sender<Answer>>,
    ) -> Result<(u64, Duration), AgentError> {
        let mut state = self.state.lock().unwrap();
        state.next_id = state.next_id.wrapping_add(1);
        let id = state.next_id;
        let session = state
            .session
            .as_mut()
            .filter(|session| session.device == device.as_str())
            .ok_or_else(|| AgentError::Canceled("Pairing ended".to_owned()))?;
        if session.cancel.is_none() {
            return Err(AgentError::Canceled("Pairing canceled".to_owned()));
        }
        if session
            .waiting
            .as_ref()
            .is_some_and(|waiting| waiting.reply.is_some())
        {
            return Err(AgentError::Rejected(
                "Another pairing reply is pending".to_owned(),
            ));
        }
        let prompt = Prompt {
            id,
            device: device.to_string(),
            kind,
        };
        session
            .events
            .try_send(PairEvent::Prompt(prompt.clone()))
            .map_err(|_| AgentError::Canceled("Pairing UI is unavailable".to_owned()))?;
        session.waiting = Some(Waiting { prompt, reply });
        Ok((
            session.id,
            session.deadline.saturating_duration_since(Instant::now()),
        ))
    }

    async fn request(
        &self,
        device: OwnedObjectPath,
        kind: PromptKind,
    ) -> Result<Answer, AgentError> {
        let (sender, receiver) = oneshot::channel();
        let (id, remaining) = self.prompt(device, kind, Some(sender))?;
        let result = super::worker::deadline(
            async { receiver.await.map_err(|_| "pairing canceled".to_owned()) },
            remaining,
            "pairing reply",
        )
        .await;
        match result {
            Ok(Answer::Rejected) => Err(AgentError::Rejected("Rejected by user".to_owned())),
            Ok(Answer::Canceled) | Err(_) => {
                self.cancel(Some(id));
                Err(AgentError::Canceled(
                    "Pairing canceled or expired".to_owned(),
                ))
            }
            Ok(answer) => Ok(answer),
        }
    }
}

fn validate_reply(kind: &PromptKind, reply: Reply) -> Result<Answer, String> {
    match (kind, reply) {
        (PromptKind::Pin, Reply::Text(value))
            if !value.is_empty()
                && value.len() <= 16
                && value
                    .chars()
                    .all(|ch| ch.is_ascii() && !ch.is_ascii_control()) =>
        {
            Ok(Answer::Pin(value))
        }
        (PromptKind::Passkey, Reply::Text(value))
            if !value.is_empty()
                && value.len() <= 6
                && value.chars().all(|ch| ch.is_ascii_digit()) =>
        {
            Ok(Answer::Passkey(value.parse().unwrap()))
        }
        (kind, Reply::Accept) if kind.needs_confirmation() => Ok(Answer::Accept),
        (kind, Reply::Reject) if kind.needs_input() || kind.needs_confirmation() => {
            Ok(Answer::Rejected)
        }
        (PromptKind::Pin, _) => Err("Enter a PIN of 1–16 ASCII characters".to_owned()),
        (PromptKind::Passkey, _) => Err("Enter a numeric passkey of 1–6 digits".to_owned()),
        _ => Err("This pairing request does not accept that reply".to_owned()),
    }
}

pub(crate) struct Agent(pub(crate) Arc<Hub>);

#[zbus::interface(name = "org.bluez.Agent1")]
impl Agent {
    async fn request_pin_code(
        &self,
        device: OwnedObjectPath,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<String, AgentError> {
        self.0.check(&header, Some(&device))?;
        match self.0.request(device, PromptKind::Pin).await? {
            Answer::Pin(value) => Ok(value),
            _ => unreachable!(),
        }
    }

    async fn request_passkey(
        &self,
        device: OwnedObjectPath,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<u32, AgentError> {
        self.0.check(&header, Some(&device))?;
        match self.0.request(device, PromptKind::Passkey).await? {
            Answer::Passkey(value) => Ok(value),
            _ => unreachable!(),
        }
    }

    fn display_pin_code(
        &self,
        device: OwnedObjectPath,
        pincode: String,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<(), AgentError> {
        self.0.check(&header, Some(&device))?;
        self.0
            .prompt(device, PromptKind::DisplayPin(pincode), None)?;
        Ok(())
    }

    fn display_passkey(
        &self,
        device: OwnedObjectPath,
        passkey: u32,
        entered: u16,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<(), AgentError> {
        self.0.check(&header, Some(&device))?;
        validate_passkey(passkey, entered)?;
        self.0.prompt(
            device,
            PromptKind::DisplayPasskey { passkey, entered },
            None,
        )?;
        Ok(())
    }

    async fn request_confirmation(
        &self,
        device: OwnedObjectPath,
        passkey: u32,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<(), AgentError> {
        self.0.check(&header, Some(&device))?;
        validate_passkey(passkey, 0)?;
        self.0
            .request(device, PromptKind::Confirmation(passkey))
            .await?;
        Ok(())
    }

    async fn request_authorization(
        &self,
        device: OwnedObjectPath,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<(), AgentError> {
        self.0.check(&header, Some(&device))?;
        self.0.request(device, PromptKind::Authorization).await?;
        Ok(())
    }

    async fn authorize_service(
        &self,
        device: OwnedObjectPath,
        uuid: String,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<(), AgentError> {
        self.0.check(&header, Some(&device))?;
        self.0.request(device, PromptKind::Service(uuid)).await?;
        Ok(())
    }

    fn cancel(&self, #[zbus(header)] header: Header<'_>) -> Result<(), AgentError> {
        self.0.check(&header, None)?;
        self.0.cancel(None);
        Ok(())
    }

    fn release(&self, #[zbus(header)] header: Header<'_>) -> Result<(), AgentError> {
        self.0.check(&header, None)?;
        self.0.cancel(None);
        if let Some(sender) = self.0.released.lock().unwrap().as_ref() {
            let _ = sender.unbounded_send(());
        }
        Ok(())
    }
}

fn validate_passkey(passkey: u32, entered: u16) -> Result<(), AgentError> {
    if passkey <= 999_999 && entered <= 6 {
        Ok(())
    } else {
        Err(AgentError::Rejected(
            "Invalid six-digit passkey or entered count".to_owned(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::futures::FutureExt;

    #[test]
    fn validates_typed_replies_and_preserves_leading_zero_passkeys() {
        assert!(matches!(
            validate_reply(&PromptKind::Passkey, Reply::Text("000123".to_owned())),
            Ok(Answer::Passkey(123))
        ));
        for invalid in ["", "1234567", "12x34", "-1", "١٢٣"] {
            assert!(validate_reply(&PromptKind::Passkey, Reply::Text(invalid.to_owned())).is_err());
        }
        assert!(validate_reply(&PromptKind::Pin, Reply::Text("abcd-1234".to_owned())).is_ok());
        assert!(validate_reply(&PromptKind::Pin, Reply::Text("\n".to_owned())).is_err());
        assert!(
            validate_reply(
                &PromptKind::Confirmation(123),
                Reply::Text("123".to_owned())
            )
            .is_err()
        );
    }

    #[test]
    fn stale_replies_and_cancellation_cannot_affect_another_session() {
        let hub = Hub::new();
        hub.set_owner(Some(":1.1".to_owned()));
        let (id, _events, cancellation, _) = hub
            .start("/org/bluez/hci0/dev_1".to_owned(), Duration::from_secs(90))
            .unwrap();
        assert!(hub.respond(99, Reply::Accept).is_err());
        hub.cancel(Some(id + 1));
        assert!(cancellation.now_or_never().is_none());
        hub.cancel(Some(id));
        hub.finish(id, Err("canceled".to_owned()));
        assert!(hub.active().is_none());
    }

    #[test]
    fn expired_old_agent_request_cannot_cancel_a_new_pairing() {
        let hub = Hub::new();
        hub.set_owner(Some(":1.1".to_owned()));
        let path = OwnedObjectPath::try_from("/org/bluez/hci0/dev_1").unwrap();
        let (old, _events, _cancel, _completion) = hub
            .start(path.to_string(), Duration::from_secs(90))
            .unwrap();
        let request = hub.request(path, PromptKind::Pin);
        iced::futures::pin_mut!(request);
        assert!(request.as_mut().now_or_never().is_none());
        hub.finish(old, Err("old pairing ended".to_owned()));
        let (new, _events, mut cancel, _completion) = hub
            .start("/org/bluez/hci0/dev_2".to_owned(), Duration::from_secs(90))
            .unwrap();
        assert!(iced::futures::executor::block_on(request).is_err());
        assert_eq!(hub.active().unwrap().0, new);
        assert!(cancel.try_recv().unwrap().is_none());
        assert!(validate_passkey(1_000_000, 0).is_err());
        assert!(validate_passkey(123, 7).is_err());
    }
}
