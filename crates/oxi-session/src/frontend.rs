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
use oxi_auth_ui::visuals::{KeyboardStatus, Visuals};
use oxi_auth_ui::{
    Phase as AuthPhase,
    view::{self, Focus, Purpose},
};
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

#[path = "username_input.rs"]
mod username_input;

#[derive(Clone)]
enum Message {
    Username(String),
    Session(Session),
    Auth(view::Message),
    Key(keyboard::Event, iced::event::Status, bool),
    Tick,
    Reconnect,
    Power(Power),
    ConfirmPower,
    CancelPower,
    PowerResult(bool),
    PowerCapabilities((bool, bool)),
    ChangeUser,
    ChooseUser,
    Focus(Control),
}
impl fmt::Debug for Message {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Username(_) => "Username",
            Self::Session(_) => "Session",
            Self::Auth(_) => "Authentication action",
            Self::Key(_, _, _) => "Keyboard input [REDACTED]",
            Self::Tick => "Tick",
            Self::Reconnect => "Reconnect",
            Self::ChangeUser | Self::ChooseUser | Self::Focus(_) => "Focus action",
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
    focus: Control,
    editing_user: bool,
    username_draft: String,
    prepare_pending: bool,
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Control {
    Username,
    ChooseUser,
    ChangeUser,
    Session,
    Auth(Focus),
    Reconnect,
    PowerOff,
    Reboot,
    ConfirmPower,
    CancelPower,
}
impl Greeter {
    fn new(
        mut model: Model,
        username: Option<String>,
        accepted: Arc<AtomicBool>,
        visuals: Visuals,
        keyboard: KeyboardStatus,
    ) -> Self {
        if let Some(username) = username {
            model.set_username(username);
        }
        let username_draft = model.username.clone();
        let editing_user = username_draft.is_empty();
        Self {
            model,
            accepted,
            visuals,
            keyboard,
            username_draft,
            editing_user,
            prepare_pending: !editing_user,
            focus: if editing_user {
                Control::Username
            } else {
                Control::ChangeUser
            },
            power: None,
            power_busy: false,
            power_off: false,
            reboot: false,
        }
    }
    fn can_interrupt(&self) -> bool {
        !self.power_busy && !matches!(self.model.handoff.phase(), Phase::Starting | Phase::Handoff)
    }
    fn auth_ready(&self) -> bool {
        self.model.client.is_some()
            && !self.model.cancelling
            && !self.editing_user
            && self.power.is_none()
            && !self.power_busy
    }
    fn controls(&self) -> Vec<Control> {
        if !self.can_interrupt() {
            return Vec::new();
        }
        if self.power.is_some() {
            return if self.model.can_choose_user() {
                vec![Control::ConfirmPower, Control::CancelPower]
            } else {
                vec![Control::CancelPower]
            };
        }
        let mut controls = if self.editing_user {
            let mut controls = vec![Control::Username];
            if !self.username_draft.trim().is_empty() {
                controls.push(Control::ChooseUser);
            }
            controls
        } else {
            vec![Control::ChangeUser]
        };
        if self.model.can_choose_session() {
            controls.push(Control::Session);
        }
        if self.model.client.is_none() {
            controls.push(Control::Reconnect);
        }
        if !self.editing_user {
            for focus in [Focus::Input, Focus::Submit] {
                if focus.enabled(&self.model.interaction, self.auth_ready()) {
                    controls.push(Control::Auth(focus));
                }
            }
            if self.model.interaction.phase == AuthPhase::Prompt {
                controls.push(Control::Auth(Focus::Cancel));
            }
        }
        if self.power_off {
            controls.push(Control::PowerOff);
        }
        if self.reboot {
            controls.push(Control::Reboot);
        }
        controls
    }
    fn focus_control(&mut self, control: Control) -> Task<Message> {
        if self.controls().contains(&control) {
            self.focus = control;
        }
        Task::none()
    }
    fn move_focus(&mut self, reverse: bool) -> Task<Message> {
        let controls = self.controls();
        if controls.is_empty() {
            return Task::none();
        }
        let index = controls
            .iter()
            .position(|control| *control == self.focus)
            .unwrap_or(0);
        let next = (index + if reverse { controls.len() - 1 } else { 1 }) % controls.len();
        self.focus_control(controls[next])
    }
    fn activate(&mut self) -> Task<Message> {
        if !self.controls().contains(&self.focus) {
            return Task::none();
        }
        let message = match self.focus {
            Control::Username | Control::ChooseUser => Message::ChooseUser,
            Control::ChangeUser => Message::ChangeUser,
            Control::Auth(Focus::Input | Focus::Submit) => Message::Auth(view::Message::Submit),
            Control::Auth(Focus::Cancel) => Message::Auth(view::Message::Cancel),
            Control::Reconnect => Message::Reconnect,
            Control::PowerOff => Message::Power(Power::PowerOff),
            Control::Reboot => Message::Power(Power::Reboot),
            Control::ConfirmPower => Message::ConfirmPower,
            Control::CancelPower => Message::CancelPower,
            Control::Session => return Task::none(),
        };
        self.update(message)
    }
    fn cycle_session(&mut self, reverse: bool) {
        if !self.model.can_choose_session() || self.model.sessions.is_empty() {
            return;
        }
        let index = self
            .model
            .selected
            .as_ref()
            .and_then(|selected| {
                self.model
                    .sessions
                    .iter()
                    .position(|session| session == selected)
            })
            .unwrap_or(0);
        let length = self.model.sessions.len();
        self.model.select(
            self.model.sessions[(index + if reverse { length - 1 } else { 1 }) % length].clone(),
        );
    }
    fn keyboard_input(
        &mut self,
        event: keyboard::Event,
        status: iced::event::Status,
        username_widget: bool,
    ) -> Task<Message> {
        use keyboard::{Key, key::Named};
        self.keyboard.observe(&event);
        if self.power_busy {
            return Task::none();
        }
        let keyboard::Event::KeyPressed {
            key,
            modified_key,
            modifiers,
            text,
            repeat,
            ..
        } = event
        else {
            return Task::none();
        };
        if !repeat
            && modifiers.control()
            && modifiers.shift()
            && !modifiers.alt()
            && !modifiers.logo()
            && let Key::Character(key) = &key
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
        // The native field already handled captured events, even if its view has since changed.
        if username_widget && status == iced::event::Status::Captured {
            return Task::none();
        }
        if key == Key::Named(Named::Tab) {
            return self.move_focus(modifiers.shift());
        }
        if key == Key::Named(Named::Escape) {
            return if self.power.is_some() {
                self.update(Message::CancelPower)
            } else if self.editing_user && !self.model.username.is_empty() {
                self.username_draft = self.model.username.clone();
                self.update(Message::ChooseUser)
            } else {
                self.update(Message::Auth(view::Message::Cancel))
            };
        }
        if self.focus == Control::Session {
            match key.as_ref() {
                Key::Named(Named::ArrowUp | Named::ArrowLeft) => self.cycle_session(true),
                Key::Named(Named::ArrowDown | Named::ArrowRight) => self.cycle_session(false),
                _ => {}
            }
            return Task::none();
        }
        if self.focus == Control::Username {
            if key == Key::Named(Named::Enter) && status == iced::event::Status::Ignored {
                return self.activate();
            }
            return Task::none();
        }
        if key == Key::Named(Named::Enter) {
            return self.activate();
        }
        if self.focus != Control::Auth(Focus::Input) {
            if matches!(key.as_ref(), Key::Named(Named::Space) | Key::Character(" ")) {
                return self.activate();
            }
            return Task::none();
        }
        // Masked prompts are controller-owned, including keys captured by scene widgets.
        if self.model.interaction.phase == AuthPhase::Prompt && self.auth_ready() {
            let text =
                text.as_deref()
                    .filter(|text| !text.is_empty())
                    .or_else(|| match &modified_key {
                        Key::Character(value) => Some(value.as_str()),
                        Key::Named(Named::Space) => Some(" "),
                        _ => None,
                    });
            self.model.input(&key, modifiers, text);
        }
        Task::none()
    }
    fn update(&mut self, message: Message) -> Task<Message> {
        let previous_phase = self.model.interaction.phase;
        if self.power_busy
            && !matches!(
                &message,
                Message::PowerResult(_) | Message::Tick | Message::Key(_, _, _)
            )
        {
            return Task::none();
        }
        let mut task = Task::none();
        match message {
            Message::Key(event, status, username_widget) => {
                task = self.keyboard_input(event, status, username_widget)
            }
            Message::PowerCapabilities((power_off, reboot)) => {
                self.power_off = power_off;
                self.reboot = reboot;
            }
            Message::Power(power)
                if self.can_interrupt()
                    && match power {
                        Power::PowerOff => self.power_off,
                        Power::Reboot => self.reboot,
                    } =>
            {
                self.model.cancel();
                self.prepare_pending = false;
                self.power = Some(power);
                self.focus = Control::CancelPower;
            }
            Message::CancelPower => {
                self.power = None;
                self.prepare_pending = !self.editing_user;
            }
            Message::ConfirmPower if self.model.can_choose_user() => {
                if let Some(power) = self.power.take() {
                    self.power_busy = true;
                    return Task::perform(power_action(power), Message::PowerResult);
                }
            }
            Message::PowerResult(success) => {
                self.power_busy = false;
                self.prepare_pending = !self.editing_user;
                if !success {
                    self.model.interaction.notice = "Power action denied by system policy.".into();
                }
            }
            Message::Username(username)
                if self.editing_user && self.power.is_none() && self.focus == Control::Username =>
            {
                self.username_draft = username
                    .chars()
                    .filter(|value| !value.is_control())
                    .take(256)
                    .collect();
                while self.username_draft.len() > 256 {
                    self.username_draft.pop();
                }
                self.focus = Control::Username;
            }
            Message::ChangeUser if self.can_interrupt() => {
                self.model.cancel();
                self.username_draft = self.model.username.clone();
                self.editing_user = true;
                self.prepare_pending = false;
                task = self.focus_control(Control::Username);
            }
            Message::ChooseUser if self.editing_user && !self.username_draft.trim().is_empty() => {
                self.editing_user = false;
                self.prepare_pending = true;
            }
            Message::Session(session) if self.power.is_none() => {
                self.model.select(session);
                task = self.focus_control(Control::Session);
            }
            Message::Focus(control) => task = self.focus_control(control),
            Message::Auth(view::Message::Submit) if self.auth_ready() => {
                self.prepare_pending = false;
                self.model.submit();
            }
            Message::Auth(view::Message::Cancel) if self.power.is_none() => {
                self.model.interaction.input.clear();
                if self.model.interaction.phase == AuthPhase::Prompt {
                    task = self.focus_control(Control::Auth(Focus::Input));
                }
            }
            Message::Auth(view::Message::Focus(focus))
                if focus.enabled(&self.model.interaction, self.auth_ready()) =>
            {
                task = self.focus_control(Control::Auth(focus));
            }
            Message::Tick => {
                self.visuals.tick();
                if self.model.poll() {
                    self.accepted.store(true, Ordering::Release);
                    return iced::exit();
                }
            }
            Message::Reconnect => {
                self.model.reconnect();
                self.prepare_pending = !self.editing_user;
            }
            _ => {}
        }
        if self.model.retry_prompt && !self.editing_user && self.power.is_none() {
            self.model.retry_prompt = false;
            self.prepare_pending = true;
        }
        if self.prepare_pending
            && !self.editing_user
            && self.power.is_none()
            && !self.power_busy
            && self.model.can_choose_user()
            && self.model.client.is_some()
        {
            self.model.set_username(self.username_draft.clone());
            self.model.prepare();
            self.prepare_pending = false;
        }
        let controls = self.controls();
        if self.model.interaction.phase == AuthPhase::Prompt
            && previous_phase != AuthPhase::Prompt
            && self.auth_ready()
        {
            task = Task::batch([task, self.focus_control(Control::Auth(Focus::Input))]);
        } else if !controls.contains(&self.focus)
            && let Some(control) = controls.first()
        {
            task = Task::batch([task, self.focus_control(*control)]);
        }
        task
    }
    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            iced::time::every(Duration::from_millis(30)).map(|_| Message::Tick),
            iced::event::listen_with(|event, status, _| match event {
                iced::Event::Keyboard(event) => Some((event, status)),
                _ => None,
            })
            .with(self.editing_user && self.power.is_none())
            .map(|(username_widget, (event, status))| Message::Key(event, status, username_widget)),
        ])
    }
    fn view(&self) -> Element<'_, Message> {
        let mut content = Column::new()
            .spacing(OXITHEME.padding_lg)
            .align_x(iced::Alignment::Center)
            .push(self.visuals.heading())
            .push(oxi_plugin::text("Welcome").size(OXITHEME.font_xxl));
        if self.editing_user && self.power.is_none() {
            let controls = self.controls();
            content = content.push(username_input::input(
                oxi_text_input::text_input("Username", &self.username_draft, Message::Username)
                    .id(USERNAME_ID)
                    .padding(OXITHEME.padding_md)
                    .into(),
                self.focus == Control::Username && controls.contains(&Control::Username),
                controls.last().copied().unwrap_or(Control::Username),
                controls.get(1).copied().unwrap_or(Control::Username),
            ));
            content = content.push(
                self.control_button("Use this user", Control::ChooseUser, true)
                    .on_press_maybe(
                        self.controls()
                            .contains(&Control::ChooseUser)
                            .then_some(Message::ChooseUser),
                    ),
            );
        } else {
            content = content.push(
                Row::new()
                    .spacing(OXITHEME.padding_md)
                    .align_y(iced::Alignment::Center)
                    .push(oxi_plugin::text(self.model.username.clone()).size(OXITHEME.font_lg))
                    .push(
                        self.control_button("Change user", Control::ChangeUser, false)
                            .on_press_maybe(
                                (self.can_interrupt() && self.power.is_none())
                                    .then_some(Message::ChangeUser),
                            ),
                    ),
            );
        }
        if self.model.can_choose_session() && self.power.is_none() {
            content = content.push(
                oxi_picklist::pick_list(
                    self.model.sessions.as_slice(),
                    self.model.selected.as_ref(),
                    Message::Session,
                )
                .width(Length::Fill)
                .on_open(Message::Focus(Control::Session))
                .style({
                    let focused = self.focus == Control::Session;
                    move |theme, status| {
                        let mut style = oxi_picklist::picklist_style(theme, status);
                        if focused {
                            style.border.color = OXITHEME.primary;
                            style.border.width = 2.0;
                        }
                        style
                    }
                }),
            );
            if self.focus == Control::Session {
                content = content.push(
                    oxi_plugin::text("Session: use arrow keys to change").size(OXITHEME.font_sm),
                );
            }
        } else if let Some(session) = &self.model.selected {
            content = content.push(oxi_plugin::text(session.name.clone()));
        }
        if self.model.client.is_none() {
            content = content.push(
                self.control_button("Reconnect", Control::Reconnect, true)
                    .on_press_maybe(
                        self.controls()
                            .contains(&Control::Reconnect)
                            .then_some(Message::Reconnect),
                    ),
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
            view::prompt_card_focused(
                "Sign in",
                &self.model.interaction,
                self.auth_ready(),
                Purpose::Login,
                match self.focus {
                    Control::Auth(focus) => Some(focus),
                    _ => None,
                },
            )
            .map(Message::Auth),
        );
        content = content.push(self.keyboard.view());
        if !self.visuals.notice.is_empty() {
            content =
                content.push(oxi_plugin::text(self.visuals.notice.clone()).size(OXITHEME.font_sm));
        }
        if self.can_interrupt() {
            let actions = if let Some(power) = self.power {
                Row::new()
                    .spacing(OXITHEME.padding_md)
                    .push(oxi_plugin::text(format!("{} this machine?", power.label())))
                    .push(
                        self.control_button("Confirm", Control::ConfirmPower, true)
                            .on_press_maybe(
                                self.model
                                    .can_choose_user()
                                    .then_some(Message::ConfirmPower),
                            ),
                    )
                    .push(
                        self.control_button("Cancel", Control::CancelPower, false)
                            .on_press(Message::CancelPower),
                    )
            } else {
                Row::new()
                    .spacing(OXITHEME.padding_md)
                    .push(
                        self.control_button("Power off", Control::PowerOff, false)
                            .on_press_maybe(
                                self.power_off.then_some(Message::Power(Power::PowerOff)),
                            ),
                    )
                    .push(
                        self.control_button("Restart", Control::Reboot, false)
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
    fn control_button<'a>(
        &self,
        label: &'static str,
        control: Control,
        primary: bool,
    ) -> oxiced::widgets::oxi_control::Button<'a, Message> {
        let focused = self.focus == control;
        oxi_plugin::surface_button(oxi_plugin::text(label), primary).style(move |theme, status| {
            let mut style = if primary {
                oxi_plugin::flat_primary_bg_button_style(theme, status)
            } else {
                oxi_plugin::flat_secondary_bg_button_style(theme, status)
            };
            if focused {
                style.border.color = OXITHEME.primary;
                style.border.width = 2.0;
            }
            style
        })
    }
}

pub fn run(client: Client, catalog: Catalog) -> Result<(), Box<dyn std::error::Error>> {
    let accepted = Arc::new(AtomicBool::new(false));
    let state = Mutex::new(Some(Greeter::new(
        Model::new(client, catalog),
        crate::users::default_username(),
        accepted.clone(),
        Visuals::load(Purpose::Login),
        KeyboardStatus::configured(),
    )));
    iced::application(
        move || {
            let mut state = state
                .lock()
                .expect("greeter initialization lock")
                .take()
                .expect("greeter initializes once");
            let initialize = state.update(Message::Tick);
            let focus = state.focus_control(state.focus);
            (
                state,
                Task::batch([
                    initialize,
                    focus,
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

#[cfg(test)]
#[path = "frontend_tests.rs"]
mod tests;

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
