use iced::{
    Alignment, Element, Length,
    widget::{Column, Row, Space, mouse_area, scrollable},
};
use oxibar_plugin_api::PluginMsg;
use oxiced::widgets::oxi_plugin::text;
use oxiced::{
    theme::theme_impl::OXITHEME,
    widgets::{oxi_picklist, oxi_plugin, oxi_text_input, oxi_toggler},
};

use super::{ICON, Message, Model, agent::PromptKind, msg, system::BluetoothDevice};

pub(super) fn bar(model: &Model, show_labels: bool) -> Element<'static, PluginMsg> {
    let count = model.snapshot.connected.len();
    let label = if count == 0 || !show_labels {
        ICON.to_owned()
    } else {
        format!("{ICON} {count}")
    };
    oxi_plugin::bar_button(
        text(label)
            .size(OXITHEME.font_md)
            .align_y(Alignment::Center),
    )
    .on_press(msg(Message::TogglePopup))
    .into()
}

pub(super) fn controls(model: &Model, embedded: bool) -> Element<'static, PluginMsg> {
    let gap = if embedded {
        OXITHEME.padding_lg
    } else {
        OXITHEME.padding_sm
    };
    let mut content = Column::new()
        .spacing(gap)
        .width(Length::Fill)
        .padding(if embedded {
            [0.0, 0.0]
        } else {
            [OXITHEME.padding_md, OXITHEME.padding_lg]
        });
    let selected = model
        .adapter
        .as_deref()
        .and_then(|path| model.snapshot.adapter(path));
    if let Some(adapter) = selected {
        let picker = oxi_picklist::pick_list(
            model.snapshot.adapters.clone(),
            Some(adapter.clone()),
            |adapter| msg(Message::SelectAdapter(adapter.path)),
        )
        .width(Length::Fill)
        .padding(OXITHEME.padding_md);
        let power = oxi_toggler::toggler::<PluginMsg>(adapter.powered)
            .label("Power")
            .on_toggle_maybe(
                (model.client.is_ready() && !model.busy())
                    .then_some(|enabled| msg(Message::Power(enabled))),
            );
        content = content.push(
            Row::new()
                .push(picker)
                .push(power)
                .spacing(OXITHEME.padding_lg)
                .align_y(Alignment::Center),
        );
        let scan = action_button(
            if adapter.scanning {
                "Stop scan"
            } else {
                "Scan"
            },
            false,
        )
        .on_press_maybe(
            (model.client.is_ready() && !model.busy() && adapter.powered)
                .then(|| msg(Message::Scan)),
        );
        let hint = model
            .pending
            .as_deref()
            .or(model.pair_label.as_deref())
            .unwrap_or(if adapter.scanning {
                "Scanning · stops automatically"
            } else if adapter.discovering {
                "Another application is discovering devices"
            } else {
                "Discover nearby devices"
            });
        let mut header = Row::new()
            .push(
                text(hint.to_owned())
                    .size(OXITHEME.font_md)
                    .style(oxi_plugin::text_muted),
            )
            .push(Space::new().width(Length::Fill))
            .push(scan)
            .spacing(OXITHEME.padding_md)
            .align_y(Alignment::Center);
        if model.active_pair.is_some() {
            header = header
                .push(action_button("Cancel pairing", false).on_press(msg(Message::CancelPair)));
        }
        content = content.push(header);
    } else {
        content = content.push(copy("No Bluetooth adapters found"));
    }
    if let Some(error) = model.health.as_ref().or(model.action_error.as_ref()) {
        content = content.push(
            oxi_plugin::settings_card(
                text(error.clone())
                    .size(OXITHEME.font_md)
                    .style(oxi_plugin::text_muted),
            )
            .padding(OXITHEME.padding_lg),
        );
    }
    for (title, devices) in [
        ("Connected devices", &model.snapshot.connected),
        ("Available devices", &model.snapshot.available),
    ] {
        content = content.push(
            text(title)
                .size(OXITHEME.font_lg)
                .style(oxi_plugin::text_primary),
        );
        let devices: Vec<_> = devices
            .iter()
            .filter(|device| selected.is_none_or(|adapter| adapter.path == device.adapter))
            .collect();
        if devices.is_empty() {
            content = content.push(copy(if title == "Connected devices" {
                "No connected devices"
            } else {
                "No available devices"
            }));
        }
        for device in devices {
            content = content.push(device_card(model, device, embedded));
        }
    }
    if embedded {
        content.into()
    } else {
        scrollable(content).height(Length::Fill).into()
    }
}

