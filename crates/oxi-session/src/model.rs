use crate::{
    Handoff, Phase,
    catalog::{Catalog, Session},
    ipc::{Client, Operation, Response},
};
use oxi_auth_ui::{Attempt, Interaction, Phase as AuthPhase, Prompt, PromptKind};
use std::io;

pub struct Model {
    pub(crate) client: Option<Client>,
    pub(crate) interaction: Interaction,
    pub(crate) handoff: Handoff,
    pub(crate) username: String,
    pub(crate) sessions: Vec<Session>,
    pub(crate) selected: Option<Session>,
    pub(crate) cancelling: bool,
    pub(crate) retry_prompt: bool,
}
impl Model {
    pub fn new(client: Client, catalog: Catalog) -> Self {
        let selected = catalog.sessions.first().cloned();
        let mut interaction = Interaction::default();
        if selected.is_none() {
            interaction.notice = "No trusted Wayland sessions found.".into();
        }
        Self {
            client: Some(client),
            interaction,
            handoff: Handoff::default(),
            username: String::new(),
            sessions: catalog.sessions,
            selected,
            cancelling: false,
            retry_prompt: false,
        }
    }
    pub fn can_choose_user(&self) -> bool {
        self.interaction.attempt.is_none() && !self.cancelling && !self.busy()
    }
    pub fn can_choose_session(&self) -> bool {
        (matches!(self.handoff.phase(), Phase::Choosing | Phase::Authenticated)
            || (self.handoff.phase() == Phase::Authenticating
                && self.interaction.phase == AuthPhase::Prompt))
            && !self.cancelling
            && !self.busy()
    }
    pub fn busy(&self) -> bool {
        self.client.as_ref().is_some_and(Client::busy)
    }
    pub fn set_username(&mut self, username: String) {
        if self.can_choose_user() {
            self.username = username
                .chars()
                .filter(|character| !character.is_control())
                .take(256)
                .collect();
            while self.username.len() > 256 {
                self.username.pop();
            }
        }
    }
    pub fn select(&mut self, session: Session) {
        if self.can_choose_session() && self.sessions.contains(&session) {
            self.selected = Some(session);
        }
    }
    pub fn reconnect(&mut self) {
        if self.client.is_none() {
            match Client::connect() {
                Ok(client) => {
                    self.client = Some(client);
                    self.interaction.notice.clear();
                }
                Err(_) => {
                    self.interaction.notice = "Login service unavailable. Try reconnecting.".into()
                }
            }
        }
    }
    pub fn submit(&mut self) {
        if self.client.is_none() {
            self.reconnect();
        }
        if self.cancelling || self.busy() || self.client.is_none() {
            return;
        }
        match self.interaction.phase {
            AuthPhase::Idle if self.selected.is_some() && !self.username.trim().is_empty() => {
                self.username = self.username.trim().to_owned();
                let Some(attempt) = self.interaction.begin() else {
                    return;
                };
                self.handoff.authenticating();
                let result = self
                    .client
                    .as_mut()
                    .expect("connected client")
                    .create(attempt, self.username.trim());
                if result.is_err() {
                    self.connection_failed();
                }
            }
            AuthPhase::Prompt => {
                let attempt = self.interaction.attempt.expect("active prompt");
                if let Some(response) = self.interaction.respond()
                    && self
                        .client
                        .as_mut()
                        .expect("connected client")
                        .respond(attempt, Some(&response))
                        .is_err()
                {
                    self.connection_failed();
                }
            }
            AuthPhase::Authenticated => self.start(),
            _ => {}
        }
    }
    pub(crate) fn prepare(&mut self) {
        if self.client.is_some()
            && self.can_choose_user()
            && self.interaction.phase == AuthPhase::Idle
        {
            let notice = std::mem::take(&mut self.interaction.notice);
            self.submit();
            if self.interaction.notice.is_empty() {
                self.interaction.notice = notice;
            }
        }
    }
    pub fn cancel(&mut self) {
        if matches!(self.handoff.phase(), Phase::Starting | Phase::Handoff) {
            return;
        }
        let active = self.interaction.cancel().is_some();
        self.retry_prompt = false;
        self.handoff.cancel();
        if active {
            self.cancelling = true;
            self.send_cancel();
        }
    }
    fn send_cancel(&mut self) {
        if self.cancelling
            && !self.busy()
            && self
                .client
                .as_mut()
                .is_none_or(|client| client.cancel().is_err())
        {
            self.connection_failed();
        }
    }
    fn start(&mut self) {
        let (Some(attempt), Some(session)) = (self.interaction.attempt, self.selected.as_ref())
        else {
            return;
        };
        if self.busy() || !self.handoff.start() {
            return;
        }
        if self
            .client
            .as_mut()
            .expect("connected client")
            .start(attempt, &session.cmd, &session.env)
            .is_err()
        {
            self.handoff.acknowledge(false);
            self.interaction.notice = "Selected session could not be started.".into();
        }
    }
    fn connection_failed(&mut self) {
        self.client = None;
        self.interaction.cancel();
        self.handoff.cancel();
        self.cancelling = false;
        self.retry_prompt = false;
        self.interaction.notice = "Login service disconnected. Reconnect to retry.".into();
    }
    pub fn poll(&mut self) -> bool {
        loop {
            let reply = match self.client.as_mut().map(Client::poll) {
                Some(Ok(Some(reply))) => reply,
                Some(Err(error)) => {
                    eprintln!(
                        "oxisession: login service request failed: {:?}",
                        error.kind()
                    );
                    self.connection_failed();
                    break;
                }
                _ => break,
            };
            let (operation, response) = reply;
            if self.cancelling && operation != Operation::Cancel {
                self.send_cancel();
                continue;
            }
            match operation {
                Operation::Cancel => {
                    self.cancelling = false;
                    match response {
                        Response::Success => {}
                        // greetd removes the configuring session before cancelling its worker.
                        // An exited PAM worker can return a cleanup error on a live connection.
                        Response::Error { description, .. } => {
                            if self.interaction.notice.is_empty() {
                                self.interaction.notice = description;
                            }
                        }
                        _ => self.connection_failed(),
                    }
                }
                Operation::Create(attempt) | Operation::Respond(attempt)
                    if self.interaction.attempt == Some(attempt) =>
                {
                    self.authentication_reply(attempt, operation, response)
                }
                Operation::Start(attempt)
                    if self.interaction.attempt == Some(attempt)
                        && self.handoff.phase() == Phase::Starting =>
                {
                    match response {
                        Response::Success => {
                            return self.handoff.acknowledge(true);
                        }
                        Response::Error {
                            error_type,
                            description,
                        } => {
                            self.handoff.acknowledge(false);
                            if error_type == "auth_error" {
                                self.cancel();
                            }
                            self.interaction.notice = description;
                        }
                        _ => self.connection_failed(),
                    }
                }
                _ => self.connection_failed(),
            }
        }
        self.handoff.phase() == Phase::Handoff
    }
    fn authentication_reply(&mut self, attempt: Attempt, operation: Operation, response: Response) {
        match response {
            Response::AuthMessage {
                auth_message_type,
                auth_message,
            } => {
                let kind = match auth_message_type.as_str() {
                    "secret" => PromptKind::Secret,
                    "visible" => PromptKind::Visible,
                    "info" => PromptKind::Info,
                    "error" => PromptKind::Error,
                    _ => {
                        self.connection_failed();
                        return;
                    }
                };
                self.interaction.receive(
                    attempt,
                    Prompt {
                        kind,
                        text: auth_message,
                    },
                );
                if matches!(kind, PromptKind::Info | PromptKind::Error)
                    && self
                        .client
                        .as_mut()
                        .expect("connected client")
                        .respond(attempt, None)
                        .is_err()
                {
                    self.connection_failed();
                }
            }
            Response::Success => {
                if self.interaction.complete(attempt, true) && self.handoff.authenticated() {
                    self.start();
                }
            }
            Response::Error { description, .. } => {
                self.cancel();
                self.retry_prompt = matches!(operation, Operation::Respond(_));
                self.interaction.notice = description;
            }
        }
    }
    pub fn input(
        &mut self,
        key: &iced::keyboard::Key,
        modifiers: iced::keyboard::Modifiers,
        text: Option<&str>,
    ) {
        use iced::keyboard::{Key, key::Named};
        match key.as_ref() {
            Key::Named(Named::Enter) => self.submit(),
            Key::Named(Named::Escape) => self.cancel(),
            Key::Named(Named::Backspace) if self.interaction.phase == AuthPhase::Prompt => {
                self.interaction.input.pop()
            }
            Key::Character("u") if modifiers.control() => self.interaction.input.clear(),
            _ if self.interaction.phase == AuthPhase::Prompt
                && !modifiers.control()
                && !modifiers.alt()
                && !modifiers.logo() =>
            {
                if let Some(text) = text {
                    self.interaction.input.push(text);
                }
            }
            _ => {}
        }
    }
}

pub fn harden_process() -> io::Result<()> {
    let limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // Apply process-local limits before rendering or collecting authentication input.
    if unsafe { libc::setrlimit(libc::RLIMIT_CORE, &limit) } != 0
        || unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0) } != 0
    {
        return Err(io::Error::last_os_error());
    }
    if unsafe { libc::getuid() != libc::geteuid() || libc::getgid() != libc::getegid() } {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "greeter must not be setuid or setgid",
        ));
    }
    Ok(())
}
