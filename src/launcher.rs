use std::time::Instant;

use iced::{
    Alignment, Element, Length, event,
    keyboard::{self, Key, key::Named},
    widget::{Column, Row, Space, container, image, scrollable, svg},
};
use iced_layershell::reexport::IcedId;
use oxibar_plugin_api::RunnerEntry;
use oxiced::widgets::oxi_plugin::text;
use oxiced::{
    theme::theme_impl::OXITHEME,
    widgets::{oxi_plugin, oxi_text_input},
};
use toml::Table;

use crate::{layout, messages::Message, plugins::PluginMap};

pub(crate) const SEARCH_ID: &str = "oxibar-launcher-search";
const DEFAULT_MAX_ENTRIES: usize = 7;
const MAX_VISIBLE_ENTRIES: usize = 50;

#[derive(Debug, Clone)]
pub enum Action {
    Toggle,
    Open,
    Close,
    Query(String),
    MoveFocus(bool),
    Launch(usize),
    LaunchFocused,
    Activate(String, usize),
    Favorite(String, usize),
}

#[derive(Debug, Clone)]
pub(crate) struct Entry {
    pub(crate) plugin_id: String,
    pub(crate) result: RunnerEntry,
    pub(crate) favorite: Option<bool>,
}

pub(crate) struct Launcher {
    pub(crate) enabled: bool,
    pub(crate) window_id: Option<IcedId>,
    pub(crate) query: String,
    pub(crate) entries: Vec<Entry>,
    pub(crate) focused: usize,
    pub(crate) size: (u32, u32),
    pub(crate) open_started_at: Option<Instant>,
    pub(crate) query_revision: u64,
    pub(crate) query_started_at: Option<Instant>,
    pub(crate) query_elapsed_us: Option<u64>,
    max_entries: usize,
    statuses: Vec<(String, oxibar_plugin_api::RunnerStatus)>,
    prefixes: Vec<String>,
}

impl Launcher {
    pub(crate) fn configure(&mut self, config: &crate::config::schema::HostConfig) {
        self.enabled = config.launcher_enabled;
        self.size = config.launcher_size;
        self.max_entries = config.launcher_limit;
        self.focused = self.focused.min(self.max_entries.saturating_sub(1));
    }
    pub(crate) fn new(config: &Table) -> Self {
        let table = config.get("launcher").and_then(|value| value.as_table());
        Self {
            enabled: table
                .and_then(|table| table.get("enabled"))
                .and_then(|value| value.as_bool())
                .unwrap_or(true),
            window_id: None,
            open_started_at: None,
            query_revision: 0,
            query_started_at: None,
            query_elapsed_us: None,
            query: String::new(),
            entries: Vec::new(),
            focused: 0,
            statuses: Vec::new(),
            prefixes: Vec::new(),
            size: layout::launcher_size_from_config(config),
            max_entries: table
                .and_then(|table| table.get("max_entries"))
                .and_then(|value| value.as_integer())
                .and_then(|value| usize::try_from(value).ok())
                .filter(|value| *value > 0)
                .unwrap_or(DEFAULT_MAX_ENTRIES)
                .min(MAX_VISIBLE_ENTRIES),
        }
    }

    pub(crate) fn accepts(
        &self,
        plugins: &PluginMap,
        funcs: &crate::plugins::PluginFuncs,
        model: &oxibar_plugin_api::PluginModel,
    ) -> bool {
        let selected = plugins
            .values()
            .flat_map(|(model, funcs)| {
                funcs
                    .runner_prefixes
                    .map(|prefixes| unsafe { prefixes(model.clone()) })
                    .unwrap_or_default()
            })
            .filter(|prefix| !prefix.is_empty() && self.query.starts_with(prefix))
            .max_by_key(String::len);
        selected.is_none_or(|selected| {
            funcs
                .runner_prefixes
                .is_some_and(|prefixes| unsafe { prefixes(model.clone()) }.contains(&selected))
        })
    }

