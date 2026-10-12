use crate::{
    Phase,
    catalog::{Catalog, Session},
    ipc::Client,
    model::Model,
};
use iced::{
    Element, Length, Subscription, Task, Theme, keyboard,
    widget::{Column, Row, container},
};
use oxi_auth_ui::view::{self, Purpose};
use oxi_auth_ui::visuals::{KeyboardStatus, Visuals};
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
    Observe(keyboard::Event),
    Power(Power),
    ConfirmPower,
    CancelPower,
    PowerResult(bool),
    PowerCapabilities((bool, bool)),
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
            Self::Observe(_) => "Keyboard status",
            Self::Power(_)
            | Self::ConfirmPower
            | Self::CancelPower
            | Self::PowerResult(_)
            | Self::PowerCapabilities(_) => "Power action",
        })
    }
}
struct Greeter {
    model: Model,
    accepted: Arc<AtomicBool>,
    visuals: Visuals,
    keyboard: KeyboardStatus,
    power: Option<Power>,
    power_busy: bool,
    power_off: bool,
    reboot: bool,
}
#[derive(Clone, Copy)]
enum Power {
    PowerOff,
    Reboot,
}
impl Power {
    fn label(self) -> &'static str {
        match self {
            Self::PowerOff => "Power off",
            Self::Reboot => "Restart",
        }
    }
}
const USERNAME_ID: &str = "oxisession-username";
impl Greeter {
    fn update(&mut self, message: Message) -> Task<Message> {
        let could_choose_user = self.model.can_choose_user();
        let had_power_confirmation = self.power.is_some();
        if self.power_busy
            && !matches!(
                &message,
                Message::PowerResult(_) | Message::Tick | Message::Observe(_)
            )
        {
            return Task::none();
        }
        match message {
            Message::Observe(event) => {
                self.keyboard.observe(&event);
                if let keyboard::Event::KeyPressed {
                    key,
                    modifiers,
                    repeat: false,
                    ..
                } = &event
                {
                    if self.power.is_some()
                        && *key == keyboard::Key::Named(keyboard::key::Named::Escape)
                    {
                        self.power = None;
                    } else if modifiers.control()
                        && modifiers.shift()
                        && !modifiers.alt()
                        && !modifiers.logo()
                        && let keyboard::Key::Character(key) = key
                    {
                        let power = match key.as_str() {
                            "p" | "P" => Some(Power::PowerOff),
                            "r" | "R" => Some(Power::Reboot),
                            _ => None,
                        };
                        if let Some(power) = power {
                            return self.update(Message::Power(power));
                        }
                    }
                }
            }
            Message::PowerCapabilities((power_off, reboot)) => {
                self.power_off = power_off;
                self.reboot = reboot;
            }
            Message::Power(power)
                if self.model.can_choose_user()
                    && match power {
                        Power::PowerOff => self.power_off,
                        Power::Reboot => self.reboot,
                    } =>
            {
                self.power = Some(power)
            }
            Message::CancelPower => self.power = None,
            Message::ConfirmPower if self.model.can_choose_user() => {
                if let Some(power) = self.power.take() {
                    self.power_busy = true;
                    return Task::perform(power_action(power), Message::PowerResult);
                }
            }
            Message::PowerResult(success) => {
                self.power_busy = false;
                if !success {
                    self.model.interaction.notice = "Power action denied by system policy.".into();
                }
            }
            Message::Username(username) if self.power.is_none() => {
                self.model.set_username(username)
            }
            Message::Session(session) => self.model.select(session),
            Message::Auth(view::Message::Submit) if self.power.is_none() => self.model.submit(),
            Message::Auth(view::Message::Cancel) => self.model.cancel(),
            Message::Key(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Escape),
                ..
            }) if self.power.is_some() => self.power = None,
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
            }) if self.power.is_none() => self.model.input(&key, modifiers, text.as_deref()),
            Message::Tick => {
                self.visuals.tick();
                if self.model.poll() {
                    self.accepted.store(true, Ordering::Release);
                    return iced::exit();
                }
            }
            Message::Reconnect => self.model.reconnect(),
            _ => {}
        }
        if self.model.can_choose_user()
            && (!could_choose_user || (had_power_confirmation && self.power.is_none()))
        {
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
            iced::event::listen_with(|event, _, _| match event {
                iced::Event::Keyboard(event) => Some(Message::Observe(event)),
                _ => None,
            }),
        ])
    }
    fn view(&self) -> Element<'_, Message> {
        let mut content = Column::new()
            .spacing(OXITHEME.padding_lg)
            .align_x(iced::Alignment::Center)
            .push(self.visuals.heading())
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
            view::prompt_card(
                "Sign in",
                &self.model.interaction,
                self.model.client.is_some() && !self.model.cancelling,
                Purpose::Login,
            )
            .map(Message::Auth),
        );
        content = content.push(self.keyboard.view());
        if !self.visuals.notice.is_empty() {
            content =
                content.push(oxi_plugin::text(self.visuals.notice.clone()).size(OXITHEME.font_sm));
        }
        if self.model.can_choose_user() && !self.power_busy {
            let actions = if let Some(power) = self.power {
                Row::new()
                    .spacing(OXITHEME.padding_md)
                    .push(oxi_plugin::text(format!("{} this machine?", power.label())))
                    .push(
                        oxi_plugin::surface_button(oxi_plugin::text("Confirm"), true)
                            .on_press(Message::ConfirmPower),
                    )
                    .push(
                        oxi_plugin::surface_button(oxi_plugin::text("Cancel"), false)
                            .on_press(Message::CancelPower),
                    )
            } else {
                Row::new()
                    .spacing(OXITHEME.padding_md)
                    .push(
                        oxi_plugin::surface_button(oxi_plugin::text("Power off"), false)
                            .on_press_maybe(
                                self.power_off.then_some(Message::Power(Power::PowerOff)),
                            ),
                    )
                    .push(
                        oxi_plugin::surface_button(oxi_plugin::text("Restart"), false)
                            .on_press_maybe(self.reboot.then_some(Message::Power(Power::Reboot))),
                    )
            };
            content = content.push(actions).push(
                oxi_plugin::text(
                    "Ctrl+Shift+P: power off · Ctrl+Shift+R: restart · Escape: cancel",
                )
                .size(OXITHEME.font_sm),
            );
        }
        self.visuals.scene(
            container(
                container(oxi_plugin::scroll_panel(content).height(Length::Shrink))
                    .max_width(560)
                    .width(Length::Fill),
            )
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .padding(OXITHEME.padding_xl)
            .into(),
        )
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
        visuals: Visuals::load(Purpose::Login),
        keyboard: KeyboardStatus::configured(),
        power: None,
        power_busy: false,
        power_off: false,
        reboot: false,
    }));
    iced::application(
        move || {
            let state = state
                .lock()
                .expect("greeter initialization lock")
                .take()
                .expect("greeter initializes once");
            (
                state,
                Task::batch([
                    iced::widget::operation::focus(USERNAME_ID),
                    Task::perform(power_capabilities(), Message::PowerCapabilities),
                ]),
            )
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

async fn power_action(action: Power) -> bool {
    let result = async {
        let connection = zbus::Connection::system().await?;
        let proxy = zbus::Proxy::new(
            &connection,
            "org.freedesktop.login1",
            "/org/freedesktop/login1",
            "org.freedesktop.login1.Manager",
        )
        .await?;
        let (can, method) = match action {
            Power::PowerOff => ("CanPowerOff", "PowerOff"),
            Power::Reboot => ("CanReboot", "Reboot"),
        };
        let allowed: String = proxy.call(can, &()).await?;
        if allowed != "yes" {
            return Ok::<bool, zbus::Error>(false);
        }
        proxy.call::<_, _, ()>(method, &(false,)).await?;
        Ok(true)
    }
    .await;
    result.unwrap_or(false)
}

async fn power_capabilities() -> (bool, bool) {
    let result = async {
        let connection = zbus::Connection::system().await?;
        let proxy = zbus::Proxy::new(
            &connection,
            "org.freedesktop.login1",
            "/org/freedesktop/login1",
            "org.freedesktop.login1.Manager",
        )
        .await?;
        let off: String = proxy.call("CanPowerOff", &()).await?;
        let reboot: String = proxy.call("CanReboot", &()).await?;
        Ok::<_, zbus::Error>((off == "yes", reboot == "yes"))
    }
    .await;
    result.unwrap_or((false, false))
}
