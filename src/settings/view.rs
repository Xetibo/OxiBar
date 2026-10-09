use iced::{
    Alignment, Element, Length,
    widget::{Column, Row, Space, container, scrollable, text, text_editor},
};
use oxibar_plugin_api::{SettingDescriptor, SettingKind};
use oxiced::{
    theme::theme_impl::OXITHEME,
    widgets::{oxi_picklist, oxi_plugin, oxi_text_input, oxi_toggler},
};
use toml::Value;

use super::{Action, Category, SEARCH_ID, edit_value};
use crate::{app::OxiBar, config::store::get_path, layout, messages::Message};

impl OxiBar {
    pub(crate) fn settings_view(&self) -> Element<'_, Message> {
        let busy = self.config.jobs > 0;
        let header = Row::new()
            .push(
                Column::new()
                    .push(
                        text("Settings")
                            .size(OXITHEME.font_xxl)
                            .style(oxi_plugin::text_primary),
                    )
                    .push(
                        text("Your shell, your preferences")
                            .size(OXITHEME.font_md)
                            .style(oxi_plugin::text_muted),
                    )
                    .spacing(OXITHEME.padding_sm),
            )
            .push(Space::new().width(Length::Fill))
            .push(action_button("Reload", Action::Reload, busy))
            .push(action_button("Close", Action::Close, false))
            .spacing(OXITHEME.padding_md)
            .align_y(Alignment::Center);
        let query = self.settings.search.to_lowercase();
        let category = self.settings.category;
        let search =
            oxi_text_input::text_input("Search settings", &self.settings.search, |value| {
                Message::Settings(Action::Search(value))
            })
            .id(SEARCH_ID)
            .size(OXITHEME.font_md);
        let mut body = Column::new()
            .spacing(OXITHEME.padding_lg)
            .width(Length::Fill);
        body = body.push(
            Column::new()
                .push(
                    text(if query.is_empty() {
                        category.label()
                    } else {
                        "Search results"
                    })
                    .size(OXITHEME.font_xl)
                    .style(oxi_plugin::text_primary),
                )
                .push(
                    text(if query.is_empty() {
                        category.description()
                    } else {
                        "Matching preferences across the shell."
                    })
                    .size(OXITHEME.font_md)
                    .style(oxi_plugin::text_muted),
                )
                .spacing(OXITHEME.padding_sm),
        );
        if query.is_empty() {
            match category {
                Category::Monitors => body = body.push(self.monitor_settings_view()),
                Category::Bar => body = body.push(self.widget_layout_view(busy)),
                Category::Plugins => body = body.push(self.plugin_settings_view(busy)),
                _ => {}
            }
        }
        let mut matches = 0;
        for field in &self.config.snapshot.schema {
            let searchable =
                format!("{} {} {}", field.path, field.label, field.description).to_lowercase();
            let show = if query.is_empty() {
                category.accepts(&field.path)
            } else {
                searchable.contains(&query)
            };
            if show && !["bar.start", "bar.center", "bar.end"].contains(&field.path.as_str()) {
                matches += 1;
                body = body.push(self.setting_field_view(field, busy));
            }
        }
        if !query.is_empty() && matches == 0 {
            body = body.push(oxi_plugin::settings_card(
                text("No matching settings")
                    .size(OXITHEME.font_md)
                    .style(oxi_plugin::text_muted),
            ));
        }
        if category == Category::Advanced && query.is_empty() {
            body = body.push(oxi_plugin::settings_card(
                Column::new()
                    .push(text("GUI overrides").size(OXITHEME.font_lg).style(oxi_plugin::text_primary))
                    .push(text("Sparse TOML preserves unknown plugin settings. Your base / Nix configuration stays read-only.").size(OXITHEME.font_md).style(oxi_plugin::text_muted))
                    .push(text_editor(&self.settings.editor)
                        .height(Length::Fixed(layout::SETTINGS_EDITOR_HEIGHT))
                        .size(OXITHEME.font_md)
                        .padding(OXITHEME.padding_lg)
                        .style(oxi_text_input::editor_style)
                        .on_action(|action| Message::Settings(Action::Advanced(action))))
                    .push(action_button("Save overrides", Action::SaveAdvanced, busy))
                    .spacing(OXITHEME.padding_lg),
            ));
        }
        let content = Column::new()
            .push(search)
            .push(
                scrollable(container(body).padding(iced::Padding::ZERO.right(OXITHEME.padding_md)))
                    .height(Length::Fill),
            )
            .spacing(OXITHEME.padding_xl)
            .width(Length::Fill)
            .height(Length::Fill);
        let main: Element<'_, Message> = if self.config.snapshot.host.settings_size.0
            >= layout::SETTINGS_NAV_BREAKPOINT
        {
            let mut navigation = Column::new()
                .spacing(OXITHEME.padding_sm)
                .width(layout::SETTINGS_NAV_WIDTH);
            for item in Category::ALL {
                navigation = navigation.push(
                    oxi_plugin::selectable_row(
                        text(item.label()).size(OXITHEME.font_md),
                        item == category,
                    )
                    .padding(OXITHEME.padding_lg)
                    .on_press(Message::Settings(Action::Category(item))),
                );
            }
            Row::new()
                .push(navigation)
                .push(content)
                .spacing(OXITHEME.padding_xl)
                .height(Length::Fill)
                .into()
        } else {
            Column::new()
                .push(
                    oxi_picklist::pick_list(Category::ALL.to_vec(), Some(category), |category| {
                        Message::Settings(Action::Category(category))
                    })
                    .width(Length::Fill),
                )
                .push(content)
                .spacing(OXITHEME.padding_lg)
                .height(Length::Fill)
                .into()
        };
        container(
            Column::new()
                .push(header)
                .push(main)
                .push(self.settings_footer(busy))
                .padding(OXITHEME.padding_xl)
                .spacing(OXITHEME.padding_xl)
                .height(Length::Fill)
                .width(Length::Fill),
        )
        .style(oxi_plugin::compact_card_style(OXITHEME.mantle))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn settings_footer(&self, busy: bool) -> Element<'_, Message> {
        let mut footer = Column::new().spacing(OXITHEME.padding_sm);
        if let Some(error) = &self.settings.error {
            footer = footer.push(
                text(error)
                    .size(OXITHEME.font_md)
                    .style(oxi_plugin::text_accent),
            );
        }
        if let Some(notice) = &self.settings.notice {
            footer = footer.push(
                text(notice)
                    .size(OXITHEME.font_md)
                    .style(oxi_plugin::text_muted),
            );
        }
        let restart = self.restart_fields();
        if !restart.is_empty() {
            footer = footer.push(
                text(format!("Restart pending: {}", restart.join(", ")))
                    .size(OXITHEME.font_md)
                    .style(oxi_plugin::text_muted),
            );
        }
        footer
            .push(
                Row::new()
                    .push(
                        Column::new()
                            .push(
                                text(if busy {
                                    "Saving / reloading…".to_owned()
                                } else {
                                    format!("Revision {}", self.config.snapshot.revision)
                                })
                                .size(OXITHEME.font_md)
                                .style(oxi_plugin::text_muted),
                            )
                            .push(
                                text(self.config.directory.display().to_string())
                                    .size(OXITHEME.font_sm)
                                    .style(oxi_plugin::text_muted),
                            )
                            .spacing(OXITHEME.padding_xs)
                            .width(Length::Fill),
                    )
                    .push(action_button("Reset all overrides", Action::ResetAll, busy))
                    .spacing(OXITHEME.padding_lg)
                    .align_y(Alignment::Center),
            )
            .into()
    }

    fn setting_field_view<'a>(
        &'a self,
        field: &'a SettingDescriptor,
        busy: bool,
    ) -> Element<'a, Message> {
        let snapshot = &self.config.snapshot;
        let current = get_path(&snapshot.effective, &field.path);
        let overridden = get_path(&snapshot.overrides, &field.path).is_some();
        let mut title = Column::new()
            .push(
                text(&field.label)
                    .size(OXITHEME.font_lg)
                    .style(oxi_plugin::text_primary),
            )
            .spacing(OXITHEME.padding_sm)
            .width(Length::Fill);
        if !field.description.is_empty() {
            title = title.push(
                text(&field.description)
                    .size(OXITHEME.font_md)
                    .style(oxi_plugin::text_muted),
            );
        }
        let path = field.path.clone();
        let mut body = Column::new().spacing(OXITHEME.padding_lg);
        if matches!(field.kind, SettingKind::Boolean) {
            let enabled = current.and_then(Value::as_bool).unwrap_or(false);
            let switch =
                oxi_toggler::toggler(enabled).on_toggle_maybe((!busy).then_some(move |value| {
                    Message::Settings(Action::Set(path.clone(), Value::Boolean(value)))
                }));
            body = body.push(
                Row::new()
                    .push(title)
                    .push(switch)
                    .spacing(OXITHEME.padding_xl)
                    .align_y(Alignment::Center),
            );
        } else {
            body = body.push(title);
            let control: Element<'_, Message> = match &field.kind {
                SettingKind::Choice(options) => oxi_picklist::pick_list(
                    options.clone(),
                    current.and_then(Value::as_str).map(str::to_owned),
                    move |value| Message::Settings(Action::Set(path.clone(), Value::String(value))),
                )
                .width(Length::Fill)
                .into(),
                _ => {
                    let value = self
                        .settings
                        .drafts
                        .get(&field.path)
                        .map(|draft| draft.value.clone())
                        .unwrap_or_else(|| edit_value(current, &field.kind));
                    let mut input =
                        oxi_text_input::text_input("Automatic / unset", &value, move |value| {
                            Message::Settings(Action::Edit(path.clone(), value))
                        })
                        .size(OXITHEME.font_md)
                        .on_submit(Message::Settings(Action::Apply(field.path.clone())));
                    if busy {
                        input = input.on_input_maybe(None::<fn(String) -> Message>);
                    }
                    Row::new()
                        .push(input)
                        .push(action_button(
                            "Apply",
                            Action::Apply(field.path.clone()),
                            busy,
                        ))
                        .spacing(OXITHEME.padding_md)
                        .align_y(Alignment::Center)
                        .into()
                }
            };
            body = body.push(control);
        }
        let base = snapshot.base_value(&field.path);
        body = body.push(
            Row::new()
                .push(oxi_plugin::badge(
                    snapshot.source(&field.path).label(),
                    overridden,
                ))
                .push(oxi_plugin::badge(field.apply.label(), false))
                .push(Space::new().width(Length::Fill))
                .push(action_button(
                    "Reset",
                    Action::Reset(field.path.clone()),
                    busy || !overridden,
                ))
                .spacing(OXITHEME.padding_sm)
                .align_y(Alignment::Center),
        );
        body = body.push(
            text(format!(
                "{} · base / default: {}",
                field.path,
                base.map(|value| value.to_string())
                    .unwrap_or_else(|| "Automatic / unset".to_owned())
            ))
            .size(OXITHEME.font_sm)
            .style(oxi_plugin::text_muted),
        );
        oxi_plugin::settings_card(body).into()
    }

    fn monitor_settings_view(&self) -> Element<'_, Message> {
        let mut body = Column::new().spacing(OXITHEME.padding_lg);
        if let Some(inventory) = &self.shell.inventory {
            body = body.push(
                text(format!("{} · read-only inventory", inventory.backend))
                    .size(OXITHEME.font_md)
                    .style(oxi_plugin::text_muted),
            );
            for output in &inventory.outputs {
                body = body.push(oxi_plugin::settings_card(
                    Column::new()
                        .push(
                            Row::new()
                                .push(
                                    text(&output.name)
                                        .size(OXITHEME.font_lg)
                                        .style(oxi_plugin::text_primary),
                                )
                                .push(Space::new().width(Length::Fill))
                                .push(oxi_plugin::badge(
                                    if !output.enabled {
                                        "Disabled"
                                    } else if output.focused {
                                        "Focused"
                                    } else {
                                        "Connected"
                                    },
                                    output.focused,
                                ))
                                .align_y(Alignment::Center),
                        )
                        .push(
                            text(&output.description)
                                .size(OXITHEME.font_md)
                                .style(oxi_plugin::text_muted),
                        )
                        .push(
                            text(format!(
                                "{} × {} pixels · {:.2} Hz",
                                output.physical_size.0, output.physical_size.1, output.refresh_hz
                            ))
                            .size(OXITHEME.font_md)
                            .style(oxi_plugin::text_primary),
                        )
                        .push(
                            text(format!(
                                "{} × {} logical · scale {:.3}\nPosition {}, {} · transform {}",
                                output.logical_size.0,
                                output.logical_size.1,
                                output.scale,
                                output.position.0,
                                output.position.1,
                                output.transform
                            ))
                            .size(OXITHEME.font_md)
                            .style(oxi_plugin::text_muted),
                        )
                        .spacing(OXITHEME.padding_md),
                ));
            }
        }
        if let Some(error) = &self.shell.inventory_error {
            body = body.push(
                text(error)
                    .size(OXITHEME.font_md)
                    .style(oxi_plugin::text_muted),
            );
        }
        body.into()
    }

    fn plugin_settings_view(&self, busy: bool) -> Element<'_, Message> {
        let enabled = crate::config::get_allowed_plugins(&self.config.snapshot.effective);
        let mut body = Column::new()
            .push(
                text("Installed libraries · enabling or disabling requires a restart")
                    .size(OXITHEME.font_md)
                    .style(oxi_plugin::text_muted),
            )
            .spacing(OXITHEME.padding_lg);
        for plugin in &self.settings.catalog {
            let file = plugin.filename.clone();
            let switch = oxi_toggler::toggler(enabled.contains(&file.as_str())).on_toggle_maybe(
                (!busy).then_some(move |enabled| {
                    Message::Settings(Action::EnablePlugin(file.clone(), enabled))
                }),
            );
            body = body.push(oxi_plugin::settings_card(
                Row::new()
                    .push(
                        Column::new()
                            .push(
                                text(&plugin.label)
                                    .size(OXITHEME.font_lg)
                                    .style(oxi_plugin::text_primary),
                            )
                            .push(
                                text(&plugin.state)
                                    .size(OXITHEME.font_md)
                                    .style(oxi_plugin::text_muted),
                            )
                            .push(
                                text(&plugin.filename)
                                    .size(OXITHEME.font_sm)
                                    .style(oxi_plugin::text_muted),
                            )
                            .spacing(OXITHEME.padding_sm)
                            .width(Length::Fill),
                    )
                    .push(switch)
                    .spacing(OXITHEME.padding_xl)
                    .align_y(Alignment::Center),
            ));
        }
        body.into()
    }

    fn widget_layout_view(&self, busy: bool) -> Element<'_, Message> {
        let available: Vec<String> = std::iter::once("settings".to_owned())
            .chain(self.launcher.enabled.then_some("launcher".to_owned()))
            .chain(
                self.config
                    .snapshot
                    .host
                    .control_center_enabled
                    .then_some("control-center".to_owned()),
            )
            .chain(
                self.plugins
                    .iter()
                    .filter(|(_, (model, funcs))| {
                        !crate::plugins::render_plugin(funcs, model).is_empty()
                    })
                    .map(|(name, _)| name.clone()),
            )
            .filter(|name| {
                !self
                    .start_widgets
                    .iter()
                    .chain(&self.center_widgets)
                    .chain(&self.end_widgets)
                    .any(|shown| shown.eq_ignore_ascii_case(name))
            })
            .collect();
        let mut sections = Column::new().spacing(OXITHEME.padding_lg);
        for (section, label, entries) in [
            ("start", "Start", &self.start_widgets),
            ("center", "Center", &self.center_widgets),
            ("end", "End", &self.end_widgets),
        ] {
            let mut column = Column::new()
                .push(
                    text(label)
                        .size(OXITHEME.font_lg)
                        .style(oxi_plugin::text_primary),
                )
                .spacing(OXITHEME.padding_md);
            for (index, name) in entries.iter().enumerate() {
                column = column.push(
                    Row::new()
                        .push(
                            text(name)
                                .size(OXITHEME.font_md)
                                .style(oxi_plugin::text_primary)
                                .width(Length::Fill),
                        )
                        .push(action_button(
                            "↑",
                            Action::MoveWidget(section.to_owned(), index, false),
                            busy || index == 0,
                        ))
                        .push(action_button(
                            "↓",
                            Action::MoveWidget(section.to_owned(), index, true),
                            busy || index + 1 == entries.len(),
                        ))
                        .push(action_button(
                            "Remove",
                            Action::RemoveWidget(section.to_owned(), index),
                            busy,
                        ))
                        .spacing(OXITHEME.padding_sm)
                        .align_y(Alignment::Center),
                );
            }
            if entries.is_empty() {
                column = column.push(
                    text("No widgets in this section")
                        .size(OXITHEME.font_md)
                        .style(oxi_plugin::text_muted),
                );
            }
            let key = section.to_owned();
            column = column.push(
                Row::new()
                    .push(
                        oxi_picklist::pick_list(
                            available.clone(),
                            self.settings.widget_choice.get(section).cloned(),
                            move |value| {
                                Message::Settings(Action::SelectWidget(key.clone(), value))
                            },
                        )
                        .placeholder("Choose a widget")
                        .width(Length::Fill),
                    )
                    .push(action_button(
                        "Add",
                        Action::AddWidget(section.to_owned()),
                        busy || !self.settings.widget_choice.contains_key(section),
                    ))
                    .spacing(OXITHEME.padding_md)
                    .align_y(Alignment::Center),
            );
            sections = sections.push(oxi_plugin::settings_card(column));
        }
        sections.into()
    }
}

fn action_button<'a>(
    label: &'a str,
    action: Action,
    disabled: bool,
) -> iced::widget::Button<'a, Message> {
    let primary = matches!(
        action,
        Action::Apply(_) | Action::SaveAdvanced | Action::AddWidget(_)
    );
    oxi_plugin::surface_button(text(label).size(OXITHEME.font_md), primary)
        .on_press_maybe((!disabled).then_some(Message::Settings(action)))
}
