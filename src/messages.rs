use iced_layershell::{
    actions::{ActionCallback, LayershellCustomAction, LayershellCustomActionWithId},
    reexport::{Anchor, IcedId, KeyboardInteractivity, Layer, NewLayerShellSettings, OutputOption},
};
use oxibar_plugin_api::{
    HOST_REQUEST_CLOSE_MODAL, HOST_REQUEST_OPEN_MODAL, HOST_REQUEST_TOAST_KEYBOARD_NONE_PREFIX,
    HOST_REQUEST_TOAST_KEYBOARD_ON_DEMAND_PREFIX, HOST_REQUEST_TOGGLE_PANEL,
    HOST_REQUEST_TOGGLE_POPUP, HostToastAction, HostToastRequest, PluginMsg,
};

use crate::layout::{
    BarDimensions, BarSection, MODAL_SIZE, PANEL_WIDTH, TOAST_MARGIN_RIGHT, popup_x,
};

#[derive(Debug, Clone)]
pub enum Message {
    Exit,
    PluginSubMsg(String, PluginMsg),
    TogglePluginPopup(String),
    TogglePluginPanel(String),
    OpenPluginModal(String),
    ClosePluginModal(String),
    SetPopupPlugin(Option<String>),
    SetPopupOpen(bool),
    LayerSurfaceResized(IcedId, u32, u32),
    RefreshPopupInputRegion(String),
    SetPopupInputRegion {
        open: bool,
        section: BarSection,
        width: u32,
        height: u32,
        bar_size: BarDimensions,
    },
    OpenModalLayer(IcedId),
    CloseModalLayer(IcedId),
    OpenPanelLayer(IcedId),
    ClosePanelLayer(IcedId),
    ShowPluginToast(String, HostToastRequest),
    ClosePluginToast(String, String),
    SetPluginToastKeyboard(String, String, KeyboardInteractivity),
    OpenToastLayer(IcedId, u32, u32, i32),
    CloseToastLayer(IcedId),
    MoveToastLayer(IcedId, i32),
    ResizeToastLayer(IcedId, u32, u32),
    SetToastKeyboardInteractivity(IcedId, KeyboardInteractivity),
}

impl TryInto<LayershellCustomActionWithId> for Message {
    type Error = Self;

    fn try_into(self) -> Result<LayershellCustomActionWithId, Self::Error> {
        match self {
            Message::SetPopupInputRegion {
                open,
                section,
                width,
                height,
                bar_size,
            } => Ok(LayershellCustomActionWithId::new(
                None,
                LayershellCustomAction::SetInputRegion(ActionCallback::new(move |region| {
                    region.add(
                        0,
                        0,
                        region_dimension(bar_size.width),
                        region_dimension(bar_size.height),
                    );
                    if open {
                        region.add(
                            popup_x(bar_size.width, section, width),
                            region_dimension(bar_size.height),
                            region_dimension(width),
                            region_dimension(height),
                        );
                    }
                })),
            )),
            Message::OpenModalLayer(id) => Ok(LayershellCustomActionWithId::new(
                None,
                LayershellCustomAction::NewLayerShell {
                    settings: NewLayerShellSettings {
                        size: Some(MODAL_SIZE),
                        layer: Layer::Overlay,
                        anchor: Anchor::empty(),
                        exclusive_zone: None,
                        margin: Some((0, 0, 0, 0)),
                        keyboard_interactivity: KeyboardInteractivity::Exclusive,
                        output_option: OutputOption::LastOutput,
                        events_transparent: false,
                        namespace: Some("OxiBar modal".to_owned()),
                    },
                    id,
                },
            )),
            Message::CloseModalLayer(id) => Ok(LayershellCustomActionWithId::new(
                Some(id),
                LayershellCustomAction::RemoveWindow,
            )),
            Message::OpenPanelLayer(id) => Ok(LayershellCustomActionWithId::new(
                None,
                LayershellCustomAction::NewLayerShell {
                    settings: NewLayerShellSettings {
                        size: Some((PANEL_WIDTH, 0)),
                        layer: Layer::Overlay,
                        anchor: Anchor::Top | Anchor::Bottom | Anchor::Right,
                        exclusive_zone: None,
                        margin: Some((0, 0, 0, 0)),
                        keyboard_interactivity: KeyboardInteractivity::OnDemand,
                        output_option: OutputOption::LastOutput,
                        events_transparent: false,
                        namespace: Some("OxiBar notification panel".to_owned()),
                    },
                    id,
                },
            )),
            Message::ClosePanelLayer(id) => Ok(LayershellCustomActionWithId::new(
                Some(id),
                LayershellCustomAction::RemoveWindow,
            )),
            Message::OpenToastLayer(id, width, height, top) => {
                Ok(LayershellCustomActionWithId::new(
                    None,
                    LayershellCustomAction::NewLayerShell {
                        settings: NewLayerShellSettings {
                            size: Some((width, height)),
                            layer: Layer::Overlay,
                            anchor: Anchor::Top | Anchor::Right,
                            exclusive_zone: None,
                            margin: Some((top, TOAST_MARGIN_RIGHT, 0, 0)),
                            keyboard_interactivity: KeyboardInteractivity::None,
                            output_option: OutputOption::LastOutput,
                            events_transparent: false,
                            namespace: Some("OxiBar toast".to_owned()),
                        },
                        id,
                    },
                ))
            }
            Message::CloseToastLayer(id) => Ok(LayershellCustomActionWithId::new(
                Some(id),
                LayershellCustomAction::RemoveWindow,
            )),
            Message::MoveToastLayer(id, top) => Ok(LayershellCustomActionWithId::new(
                Some(id),
                LayershellCustomAction::MarginChange((top, TOAST_MARGIN_RIGHT, 0, 0)),
            )),
            Message::ResizeToastLayer(id, width, height) => Ok(LayershellCustomActionWithId::new(
                Some(id),
                LayershellCustomAction::SizeChange((width, height)),
            )),
            Message::SetToastKeyboardInteractivity(id, keyboard_interactivity) => {
                Ok(LayershellCustomActionWithId::new(
                    Some(id),
                    LayershellCustomAction::KeyboardInteractivityChange(keyboard_interactivity),
                ))
            }
            message => Err(message),
        }
    }
}

