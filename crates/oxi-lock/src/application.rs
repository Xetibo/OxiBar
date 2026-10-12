use crate::backend::{Action, Application, Event, Output};
use iced::{
    Color, Element, Event as IcedEvent,
    keyboard::{Key, key::Named},
};
use oxi_auth_ui::{
    Interaction, Phase,
    view::{self, Message},
};
use oxiced::theme::theme_impl::OXITHEME;

pub struct LockScreen {
    identity: String,
    interaction: Interaction,
    worker: Option<oxi_pam::Worker>,
    locked: bool,
}
impl LockScreen {
    pub fn new() -> std::io::Result<Self> {
        Ok(Self {
            identity: oxi_pam::current_user()?,
            interaction: Interaction::default(),
            worker: None,
            locked: false,
        })
    }
    fn start(&mut self) {
        if !self.locked {
            return;
        }
        if let Some(attempt) = self.interaction.begin() {
            match oxi_pam::Worker::start(attempt) {
                Ok(worker) => self.worker = Some(worker),
                Err(_) => self.failed(),
            }
        }
    }
    fn failed(&mut self) {
        self.worker = None;
        self.interaction.cancel();
        self.interaction.notice = "Authentication unavailable. Press Enter to retry.".into();
    }
    fn submit(&mut self) {
        if self.interaction.phase == Phase::Idle {
            self.start();
        } else if let Some(response) = self.interaction.respond()
            && let Some(worker) = &mut self.worker
            && worker.respond(response).is_err()
        {
            self.failed();
        }
    }
}
impl Application for LockScreen {
    type Message = Message;
    fn update(&mut self, event: Event<Message>) -> Action {
        match event {
            Event::Locked => {
                self.locked = true;
                self.start();
            }
            Event::Message(Message::Submit) => self.submit(),
            Event::Message(Message::Cancel) => {
                self.worker = None;
                self.interaction.cancel();
            }
            Event::Input {
                event:
                    IcedEvent::Keyboard(iced::keyboard::Event::KeyPressed {
                        key,
                        modifiers,
                        text,
                        ..
                    }),
                ..
            } => match key.as_ref() {
                Key::Named(Named::Escape) => {
                    self.worker = None;
                    self.interaction.cancel();
                }
                Key::Named(Named::Enter) => self.submit(),
                Key::Named(Named::Backspace) if self.interaction.phase == Phase::Prompt => {
                    self.interaction.input.pop()
                }
                Key::Character("u") if modifiers.control() => self.interaction.input.clear(),
                _ if self.interaction.phase == Phase::Prompt
                    && !modifiers.control()
                    && !modifiers.alt()
                    && !modifiers.logo() =>
                {
                    if let Some(text) = text {
                        self.interaction.input.push(&text);
                    }
                }
                _ => return Action::None,
            },
            Event::Tick(_) => {
                let mut changed = false;
                loop {
                    let event = match self.worker.as_mut().map(oxi_pam::Worker::poll) {
                        Some(Ok(Some(event))) => event,
                        Some(Err(_)) => {
                            self.failed();
                            return Action::Redraw;
                        }
                        _ => break,
                    };
                    changed = true;
                    match event {
                        oxi_pam::Event::Prompt(attempt, prompt) => {
                            self.interaction.receive(attempt, prompt);
                        }
                        oxi_pam::Event::Finished(attempt, success) => {
                            self.worker = None;
                            if self.interaction.complete(attempt, success) {
                                return Action::AuthenticatedUnlock;
                            }
                        }
                    }
                }
                return if changed {
                    Action::Redraw
                } else {
                    Action::None
                };
            }
            _ => return Action::None,
        }
        Action::Redraw
    }
    fn view(&self, _output: &Output) -> Element<'_, Message> {
        view::prompt(&self.identity, &self.interaction, self.locked)
    }
    fn background(&self) -> Color {
        Color {
            a: 1.0,
            ..OXITHEME.base
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ui_actions_before_protocol_readiness_never_begin_authentication_or_unlock() {
        let mut app = LockScreen {
            identity: "Private fixture".into(),
            interaction: Interaction::default(),
            worker: None,
            locked: false,
        };
        assert_eq!(app.update(Event::Message(Message::Submit)), Action::Redraw);
        assert!(app.worker.is_none());
        assert!(app.interaction.attempt.is_none());
        assert_eq!(app.update(Event::Message(Message::Cancel)), Action::Redraw);
        assert_eq!(
            app.update(Event::Tick(std::time::Instant::now())),
            Action::None
        );
        assert!(!app.locked);
    }
}