    pub(crate) fn refresh(&mut self, plugins: &PluginMap) {
        self.prefixes = plugins
            .iter()
            .filter_map(|(name, (model, funcs))| {
                funcs.runner_prefixes.map(|prefixes| {
                    format!(
                        "{name}: {}",
                        unsafe { prefixes(model.clone()) }
                            .into_iter()
                            .take(16)
                            .collect::<Vec<_>>()
                            .join(" · ")
                    )
                })
            })
            .collect();
        let selected = self
            .entries
            .get(self.focused)
            .map(|entry| (entry.plugin_id.clone(), entry.result.index));
        self.statuses = plugins
            .iter()
            .filter(|(_, (model, funcs))| {
                funcs.runner_query.is_some() && self.accepts(plugins, funcs, model)
            })
            .map(|(name, (model, funcs))| {
                (
                    name.clone(),
                    funcs
                        .runner_status
                        .map(|status| unsafe { status(model.clone()) })
                        .unwrap_or(oxibar_plugin_api::RunnerStatus {
                            available: true,
                            indexing: false,
                            message: None,
                        }),
                )
            })
            .collect();
        self.entries = plugins
            .iter()
            .filter(|(_, (model, funcs))| {
                self.accepts(plugins, funcs, model)
                    && funcs
                        .runner_status
                        .is_none_or(|status| unsafe { status(model.clone()) }.available)
            })
            .flat_map(|(plugin_id, (model, funcs))| {
                // A provider needs both hooks so every displayed result can be queried.
                match (funcs.runner_query, funcs.runner_entries) {
                    (Some(_), Some(entries)) => unsafe { entries(model.clone()) }
                        .into_iter()
                        .map(|result| Entry {
                            plugin_id: plugin_id.clone(),
                            favorite: funcs
                                .runner_favorite
                                .and_then(|favorite| unsafe {
                                    favorite(model.clone(), result.index)
                                })
                                .map(|favorite| favorite.active),
                            result,
                        })
                        .collect(),
                    _ => Vec::new(),
                }
            })
            .collect();
        rank_entries(&mut self.entries, self.max_entries);
        self.focused = selected
            .and_then(|(plugin_id, index)| {
                self.entries
                    .iter()
                    .position(|entry| entry.plugin_id == plugin_id && entry.result.index == index)
            })
            .unwrap_or_else(|| self.focused.min(self.entries.len().saturating_sub(1)));
        if self.query_elapsed_us.is_none() && !self.entries.is_empty() {
            self.query_elapsed_us = self
                .query_started_at
                .map(|started| started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64);
        }
    }

    pub(crate) fn move_focus(&mut self, forward: bool) {
        let count = self.entries.len();
        self.focused = match (count, forward) {
            (0, _) => 0,
            (_, true) => (self.focused + 1) % count,
            (_, false) => (self.focused + count - 1) % count,
        };
    }

    pub(crate) fn view(&self) -> Element<'_, Message> {
        let search = oxi_text_input::text_input(
            "Find apps, commands, files or actions",
            &self.query,
            |query| Message::Launcher(Action::Query(query)),
        )
        .id(SEARCH_ID)
        .size(OXITHEME.font_md)
        .padding(OXITHEME.padding_md)
        .on_submit(
            self.entries
                .get(self.focused)
                .map(|entry| {
                    Message::Launcher(Action::Activate(
                        entry.plugin_id.clone(),
                        entry.result.index,
                    ))
                })
                .unwrap_or(Message::Ignore),
        );
        let mut results = Column::new()
            .spacing(OXITHEME.padding_xs)
            .width(Length::Fill);
        for (index, entry) in self.entries.iter().enumerate() {
            let icon_size = OXITHEME.font_lg * 2.0;
            let icon: Element<'_, Message> = match entry.result.icon.as_ref() {
                Some(path) if path.extension().is_some_and(|extension| extension == "svg") => {
                    svg(svg::Handle::from_path(path))
                        .width(icon_size)
                        .height(icon_size)
                        .into()
                }
                Some(path) => image(image::Handle::from_path(path))
                    .width(icon_size)
                    .height(icon_size)
                    .into(),
                None => container(
                    text("▸")
                        .size(OXITHEME.font_lg)
                        .style(oxi_plugin::text_accent),
                )
                .center_x(icon_size)
                .center_y(icon_size)
                .into(),
            };
            let labels = Column::new()
                .push(
                    text(&entry.result.title)
                        .size(OXITHEME.font_md)
                        .style(oxi_plugin::text_primary),
                )
                .push(
                    text(&entry.result.description)
                        .size(OXITHEME.font_sm)
                        .style(oxi_plugin::text_muted),
                );
            let content = Row::new()
                .push(icon)
                .push(labels)
                .spacing(OXITHEME.padding_md)
                .align_y(Alignment::Center);
            let selected = self.focused == index;
            let mut row = Row::new()
                .spacing(OXITHEME.padding_sm)
                .align_y(Alignment::Center)
                .push(
                    oxi_plugin::selectable_row(content, selected)
                        .on_press(Message::Launcher(Action::Activate(
                            entry.plugin_id.clone(),
                            entry.result.index,
                        )))
                        .width(Length::Fill)
                        .padding(OXITHEME.padding_sm),
                );
            if let Some(active) = entry.favorite {
                row = row.push(
                    oxi_plugin::bar_button(
                        text(if active { "★" } else { "☆" }).size(OXITHEME.font_lg),
                    )
                    .on_press(Message::Launcher(Action::Favorite(
                        entry.plugin_id.clone(),
                        entry.result.index,
                    ))),
                );
            }
            results = results.push(row);
        }
        if self.entries.is_empty() {
            results = results.push(
                text(if self.statuses.iter().any(|(_, status)| status.indexing) {
                    "Indexing…"
                } else {
                    "No matching results"
                })
                .size(OXITHEME.font_md)
                .style(oxi_plugin::text_muted),
            );
        }
        for (provider, status) in &self.statuses {
            if let Some(message) = &status.message {
                results = results.push(
                    text(format!("{provider}: {message}"))
                        .size(OXITHEME.font_sm)
                        .style(oxi_plugin::text_muted),
                );
            }
        }
        let footer = Row::new()
            .push(
                text("↑/↓ or Tab: select · Enter: launch · Esc: close")
                    .size(OXITHEME.font_sm)
                    .style(oxi_plugin::text_muted),
            )
            .push(Space::new().width(Length::Fill))
            .push(
                oxi_plugin::bar_button(text("Settings").size(OXITHEME.font_md))
                    .on_press(Message::Settings(crate::settings::Action::Open)),
            )
            .push(
                oxi_plugin::bar_button(text("Close").size(OXITHEME.font_md))
                    .on_press(Message::Launcher(Action::Close)),
            )
            .align_y(Alignment::Center);
        container(
            Column::new()
                .push(search)
                .push(scrollable(results).height(Length::Fill))
                .push(
                    text(self.prefixes.join("   "))
                        .size(OXITHEME.font_sm)
                        .style(oxi_plugin::text_muted),
                )
                .push(footer)
                .spacing(OXITHEME.padding_md)
                .padding(OXITHEME.padding_lg)
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .style(oxi_plugin::compact_card_style(OXITHEME.mantle))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}

