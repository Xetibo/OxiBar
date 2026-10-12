use crate::{app::OxiBar, messages::Message};
use iced::{
    Alignment, Element, Length, Task,
    widget::{Column, Row, Space, button, container, tooltip},
};
use iced_layershell::reexport::IcedId;
use oxiced::{
    theme::theme_impl::OXITHEME,
    widgets::oxi_plugin::{self, text},
};

pub const SIZE: (u32, u32) = (800, 480);

#[derive(Debug, Clone)]
pub enum Action {
    Open,
    Toggle,
    Close,
    Move(i32),
    Choose(String),
    LaunchFocused,
    Configure,
}

impl OxiBar {
    pub(crate) fn close_session(&mut self) -> Task<Message> {
        self.shell
            .session_window
            .take()
            .map(|id| {
                self.shell.registry.remove(id);
                Task::done(Message::RemoveSurface(id))
            })
            .unwrap_or_else(Task::none)
    }

    pub(crate) fn session_entries(&self) -> Vec<(String, oxibar_plugin_api::SessionMenuEntry)> {
        let Some((name, (model, funcs))) = self
            .plugins
            .iter()
            .find(|(_, (_, funcs))| funcs.session_menu.is_some() && funcs.command.is_some())
        else {
            return Vec::new();
        };
        let available = unsafe { funcs.session_menu.expect("checked capability")(model.clone()) };
        self.config
            .snapshot
            .host
            .menus
            .session_actions
            .iter()
            .filter_map(|action| {
                available
                    .iter()
                    .find(|entry| entry.action == *action)
                    .cloned()
                    .map(|entry| (name.clone(), entry))
            })
            .collect()
    }

    pub(crate) fn update_session_menu(&mut self, action: Action) -> Task<Message> {
        match action {
            Action::Configure => {
                let select = self.update_settings(crate::settings::Action::Category(
                    crate::settings::Category::Plugins,
                ));
                select.chain(self.update_settings(crate::settings::Action::Open))
            }
            Action::Move(_) | Action::Choose(_) | Action::LaunchFocused
                if self.shell.session_window.is_none() =>
            {
                Task::none()
            }
            Action::Close => self.close_session(),
            Action::Open if self.shell.session_window.is_some() => Task::none(),
            Action::Toggle if self.shell.session_window.is_some() => self.close_session(),
            Action::Open | Action::Toggle => {
                if !self.config.snapshot.host.menus.session_enabled {
                    return Task::none();
                }
                let dismiss = Task::batch([
                    self.close_overview(),
                    self.close_oxipaste(),
                    self.close_control_center(),
                    self.close_bar_dropdown(),
                    self.close_modal(),
                    self.close_panel(),
                    self.close_launcher(),
                    self.close_settings(),
                ]);
                self.shell.session_focused = self
                    .session_entries()
                    .iter()
                    .position(|(_, entry)| entry.enabled)
                    .unwrap_or(0);
                self.shell.session_error = None;
                let id = IcedId::unique();
                let output = self.overlay_output();
                let size = self
                    .shell
                    .inventory
                    .as_ref()
                    .and_then(|inventory| {
                        inventory
                            .outputs
                            .iter()
                            .find(|item| Some(&item.name) == output.as_ref())
                    })
                    .map(|output| {
                        (
                            (SIZE.0).min(output.logical_size.0 * 9 / 10).max(1),
                            (SIZE.1).min(output.logical_size.1 * 9 / 10).max(1),
                        )
                    })
                    .unwrap_or(SIZE);
                self.register_overlay(
                    id,
                    crate::shell::SurfaceKind::Session,
                    self.shell.active_bar,
                    output.clone(),
                );
                self.shell
                    .registry
                    .get_mut(id)
                    .expect("registered session")
                    .size = Some(size);
                self.shell.session_window = Some(id);
                dismiss.chain(Task::done(Message::OnOutput(
                    output,
                    Box::new(Message::OpenSessionLayer(id, size)),
                )))
            }
            Action::Move(delta) => {
                let entries = self.session_entries();
                let count = entries.len();
                if count > 0 {
                    let start = self.shell.session_focused.min(count - 1);
                    for step in 1..=count {
                        let index = (start as i64 + i64::from(delta) * step as i64)
                            .rem_euclid(count as i64) as usize;
                        if entries[index].1.enabled {
                            self.shell.session_focused = index;
                            break;
                        }
                    }
                }
                Task::none()
            }
            Action::LaunchFocused => self
                .session_entries()
                .get(self.shell.session_focused)
                .map(|(_, entry)| entry.action.clone())
                .map(|action| self.update_session_menu(Action::Choose(action)))
                .unwrap_or_else(Task::none),
            Action::Choose(action) => {
                let Some((provider, entry)) = self
                    .session_entries()
                    .into_iter()
                    .find(|(_, entry)| entry.action == action && entry.enabled)
                else {
                    return Task::none();
                };
                let (model, funcs) = &self.plugins[&provider];
                let record = self
                    .shell
                    .session_window
                    .and_then(|id| self.shell.registry.get(id));
                let source = record.and_then(|record| record.owner_bar);
                let output = record.and_then(|record| record.output.clone());
                match unsafe {
                    funcs.command.expect("menu command capability")(
                        model.clone(),
                        entry.action,
                        Vec::new(),
                    )
                } {
                    Ok(task) => self.close_session().chain(task.map(move |outcome| {
                        Message::OnCommandOutput(
                            output.clone(),
                            Box::new(Message::LocalCommandFinished {
                                provider: provider.clone(),
                                source,
                                outcome,
                            }),
                        )
                    })),
                    Err(error) => {
                        self.shell.session_error = Some(error);
                        Task::none()
                    }
                }
            }
        }
    }

