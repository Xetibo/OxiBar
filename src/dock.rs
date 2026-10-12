use crate::{
    Message, app::OxiBar, config::desktop::DockConfig, messages::map_plugin_message,
    plugins::dispatch_launch, shell::SurfaceKind,
};
use iced::{
    Element, Length, Task,
    widget::{Row, container, tooltip},
};
use iced_layershell::reexport::IcedId;
use oxiced::widgets::oxi_plugin::text;
use oxiced::{theme::theme_impl::OXITHEME, widgets::oxi_plugin};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

#[derive(Debug, Clone)]
pub enum Action {
    Launch {
        plugin: String,
        index: usize,
        desktop_id: String,
    },
    Focus {
        window: String,
        pid: u32,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct Item {
    pub title: String,
    pub icon: Option<PathBuf>,
    pub action: Option<Action>,
    pub focused: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct Instance {
    pub id: IcedId,
    pub output: Option<String>,
    pub size: (u32, u32),
    pub config: DockConfig,
    pub margin: u32,
    pub items: Vec<Item>,
}

#[derive(Debug, Default)]
pub(crate) struct Docks {
    pub instances: BTreeMap<Option<String>, Instance>,
}

fn normalized(value: &str) -> String {
    value.trim_end_matches(".desktop").to_lowercase()
}

fn matches(entry: &oxibar_plugin_api::DockEntry, class: &str) -> bool {
    match_score(entry, class) > 0
}
fn match_score(entry: &oxibar_plugin_api::DockEntry, class: &str) -> u8 {
    let class = normalized(class);
    if class.is_empty() {
        return 0;
    }
    let compact = |value: &str| {
        value
            .chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect::<String>()
    };
    let score = |value: &str, exact: u8, suffix: u8| {
        let value = normalized(value);
        if value.is_empty() {
            0
        } else if value == class {
            exact
        } else if value.rsplit('.').next() == class.rsplit('.').next()
            || compact(&value) == compact(&class)
        {
            suffix
        } else {
            0
        }
    };
    [
        score(&entry.window_class, 100, 85),
        score(&entry.desktop_id, 90, 75),
        score(&entry.title, 70, 65),
        if !entry
            .window_class
            .to_ascii_lowercase()
            .starts_with("steam_app_")
        {
            score(
                std::path::Path::new(&entry.executable)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&entry.executable),
                30,
                20,
            )
        } else {
            0
        },
    ]
    .into_iter()
    .max()
    .unwrap_or(0)
}

impl OxiBar {
    pub(crate) fn selected_outputs(
        &self,
        selectors: &[String],
        fallback: bool,
    ) -> Vec<(Option<String>, u32, u32)> {
        let Some(inventory) = &self.shell.inventory else {
            return if fallback {
                vec![(None, 1920, 1080)]
            } else {
                Vec::new()
            };
        };
        let enabled: Vec<_> = inventory
            .outputs
            .iter()
            .filter(|output| output.enabled)
            .collect();
        let preferred = self
            .shell
            .selected_output
            .as_deref()
            .and_then(|name| enabled.iter().copied().find(|output| output.name == name))
            .or_else(|| inventory.focused())
            .or_else(|| enabled.first().copied());
        let mut selected: Vec<_> = if selectors.is_empty() {
            preferred.into_iter().collect()
        } else {
            enabled
                .into_iter()
                .filter(|output| {
                    selectors
                        .iter()
                        .any(|name| name == "*" || name == &output.name)
                })
                .collect()
        };
        if selected.is_empty()
            && fallback
            && let Some(output) = preferred
        {
            selected.push(output);
        }
        selected
            .into_iter()
            .map(|output| {
                (
                    Some(output.name.clone()),
                    output.logical_size.0,
                    output.logical_size.1,
                )
            })
            .collect()
    }

    pub(crate) fn reconcile_docks(&mut self) -> Task<Message> {
        let config = self.config.snapshot.host.dock.clone();
        let mut tasks = Vec::new();
        let catalog: Vec<_> = self
            .plugins
            .iter()
            .filter_map(|(name, (model, funcs))| {
                funcs
                    .dock_entries
                    .map(|entries| (name.clone(), unsafe { entries(model.clone()) }))
            })
            .collect();
        let targets = if config.enabled {
            self.selected_outputs(&config.outputs, config.fallback)
        } else {
            Vec::new()
        };
        let mut keep = BTreeSet::new();
        for (output, width, _) in targets {
            let windows: Vec<_> = self
                .compositor
                .as_ref()
                .filter(|_| self.compositor_error.is_none())
                .into_iter()
                .flat_map(|snapshot| &snapshot.windows)
                .filter(|window| output.is_none() || window.output == output)
                .collect();
            let mut items = Vec::new();
            let mut consumed = BTreeSet::new();
            for desktop_id in &config.pinned {
                let found = catalog.iter().find_map(|(plugin, entries)| {
                    entries
                        .iter()
                        .find(|entry| normalized(&entry.desktop_id) == normalized(desktop_id))
                        .map(|entry| (plugin, entry))
                });
                if let Some((plugin, entry)) = found {
                    let matching: Vec<_> = windows
                        .iter()
                        .copied()
                        .filter(|window| matches(entry, &window.class))
                        .collect();
                    consumed.extend(matching.iter().map(|window| window.id.clone()));
                    items.push(item_for(
                        entry.title.clone(),
                        entry.icon.clone(),
                        Some(Action::Launch {
                            plugin: plugin.clone(),
                            index: entry.index,
                            desktop_id: entry.desktop_id.clone(),
                        }),
                        &matching,
                    ));
                } else {
                    items.push(Item {
                        title: format!("{desktop_id} — unavailable"),
                        icon: None,
                        action: None,
                        focused: false,
                    });
                }
            }
            if config.running {
                let mut groups =
                    BTreeMap::<String, Vec<&oxibar_plugin_api::CompositorWindow>>::new();
                for window in windows {
                    if !consumed.contains(&window.id) {
                        groups
                            .entry(if window.class.is_empty() {
                                window.id.clone()
                            } else {
                                normalized(&window.class)
                            })
                            .or_default()
                            .push(window);
                    }
                }
                for group in groups.values() {
                    let window = group[0];
                    let entry = catalog
                        .iter()
                        .flat_map(|(_, entries)| entries.iter())
                        .filter_map(|entry| {
                            let score = match_score(entry, &window.class);
                            (score > 0).then_some((score, entry))
                        })
                        .max_by_key(|(score, entry)| (*score, entry.icon.is_some()))
                        .map(|(_, entry)| entry);
                    items.push(item_for(
                        entry.map_or_else(
                            || {
                                if window.class.is_empty() {
                                    window.title.clone()
                                } else {
                                    window.class.clone()
                                }
                            },
                            |entry| entry.title.clone(),
                        ),
                        entry.and_then(|entry| entry.icon.clone()),
                        None,
                        group,
                    ));
                }
            }
            let item_width = config.icon_size + 2 * OXITHEME.padding_sm as u32;
            let gap = OXITHEME.padding_sm as u32;
            items.truncate(
                ((width.saturating_sub(2 * config.margin + 2 * OXITHEME.padding_md as u32) + gap)
                    / (item_width + gap).max(1))
                .min(64) as usize,
            );
            if items.is_empty() {
                continue;
            }
            let size = (
                (items.len() as u32 * item_width
                    + (items.len() as u32 - 1) * gap
                    + 2 * OXITHEME.padding_md as u32)
                    .min(width.max(1)),
                config.icon_size + 2 * OXITHEME.padding_sm as u32 + 2 * OXITHEME.padding_md as u32,
            );
            let bar_offset = self
                .shell
                .bars
                .values()
                .filter(|bar| {
                    bar.output == output
                        && (bar.config.position == crate::config::bars::Position::Bottom)
                            == config.bottom
                })
                .map(|bar| bar.config.height)
                .max()
                .unwrap_or(0);
            let margin = config.margin + bar_offset;
            keep.insert(output.clone());
            if let Some(instance) = self.docks.instances.get_mut(&output) {
                if instance.size != size || instance.config != config || instance.margin != margin {
                    tasks.push(Task::done(Message::ConfigureDockLayer(
                        instance.id,
                        size,
                        config.bottom,
                        margin,
                        config.reserve,
                    )));
                }
                instance.size = size;
                instance.config = config.clone();
                instance.margin = margin;
                instance.items = items;
            } else {
                let id = IcedId::unique();
                self.register_overlay(id, SurfaceKind::Dock, self.shell.active_bar, output.clone());
                self.docks.instances.insert(
                    output.clone(),
                    Instance {
                        id,
                        output: output.clone(),
                        size,
                        config: config.clone(),
                        margin,
                        items,
                    },
                );
                tasks.push(Task::done(Message::OnOutput(
                    output,
                    Box::new(Message::OpenDockLayer(
                        id,
                        size,
                        config.bottom,
                        margin,
                        config.reserve,
                    )),
                )));
            }
        }
        let stale: Vec<_> = self
            .docks
            .instances
            .keys()
            .filter(|output| !keep.contains(*output))
            .cloned()
            .collect();
        for output in stale {
            if let Some(instance) = self.docks.instances.remove(&output) {
                self.shell.registry.remove(instance.id);
                tasks.push(Task::done(Message::RemoveSurface(instance.id)));
            }
        }
        Task::batch(tasks)
    }

    pub(crate) fn dock_action(&mut self, action: Action) -> Task<Message> {
        let task = match action {
            Action::Focus { window, pid } => {
                if !self.compositor.as_ref().is_some_and(|snapshot| {
                    snapshot
                        .windows
                        .iter()
                        .any(|item| item.id == window && item.pid == pid)
                }) {
                    return Task::none();
                }
                self.compositor_action(Some(window), None)
            }
            Action::Launch {
                plugin,
                index,
                desktop_id,
            } => {
                let Some((model, funcs)) = self.plugins.get(&plugin) else {
                    return Task::none();
                };
                if !funcs.dock_entries.is_some_and(|entries| {
                    unsafe { entries(model.clone()) }
                        .iter()
                        .any(|entry| entry.index == index && entry.desktop_id == desktop_id)
                }) {
                    return Task::none();
                }
                let source = self.shell.request_source;
                dispatch_launch(funcs, index, model.clone())
                    .unwrap_or_else(Task::none)
                    .map(move |message| {
                        Message::PluginTask(
                            source,
                            Box::new(map_plugin_message(plugin.clone(), message)),
                        )
                    })
            }
        };
        let mut tasks = vec![
            self.close_overview(),
            task,
            self.close_settings(),
            self.close_launcher(),
            self.close_control_center(),
            self.close_modal(),
            self.close_panel(),
        ];
        if self.desktop.draft.is_some() {
            tasks.push(self.cancel_desktop_edit());
        }
        Task::batch(tasks)
    }

    pub(crate) fn dock_view(&self, id: IcedId) -> Element<'_, Message> {
        let Some(instance) = self
            .docks
            .instances
            .values()
            .find(|instance| instance.id == id)
        else {
            return iced::widget::Space::new().into();
        };
        let mut row = Row::new()
            .spacing(OXITHEME.padding_sm)
            .align_y(iced::Alignment::Center);
        for item in &instance.items {
            let icon =
                oxi_plugin::application_icon(item.icon.as_deref(), instance.config.icon_size);
            let body = container(icon)
                .width(instance.config.icon_size)
                .height(instance.config.icon_size)
                .center_x(Length::Fill)
                .center_y(Length::Fill);
            let button = iced::widget::button(body)
                .padding(OXITHEME.padding_sm)
                .style(oxi_plugin::accent_selection_style(item.focused));
            let button = if let Some(action) = &item.action {
                button.on_press(Message::Dock(action.clone()))
            } else {
                button
            };
            row = row.push(tooltip(
                button,
                text(&item.title).size(OXITHEME.font_md),
                if instance.config.bottom {
                    tooltip::Position::Top
                } else {
                    tooltip::Position::Bottom
                },
            ));
        }
        container(row)
            .padding(OXITHEME.padding_md)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(OXITHEME.mantle.into()),
                text_color: Some(OXITHEME.text),
                border: iced::Border {
                    radius: OXITHEME.border_radius.into(),
                    ..Default::default()
                },
                ..Default::default()
            })
            .into()
    }
}

