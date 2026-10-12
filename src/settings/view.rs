use iced::{
    Alignment, Element, Length,
    widget::{Column, Row, Space, container, text_editor},
};
use iced_anim::{AnimationBuilder, Easing};
use oxibar_plugin_api::{SettingDescriptor, SettingKind};
use oxiced::widgets::oxi_plugin::text;
use oxiced::{
    theme::theme_impl::OXITHEME,
    widgets::{oxi_drag_handle, oxi_picklist, oxi_plugin, oxi_text_input, oxi_toggler},
};
use toml::Value;

use super::{
    Action, SEARCH_ID, edit_value,
    navigation::{Content, Navigation, Page},
};
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
            .push(inspected(
                "@header/reset",
                action_button("Reset all overrides", Action::ResetAll, busy),
            ))
            .push(inspected(
                "@header/reload",
                action_button("Reload", Action::Reload, busy),
            ))
            .push(inspected(
                "@header/close",
                action_button("Close", Action::Close, false),
            ))
            .spacing(OXITHEME.padding_md)
            .align_y(Alignment::Center);
        let query = self.settings.search.to_lowercase();
        let navigation = &self.settings.navigation;
        let page = navigation.current();
        let search =
            oxi_text_input::text_input("Search settings", &self.settings.search, |value| {
                Message::Settings(Action::Search(value))
            })
            .id(SEARCH_ID)
            .size(OXITHEME.font_md);
        let mut body = Column::new()
            .spacing(OXITHEME.padding_lg)
            .width(Length::Fill);
        if query.is_empty() {
            let mut breadcrumbs = Row::new()
                .spacing(OXITHEME.padding_sm)
                .align_y(Alignment::Center);
            for (index, ancestor) in navigation.breadcrumbs().into_iter().enumerate() {
                if index > 0 {
                    breadcrumbs = breadcrumbs.push(
                        text("›")
                            .size(OXITHEME.font_md)
                            .style(oxi_plugin::text_muted),
                    );
                }
                breadcrumbs = breadcrumbs.push(
                    iced::widget::button(text(ancestor.label.clone()).size(OXITHEME.font_sm))
                        .style(oxi_plugin::bar_button_style)
                        .padding([OXITHEME.padding_xs, OXITHEME.padding_sm])
                        .on_press(Message::Settings(Action::Page(ancestor.id.clone()))),
                );
            }
            body = body.push(breadcrumbs);
        }
        body = body.push(
            Column::new()
                .push(
                    text(if query.is_empty() {
                        page.label.as_str()
                    } else {
                        "Search results"
                    })
                    .size(OXITHEME.font_xl)
                    .style(oxi_plugin::text_primary),
                )
                .push(
                    text(if query.is_empty() {
                        page.description.as_str()
                    } else {
                        "Matching preferences across the shell."
                    })
                    .size(OXITHEME.font_md)
                    .style(oxi_plugin::text_muted),
                )
                .spacing(OXITHEME.padding_sm),
        );
        if query.is_empty() {
            for child in &page.children {
                body = body.push(inspected(
                    format!("@page/{}", child.id),
                    oxi_plugin::surface_button(
                        Row::new()
                            .push(
                                Column::new()
                                    .push(
                                        text(child.label.clone())
                                            .size(OXITHEME.font_lg)
                                            .style(oxi_plugin::text_primary),
                                    )
                                    .push(
                                        text(child.description.clone())
                                            .size(OXITHEME.font_md)
                                            .style(oxi_plugin::text_muted),
                                    )
                                    .spacing(OXITHEME.padding_sm)
                                    .width(Length::Fill),
                            )
                            .push(
                                text(if child.field_count() == 0 {
                                    "›".into()
                                } else {
                                    format!("{}  ›", child.field_count())
                                })
                                .size(OXITHEME.font_md)
                                .style(oxi_plugin::text_muted),
                            )
                            .spacing(OXITHEME.padding_lg)
                            .align_y(Alignment::Center),
                        false,
                    )
                    .padding(OXITHEME.padding_lg)
                    .width(Length::Fill)
                    .on_press(Message::Settings(Action::Page(child.id.clone()))),
                ));
            }
            match page.content {
                Content::Monitors => body = body.push(self.monitor_settings_view()),
                Content::Layout => body = body.push(self.widget_layout_view(busy)),
                Content::Plugins => body = body.push(self.plugin_settings_view(busy)),
                Content::Desktop => body = body.push(self.desktop_settings_view()),
                Content::Providers => body = body.push(self.default_providers_view(busy)),
                _ => {}
            }
        }
        let mut matches = 0;
        for field in &self.config.snapshot.schema {
            let searchable =
                format!("{} {} {}", field.path, field.label, field.description).to_lowercase();
            let show = if query.is_empty() {
                page.fields.contains(&field.path)
            } else {
                searchable.contains(&query)
            };
            if show {
                matches += 1;
                if let Some(route) = custom_field_page(&field.path) {
                    if !query.is_empty() {
                        body = body.push(
                            oxi_plugin::surface_button(
                                text(format!("{}  ›", field.label)).size(OXITHEME.font_md),
                                false,
                            )
                            .on_press(Message::Settings(Action::Page(route.into()))),
                        );
                    }
                } else {
                    body = body.push(inspected(
                        format!("@field/{}", field.path),
                        self.setting_field_view(field, busy),
                    ));
                }
            }
        }
        if !query.is_empty() && matches == 0 {
            body = body.push(oxi_plugin::settings_card(
                text("No matching settings")
                    .size(OXITHEME.font_md)
                    .style(oxi_plugin::text_muted),
            ));
        }
        if page.content == Content::Overrides && query.is_empty() {
            body = body.push(oxi_plugin::settings_card(
                Column::new()
                    .push(text("GUI overrides").size(OXITHEME.font_lg).style(oxi_plugin::text_primary))
                    .push(text("Sparse TOML preserves unknown plugin settings. Your base / Nix configuration stays read-only.").size(OXITHEME.font_md).style(oxi_plugin::text_muted))
                    .push(text(self.config.directory.display().to_string()).size(OXITHEME.font_sm).style(oxi_plugin::text_muted))
                    .push(text_editor(&self.settings.editor)
                        .font(oxiced::theme::snapshot::current_font())
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
            .push(oxi_plugin::scroll_panel(body))
            .spacing(OXITHEME.padding_xl)
            .width(Length::Fill)
            .height(Length::Fill);
        let main: Element<'_, Message> =
            if self.config.snapshot.host.settings_size.0 >= layout::SETTINGS_NAV_BREAKPOINT {
                let mut sidebar = Column::new()
                    .spacing(OXITHEME.padding_sm)
                    .width(Length::Fill);
                for root in &navigation.roots {
                    sidebar = sidebar.push(navigation_page(navigation, root));
                }
                Row::new()
                    .push(
                        oxi_plugin::scroll_panel(sidebar)
                            .width(layout::SETTINGS_NAV_WIDTH)
                            .height(Length::Fill),
                    )
                    .push(content)
                    .spacing(OXITHEME.padding_xl)
                    .height(Length::Fill)
                    .into()
            } else {
                let choices = navigation.choices();
                let selected = choices.iter().find(|choice| choice.id == page.id).cloned();
                Column::new()
                    .push(
                        oxi_picklist::pick_list(choices, selected, |choice| {
                            Message::Settings(Action::Page(choice.id))
                        })
                        .width(Length::Fill),
                    )
                    .push(content)
                    .spacing(OXITHEME.padding_lg)
                    .height(Length::Fill)
                    .into()
            };
        let surface: Element<'_, Message> = container(
            Column::new()
                .push(header)
                .push(main)
                .push(inspected("@footer", self.settings_footer(busy)))
                .padding(OXITHEME.padding_xl)
                .spacing(OXITHEME.padding_xl)
                .height(Length::Fill)
                .width(Length::Fill),
        )
        .style(oxi_plugin::compact_card_style(OXITHEME.mantle))
        .width(Length::Fill)
        .height(Length::Fill)
        .into();
        let preview = self
            .settings
            .widget_drag
            .as_ref()
            .filter(|_| !busy)
            .map(|drag| {
                container(
                    text(format!("⠿  {}", drag.name))
                        .size(OXITHEME.font_md)
                        .style(oxi_plugin::text_accent),
                )
                .padding(OXITHEME.padding_lg)
                .width(220)
                .style(oxi_plugin::compact_card_style(OXITHEME.secondary_bg))
                .into()
            });
        oxi_drag_handle::drag_preview(surface, preview).into()
    }

    fn settings_footer(&self, busy: bool) -> Element<'_, Message> {
        let status = self
            .settings
            .error
            .as_deref()
            .or(self.settings.notice.as_deref())
            .unwrap_or("");
        let restart = self.restart_fields();
        let feedback = if restart.is_empty() {
            status.to_owned()
        } else {
            format!("{status} · Restart pending: {}", restart.join(", "))
        };
        Row::new()
            .push(
                text(if busy {
                    "Saving / reloading…".to_owned()
                } else {
                    format!("Revision {}", self.config.snapshot.revision)
                })
                .size(OXITHEME.font_sm)
                .style(oxi_plugin::text_muted),
            )
            .push(
                oxi_plugin::scroll_panel(text(feedback).size(OXITHEME.font_sm).style(
                    if self.settings.error.is_some() {
                        oxi_plugin::text_accent
                    } else {
                        oxi_plugin::text_muted
                    },
                ))
                .width(Length::Fill)
                .height(OXITHEME.font_sm * 1.5 + OXITHEME.padding_xs),
            )
            .spacing(OXITHEME.padding_lg)
            .align_y(Alignment::Center)
            .into()
    }

    fn desktop_settings_view(&self) -> Element<'_, Message> {
        let mut body=Column::new().spacing(OXITHEME.padding_lg)
            .push(text("Persistent widget instances").size(OXITHEME.font_lg).style(oxi_plugin::text_primary))
            .push(text("Add, move and resize widgets in explicit desktop edit mode. Placements use logical pixels; disconnected-output fallback preserves your saved layout.").size(OXITHEME.font_md).style(oxi_plugin::text_muted))
            .push(oxi_plugin::surface_button(text("Edit desktop widgets").size(OXITHEME.font_md),true).on_press(Message::Desktop(crate::desktop::Action::Edit)));
        for widget in &self.config.snapshot.host.desktop.widgets {
            body = body.push(
                text(format!(
                    "{} · {} · {} · {}, {} · {} × {}{}",
                    widget.id,
                    widget.plugin,
                    if widget.output.is_empty() {
                        "Selected output"
                    } else {
                        &widget.output
                    },
                    widget.x,
                    widget.y,
                    widget.width,
                    widget.height,
                    if widget.enabled { "" } else { " · hidden" }
                ))
                .size(OXITHEME.font_md)
                .style(oxi_plugin::text_muted),
            );
        }
        body = body.push(
            oxi_plugin::surface_button(
                text("Reset widget layout to base / Nix").size(OXITHEME.font_md),
                false,
            )
            .on_press(Message::Settings(Action::Reset("desktop.widgets".into()))),
        );
        oxi_plugin::settings_card(body).into()
    }

    fn setting_field_view<'a>(
        &'a self,
        field: &'a SettingDescriptor,
        busy: bool,
    ) -> Element<'a, Message> {
        let snapshot = &self.config.snapshot;
        let current_value = self.setting_value(&field.path);
        let current = current_value.as_ref();
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
        if self.settings.monitors.controller.is_some() {
            return self.monitor_controls_view();
        }
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
                text("Enable installed providers here. Changes take effect immediately.")
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
                            .push(
                                text(format!(
                                    "ABI {} · {}",
                                    plugin
                                        .abi
                                        .map(|abi| abi.to_string())
                                        .unwrap_or_else(|| "unknown".to_owned()),
                                    if plugin.capabilities.is_empty() {
                                        "bar only".to_owned()
                                    } else {
                                        plugin.capabilities.join(" · ")
                                    }
                                ))
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

    fn default_providers_view(&self, busy: bool) -> Element<'_, Message> {
        let defaults = get_path(&self.config.snapshot.effective, "launcher.default_plugins")
            .and_then(Value::as_array);
        let enabled = crate::config::DEFAULT_RUNNER_PLUGINS.iter().all(|file| {
            defaults.is_some_and(|values| values.iter().any(|value| value.as_str() == Some(file)))
        });
        let mut body = Column::new()
            .spacing(OXITHEME.padding_lg)
            .push(oxi_plugin::settings_card(
            Row::new()
                .push(
                    Column::new()
                        .push(
                            text("Default launcher plugins")
                                .size(OXITHEME.font_lg)
                                .style(oxi_plugin::text_primary),
                        )
                        .push(
                            text("Include installed Applications and Tools providers immediately.")
                                .size(OXITHEME.font_md)
                                .style(oxi_plugin::text_muted),
                        )
                        .spacing(OXITHEME.padding_sm)
                        .width(Length::Fill),
                )
                .push(inspected(
                    "@launcher/default-providers",
                    oxi_toggler::toggler(enabled).on_toggle_maybe((!busy).then_some(|enabled| {
                        Message::Settings(Action::EnableDefaultProviders(enabled))
                    })),
                ))
                .spacing(OXITHEME.padding_xl)
                .align_y(Alignment::Center),
        ));
        for filename in crate::config::DEFAULT_RUNNER_PLUGINS {
            if let Some(plugin) = self
                .settings
                .catalog
                .iter()
                .find(|plugin| plugin.filename == filename)
            {
                body = body.push(oxi_plugin::settings_card(
                    Row::new()
                        .push(
                            Column::new()
                                .push(text(&plugin.label).size(OXITHEME.font_lg))
                                .push(
                                    text(&plugin.state)
                                        .size(OXITHEME.font_md)
                                        .style(oxi_plugin::text_muted),
                                )
                                .spacing(OXITHEME.padding_sm)
                                .width(Length::Fill),
                        )
                        .push(oxi_toggler::toggler(plugin.enabled).on_toggle_maybe(
                            (!busy).then_some(move |enabled| {
                                Message::Settings(Action::EnablePlugin(
                                    filename.to_owned(),
                                    enabled,
                                ))
                            }),
                        ))
                        .spacing(OXITHEME.padding_lg)
                        .align_y(Alignment::Center),
                ));
            }
        }
        body.into()
    }

    fn widget_layout_view(&self, busy: bool) -> Element<'_, Message> {
        let available: Vec<String> = std::iter::once("settings".to_owned())
            .chain(
                self.config
                    .snapshot
                    .host
                    .menus
                    .session_enabled
                    .then_some("session".into()),
            )
            .chain(self.launcher.enabled.then_some("launcher".to_owned()))
            .chain(
                self.config
                    .snapshot
                    .host
                    .overview
                    .enabled
                    .then_some("overview".to_owned()),
            )
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
        let dragging = self.settings.widget_drag.is_some() && !busy;
        let mut sections = Row::new().spacing(OXITHEME.padding_lg).width(Length::Fill);
        for (section, label, entries) in [
            ("start", "Left", &self.start_widgets),
            ("center", "Center", &self.center_widgets),
            ("end", "Right", &self.end_widgets),
        ] {
            let mut column = Column::new()
                .push(
                    text(label)
                        .size(OXITHEME.font_lg)
                        .style(oxi_plugin::text_primary),
                )
                .spacing(OXITHEME.padding_md);
            for (index, name) in entries.iter().enumerate() {
                let label: Element<'_, Message> = {
                    let section = section.to_owned();
                    let name = name.clone();
                    let revision = self.config.snapshot.revision;
                    oxi_drag_handle::drag_handle(
                        container(
                            text(format!("⠿  {name}"))
                                .size(OXITHEME.font_md)
                                .style(oxi_plugin::text_primary),
                        )
                        .height(OXITHEME.font_md * 1.5 + OXITHEME.padding_md * 2.0)
                        .align_y(Alignment::Center)
                        .width(Length::Fill),
                        move |_| {
                            if busy {
                                Message::Ignore
                            } else {
                                Message::Settings(Action::BeginWidgetDrag {
                                    section: section.clone(),
                                    index,
                                    name: name.clone(),
                                    revision,
                                })
                            }
                        },
                    )
                    .into()
                };
                let selected = self
                    .settings
                    .widget_drag
                    .as_ref()
                    .is_some_and(|drag| drag.section == section && drag.index == index);
                let card = container(
                    Column::new()
                        .push(inspected(format!("@bar/{section}/{index}"), label))
                        .push(
                            Row::new()
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
                                .push(Space::new().width(Length::Fill))
                                .push(action_button(
                                    "Remove",
                                    Action::RemoveWidget(section.to_owned(), index),
                                    busy,
                                ))
                                .spacing(OXITHEME.padding_xs)
                                .align_y(Alignment::Center),
                        )
                        .spacing(OXITHEME.padding_sm),
                )
                .padding(OXITHEME.padding_md)
                .width(Length::Fill)
                .style(oxi_plugin::compact_card_style(if selected {
                    OXITHEME.primary_bg_active
                } else {
                    OXITHEME.secondary_bg
                }));
                column = column.push(oxi_drag_handle::drop_zone(
                    card,
                    dragging,
                    Message::Settings(Action::DropWidget(section.to_owned(), index)),
                ));
            }
            column = column.push(oxi_drag_handle::drop_zone(
                container(
                    text(if dragging { "Drop here to append" } else { "" })
                        .size(OXITHEME.font_md)
                        .style(oxi_plugin::text_accent),
                )
                .padding(OXITHEME.padding_md)
                .width(Length::Fill),
                dragging,
                Message::Settings(Action::DropWidget(section.to_owned(), entries.len())),
            ));
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
            sections = sections.push(
                oxi_plugin::settings_card(column)
                    .padding(OXITHEME.padding_md)
                    .width(Length::FillPortion(1)),
            );
        }
        Column::new().push(text(if dragging {
            "Drop onto a card or the end of a column to move it."
        } else {
            "Drag widget cards between Left, Center and Right. Arrow buttons reorder within a column."
        }).size(OXITHEME.font_md).style(oxi_plugin::text_muted))
            .push(sections).spacing(OXITHEME.padding_lg).into()
    }
}

