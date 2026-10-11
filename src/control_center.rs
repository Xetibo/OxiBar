use iced::{
    Alignment, Element, Length, Task,
    widget::{Column, Row, Space, container, scrollable},
};
use iced_layershell::reexport::IcedId;
use oxiced::widgets::oxi_plugin::text;
use oxiced::{theme::theme_impl::OXITHEME, widgets::oxi_plugin};

use crate::{
    app::OxiBar,
    layout,
    messages::{Message, map_plugin_message},
    plugins::render_plugin_control_center,
    shell::SurfaceKind,
};

#[derive(Debug, Clone)]
pub enum Action {
    Open,
    Toggle,
    Close,
    Select(String),
    SelectIndex(usize),
}

#[derive(Default)]
pub(crate) struct ControlCenter {
    pub window_id: Option<IcedId>,
    pub selected: Option<String>,
}

impl OxiBar {
    pub(crate) fn control_center_plugins(&self) -> Vec<String> {
        let mut names = Vec::new();
        for configured in &self.config.snapshot.host.control_center_widgets {
            if let Some((name, _)) = self.plugins.iter().find(|(name, (_, funcs))| {
                name.eq_ignore_ascii_case(configured)
                    && (funcs.control_center_view.is_some()
                        || funcs.popup_view.is_some()
                        || funcs.panel_view.is_some())
            }) && !names.contains(name)
            {
                names.push(name.clone());
            }
        }
        names
    }

    pub(crate) fn normalize_control_center_selection(&mut self) {
        let available = self.control_center_plugins();
        if self
            .control_center
            .selected
            .as_ref()
            .is_none_or(|name| !available.contains(name))
        {
            self.control_center.selected = available.into_iter().next();
        }
    }

    pub(crate) fn update_control_center(&mut self, action: Action) -> Task<Message> {
        match action {
            Action::Toggle if self.control_center.window_id.is_some() => {
                self.close_control_center()
            }
            Action::Toggle | Action::Open => {
                if !self.config.snapshot.host.control_center_enabled
                    || self.control_center.window_id.is_some()
                {
                    return Task::none();
                }
                let id = IcedId::unique();
                let output = self.overlay_output();
                self.register_overlay(
                    id,
                    SurfaceKind::ControlCenter,
                    self.shell.active_bar,
                    output.clone(),
                );
                self.control_center.window_id = Some(id);
                self.normalize_control_center_selection();
                self.popup_open = false;
                let size = self.config.snapshot.host.control_center_size;
                let close_editor = if self.desktop.draft.is_some() {
                    self.cancel_desktop_edit()
                } else {
                    Task::none()
                };
                Task::batch([
                    self.close_launcher(),
                    self.close_settings(),
                    self.close_modal(),
                    self.close_panel(),
                    self.close_control_center_toasts(),
                    close_editor,
                    self.current_input_region_task(),
                    Task::done(Message::OnOutput(
                        output,
                        Box::new(Message::OpenControlCenterLayer(id, size.0, size.1)),
                    )),
                ])
            }
            Action::Close => self.close_control_center(),
            Action::SelectIndex(index) => {
                let Some(name) = self.control_center_plugins().get(index).cloned() else {
                    return Task::none();
                };
                self.update_control_center(Action::Select(name))
            }
            Action::Select(name) => {
                let Some(name) = self
                    .control_center_plugins()
                    .into_iter()
                    .find(|entry| entry.eq_ignore_ascii_case(&name))
                else {
                    return Task::none();
                };
                self.control_center.selected = Some(name);
                self.close_control_center_toasts()
            }
        }
    }

    pub(crate) fn close_control_center_toasts(&mut self) -> Task<Message> {
        if self.control_center.window_id.is_some()
            && self.control_center.selected.as_ref().is_some_and(|name| {
                self.plugins
                    .get(name)
                    .is_some_and(|(_, funcs)| funcs.toast_view.is_some())
            })
        {
            self.close_all_toasts()
        } else {
            Task::none()
        }
    }

    pub(crate) fn close_control_center(&mut self) -> Task<Message> {
        self.control_center
            .window_id
            .take()
            .map(|id| {
                self.shell.registry.remove(id);
                Task::done(Message::RemoveSurface(id))
            })
            .unwrap_or_else(Task::none)
    }