fn device_card(
    model: &Model,
    device: &BluetoothDevice,
    embedded: bool,
) -> Element<'static, PluginMsg> {
    let busy = model.busy() || !model.client.is_ready();
    let path = device.path.clone();
    let detail = format!(
        "{}{} · {}",
        if device.connected {
            "Connected"
        } else if device.paired {
            "Paired"
        } else {
            "Not paired"
        },
        if device.trusted { " · Trusted" } else { "" },
        device.mac
    );
    let primary = action_button(
        if device.connected {
            "Disconnect"
        } else if device.paired {
            "Connect"
        } else {
            "Pair"
        },
        !device.connected,
    )
    .on_press_maybe((!busy).then(|| {
        msg(if device.connected {
            Message::Disconnect(path.clone())
        } else {
            Message::Pair(path.clone())
        })
    }));
    let name = Column::new()
        .push(
            text(device.name.clone())
                .size(OXITHEME.font_lg)
                .style(oxi_plugin::text_primary),
        )
        .push(
            text(detail)
                .size(OXITHEME.font_md)
                .style(oxi_plugin::text_muted),
        )
        .spacing(OXITHEME.padding_xs)
        .width(Length::Fill);
    let mut body = Column::new().spacing(OXITHEME.padding_md).push(
        Row::new()
            .push(
                text(icon_for(device))
                    .size(OXITHEME.font_xl)
                    .style(oxi_plugin::text_primary),
            )
            .push(name)
            .push(primary)
            .spacing(OXITHEME.padding_lg)
            .align_y(Alignment::Center),
    );
    if device.paired {
        let trust = action_button(if device.trusted { "Untrust" } else { "Trust" }, false)
            .on_press_maybe((!busy).then(|| msg(Message::Trust(path.clone(), !device.trusted))));
        let forget = action_button("Forget", false)
            .on_press_maybe((!busy).then(|| msg(Message::Forget(path.clone()))));
        body = body.push(
            Row::new()
                .push(Space::new().width(Length::Fill))
                .push(trust)
                .push(forget)
                .spacing(OXITHEME.padding_md),
        );
    }
    let card = oxi_plugin::settings_card(body).padding(if embedded {
        OXITHEME.padding_lg
    } else {
        OXITHEME.padding_sm
    });
    mouse_area(card)
        .on_enter(msg(Message::Hover(Some(path))))
        .on_exit(msg(Message::Hover(None)))
        .into()
}

pub(super) fn modal(model: &Model) -> Element<'static, PluginMsg> {
    let Some(pairing) = &model.pairing else {
        return copy("No pairing request");
    };
    let device = model.snapshot.device(&pairing.prompt.device);
    let name = device
        .map(|device| device.name.clone())
        .unwrap_or_else(|| pairing.prompt.device.clone());
    let mut body = Column::new()
        .spacing(OXITHEME.padding_lg)
        .width(Length::Fill)
        .push(
            text("Pair device")
                .size(OXITHEME.font_xl)
                .style(oxi_plugin::text_primary),
        )
        .push(
            text(name)
                .size(OXITHEME.font_lg)
                .style(oxi_plugin::text_primary),
        );
    if let Some(device) = device {
        body = body.push(copy(&device.mac));
    }
    let instructions = match &pairing.prompt.kind {
        PromptKind::Pin => "Enter the PIN shown by the device (1–16 characters).",
        PromptKind::Passkey => "Enter the numeric passkey shown by the device.",
        PromptKind::DisplayPin(_) => "Enter this PIN on the other device.",
        PromptKind::DisplayPasskey { .. } => "Enter this passkey on the other device.",
        PromptKind::Confirmation(_) => "Confirm this code matches the other device.",
        PromptKind::Authorization => "Allow this device to pair?",
        PromptKind::Service(_) => "Allow the device to use this Bluetooth service?",
    };
    body = body.push(copy(instructions));
    match &pairing.prompt.kind {
        PromptKind::DisplayPin(value) => {
            body = body.push(code(value.clone()));
        }
        PromptKind::DisplayPasskey { passkey, entered } => {
            body = body
                .push(code(format!("{passkey:06}")))
                .push(copy(&format!("{entered} of 6 digits entered")));
        }
        PromptKind::Confirmation(passkey) => {
            body = body.push(code(format!("{passkey:06}")));
        }
        PromptKind::Service(uuid) => {
            body = body.push(copy(uuid));
        }
        _ => {}
    }
    if pairing.prompt.kind.needs_input() && !pairing.waiting {
        body = body.push(
            oxi_text_input::text_input("PIN / passkey", &pairing.code, |value| {
                msg(Message::CodeChanged(value))
            })
            .on_submit(msg(Message::SubmitCode))
            .width(Length::Fill),
        );
    }
    if pairing.waiting {
        body = body.push(copy("Waiting for the device…"));
    }
    if let Some(error) = &pairing.error {
        body = body.push(copy(error));
    }
    let mut actions = Row::new()
        .spacing(OXITHEME.padding_md)
        .align_y(Alignment::Center)
        .push(Space::new().width(Length::Fill))
        .push(action_button("Cancel", false).on_press(msg(Message::CancelPair)));
    if !pairing.waiting {
        if pairing.prompt.kind.needs_input() {
            actions =
                actions.push(action_button("Submit", true).on_press(msg(Message::SubmitCode)));
        } else if pairing.prompt.kind.needs_confirmation() {
            actions = actions
                .push(action_button("Reject", false).on_press(msg(Message::Answer(false))))
                .push(action_button("Allow", true).on_press(msg(Message::Answer(true))));
        }
    }
    scrollable(body.push(actions)).height(Length::Fill).into()
}

fn action_button(label: &str, primary: bool) -> iced::widget::button::Button<'static, PluginMsg> {
    oxi_plugin::surface_button(text(label.to_owned()).size(OXITHEME.font_md), primary)
}
fn copy(label: &str) -> Element<'static, PluginMsg> {
    text(label.to_owned())
        .size(OXITHEME.font_md)
        .style(oxi_plugin::text_muted)
        .into()
}
fn code(value: String) -> Element<'static, PluginMsg> {
    oxi_plugin::settings_card(
        text(value)
            .size(OXITHEME.font_xxl)
            .style(oxi_plugin::text_primary),
    )
    .into()
}
fn icon_for(device: &BluetoothDevice) -> &'static str {
    if device.icon.contains("audio") || device.icon.contains("headset") {
        "󰥰"
    } else if device.icon.contains("input") || device.icon.contains("keyboard") {
        "󰌌"
    } else if device.icon.contains("phone") {
        "󰏲"
    } else {
        ICON
    }
}