fn region_dimension(value: u32) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

pub(crate) fn map_plugin_message(plugin_id: String, msg: PluginMsg) -> Message {
    if let Some(request) = msg.downcast_ref::<HostToastRequest>() {
        return match request.action {
            HostToastAction::Show => Message::ShowPluginToast(plugin_id, request.clone()),
            HostToastAction::Close => {
                Message::ClosePluginToast(plugin_id, request.toast_id.clone())
            }
        };
    }

    if let Some(request) = msg.downcast_ref::<String>() {
        if let Some(toast_request) = HostToastRequest::parse_host_request(request) {
            return match toast_request.action {
                HostToastAction::Show => Message::ShowPluginToast(plugin_id, toast_request),
                HostToastAction::Close => {
                    Message::ClosePluginToast(plugin_id, toast_request.toast_id)
                }
            };
        }

        if let Some((toast_id, keyboard_interactivity)) = parse_toast_keyboard_request(request) {
            return Message::SetPluginToastKeyboard(plugin_id, toast_id, keyboard_interactivity);
        }

        match request.as_str() {
            HOST_REQUEST_TOGGLE_POPUP => return Message::TogglePluginPopup(plugin_id),
            HOST_REQUEST_OPEN_MODAL => return Message::OpenPluginModal(plugin_id),
            HOST_REQUEST_CLOSE_MODAL => return Message::ClosePluginModal(plugin_id),
            HOST_REQUEST_TOGGLE_PANEL => return Message::TogglePluginPanel(plugin_id),
            _ => {}
        }
    }
    Message::PluginSubMsg(plugin_id, msg)
}