    pub(crate) fn control_center_view(&self) -> Element<'_, Message> {
        let plugins = self.control_center_plugins();
        let header = Row::new()
            .push(
                Column::new()
                    .push(
                        text("Control center")
                            .size(OXITHEME.font_xxl)
                            .style(oxi_plugin::text_primary),
                    )
                    .push(
                        text("Everything within reach")
                            .size(OXITHEME.font_md)
                            .style(oxi_plugin::text_muted),
                    )
                    .spacing(OXITHEME.padding_sm)
                    .width(Length::Fill),
            )
            .push(
                oxi_plugin::surface_button(text("Close").size(OXITHEME.font_md), false)
                    .on_press(Message::ControlCenter(Action::Close)),
            )
            .spacing(OXITHEME.padding_lg)
            .align_y(Alignment::Center);
        let columns = if self.config.snapshot.host.control_center_size.0
            < layout::CONTROL_CENTER_SINGLE_COLUMN_WIDTH
        {
            1
        } else {
            2
        };
        let mut body = Column::new()
            .spacing(OXITHEME.padding_xl)
            .width(Length::Fill);
        let mut tiles = Column::new().spacing(OXITHEME.padding_md);
        for group in plugins.chunks(columns) {
            let mut row = Row::new().spacing(OXITHEME.padding_md);
            for plugin in group {
                let selected = self.control_center.selected.as_ref() == Some(plugin);
                row = row.push(
                    oxi_plugin::selectable_row(
                        Column::new()
                            .push(
                                text(plugin.clone())
                                    .size(OXITHEME.font_lg)
                                    .style(oxi_plugin::text_primary),
                            )
                            .push(
                                text(control_description(plugin))
                                    .size(OXITHEME.font_md)
                                    .style(oxi_plugin::text_muted),
                            )
                            .spacing(OXITHEME.padding_sm),
                        selected,
                    )
                    .padding(OXITHEME.padding_lg)
                    .on_press(Message::ControlCenter(Action::Select(plugin.clone()))),
                );
            }
            if group.len() < columns {
                row = row.push(Space::new().width(Length::Fill));
            }
            tiles = tiles.push(row);
        }
        body = body.push(tiles);
        if let Some(selected) = &self.control_center.selected {
            if let Some((model, funcs)) = self.plugins.get(selected) {
                let mut detail = Column::new()
                    .push(
                        text(selected)
                            .size(OXITHEME.font_xl)
                            .style(oxi_plugin::text_primary),
                    )
                    .spacing(OXITHEME.padding_lg)
                    .width(Length::Fill);
                match render_plugin_control_center(funcs, model) {
                    Ok(elements) if !elements.is_empty() => {
                        for element in elements {
                            let plugin = selected.clone();
                            detail =
                                detail.push(element.map(move |message| {
                                    map_plugin_message(plugin.clone(), message)
                                }));
                        }
                    }
                    Ok(_) => {
                        detail = detail.push(
                            text("No controls available right now")
                                .size(OXITHEME.font_md)
                                .style(oxi_plugin::text_muted),
                        )
                    }
                    Err(error) => {
                        detail = detail.push(
                            text(format!("Controls unavailable: {error}"))
                                .size(OXITHEME.font_md)
                                .style(oxi_plugin::text_muted),
                        )
                    }
                }
                body = body.push(oxi_plugin::settings_card(detail));
            }
        } else {
            body = body.push(oxi_plugin::settings_card(Column::new()
                .push(text("No control widgets available").size(OXITHEME.font_lg).style(oxi_plugin::text_primary))
                .push(text("Enable plugins in Settings, then choose their names in Control center preferences.").size(OXITHEME.font_md).style(oxi_plugin::text_muted))
                .spacing(OXITHEME.padding_md)));
        }
        let footer = Row::new()
            .push(
                text("Alt + number to switch · Esc to close")
                    .size(OXITHEME.font_sm)
                    .style(oxi_plugin::text_muted),
            )
            .push(Space::new().width(Length::Fill))
            .push(
                oxi_plugin::surface_button(text("Settings").size(OXITHEME.font_md), true)
                    .on_press(Message::Settings(crate::settings::Action::Open)),
            )
            .spacing(OXITHEME.padding_lg)
            .align_y(Alignment::Center);
        container(
            Column::new()
                .push(header)
                .push(
                    scrollable(
                        container(body).padding(iced::Padding::ZERO.right(OXITHEME.padding_md)),
                    )
                    .height(Length::Fill),
                )
                .push(footer)
                .padding(OXITHEME.padding_xl)
                .spacing(OXITHEME.padding_xl)
                .height(Length::Fill)
                .width(Length::Fill),
        )
        .style(oxi_plugin::compact_card_style(OXITHEME.mantle))
        .height(Length::Fill)
        .width(Length::Fill)
        .into()
    }
}

fn control_description(name: &str) -> &'static str {
    match name.to_ascii_lowercase().as_str() {
        "audio" => "Sound & media",
        "network" => "Wi-Fi & connections",
        "bluetooth" => "Connected devices",
        "battery" => "Charge & remaining time",
        "notifications" => "Inbox & do not disturb",
        "clock" => "Calendar & events",
        _ => "Plugin controls",
    }
}

pub(crate) fn key_event(
    event: iced::Event,
    status: iced::event::Status,
    id: IcedId,
) -> Option<Message> {
    use iced::keyboard::{Event, Key, key::Named};
    let iced::Event::Keyboard(Event::KeyPressed { key, modifiers, .. }) = event else {
        return None;
    };
    match key {
        Key::Named(Named::Escape) => Some(Message::ControlCenterKey(id, Action::Close)),
        Key::Named(Named::Tab) if status == iced::event::Status::Ignored => {
            Some(Message::ControlCenterFocus(id, !modifiers.shift()))
        }
        Key::Character(key) if modifiers.alt() => key
            .parse::<usize>()
            .ok()
            .and_then(|index| index.checked_sub(1))
            .map(|index| Message::ControlCenterKey(id, Action::SelectIndex(index))),
        _ => None,
    }
}
