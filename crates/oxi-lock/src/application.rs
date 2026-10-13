use crate::backend::{Action, Application, Event, Output};
use iced::{
    Color, Element, Event as IcedEvent,
    keyboard::{Key, key::Named},
};
use oxi_auth_ui::{
    Interaction, Phase,
    view::{self, Focus, Message},
    visuals::{KeyboardStatus, Visuals},
};
use oxiced::theme::theme_impl::OXITHEME;

pub struct LockScreen {
    identity: String,
    interaction: Interaction,
    worker: Option<oxi_pam::Worker>,
    locked: bool,
    visuals: Visuals,
    keyboard: KeyboardStatus,
    focus: Focus,
}
impl LockScreen {
    pub fn new() -> std::io::Result<Self> {
        Ok(Self {
            identity: oxi_pam::current_user()?,
            interaction: Interaction::default(),
            worker: None,
            locked: false,
            visuals: Visuals::load(view::Purpose::Lock),
            keyboard: KeyboardStatus::default(),
            focus: Focus::Submit,
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
        self.focus = Focus::Submit;
    }
    fn cancel(&mut self) {
        self.worker = None;
        self.interaction.cancel();
        self.focus = Focus::Submit;
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
            Event::KeyboardStatus(status) => self.keyboard = status,
            Event::Message(Message::Submit) => {
                self.focus = Focus::Submit;
                self.submit();
            }
            Event::Message(Message::Cancel) => self.cancel(),
            Event::Message(Message::Focus(focus)) => {
                if focus.enabled(&self.interaction, self.locked) {
                    self.focus = focus;
                }
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
                    self.cancel();
                }
                Key::Named(Named::Tab) => {
                    self.focus = self
                        .focus
                        .next(&self.interaction, self.locked, modifiers.shift());
                }
                Key::Named(Named::Enter) => {
                    if self.focus == Focus::Cancel {
                        self.cancel();
                    } else {
                        self.submit();
                    }
                }
                Key::Named(Named::Space) | Key::Character(" ") if self.focus != Focus::Input => {
                    if self.focus == Focus::Cancel {
                        self.cancel();
                    } else {
                        self.submit();
                    }
                }
                Key::Named(Named::Backspace)
                    if self.interaction.phase == Phase::Prompt && self.focus == Focus::Input =>
                {
                    self.interaction.input.pop()
                }
                Key::Character("u") if modifiers.control() && self.focus == Focus::Input => {
                    self.interaction.input.clear()
                }
                _ if self.interaction.phase == Phase::Prompt
                    && self.focus == Focus::Input
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
                let mut changed = self.visuals.tick();
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
                            if self.interaction.receive(attempt, prompt)
                                && self.interaction.phase == Phase::Prompt
                            {
                                self.focus = Focus::Input;
                            }
                        }
                        oxi_pam::Event::Finished(attempt, success) => {
                            self.worker = None;
                            if self.interaction.complete(attempt, success) {
                                return Action::AuthenticatedUnlock;
                            }
                            self.focus = Focus::Submit;
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
        use iced::{
            Length,
            widget::{Column, container},
        };
        let mut content = Column::new()
            .spacing(OXITHEME.padding_lg)
            .align_x(iced::Alignment::Center)
            .push(self.visuals.heading())
            .push(view::prompt_card_focused(
                &self.identity,
                &self.interaction,
                self.locked,
                view::Purpose::Lock,
                Some(self.focus),
            ))
            .push(self.keyboard.view());
        if !self.visuals.notice.is_empty() {
            content = content.push(
                oxiced::widgets::oxi_plugin::text(self.visuals.notice.clone())
                    .size(OXITHEME.font_sm),
            );
        }
        self.visuals.scene(
            container(oxiced::widgets::oxi_plugin::scroll_panel(content).height(Length::Shrink))
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .padding(OXITHEME.padding_lg)
                .into(),
        )
    }
    fn theme(&self) -> iced::Theme {
        oxiced::theme::theme_impl::get_derived_iced_theme()
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
    fn fixture() -> LockScreen {
        LockScreen {
            identity: "Private fixture".into(),
            interaction: Interaction::default(),
            worker: None,
            locked: false,
            visuals: Visuals::default(),
            keyboard: KeyboardStatus::default(),
            focus: Focus::Submit,
        }
    }
    fn press(
        app: &mut LockScreen,
        key: Key,
        text: Option<&str>,
        modifiers: iced::keyboard::Modifiers,
    ) {
        app.update(Event::Input {
            output: 1,
            event: IcedEvent::Keyboard(iced::keyboard::Event::KeyPressed {
                modified_key: key.clone(),
                key,
                physical_key: iced::keyboard::key::Physical::Unidentified(
                    iced::keyboard::key::NativeCode::Unidentified,
                ),
                location: iced::keyboard::Location::Standard,
                modifiers,
                text: text.map(Into::into),
                repeat: false,
            }),
        });
    }
    #[test]
    fn password_click_focus_tab_reverse_tab_and_clear_activation_do_not_unlock() {
        let mut app = fixture();
        app.locked = true;
        let attempt = app.interaction.begin().unwrap();
        app.interaction.receive(
            attempt,
            oxi_auth_ui::Prompt {
                kind: oxi_auth_ui::PromptKind::Secret,
                text: "Private password".into(),
            },
        );
        app.update(Event::Message(Message::Focus(Focus::Input)));
        press(
            &mut app,
            Key::Character("x".into()),
            Some("private"),
            iced::keyboard::Modifiers::empty(),
        );
        assert_eq!(app.interaction.input.masked(), "•••••••");
        press(
            &mut app,
            Key::Named(Named::Tab),
            None,
            iced::keyboard::Modifiers::empty(),
        );
        assert_eq!(app.focus, Focus::Submit);
        press(
            &mut app,
            Key::Character("x".into()),
            Some("ignored"),
            iced::keyboard::Modifiers::empty(),
        );
        assert_eq!(app.interaction.input.masked(), "•••••••");
        press(
            &mut app,
            Key::Named(Named::Tab),
            None,
            iced::keyboard::Modifiers::empty(),
        );
        assert_eq!(app.focus, Focus::Cancel);
        press(
            &mut app,
            Key::Named(Named::Tab),
            None,
            iced::keyboard::Modifiers::SHIFT,
        );
        assert_eq!(app.focus, Focus::Submit);
        app.update(Event::Message(Message::Focus(Focus::Input)));
        press(
            &mut app,
            Key::Named(Named::Tab),
            None,
            iced::keyboard::Modifiers::SHIFT,
        );
        assert_eq!(app.focus, Focus::Cancel);
        press(
            &mut app,
            Key::Named(Named::Enter),
            None,
            iced::keyboard::Modifiers::empty(),
        );
        assert_eq!(app.interaction.phase, Phase::Idle);
        assert!(app.interaction.input.is_empty());
        assert!(app.interaction.attempt.is_none());
        assert!(app.locked);
    }
    #[test]
    fn ui_actions_before_protocol_readiness_never_begin_authentication_or_unlock() {
        let mut app = fixture();
        assert_eq!(app.update(Event::Message(Message::Submit)), Action::Redraw);
        assert!(app.worker.is_none());
        assert!(app.interaction.attempt.is_none());
        assert_eq!(app.update(Event::Message(Message::Cancel)), Action::Redraw);
        assert_ne!(
            app.update(Event::Tick(std::time::Instant::now())),
            Action::AuthenticatedUnlock
        );
        assert!(!app.locked);
    }
}