fn parse_toast_keyboard_request(value: &str) -> Option<(String, KeyboardInteractivity)> {
    let parse = |prefix: &str, keyboard_interactivity| {
        value
            .strip_prefix(prefix)
            .filter(|toast_id| !toast_id.is_empty())
            .map(|toast_id| (toast_id.to_owned(), keyboard_interactivity))
    };
    parse(
        HOST_REQUEST_TOAST_KEYBOARD_NONE_PREFIX,
        KeyboardInteractivity::None,
    )
    .or_else(|| {
        parse(
            HOST_REQUEST_TOAST_KEYBOARD_ON_DEMAND_PREFIX,
            KeyboardInteractivity::OnDemand,
        )
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn plugin_msg(value: &str) -> PluginMsg {
        Arc::new(value.to_owned())
    }

    #[test]
    fn maps_host_request_messages() {
        assert!(matches!(
            map_plugin_message("Clock".to_owned(), plugin_msg(HOST_REQUEST_TOGGLE_POPUP)),
            Message::TogglePluginPopup(id) if id == "Clock"
        ));
        assert!(matches!(
            map_plugin_message("Bluetooth".to_owned(), plugin_msg(HOST_REQUEST_OPEN_MODAL)),
            Message::OpenPluginModal(id) if id == "Bluetooth"
        ));
        assert!(matches!(
            map_plugin_message("Bluetooth".to_owned(), plugin_msg(HOST_REQUEST_CLOSE_MODAL)),
            Message::ClosePluginModal(id) if id == "Bluetooth"
        ));
        assert!(matches!(
            map_plugin_message("Notifications".to_owned(), plugin_msg(HOST_REQUEST_TOGGLE_PANEL)),
            Message::TogglePluginPanel(id) if id == "Notifications"
        ));
    }

    #[test]
    fn maps_typed_toast_requests() {
        assert!(matches!(
            map_plugin_message(
                "Notifications".to_owned(),
                Arc::new(HostToastRequest::show("n1", 380, 144))
            ),
            Message::ShowPluginToast(id, request)
                if id == "Notifications" && request.toast_id == "n1"
        ));

        assert!(matches!(
            map_plugin_message(
                "Notifications".to_owned(),
                Arc::new(HostToastRequest::close("n1"))
            ),
            Message::ClosePluginToast(id, toast_id)
                if id == "Notifications" && toast_id == "n1"
        ));

        assert!(matches!(
            map_plugin_message(
                "Notifications".to_owned(),
                Arc::new(HostToastRequest::show("n2", 380, 144).to_host_request_string())
            ),
            Message::ShowPluginToast(id, request)
                if id == "Notifications" && request.toast_id == "n2"
        ));
    }

    #[test]
    fn maps_toast_keyboard_requests() {
        let enable = format!("{HOST_REQUEST_TOAST_KEYBOARD_ON_DEMAND_PREFIX}n1");
        assert!(matches!(
            map_plugin_message("Notifications".to_owned(), plugin_msg(&enable)),
            Message::SetPluginToastKeyboard(id, toast_id, KeyboardInteractivity::OnDemand)
                if id == "Notifications" && toast_id == "n1"
        ));

        let disable = format!("{HOST_REQUEST_TOAST_KEYBOARD_NONE_PREFIX}n1");
        assert!(matches!(
            map_plugin_message("Notifications".to_owned(), plugin_msg(&disable)),
            Message::SetPluginToastKeyboard(id, toast_id, KeyboardInteractivity::None)
                if id == "Notifications" && toast_id == "n1"
        ));
    }

    #[test]
    fn toast_layers_start_keyboard_disabled() {
        let id = IcedId::unique();
        let action = <Message as TryInto<LayershellCustomActionWithId>>::try_into(
            Message::OpenToastLayer(id, 380, 144, 16),
        )
        .unwrap();

        let LayershellCustomActionWithId(
            None,
            LayershellCustomAction::NewLayerShell {
                settings,
                id: action_id,
            },
        ) = action
        else {
            panic!("expected toast NewLayerShell action");
        };
        assert_eq!(action_id, id);
        assert_eq!(settings.keyboard_interactivity, KeyboardInteractivity::None);
    }

    #[test]
    fn toast_keyboard_changes_target_existing_window() {
        let id = IcedId::unique();
        let action = <Message as TryInto<LayershellCustomActionWithId>>::try_into(
            Message::SetToastKeyboardInteractivity(id, KeyboardInteractivity::OnDemand),
        )
        .unwrap();

        let LayershellCustomActionWithId(
            Some(action_id),
            LayershellCustomAction::KeyboardInteractivityChange(keyboard_interactivity),
        ) = action
        else {
            panic!("expected keyboard interactivity action");
        };
        assert_eq!(action_id, id);
        assert_eq!(keyboard_interactivity, KeyboardInteractivity::OnDemand);
    }

    #[test]
    fn leaves_non_host_messages_wrapped() {
        match map_plugin_message("Clock".to_owned(), plugin_msg("plugin-local")) {
            Message::PluginSubMsg(id, msg) => {
                assert_eq!(id, "Clock");
                assert_eq!(msg.downcast_ref::<String>().unwrap(), "plugin-local");
            }
            other => panic!("unexpected message: {other:?}"),
        }
    }
}