fn rank_entries(entries: &mut Vec<Entry>, max_entries: usize) {
    entries.sort_by(|first, second| {
        second
            .result
            .score
            .cmp(&first.result.score)
            .then_with(|| first.result.title.cmp(&second.result.title))
            .then_with(|| first.plugin_id.cmp(&second.plugin_id))
            .then_with(|| first.result.index.cmp(&second.result.index))
    });
    entries.truncate(max_entries);
}

pub(crate) fn key_event(event: iced::Event, status: event::Status, id: IcedId) -> Option<Message> {
    let iced::Event::Keyboard(keyboard::Event::KeyPressed {
        key: Key::Named(key),
        modifiers,
        ..
    }) = event
    else {
        return None;
    };
    let action = match key {
        Named::Escape => Action::Close,
        Named::ArrowUp => Action::MoveFocus(false),
        Named::ArrowDown => Action::MoveFocus(true),
        Named::Tab => Action::MoveFocus(!modifiers.shift()),
        Named::Enter if status == event::Status::Ignored => Action::LaunchFocused,
        _ => return None,
    };
    Some(Message::LauncherKey(id, action))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(plugin_id: &str, index: usize, score: i64) -> Entry {
        Entry {
            plugin_id: plugin_id.to_owned(),
            result: RunnerEntry {
                index,
                score,
                title: format!("{plugin_id}-{index}"),
                description: String::new(),
                icon: None,
            },
            favorite: None,
        }
    }

    #[test]
    fn global_ranking_keeps_provider_local_launch_indexes() {
        let mut entries = vec![
            entry("First", 4, 20),
            entry("Second", 8, 100),
            entry("First", 2, 50),
        ];
        rank_entries(&mut entries, 2);
        assert_eq!(entries[0].plugin_id, "Second");
        assert_eq!(entries[0].result.index, 8);
        assert_eq!(entries[1].plugin_id, "First");
        assert_eq!(entries[1].result.index, 2);
    }

    #[test]
    fn navigation_wraps_and_handles_empty_results() {
        let mut launcher = Launcher::new(&Table::new());
        launcher.move_focus(false);
        assert_eq!(launcher.focused, 0);
        launcher.entries = vec![entry("First", 0, 0), entry("First", 1, 0)];
        launcher.move_focus(false);
        assert_eq!(launcher.focused, 1);
        launcher.move_focus(true);
        assert_eq!(launcher.focused, 0);
    }
}
