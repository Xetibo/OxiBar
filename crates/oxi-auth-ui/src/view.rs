use iced::{
    Element, Length,
    widget::{Column, Row, container},
};
use oxiced::{theme::theme_impl::OXITHEME, widgets::oxi_plugin};

use crate::{Interaction, Phase, PromptKind};

#[derive(Clone, Copy, Debug)]
pub enum Message {
    Submit,
    Cancel,
}

pub fn prompt<'a>(identity: &str, interaction: &Interaction, ready: bool) -> Element<'a, Message> {
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
                "Securing outputs…".into()
            })
            .size(OXITHEME.font_md),
        )
        .push(
            container(
                oxi_plugin::text(if value.is_empty() { " ".into() } else { value })
                    .size(OXITHEME.font_xl),
            )
            .padding(OXITHEME.padding_md)
            .width(Length::Fill)
            .style(oxi_plugin::compact_card_style(OXITHEME.secondary_bg)),
        )
        .push(oxi_plugin::text(interaction.notice.clone()).size(OXITHEME.font_sm))
        .push(
            Row::new()
                .spacing(OXITHEME.padding_md)
                .push(
                    oxi_plugin::surface_button(oxi_plugin::text("Unlock"), true).on_press_maybe(
                        (ready && interaction.phase != Phase::Waiting).then_some(Message::Submit),
                    ),
                )
                .push(
                    oxi_plugin::surface_button(oxi_plugin::text("Clear"), false)
                        .on_press(Message::Cancel),
                ),
        );
    body = body.push(
        oxi_plugin::text("Enter to submit · Escape to clear · Ctrl+U to erase")
            .size(OXITHEME.font_sm),
    );
    container(
        container(oxi_plugin::settings_card(body))
            .max_width(440)
            .width(Length::Fill),
    )
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .padding(OXITHEME.padding_lg)
    .into()
}
