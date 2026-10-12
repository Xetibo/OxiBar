use iced::{
    Element, Length,
    widget::{Column, Row, slider},
};
use oxibar_plugin_api::PluginMsg;
use oxiced::widgets::oxi_plugin::text;
use oxiced::{
    theme::theme_impl::OXITHEME,
    widgets::{oxi_picklist, oxi_plugin},
};

use crate::{
    Message, Model, plugin_message,
    system::{Capability, Domain, SessionAction},
};

fn label(value: impl Into<String>) -> iced::widget::Text<'static> {
    text(value.into())
        .size(OXITHEME.font_md)
        .style(oxi_plugin::text_primary)
}
fn action(
    value: &'static str,
    message: Option<Message>,
    primary: bool,
) -> iced::widget::Button<'static, PluginMsg> {
    oxi_plugin::surface_button(label(value), primary).on_press_maybe(message.map(plugin_message))
}
fn title(value: &str) -> String {
    match value {
        "power-saver" => "Power saver".to_owned(),
        "balanced" => "Balanced".to_owned(),
        "performance" => "Performance".to_owned(),
        _ => value.to_owned(),
    }
}

pub fn bar(model: &Model, show_labels: bool) -> Element<'static, PluginMsg> {
    let value = model
        .snapshot
        .backlight(&model.preferred)
        .map(|device| format!("{}%", device.percent()))
        .unwrap_or_else(|| "System".to_owned());
    let mut row = Row::new().push(oxi_plugin::icon(oxi_plugin::ShellIcon::Brightness));
    if show_labels {
        row = row.push(label(value));
    }
    oxi_plugin::bar_button(
        row.spacing(OXITHEME.padding_sm)
            .align_y(iced::Alignment::Center),
    )
    .on_press(plugin_message(Message::TogglePopup))
    .into()
}

pub fn controls(model: &Model, include_session: bool) -> Element<'static, PluginMsg> {
    let mut body = Column::new()
        .spacing(OXITHEME.padding_xl)
        .width(Length::Fill);
    let mut brightness = Column::new().spacing(OXITHEME.padding_lg).push(
        text("Brightness")
            .size(OXITHEME.font_lg)
            .style(oxi_plugin::text_primary),
    );
    if let Some(device) = model.snapshot.backlight(&model.preferred) {
        let names: Vec<String> = model
            .snapshot
            .backlights
            .iter()
            .map(|device| device.name.clone())
            .collect();
        brightness = brightness.push(
            oxi_picklist::pick_list(names, Some(device.name.clone()), |name| {
                plugin_message(Message::SelectBacklight(name))
            })
            .width(Length::Fill),
        );
        brightness = brightness.push(
            Row::new()
                .push(label(format!("{}%", device.percent())))
                .push(iced::widget::Space::new().width(Length::Fill))
                .push(label(format!("{} / {}", device.current, device.maximum))),
        );
        let enabled = model
            .client
            .ready
            .load(std::sync::atomic::Ordering::Acquire)
            && model.snapshot.brightness_error.is_none();
        if enabled {
            brightness = brightness.push(
                slider(0..=100, device.percent(), |value| {
                    plugin_message(Message::Brightness(value))
                })
                .step(1u8)
                .style(oxiced::widgets::oxi_slider::slider_style),
            );
        } else {
            brightness =
                brightness.push(oxi_plugin::level_meter(f32::from(device.percent()), 100.0));
        }
    } else {
        brightness = brightness.push(label("No backlight is available."));
    }
    if let Some(error) = &model.snapshot.brightness_error {
        brightness = brightness.push(label(error.clone()));
    }
    body = body.push(oxi_plugin::settings_card(brightness));

    let mut profiles = Column::new().spacing(OXITHEME.padding_lg).push(
        text("Power profile")
            .size(OXITHEME.font_lg)
            .style(oxi_plugin::text_primary),
    );
    if model.snapshot.profiles.is_empty() {
        profiles = profiles.push(label("No power-profile service is available."));
    }
    for profile in &model.snapshot.profiles {
        let enabled = model.snapshot.profile_ready && !model.pending.contains_key(&Domain::Profile);
        let active = model.snapshot.active_profile.as_ref() == Some(profile);
        profiles = profiles.push(
            oxi_plugin::selectable_row(label(title(profile)), active)
                .padding(OXITHEME.padding_lg)
                .on_press_maybe(enabled.then(|| plugin_message(Message::Profile(profile.clone())))),
        );
    }
    if let Some(reason) = &model.snapshot.degraded {
        profiles = profiles.push(label(format!("Performance limited: {reason}")));
    }
    if let Some(error) = &model.snapshot.profile_error {
        profiles = profiles.push(label(error.clone()));
    }
    body = body.push(oxi_plugin::settings_card(profiles));

    if include_session {
        let mut session = Column::new().spacing(OXITHEME.padding_lg).push(
            text("Session")
                .size(OXITHEME.font_lg)
                .style(oxi_plugin::text_primary),
        );
        let busy = model.pending.contains_key(&Domain::Session);
        let lock = model.lock_enabled && model.locker.is_some() && !busy;
        session = session.push(action("Lock", lock.then_some(Message::Lock), true));
        if model.lock_enabled && model.locker.is_none() {
            session = session.push(label("Set system.lock_command to your dedicated locker."));
        }
        for action_kind in [
            SessionAction::Suspend,
            SessionAction::Hibernate,
            SessionAction::Logout,
            SessionAction::Reboot,
            SessionAction::PowerOff,
        ] {
            let allowed = model.snapshot.can(action_kind) && !busy;
            let mut row = Row::new()
                .spacing(OXITHEME.padding_lg)
                .align_y(iced::Alignment::Center)
                .push(action(
                    action_kind.label(),
                    allowed.then_some(Message::Request(action_kind)),
                    false,
                ));
            if model.snapshot.capabilities.get(&action_kind) == Some(&Capability::Challenge) {
                row = row.push(
                    text("Authentication required")
                        .size(OXITHEME.font_md)
                        .style(oxi_plugin::text_muted),
                );
            }
            session = session.push(row);
        }
        if let Some(error) = &model.snapshot.session_error {
            session = session.push(label(error.clone()));
        }
        body = body.push(oxi_plugin::settings_card(session));
    }
    if !model.pending.is_empty() {
        body = body.push(label("Applying system preference…"));
    }
    if let Some(error) = &model.error {
        body = body.push(label(error.clone()));
    }
    body.push(action("Refresh", Some(Message::Refresh), false))
        .into()
}