fn item_for(
    title: String,
    icon: Option<PathBuf>,
    launch: Option<Action>,
    windows: &[&oxibar_plugin_api::CompositorWindow],
) -> Item {
    let selected = windows
        .iter()
        .position(|window| window.focused)
        .map_or(0, |index| (index + 1) % windows.len().max(1));
    Item {
        title,
        icon,
        action: windows
            .get(selected)
            .map(|window| Action::Focus {
                window: window.id.clone(),
                pid: window.pid,
            })
            .or(launch),
        focused: windows.iter().any(|window| window.focused),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn desktop_class_and_game_identity_rank_above_shared_executables() {
        let entry = oxibar_plugin_api::DockEntry {
            index: 0,
            desktop_id: "org.mozilla.Firefox.desktop".into(),
            title: "Firefox".into(),
            icon: None,
            window_class: "firefox".into(),
            executable: "/private/bin/firefox".into(),
        };
        assert!(matches(&entry, "org.mozilla.firefox"));
        assert!(!matches(&entry, "unrelated"));
        let mut alternate_class = entry.clone();
        alternate_class.window_class = "Firefox-nightly".into();
        alternate_class.desktop_id = "nightly.desktop".into();
        alternate_class.title = "Nightly".into();
        assert_eq!(match_score(&alternate_class, "firefox"), 30);
        assert!(match_score(&entry, "firefox") > match_score(&alternate_class, "firefox"));
        let mut game = entry.clone();
        game.desktop_id = "game.desktop".into();
        game.window_class = "steam_app_435150".into();
        game.executable = "steam".into();
        let mut client = entry;
        client.desktop_id = "steam.desktop".into();
        client.window_class = "steam".into();
        client.executable = "steam".into();
        assert!(!matches(&game, "Steam"));
        assert!(match_score(&game, "steam_app_435150") > match_score(&client, "steam_app_435150"));
        assert!(match_score(&client, "Steam") > match_score(&game, "Steam"));
    }
}