fn custom_field_page(path: &str) -> Option<&'static str> {
    match path {
        "bar.start" | "bar.center" | "bar.end" => Some("bar/layout"),
        "desktop.widgets" => Some("desktop/widgets"),
        "launcher.default_plugins" => Some("launcher/providers"),
        "plugins" => Some("plugins/installed"),
        _ => None,
    }
}

pub(crate) fn inspected<'a>(
    key: impl Into<String>,
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    let key = key.into();
    let id = key.clone();
    oxi_drag_handle::capture_visible_bounds(content, move |bounds, _| {
        Message::Settings(Action::NavigationBounds(id.clone(), bounds))
    })
    .with_identity(key)
    .into()
}

fn navigation_row_height() -> f32 {
    OXITHEME.font_md * 1.5 + OXITHEME.padding_lg
}

fn navigation_height(navigation: &Navigation, page: &Page, full: bool) -> f32 {
    navigation_row_height()
        + if page.children.is_empty() {
            0.0
        } else {
            OXITHEME.padding_xs
                + if full || navigation.expanded.contains(&page.id) {
                    navigation_children_height(navigation, page, full)
                } else {
                    1.0
                }
        }
}

fn navigation_children_height(navigation: &Navigation, page: &Page, full: bool) -> f32 {
    page.children
        .iter()
        .map(|child| navigation_height(navigation, child, full))
        .sum::<f32>()
        + page.children.len().saturating_sub(1) as f32 * OXITHEME.padding_xs
}

