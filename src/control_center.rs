use iced::{
    Alignment, Element, Length, Task,
    widget::{Column, Row, Space, container},
};
use iced_layershell::reexport::IcedId;
use oxiced::widgets::oxi_plugin::text;
use oxiced::{theme::theme_impl::OXITHEME, widgets::oxi_plugin};

use crate::{
    app::OxiBar,
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
    EditOverview,
    AddCard(String),
    RemoveCard(usize),
    MoveCard(usize, bool),
}

#[derive(Default)]
pub(crate) struct ControlCenter {
    pub window_id: Option<IcedId>,
    pub attached_to: Option<IcedId>,
    pub selected: Option<String>,
    pub editing_overview: bool,
}

impl OxiBar {
    pub(crate) fn control_center_plugins(&self) -> Vec<String> {
        let mut names = Vec::new();
        for configured in &self.config.snapshot.host.control_center_widgets {
            if configured.eq_ignore_ascii_case("Overview") {
                if !names.iter().any(|name: &String| name == "Overview") {
                    names.push("Overview".into());
                }
                continue;
            }
            if let Some((name, _)) = self.plugins.iter().find(|(name, (model, funcs))| {
                name.eq_ignore_ascii_case(configured)
                    && funcs
                        .control_center_available
                        .is_none_or(|available| unsafe { available(model.clone()) })
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
            Action::EditOverview => {
                self.control_center.editing_overview = !self.control_center.editing_overview;
                Task::none()
            }
            Action::AddCard(_) | Action::RemoveCard(_) | Action::MoveCard(..) => {
                if self.config.jobs > 0 {
                    return Task::none();
                }
                let mut widgets = self
                    .config
                    .snapshot
                    .host
                    .control_center_overview_widgets
                    .clone();
                match action {
                    Action::AddCard(id)
                        if self.subwidget_catalog().iter().any(|(key, _)| *key == id)
                            && !widgets.contains(&id) =>
                    {
                        widgets.push(id)
                    }
                    Action::RemoveCard(index) if index < widgets.len() => {
                        widgets.remove(index);
                    }
                    Action::MoveCard(index, forward) if index < widgets.len() => {
                        let target = if forward {
                            index.saturating_add(1)
                        } else {
                            index.saturating_sub(1)
                        };
                        if target < widgets.len() {
                            widgets.swap(index, target);
                        }
                    }
                    _ => return Task::none(),
                }
                self.config_job(
                    Some(crate::config::Mutation::Set(
                        "control_center.overview.widgets".into(),
                        toml::Value::Array(widgets.into_iter().map(toml::Value::String).collect()),
                    )),
                    Some(self.config.snapshot.revision),
                    None,
                    None,
                    false,
                )
            }
            Action::Toggle if self.control_center.window_id.is_some() => {
                self.close_control_center()
            }
            Action::Toggle | Action::Open => {
                if !self.config.snapshot.host.control_center_enabled
                    || self.control_center.window_id.is_some()
                {
                    return Task::none();
                }
                if self.config.snapshot.host.menus.controls_dropdown
                    && let Some(bar) = self.dropdown_bar()
                {
                    return self.open_attached_controls(bar, None);
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
                    self.close_overview(),
                    self.close_oxipaste(),
                    self.close_launcher(),
                    self.close_settings(),
                    self.close_modal(),
                    self.close_panel(),
                    self.close_bar_dropdown(),
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
        if self.control_center.attached_to.is_some() {
            return self.close_bar_dropdown();
        }
        self.control_center
            .window_id
            .take()
            .map(|id| {
                self.shell.registry.remove(id);
                Task::done(Message::RemoveSurface(id))
            })
            .unwrap_or_else(Task::none)
    }

    pub(crate) fn open_attached_controls(
        &mut self,
        bar: IcedId,
        selected: Option<String>,
    ) -> Task<Message> {
        if self.shell.bar(bar).is_none() || !self.config.snapshot.host.control_center_enabled {
            return Task::none();
        }
        let close = self.close_control_center();
        let close_popup = self.close_bar_dropdown();
        let dismiss = Task::batch([
            self.close_overview(),
            self.close_oxipaste(),
            close,
            close_popup,
            self.close_launcher(),
            self.close_settings(),
            self.close_modal(),
            self.close_panel(),
        ]);
        self.control_center.window_id = Some(bar);
        self.control_center.attached_to = Some(bar);
        self.normalize_control_center_selection();
        if let Some(selected) = selected.and_then(|selected| {
            self.control_center_plugins()
                .into_iter()
                .find(|name| name.eq_ignore_ascii_case(&selected))
        }) {
            self.control_center.selected = Some(selected);
        }
        self.shell.popup_bar = Some(bar);
        self.popup_plugin = Some("@control-center".into());
        self.popup_open = true;
        dismiss.chain(Task::batch([
            self.current_input_region_task(),
            self.close_control_center_toasts(),
            Task::done(Message::SetToastKeyboardInteractivity(
                bar,
                iced_layershell::reexport::KeyboardInteractivity::Exclusive,
            )),
        ]))
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
                    .spacing(OXITHEME.padding_sm)
                    .width(Length::Fill),
            )
            .push(
                oxi_plugin::surface_button(text("Close").size(OXITHEME.font_md), false)
                    .on_press(Message::ControlCenter(Action::Close)),
            )
            .spacing(OXITHEME.padding_lg)
            .align_y(Alignment::Center);
        let mut navigation = Column::new().spacing(OXITHEME.padding_xs).width(180);
        for plugin in &plugins {
            navigation = navigation.push(
                oxi_plugin::navigation_row(
                    text(plugin.clone()).size(OXITHEME.font_md),
                    self.control_center.selected.as_ref() == Some(plugin),
                )
                .padding([OXITHEME.padding_sm, OXITHEME.padding_md])
                .on_press(Message::ControlCenter(Action::Select(plugin.clone()))),
            );
        }
        let fill_height = self
            .control_center
            .selected
            .as_deref()
            .is_some_and(|name| name.eq_ignore_ascii_case("clock"));
        let mut body = Column::new()
            .spacing(OXITHEME.padding_lg)
            .width(Length::Fill);
        if let Some(selected) = &self.control_center.selected {
            if selected == "Overview" {
                body = body.push(self.control_overview_view());
            } else if let Some((model, funcs)) = self.plugins.get(selected) {
                let mut detail = Column::new()
                    .push(
                        text(selected)
                            .size(OXITHEME.font_xl)
                            .style(oxi_plugin::text_primary),
                    )
                    .push(
                        text(control_description(selected))
                            .size(OXITHEME.font_md)
                            .style(oxi_plugin::text_muted),
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
                body = body.push(if fill_height {
                    oxi_plugin::settings_card(detail.height(Length::Fill)).height(Length::Fill)
                } else {
                    oxi_plugin::settings_card(detail)
                });
            }
        } else {
            body = body.push(oxi_plugin::settings_card(Column::new()
                .push(text("No control widgets available").size(OXITHEME.font_lg).style(oxi_plugin::text_primary))
                .push(text("Enable plugins in Settings, then choose their names in Control center preferences.").size(OXITHEME.font_md).style(oxi_plugin::text_muted))
                .spacing(OXITHEME.padding_md)));
        }
        let content: Element<'_, Message> = if fill_height {
            body.height(Length::Fill).into()
        } else {
            oxi_plugin::scroll_panel(body).into()
        };
        let footer = Row::new()
            .push(
                text(format!(
                    "{}: first section · {}: close · All keys in Shortcuts",
                    self.shortcut_label("controls.section_1"),
                    self.shortcut_label("close")
                ))
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
                    Row::new()
                        .push(navigation)
                        .push(content)
                        .spacing(OXITHEME.padding_xl)
                        .width(Length::Fill)
                        .height(Length::Fill),
                )
                .push(footer)
                .padding(OXITHEME.padding_lg)
                .spacing(OXITHEME.padding_lg)
                .height(Length::Fill)
                .width(Length::Fill),
        )
        .style(oxi_plugin::compact_card_style(OXITHEME.mantle))
        .height(Length::Fill)
        .width(Length::Fill)
        .into()
    }

    fn subwidget_catalog(&self) -> Vec<(String, String)> {
        self.plugins
            .iter()
            .flat_map(|(name, (model, funcs))| {
                if funcs.subwidget_view.is_none() {
                    return Vec::new();
                }
                funcs
                    .subwidgets
                    .map(|get| {
                        unsafe { get(model.clone()) }
                            .into_iter()
                            .map(|widget| (format!("{name}:{}", widget.id), widget.title))
                            .collect()
                    })
                    .unwrap_or_default()
            })
            .collect()
    }

    pub(crate) fn control_center_uses_subwidgets(&self, provider: &str) -> bool {
        self.control_center.selected.as_deref() == Some("Overview")
            && self
                .config
                .snapshot
                .host
                .control_center_overview_widgets
                .iter()
                .any(|id| {
                    id.split_once(':')
                        .is_some_and(|(name, _)| name.eq_ignore_ascii_case(provider))
                })
    }

    fn control_overview_view(&self) -> Element<'_, Message> {
        let catalog = self.subwidget_catalog();
        let editing = self.control_center.editing_overview;
        let configured = &self.config.snapshot.host.control_center_overview_widgets;
        let mut content = Column::new().spacing(OXITHEME.padding_lg).push(
            Row::new()
                .push(text("Overview").size(OXITHEME.font_xl))
                .push(Space::new().width(Length::Fill))
                .push(
                    oxi_plugin::surface_button(
                        text(if editing { "Done" } else { "Edit cards" }),
                        editing,
                    )
                    .on_press(Message::ControlCenter(Action::EditOverview)),
                ),
        );
        if editing {
            for (index, id) in configured.iter().enumerate() {
                let label = catalog
                    .iter()
                    .find(|(key, _)| key == id)
                    .map(|(_, title)| title.as_str())
                    .unwrap_or(id);
                let mut row = Row::new()
                    .spacing(OXITHEME.padding_sm)
                    .push(text(label.to_owned()).width(Length::Fill));
                for (label, action, enabled) in [
                    ("↑", Action::MoveCard(index, false), index > 0),
                    (
                        "↓",
                        Action::MoveCard(index, true),
                        index + 1 < configured.len(),
                    ),
                    ("Remove", Action::RemoveCard(index), true),
                ] {
                    row = row.push(
                        oxi_plugin::surface_button(text(label), false).on_press_maybe(
                            (enabled && self.config.jobs == 0)
                                .then_some(Message::ControlCenter(action)),
                        ),
                    );
                }
                content = content.push(row);
            }
            for (id, title) in &catalog {
                if !configured.contains(id) {
                    content = content.push(
                        oxi_plugin::surface_button(text(format!("Add {title}")), false)
                            .on_press_maybe(
                                (self.config.jobs == 0)
                                    .then_some(Message::ControlCenter(Action::AddCard(id.clone()))),
                            ),
                    );
                }
            }
        }
        let columns = self
            .config
            .snapshot
            .host
            .control_center_overview_columns
            .max(1);
        let mut cards = Vec::new();
        for id in configured {
            let Some((_, title)) = catalog.iter().find(|(key, _)| key == id) else {
                continue;
            };
            let Some((provider, widget)) = id.split_once(':') else {
                continue;
            };
            let Some((name, (model, funcs))) = self
                .plugins
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(provider))
            else {
                continue;
            };
            let Some(view) = funcs.subwidget_view else {
                continue;
            };
            let mut card = Column::new()
                .spacing(OXITHEME.padding_lg)
                .width(Length::Fill)
                .push(text(title.clone()).size(OXITHEME.font_lg));
            match unsafe { view(model.clone(), widget) } {
                Ok(elements) => {
                    for element in elements {
                        let provider = name.clone();
                        card = card.push(
                            element
                                .map(move |message| map_plugin_message(provider.clone(), message)),
                        );
                    }
                }
                Err(error) => {
                    card = card.push(text(error.to_string()).style(oxi_plugin::text_muted));
                }
            }
            cards.push(oxi_plugin::settings_card(card).width(Length::FillPortion(1)));
        }
        let empty = cards.is_empty();
        let mut cards = cards.into_iter();
        loop {
            let mut row = Row::new().spacing(OXITHEME.padding_lg).width(Length::Fill);
            let mut count = 0;
            for _ in 0..columns {
                if let Some(card) = cards.next() {
                    row = row.push(card);
                    count += 1;
                }
            }
            if count == 0 {
                break;
            }
            for _ in count..columns {
                row = row.push(Space::new().width(Length::FillPortion(1)));
            }
            content = content.push(row);
        }
        if empty {
            content = content.push(
                text("Enable card providers in Settings, then add cards here.")
                    .style(oxi_plugin::text_muted),
            );
        }
        content.into()
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
        "system" => "Brightness & power",
        _ => "Plugin controls",
    }
}
