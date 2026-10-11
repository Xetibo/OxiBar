use iced_layershell::{
    actions::{ActionCallback, LayershellCustomAction, LayershellCustomActionWithId},
    reexport::{Anchor, IcedId, KeyboardInteractivity, Layer, NewLayerShellSettings, OutputOption},
};
use oxibar_plugin_api::{
    HOST_REQUEST_CLOSE_MODAL, HOST_REQUEST_OPEN_MODAL, HOST_REQUEST_TOAST_KEYBOARD_NONE_PREFIX,
    HOST_REQUEST_TOAST_KEYBOARD_ON_DEMAND_PREFIX, HOST_REQUEST_TOGGLE_LAUNCHER,
    HOST_REQUEST_TOGGLE_PANEL, HOST_REQUEST_TOGGLE_POPUP, HostToastAction, HostToastRequest,
    PluginMsg,
};

use crate::layout::{
    BarDimensions, BarSection, MODAL_SIZE, PANEL_WIDTH, TOAST_MARGIN_RIGHT, popup_x,
};

#[derive(Debug, Clone)]
pub enum Message {
    ThemeFontLoaded(String, Result<crate::font::FontData, String>),
    ThemeFontReady(String, Result<String, String>),
    OverlayPointer(IcedId, Option<iced::Point>),
    OverlayPress(IcedId),
    Dock(crate::dock::Action),
    Desktop(crate::desktop::Action),
    CompositorChanged(Result<oxibar_plugin_api::CompositorSnapshot, String>),
    CompositorActionDone(Result<(), String>),
    Workspace(i64),
    OpenDockLayer(IcedId, (u32, u32), bool, u32, bool),
    ConfigureDockLayer(IcedId, (u32, u32), bool, u32, bool),
    OpenDesktopLayer(IcedId, crate::desktop::Rect, bool, bool),
    ConfigureDesktopLayer(IcedId, crate::desktop::Rect),
    SetLayerAnchor(IcedId, bool),
    ShowOsd(String, oxibar_plugin_api::HostOsdRequest),
    ExpireOsd(IcedId, u64),
    OpenOsdLayer(IcedId, crate::osd::OsdConfig),
    ConfigureOsdLayer(IcedId, crate::osd::OsdConfig),
    Ignore,
    SetLayerMargin(IcedId, (i32, i32, i32, i32)),
    OnCommandOutput(Option<String>, Box<Message>),
    CommandFinished {
        plugin: String,
        outcome: oxibar_plugin_api::CommandOutcome,
        reply: crate::ipc::Reply,
        output: Option<String>,
    },
    Exit,
    OutputsChanged(Result<crate::monitor::OutputSnapshot, String>),
    SurfaceClosed(IcedId),
    FromSurface(IcedId, Box<Message>),
    PluginTask(Option<IcedId>, Box<Message>),
    OnOutput(Option<String>, Box<Message>),
    OpenBarLayer(IcedId, Option<String>, crate::config::bars::BarConfig),
    ConfigureBar(IcedId, crate::config::bars::BarConfig),
    RemoveSurface(IcedId),
    SetBarInputRegion {
        id: IcedId,
        open: bool,
        section: BarSection,
        width: u32,
        height: u32,
        bar_size: BarDimensions,
        bottom: bool,
    },
    Settings(crate::settings::Action),
    ControlCenter(crate::control_center::Action),
    ControlCenterKey(IcedId, crate::control_center::Action),
    ControlCenterFocus(IcedId, bool),
    OpenControlCenterLayer(IcedId, u32, u32),
    SettingsEscape(IcedId),
    SettingsFocus(IcedId, bool),
    SettingsSearchFocus(IcedId),
    SettingsCategory(IcedId, crate::settings::Category),
    ControlRequest(crate::ipc::ControlRequest),
    ReloadConfig,
    ConfigUpdated(std::sync::Arc<crate::settings::ConfigOutcome>),
    OpenSettingsLayer(IcedId, u32, u32),
    CloseSettingsLayer(IcedId),
    ResizeLayer(IcedId, u32, u32),
    SetExclusiveZone(IcedId, i32),
    ControlLauncher(crate::ipc::LauncherCommand),
    Launcher(crate::launcher::Action),
    LauncherKey(IcedId, crate::launcher::Action),
    OpenLauncherLayer(IcedId, u32, u32),
    CloseLauncherLayer(IcedId),
    PluginSubMsg(String, PluginMsg),
    TogglePluginPopup(String),
    TogglePluginPanel(String),
    OpenPluginModal(String),
    ClosePluginModal(String),
    ModalEscape(IcedId),
    ModalFocus(IcedId, bool),
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
            Message::OpenDockLayer(id, size, bottom, margin, reserve) => {
                Ok(LayershellCustomActionWithId::new(
                    None,
                    LayershellCustomAction::NewLayerShell {
                        settings: NewLayerShellSettings {
                            size: Some(size),
                            layer: Layer::Top,
                            anchor: if bottom { Anchor::Bottom } else { Anchor::Top },
                            exclusive_zone: Some(if reserve {
                                (size.1 + margin) as i32
                            } else {
                                0
                            }),
                            margin: Some(if bottom {
                                (0, 0, margin as i32, 0)
                            } else {
                                (margin as i32, 0, 0, 0)
                            }),
                            keyboard_interactivity: KeyboardInteractivity::None,
                            output_option: OutputOption::LastOutput,
                            events_transparent: false,
                            namespace: Some("OxiBar dock".into()),
                        },
                        id,
                    },
                ))
            }
            Message::ConfigureDockLayer(id, size, bottom, margin, reserve) => Err(
                Message::ConfigureDockLayer(id, size, bottom, margin, reserve),
            ),
            Message::OpenDesktopLayer(id, rect, edit, interactive) => {
                Ok(LayershellCustomActionWithId::new(
                    None,
                    LayershellCustomAction::NewLayerShell {
                        settings: NewLayerShellSettings {
                            size: Some((rect.width, rect.height)),
                            layer: if edit {
                                Layer::Overlay
                            } else {
                                Layer::Background
                            },
                            anchor: Anchor::Top | Anchor::Left,
                            // Desktop placement uses the full output, not its bar-reserved area.
                            exclusive_zone: Some(-1),
                            margin: Some((rect.y as i32, 0, 0, rect.x as i32)),
                            keyboard_interactivity: if edit {
                                KeyboardInteractivity::Exclusive
                            } else {
                                KeyboardInteractivity::None
                            },
                            output_option: OutputOption::LastOutput,
                            events_transparent: !edit && !interactive,
                            namespace: Some(
                                if edit {
                                    "OxiBar widget editor"
                                } else {
                                    "OxiBar desktop widget"
                                }
                                .into(),
                            ),
                        },
                        id,
                    },
                ))
            }
            Message::ConfigureDesktopLayer(id, rect) => {
                Err(Message::ConfigureDesktopLayer(id, rect))
            }
            Message::SetLayerAnchor(id, bottom) => Ok(LayershellCustomActionWithId::new(
                Some(id),
                LayershellCustomAction::AnchorChange(if bottom {
                    Anchor::Bottom
                } else {
                    Anchor::Top
                }),
            )),
            Message::OnCommandOutput(output, message) => match (*message).try_into() {
                Ok(action) => Ok(action),
                Err(message) => Err(Message::OnCommandOutput(output, Box::new(message))),
            },
            Message::SetLayerMargin(id, margin) => Ok(LayershellCustomActionWithId::new(
                Some(id),
                LayershellCustomAction::MarginChange(margin),
            )),
            Message::OpenOsdLayer(id, config) => Ok(LayershellCustomActionWithId::new(
                None,
                LayershellCustomAction::NewLayerShell {
                    settings: NewLayerShellSettings {
                        size: Some(config.size),
                        layer: Layer::Overlay,
                        anchor: if config.bottom {
                            Anchor::Bottom
                        } else {
                            Anchor::Top
                        },
                        exclusive_zone: Some(0),
                        margin: Some(osd_margin(&config)),
                        keyboard_interactivity: KeyboardInteractivity::None,
                        output_option: OutputOption::LastOutput,
                        events_transparent: true,
                        namespace: Some("OxiBar OSD".to_owned()),
                    },
                    id,
                },
            )),
            Message::ConfigureOsdLayer(id, config) => Ok(LayershellCustomActionWithId::new(
                Some(id),
                LayershellCustomAction::AnchorSizeChange(
                    if config.bottom {
                        Anchor::Bottom
                    } else {
                        Anchor::Top
                    },
                    config.size,
                ),
            )),
            Message::OnOutput(output, message) => {
                let mut action: LayershellCustomActionWithId = (*message)
                    .try_into()
                    .map_err(|message| Message::OnOutput(output.clone(), Box::new(message)))?;
                if let LayershellCustomAction::NewLayerShell { settings, .. } = &mut action.1 {
                    settings.output_option = output
                        .map(OutputOption::OutputName)
                        .unwrap_or(OutputOption::LastOutput);
                }
                Ok(action)
            }
            Message::OpenBarLayer(id, output, config) => Ok(LayershellCustomActionWithId::new(
                None,
                LayershellCustomAction::NewLayerShell {
                    settings: NewLayerShellSettings {
                        size: Some((
                            config.width.unwrap_or(0),
                            config.height + crate::layout::POPUP_MAX_HEIGHT,
                        )),
                        layer: Layer::Top,
                        anchor: bar_anchor(config.position),
                        exclusive_zone: Some(config.height as i32),
                        margin: Some((0, 0, 0, 0)),
                        keyboard_interactivity: KeyboardInteractivity::None,
                        output_option: output
                            .map(OutputOption::OutputName)
                            .unwrap_or(OutputOption::None),
                        events_transparent: false,
                        namespace: Some("OxiBar".to_owned()),
                    },
                    id,
                },
            )),
            Message::ConfigureBar(id, config) => Ok(LayershellCustomActionWithId::new(
                Some(id),
                LayershellCustomAction::AnchorSizeChange(
                    bar_anchor(config.position),
                    (
                        config.width.unwrap_or(0),
                        config.height + crate::layout::POPUP_MAX_HEIGHT,
                    ),
                ),
            )),
            Message::RemoveSurface(id) => Ok(LayershellCustomActionWithId::new(
                Some(id),
                LayershellCustomAction::RemoveWindow,
            )),
            Message::SetBarInputRegion {
                id,
                open,
                section,
                width,
                height,
                bar_size,
                bottom,
            } => Ok(LayershellCustomActionWithId::new(
                Some(id),
                LayershellCustomAction::SetInputRegion(ActionCallback::new(move |region| {
                    let bar_y = if bottom {
                        crate::layout::POPUP_MAX_HEIGHT as i32
                    } else {
                        0
                    };
                    region.add(
                        0,
                        bar_y,
                        region_dimension(bar_size.width),
                        region_dimension(bar_size.height),
                    );
                    if open {
                        let popup_y = if bottom {
                            bar_y - region_dimension(height)
                        } else {
                            region_dimension(bar_size.height)
                        };
                        region.add(
                            popup_x(bar_size.width, section, width),
                            popup_y,
                            region_dimension(width),
                            region_dimension(height),
                        );
                    }
                })),
            )),
            Message::ResizeLayer(id, width, height) => Ok(LayershellCustomActionWithId::new(
                Some(id),
                LayershellCustomAction::SizeChange((width, height)),
            )),
            Message::SetExclusiveZone(id, zone) => Ok(LayershellCustomActionWithId::new(
                Some(id),
                LayershellCustomAction::ExclusiveZoneChange(zone),
            )),
            Message::OpenControlCenterLayer(id, width, height) => {
                Ok(LayershellCustomActionWithId::new(
                    None,
                    LayershellCustomAction::NewLayerShell {
                        settings: NewLayerShellSettings {
                            size: Some((width, height)),
                            layer: Layer::Overlay,
                            anchor: Anchor::empty(),
                            exclusive_zone: Some(0),
                            margin: Some((0, 0, 0, 0)),
                            keyboard_interactivity: KeyboardInteractivity::Exclusive,
                            output_option: OutputOption::LastOutput,
                            events_transparent: false,
                            namespace: Some("OxiBar control center".to_owned()),
                        },
                        id,
                    },
                ))
            }
            Message::OpenSettingsLayer(id, width, height) => Ok(LayershellCustomActionWithId::new(
                None,
                LayershellCustomAction::NewLayerShell {
                    settings: NewLayerShellSettings {
                        size: Some((width, height)),
                        layer: Layer::Overlay,
                        anchor: Anchor::empty(),
                        exclusive_zone: Some(0),
                        margin: Some((0, 0, 0, 0)),
                        keyboard_interactivity: KeyboardInteractivity::Exclusive,
                        output_option: OutputOption::LastOutput,
                        events_transparent: false,
                        namespace: Some("OxiBar settings".to_owned()),
                    },
                    id,
                },
            )),
            Message::CloseSettingsLayer(id) => Ok(LayershellCustomActionWithId::new(
                Some(id),
                LayershellCustomAction::RemoveWindow,
            )),
            Message::OpenLauncherLayer(id, width, height) => Ok(LayershellCustomActionWithId::new(
                None,
                LayershellCustomAction::NewLayerShell {
                    settings: NewLayerShellSettings {
                        size: Some((width, height)),
                        layer: Layer::Overlay,
                        anchor: Anchor::empty(),
                        exclusive_zone: Some(0),
                        margin: Some((0, 0, 0, 0)),
                        keyboard_interactivity: KeyboardInteractivity::Exclusive,
                        output_option: OutputOption::LastOutput,
                        events_transparent: false,
                        namespace: Some("OxiBar launcher".to_owned()),
                    },
                    id,
                },
            )),
            Message::CloseLauncherLayer(id) => Ok(LayershellCustomActionWithId::new(
                Some(id),
                LayershellCustomAction::RemoveWindow,
            )),
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

fn bar_anchor(position: crate::config::bars::Position) -> Anchor {
    let edge = match position {
        crate::config::bars::Position::Top => Anchor::Top,
        crate::config::bars::Position::Bottom => Anchor::Bottom,
    };
    edge | Anchor::Left | Anchor::Right
}

pub(crate) fn osd_margin(config: &crate::osd::OsdConfig) -> (i32, i32, i32, i32) {
    if config.bottom {
        (0, 0, config.margin as i32, 0)
    } else {
        (config.margin as i32, 0, 0, 0)
    }
}

fn region_dimension(value: u32) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

pub(crate) fn map_plugin_message(plugin_id: String, msg: PluginMsg) -> Message {
    if let Some(value) = msg
        .downcast_ref::<String>()
        .and_then(|value| value.strip_prefix(oxibar_plugin_api::HOST_REQUEST_WORKSPACE_PREFIX))
    {
        return value
            .parse::<i64>()
            .ok()
            .filter(|id| *id > 0 && *id <= u32::MAX as i64)
            .map_or(Message::Ignore, Message::Workspace);
    }
    if let Some(request) = msg.downcast_ref::<oxibar_plugin_api::HostOsdRequest>() {
        return Message::ShowOsd(plugin_id, request.clone());
    }
    if let Some(request) = msg.downcast_ref::<HostToastRequest>() {
        return match request.action {
            HostToastAction::Show => Message::ShowPluginToast(plugin_id, request.clone()),
            HostToastAction::Close => {
                Message::ClosePluginToast(plugin_id, request.toast_id.clone())
            }
        };
    }

    if let Some(request) = msg.downcast_ref::<String>() {
        if let Some(request) = oxibar_plugin_api::HostOsdRequest::parse_host_request(request) {
            return Message::ShowOsd(plugin_id, request);
        }
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
            HOST_REQUEST_TOGGLE_LAUNCHER => {
                return Message::Launcher(crate::launcher::Action::Toggle);
            }
            oxibar_plugin_api::HOST_REQUEST_OPEN_SETTINGS => {
                return Message::Settings(crate::settings::Action::Open);
            }
            oxibar_plugin_api::HOST_REQUEST_TOGGLE_CONTROL_CENTER => {
                return Message::ControlCenter(crate::control_center::Action::Toggle);
            }
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
    fn serialized_osds_map_and_command_context_survives_layer_action_conversion() {
        let request = oxibar_plugin_api::HostOsdRequest::new("volume").follow_focus();
        assert!(
            matches!(map_plugin_message("Audio".to_owned(), request.into_message()),
            Message::ShowOsd(id, request) if id == "Audio" && request.follow_focus)
        );
        let wrapped = Message::OnCommandOutput(
            Some("DP-2".to_owned()),
            Box::new(Message::PluginSubMsg(
                "Audio".to_owned(),
                plugin_msg("finished"),
            )),
        );
        let result: Result<LayershellCustomActionWithId, _> = wrapped.try_into();
        assert!(
            matches!(result, Err(Message::OnCommandOutput(Some(output), _)) if output == "DP-2")
        );
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
    fn panel_layers_stay_on_bar_output() {
        let id = IcedId::unique();
        let action = <Message as TryInto<LayershellCustomActionWithId>>::try_into(
            Message::OpenPanelLayer(id),
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
            panic!("expected panel NewLayerShell action");
        };
        assert_eq!(action_id, id);
        assert_eq!(settings.output_option, OutputOption::LastOutput);
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
