use crate::{
    Phase,
    catalog::{Catalog, Session},
    ipc::Client,
    model::Model,
};
use iced::{
    Element, Length, Subscription, Task, Theme, keyboard,
    widget::{Column, container},
};
use oxi_auth_ui::view::{self, Purpose};
use oxiced::{
    theme::theme_impl::{OXITHEME, get_derived_iced_theme},
    widgets::{oxi_picklist, oxi_plugin, oxi_text_input},
};
use std::{
    fmt,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

#[derive(Clone)]
enum Message {
    Username(String),
    Session(Session),
    Auth(view::Message),
    Key(keyboard::Event),
    Tick,
    Reconnect,
}
impl fmt::Debug for Message {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Username(_) => "Username",
            Self::Session(_) => "Session",
            Self::Auth(_) => "Authentication action",
            Self::Key(_) => "Keyboard input [REDACTED]",
            Self::Tick => "Tick",
            Self::Reconnect => "Reconnect",
        })
    }
}
struct Greeter {
    model: Model,
    accepted: Arc<AtomicBool>,
}
const USERNAME_ID: &str = "oxisession-username";
impl Greeter {
    fn update(&mut self, message: Message) -> Task<Message> {
        let could_choose_user = self.model.can_choose_user();
        match message {
            Message::Username(username) => self.model.set_username(username),
            Message::Session(session) => self.model.select(session),
            Message::Auth(view::Message::Submit) => self.model.submit(),
            Message::Auth(view::Message::Cancel) => self.model.cancel(),
            Message::Key(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Tab),
                modifiers,
                ..
            }) => {
                return if modifiers.shift() {
                    iced::widget::operation::focus_previous()
                } else {
                    iced::widget::operation::focus_next()
                };
            }
            Message::Key(keyboard::Event::KeyPressed {
                key,
                modifiers,
                text,
                ..
            }) => self.model.input(&key, modifiers, text.as_deref()),
            Message::Tick if self.model.poll() => {
                self.accepted.store(true, Ordering::Release);
                return iced::exit();
            }
            Message::Reconnect => self.model.reconnect(),
            _ => {}
        }
        if !could_choose_user && self.model.can_choose_user() {
            iced::widget::operation::focus(USERNAME_ID)
        } else {
            Task::none()
        }
    }
    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            iced::time::every(Duration::from_millis(30)).map(|_| Message::Tick),
            iced::event::listen_with(|event, status, _| match event {
                iced::Event::Keyboard(event) if status == iced::event::Status::Ignored => {
                    Some(Message::Key(event))
                }
                _ => None,
            }),
        ])
    }
    fn view(&self) -> Element<'_, Message> {
        let mut content = Column::new()
            .spacing(OXITHEME.padding_lg)
            .push(oxi_plugin::text("Welcome").size(OXITHEME.font_xxl));
        if self.model.can_choose_user() {
            content = content.push(
                oxi_text_input::text_input("Username", &self.model.username, Message::Username)
                    .id(USERNAME_ID)
                    .on_submit(Message::Auth(view::Message::Submit))
                    .padding(OXITHEME.padding_md),
            );
        } else {
            content =
                content.push(oxi_plugin::text(self.model.username.clone()).size(OXITHEME.font_lg));
        }
        if self.model.can_choose_session() {
            content = content.push(
                oxi_picklist::pick_list(
                    self.model.sessions.as_slice(),
                    self.model.selected.as_ref(),
                    Message::Session,
                )
                .width(Length::Fill),
            );
        } else if let Some(session) = &self.model.selected {
            content = content.push(oxi_plugin::text(session.name.clone()));
        }
        if self.model.client.is_none() {
            content = content.push(
                oxi_plugin::surface_button(oxi_plugin::text("Reconnect"), true)
                    .on_press(Message::Reconnect),
            );
        }
        content = content.push(
            oxi_plugin::text(match self.model.handoff.phase() {
                Phase::Starting => "Starting selected session…",
                Phase::Handoff => "Session accepted",
                _ => "Choose a session and authenticate",
            })
            .size(OXITHEME.font_sm),
        );
        content = content.push(
            container(
                view::prompt_for(
                    "Sign in",
                    &self.model.interaction,
                    self.model.client.is_some() && !self.model.cancelling,
                    Purpose::Login,
                )
                .map(Message::Auth),
            )
            .height(Length::Fill),
        );
        container(container(content).max_width(560).width(Length::Fill))
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .padding(OXITHEME.padding_xl)
            .into()
    }
    fn theme(&self) -> Theme {
        get_derived_iced_theme()
    }
}

pub fn run(client: Client, catalog: Catalog) -> Result<(), Box<dyn std::error::Error>> {
    let accepted = Arc::new(AtomicBool::new(false));
    let state = Mutex::new(Some(Greeter {
        model: Model::new(client, catalog),
        accepted: accepted.clone(),
    }));
    iced::application(
        move || {
            let state = state
                .lock()
                .expect("greeter initialization lock")
                .take()
                .expect("greeter initializes once");
            (state, iced::widget::operation::focus(USERNAME_ID))
        },
        Greeter::update,
        Greeter::view,
    )
    .title("OxiSession")
    .theme(Greeter::theme)
    .subscription(Greeter::subscription)
    .window(iced::window::Settings {
        fullscreen: true,
        decorations: false,
        closeable: false,
        exit_on_close_request: false,
        platform_specific: iced::window::settings::PlatformSpecific {
            application_id: "oxisession".into(),
            ..Default::default()
        },
        ..Default::default()
    })
    .run()?;
    if !accepted.load(Ordering::Acquire) {
        return Err("greeter exited without acknowledged handoff".into());
    }
    Ok(())
}
