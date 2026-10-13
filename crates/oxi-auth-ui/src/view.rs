use iced::{
    Element, Length,
    widget::{Column, Row, container, mouse_area},
};
use oxiced::{theme::theme_impl::OXITHEME, widgets::oxi_plugin};

use crate::{Interaction, Phase, PromptKind};

#[derive(Clone, Copy, Debug)]
pub enum Message {
    Submit,
    Cancel,
    Focus(Focus),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Input,
    Submit,
    Cancel,
}
impl Focus {
    pub fn enabled(self, interaction: &Interaction, ready: bool) -> bool {
        match self {
            Self::Input => ready && interaction.phase == Phase::Prompt,
            Self::Submit => ready && matches!(interaction.phase, Phase::Idle | Phase::Prompt),
            Self::Cancel => true,
        }
    }
    pub fn next(self, interaction: &Interaction, ready: bool, reverse: bool) -> Self {
        let targets = [Self::Input, Self::Submit, Self::Cancel];
        let mut index = targets
            .iter()
            .position(|target| *target == self)
            .unwrap_or(0);
        for _ in 0..targets.len() {
            index = (index + if reverse { targets.len() - 1 } else { 1 }) % targets.len();
            if targets[index].enabled(interaction, ready) {
                return targets[index];
            }
        }
        Self::Cancel
    }
}

#[derive(Clone, Copy)]
pub enum Purpose {
    Lock,
    Login,
}

pub fn prompt<'a>(identity: &str, interaction: &Interaction, ready: bool) -> Element<'a, Message> {
    prompt_for(identity, interaction, ready, Purpose::Lock)
}

pub fn prompt_for<'a>(
    identity: &str,
    interaction: &Interaction,
    ready: bool,
    purpose: Purpose,
) -> Element<'a, Message> {
    container(prompt_card(identity, interaction, ready, purpose))
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .padding(OXITHEME.padding_lg)
        .into()
}

pub fn prompt_card<'a>(
    identity: &str,
    interaction: &Interaction,
    ready: bool,
    purpose: Purpose,
) -> Element<'a, Message> {
    prompt_card_focused(identity, interaction, ready, purpose, None)
}

pub fn prompt_card_focused<'a>(
    identity: &str,
    interaction: &Interaction,
    ready: bool,
    purpose: Purpose,
    focus: Option<Focus>,
) -> Element<'a, Message> {
    let input_enabled = Focus::Input.enabled(interaction, ready);
    let submit_enabled = Focus::Submit.enabled(interaction, ready);
    let label = interaction
        .prompt
        .as_ref()
        .map(|prompt| prompt.text.as_str())
        .unwrap_or(match interaction.phase {
            Phase::Idle => "Press Enter to authenticate",
            Phase::Waiting => "Authenticating…",
            Phase::Authenticated => "Authenticated",
            Phase::Prompt => "Password",
        });
    let value = if interaction
        .prompt
        .as_ref()
        .is_some_and(|prompt| prompt.kind == PromptKind::Visible)
    {
        interaction.input.expose().to_owned()
    } else {
        interaction.input.masked()
    };
    let mut body = Column::new()
        .spacing(OXITHEME.padding_lg)
        .push(oxi_plugin::text(identity.to_owned()).size(OXITHEME.font_xxl))
        .push(
            oxi_plugin::text(if ready {
                label.to_owned()
            } else {
                match purpose {
                    Purpose::Lock => "Securing outputs…",
                    Purpose::Login => "Connecting to login service…",
                }
                .into()
            })
            .size(OXITHEME.font_md),
        )
        .push(
            mouse_area(
                container(
                    oxi_plugin::text(if value.is_empty() { " ".into() } else { value })
                        .size(OXITHEME.font_xl),
                )
                .padding(OXITHEME.padding_md)
                .width(Length::Fill)
                .style(move |theme| {
                    let mut style = oxi_plugin::compact_card_style(OXITHEME.secondary_bg)(theme);
                    if focus == Some(Focus::Input) && input_enabled {
                        style.border.color = OXITHEME.primary;
                        style.border.width = 2.0;
                    }
                    style
                }),
            )
            .interaction(if input_enabled {
                iced::mouse::Interaction::Text
            } else {
                iced::mouse::Interaction::Idle
            })
            .on_press(Message::Focus(Focus::Input)),
        );
    if !interaction.notice.is_empty() {
        body = body.push(oxi_plugin::text(interaction.notice.clone()).size(OXITHEME.font_sm));
    }
    body = body.push(
        Row::new()
            .spacing(OXITHEME.padding_md)
            .push(
                oxi_plugin::surface_button(
                    oxi_plugin::text(match purpose {
                        Purpose::Lock => "Unlock",
                        Purpose::Login => "Continue",
                    }),
                    true,
                )
                .style(move |theme, status| {
                    let mut style = oxi_plugin::flat_primary_bg_button_style(theme, status);
                    if focus == Some(Focus::Submit) && submit_enabled {
                        style.border.color = OXITHEME.primary;
                        style.border.width = 2.0;
                    }
                    style
                })
                .on_press_maybe(submit_enabled.then_some(Message::Submit)),
            )
            .push(
                oxi_plugin::surface_button(oxi_plugin::text("Clear"), false)
                    .style(move |theme, status| {
                        let mut style = oxi_plugin::flat_secondary_bg_button_style(theme, status);
                        if focus == Some(Focus::Cancel) {
                            style.border.color = OXITHEME.primary;
                            style.border.width = 2.0;
                        }
                        style
                    })
                    .on_press(Message::Cancel),
            ),
    );
    body = body.push(
        oxi_plugin::text(if focus.is_some() {
            "Tab / Shift+Tab: focus · Enter: activate · Escape: clear · Ctrl+U: erase"
        } else {
            "Enter to submit · Escape to clear · Ctrl+U to erase"
        })
        .size(OXITHEME.font_sm),
    );
    container(oxi_plugin::settings_card(body))
        .max_width(440)
        .width(Length::Fill)
        .into()
}