pub fn confirmation(model: &Model) -> Element<'static, PluginMsg> {
    let Some(action_kind) = model.confirmation else {
        return label("No pending session action.").into();
    };
    let mut body = Column::new()
        .spacing(OXITHEME.padding_lg)
        .push(
            text(format!("{}?", action_kind.label()))
                .size(OXITHEME.font_xl)
                .style(oxi_plugin::text_primary),
        )
        .push(label(match action_kind {
            SessionAction::Logout => "End your selected login session.",
            SessionAction::PowerOff => "Shut down this computer.",
            SessionAction::Reboot => "Restart this computer.",
            SessionAction::Suspend => "Suspend this computer.",
            SessionAction::Hibernate => "Hibernate this computer.",
        }));
    if let Some(session) = &model.snapshot.session {
        body = body.push(
            text(format!("Session {}", session.id))
                .size(OXITHEME.font_md)
                .style(oxi_plugin::text_muted),
        );
    }
    if model.snapshot.capabilities.get(&action_kind) == Some(&Capability::Challenge) {
        body = body.push(label(
            "The system authentication agent may request approval.",
        ));
    }
    body = body.push(
        Row::new()
            .spacing(OXITHEME.padding_lg)
            .push(action("Cancel", Some(Message::Cancel), false))
            .push(iced::widget::Space::new().width(Length::Fill))
            .push(action(
                "Confirm",
                model.snapshot.can(action_kind).then_some(Message::Confirm),
                true,
            )),
    );
    iced::widget::scrollable(body).height(Length::Fill).into()
}

pub fn osd(model: &Model, topic: &str) -> Result<Element<'static, PluginMsg>, std::io::Error> {
    match topic {
        "brightness" => {
            let device = model
                .snapshot
                .backlight(&model.preferred)
                .ok_or_else(|| std::io::Error::other("Backlight is unavailable"))?;
            Ok(oxi_plugin::osd_content(
                "󰃠",
                "Brightness",
                format!("{}%", device.percent()),
                Some((device.percent().into(), 100.0)),
            ))
        }
        "profile" => Ok(oxi_plugin::osd_content(
            "󰌪",
            "Power profile",
            model
                .snapshot
                .active_profile
                .as_deref()
                .map(title)
                .unwrap_or_else(|| "Unavailable".to_owned()),
            None,
        )),
        _ => Err(std::io::Error::other("Unknown System OSD topic")),
    }
}