    pub(crate) fn session_menu_view(&self) -> Element<'_, Message> {
        let entries = self.session_entries();
        let mut body = Column::new()
            .push(
                Row::new()
                    .push(text("Session").size(OXITHEME.font_xl))
                    .push(Space::new().width(Length::Fill))
                    .push(
                        oxi_plugin::surface_button(text("Close"), false)
                            .on_press(Message::Session(Action::Close)),
                    )
                    .spacing(OXITHEME.padding_md)
                    .align_y(Alignment::Center),
            )
            .spacing(OXITHEME.padding_lg)
            .padding(OXITHEME.padding_xl)
            .width(Length::Fill)
            .height(Length::Fill);
        if let Some(error) = &self.shell.session_error {
            body = body.push(
                text(error)
                    .size(OXITHEME.font_md)
                    .style(oxi_plugin::text_muted),
            );
        }
        if entries.is_empty() {
            let has_provider = self
                .plugins
                .values()
                .any(|(_, funcs)| funcs.session_menu.is_some() && funcs.command.is_some());
            body = body.push(
                text(if has_provider {
                    "No session actions configured. Choose actions in Settings."
                } else {
                    "Session actions come from the System provider. Enable it in Settings → Plugins."
                })
                    .size(OXITHEME.font_md)
                    .style(oxi_plugin::text_muted),
            ).push(oxi_plugin::surface_button(text("Plugin settings"), false)
                .on_press(Message::Session(Action::Configure)));
        }
        let mut tiles = Column::new()
            .spacing(OXITHEME.padding_xl)
            .height(Length::Fill);
        for (row_index, chunk) in entries.chunks(3).enumerate() {
            let mut row = Row::new()
                .spacing(OXITHEME.padding_xl)
                .height(Length::FillPortion(1));
            for (column_index, (_, entry)) in chunk.iter().enumerate() {
                let selected = row_index * 3 + column_index == self.shell.session_focused;
                let enabled = entry.enabled;
                let icon = oxi_plugin::icon(session_icon(&entry.action))
                    .width(80)
                    .height(80)
                    .style(move |_, _| iced::widget::svg::Style {
                        color: Some(if enabled {
                            OXITHEME.primary
                        } else {
                            OXITHEME.text_muted
                        }),
                    });
                let tile = button(container(icon).center(Length::Fill))
                    .style(move |theme, status| {
                        let mut style = oxi_plugin::flat_primary_bg_button_style(theme, status);
                        if selected && enabled && status == button::Status::Active {
                            style.background = Some(OXITHEME.primary_bg_hover.into());
                            style.border.color = OXITHEME.border_color_strong;
                            style.border.width = 1.0;
                        }
                        style
                    })
                    .width(Length::FillPortion(1))
                    .height(Length::Fill)
                    .on_press_maybe(
                        enabled.then_some(Message::Session(Action::Choose(entry.action.clone()))),
                    );
                row = row.push(
                    tooltip(
                        tile,
                        text(format!("{} · {}", entry.label, entry.description))
                            .size(OXITHEME.font_md),
                        tooltip::Position::FollowCursor,
                    )
                    .gap(OXITHEME.padding_sm),
                );
            }
            for _ in chunk.len()..3 {
                row = row.push(Space::new().width(Length::FillPortion(1)));
            }
            tiles = tiles.push(row);
        }
        if !entries.is_empty() {
            body = body.push(tiles).push(
                text(
                    entries
                        .get(self.shell.session_focused)
                        .map(|(_, entry)| entry.label.clone())
                        .unwrap_or_default(),
                )
                .size(OXITHEME.font_md)
                .style(oxi_plugin::text_accent),
            );
        } else {
            body = body.push(Space::new().height(Length::Fill));
        }
        let body = body.push(
            text(format!(
                "{}: choose · {}: open · {}: close",
                self.shortcut_label("session.next"),
                self.shortcut_label("session.activate"),
                self.shortcut_label("close")
            ))
            .size(OXITHEME.font_sm)
            .style(oxi_plugin::text_muted),
        );
        container(body)
            .style(oxi_plugin::compact_card_style(OXITHEME.mantle))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

fn session_icon(action: &str) -> oxi_plugin::ShellIcon {
    use oxi_plugin::ShellIcon::*;
    match action {
        "lock" => Lock,
        "logout" => Logout,
        "suspend" => Sleep,
        "hibernate" => Hibernate,
        "reboot" => Reboot,
        _ => Power,
    }
}