fn navigation_page<'a>(navigation: &'a Navigation, page: &'a Page) -> Element<'a, Message> {
    let open = navigation.expanded.contains(&page.id);
    let branch = !page.children.is_empty();
    let row = oxi_plugin::navigation_row(
        Row::new()
            .push(
                text(if branch {
                    if open { "▾" } else { "▸" }
                } else {
                    "·"
                })
                .size(OXITHEME.font_md)
                .width(OXITHEME.font_md),
            )
            .push(
                text(page.label.clone())
                    .size(OXITHEME.font_md)
                    .width(Length::Fill),
            )
            .spacing(OXITHEME.padding_sm)
            .align_y(Alignment::Center),
        navigation.selected == page.id,
    )
    .padding([OXITHEME.padding_sm, OXITHEME.padding_md])
    .height(navigation_row_height())
    .on_press(Message::Settings(Action::TogglePage(page.id.clone())));
    let id = page.id.clone();
    let row = oxi_drag_handle::capture_visible_bounds(row, move |bounds, _| {
        Message::Settings(Action::NavigationBounds(id.clone(), bounds))
    })
    .with_identity(page.id.clone());
    if !branch {
        return row.into();
    }
    let full = navigation_children_height(navigation, page, true);
    let target = if open {
        navigation_children_height(navigation, page, false)
    } else {
        0.0
    };
    let children = move || -> Element<'a, Message> {
        let mut column = Column::new()
            .spacing(OXITHEME.padding_xs)
            .width(Length::Fill);
        for child in &page.children {
            column = column.push(navigation_page(navigation, child));
        }
        container(column)
            .padding(iced::Padding {
                left: OXITHEME.padding_md,
                ..Default::default()
            })
            .width(Length::Fill)
            .into()
    };
    let motion = oxiced::theme::snapshot::current_snapshot();
    let reveal: Element<'a, Message> = if motion.reduced_motion || motion.animation_ms == 0 {
        oxiced::widgets::oxi_reveal::reveal(children(), full, target).into()
    } else {
        AnimationBuilder::new(target, move |height| {
            oxiced::widgets::oxi_reveal::reveal(children(), full, height).into()
        })
        .animation(
            Easing::EASE_OUT
                .with_duration(motion.duration())
                .reversible(true),
        )
        .animates_layout(true)
        .into()
    };
    Column::new()
        .push(row)
        .push(reveal)
        .spacing(OXITHEME.padding_xs)
        .width(Length::Fill)
        .into()
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
