pub mod capture;
#[cfg(test)]
mod capture_tests;
#[cfg(test)]
mod modern_tests;
mod pixels;
mod protocols;

use crate::{app::OxiBar, messages::Message, shell::SurfaceKind};
use iced::{
    Alignment, Element, Length, Point, Rectangle, Task,
    widget::{Column, Row, Space, container, image},
};
use iced_layershell::reexport::{IcedId, KeyboardInteractivity};
use oxibar_plugin_api::{ApplyMode, SettingDescriptor, SettingKind};
use oxiced::{
    theme::theme_impl::OXITHEME,
    widgets::oxi_plugin::{self, text},
};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    sync::Arc,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub enabled: bool,
    pub fps: u32,
    pub thumbnail_size: u32,
    pub columns: u32,
    pub width: u32,
    pub height: u32,
}
impl Config {
    pub(crate) fn read(table: &toml::Table) -> Self {
        let value = |name: &str, default| {
            table
                .get("overview")
                .and_then(|v| v.get(name))
                .and_then(toml::Value::as_integer)
                .unwrap_or(default) as u32
        };
        Self {
            enabled: table
                .get("overview")
                .and_then(|v| v.get("enabled"))
                .and_then(toml::Value::as_bool)
                .unwrap_or(true),
            fps: value("refresh_fps", 8),
            thumbnail_size: value("thumbnail_size", 512),
            columns: value("columns", 4),
            width: value("width", 1400),
            height: value("height", 900),
        }
    }
}
pub(crate) fn schema() -> Vec<SettingDescriptor> {
    vec![
        SettingDescriptor::new(
            "overview.enabled",
            "Window overview",
            SettingKind::Boolean,
            Some(toml::Value::Boolean(true)),
            ApplyMode::Live,
        ),
        SettingDescriptor::new(
            "overview.refresh_fps",
            "Maximum preview updates per second",
            SettingKind::Integer { min: 1, max: 8 },
            Some(toml::Value::Integer(8)),
            ApplyMode::Live,
        ),
        SettingDescriptor::new(
            "overview.thumbnail_size",
            "Maximum preview pixels",
            SettingKind::Integer { min: 128, max: 640 },
            Some(toml::Value::Integer(512)),
            ApplyMode::Surface,
        ),
        SettingDescriptor::new(
            "overview.columns",
            "Maximum preview columns",
            SettingKind::Integer { min: 1, max: 8 },
            Some(toml::Value::Integer(4)),
            ApplyMode::Live,
        ),
        SettingDescriptor::new(
            "overview.width",
            "Overview maximum width",
            SettingKind::Integer {
                min: 320,
                max: 7680,
            },
            Some(toml::Value::Integer(1400)),
            ApplyMode::Surface,
        )
        .description("Centered modal, bounded to 90% of each monitor."),
        SettingDescriptor::new(
            "overview.height",
            "Overview maximum height",
            SettingKind::Integer {
                min: 240,
                max: 4320,
            },
            Some(toml::Value::Integer(900)),
            ApplyMode::Surface,
        ),
    ]
}

#[derive(Debug, Clone)]
pub enum Action {
    Open,
    Toggle,
    Close,
    Select(u64),
    Activate(u64),
    ActivateSelected,
    Move(i32, i32),
    Cycle(i32),
    MonitorDirection(i32, i32),
    Monitor(i32),
    First,
    Last,
}
pub(crate) struct Surface {
    pub id: IcedId,
    pub size: (u32, u32),
}
#[derive(Default)]
pub(crate) struct Overview {
    pub surfaces: BTreeMap<String, Surface>,
    pub active: Option<String>,
    pub selected: Option<u64>,
    pub client: Option<Arc<capture::Client>>,
    pub snapshot: capture::Snapshot,
    pub generation: u64,
    pub closing: bool,
    retiring: HashSet<IcedId>,
    activation: Option<u64>,
    retirement_timer: Option<iced::task::Handle>,
    pointers: HashMap<IcedId, Point>,
    bounds: HashMap<IcedId, BTreeMap<Option<u64>, Rectangle>>,
}
impl OxiBar {
    pub(crate) fn overview_windows(&self, output: &str) -> Vec<&capture::Window> {
        self.overview
            .snapshot
            .windows
            .iter()
            .filter(|w| {
                w.outputs.iter().any(|o| o == output)
                    || w.outputs.is_empty() && self.overview.active.as_deref() == Some(output)
            })
            .collect()
    }
    fn overview_columns(&self, output: &str) -> usize {
        let width = self
            .overview
            .surfaces
            .get(output)
            .map_or(1280, |s| s.size.0);
        ((width.saturating_sub(2 * OXITHEME.padding_xl as u32) / 240)
            .max(1)
            .min(self.config.snapshot.host.overview.columns)) as usize
    }
    pub(crate) fn update_overview(&mut self, action: Action) -> Task<Message> {
        match action {
            Action::Toggle if self.overview.client.is_some() => self.close_overview(),
            Action::Open | Action::Toggle => {
                if !self.config.snapshot.host.overview.enabled || self.overview.client.is_some() {
                    return Task::none();
                }
                self.overview.generation = self.overview.generation.wrapping_add(1);
                let config = &self.config.snapshot.host.overview;
                match capture::Client::new(
                    config.fps,
                    config.thumbnail_size,
                    self.compositor.clone().unwrap_or_default(),
                ) {
                    Ok(client) => self.overview.client = Some(client),
                    Err(error) => {
                        self.overview.snapshot.error = Some(error);
                        return Task::none();
                    }
                }
                self.overview.active = self.overlay_output();
                self.overview.closing = false;
                let dismiss = Task::batch([
                    self.close_launcher(),
                    self.close_settings(),
                    self.close_control_center(),
                    self.close_bar_dropdown(),
                    self.close_modal(),
                    self.close_panel(),
                    if self.desktop.draft.is_some() {
                        self.cancel_desktop_edit()
                    } else {
                        Task::none()
                    },
                ]);
                dismiss.chain(self.reconcile_overview())
            }
            Action::Close => self.close_overview(),
            Action::Select(id) => {
                if let Some(window) = self.overview.snapshot.windows.iter().find(|w| w.id == id) {
                    let output = self
                        .shell
                        .request_source
                        .and_then(|source| self.shell.registry.get(source))
                        .and_then(|record| record.output.clone())
                        .filter(|output| self.overview_windows(output).iter().any(|w| w.id == id))
                        .or_else(|| {
                            self.overview.active.clone().filter(|output| {
                                self.overview_windows(output).iter().any(|w| w.id == id)
                            })
                        })
                        .or_else(|| {
                            window
                                .outputs
                                .iter()
                                .find(|o| self.overview.surfaces.contains_key(*o))
                                .cloned()
                        })
                        .or(self.overview.active.clone());
                    self.overview.selected = Some(id);
                    if let Some(client) = &self.overview.client {
                        client.priority(id)
                    }
                    return Task::batch([
                        self.overview_focus(output),
                        self.scroll_overview_selection(),
                    ]);
                }
                Task::none()
            }
            Action::Activate(id) => {
                let Some(window) = self
                    .overview
                    .snapshot
                    .windows
                    .iter()
                    .find(|w| w.id == id && w.activatable)
                else {
                    return Task::none();
                };
                let Some(client) = self.overview.client.as_ref() else {
                    return Task::none();
                };
                let id = window.id;
                if self.overview.closing {
                    return Task::none();
                };
                self.overview.closing = true;
                self.overview.selected = Some(id);
                self.overview.activation = Some(id);
                self.overview.retiring = self
                    .overview
                    .surfaces
                    .values()
                    .map(|surface| surface.id)
                    .collect();
                let timed = client.is_ready();
                let close = self.remove_overview_surfaces();
                let generation = self.overview.generation;
                if self.overview.retiring.is_empty() {
                    close.chain(self.activate_retired_overview())
                } else if timed {
                    let (timer, handle) = Task::perform(
                        async {
                            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                        },
                        move |_| Message::OverviewRetirementTimeout(generation),
                    )
                    .abortable();
                    self.overview.retirement_timer = Some(handle.abort_on_drop());
                    Task::batch([close, timer])
                } else {
                    close
                }
            }
            Action::ActivateSelected => self
                .overview
                .selected
                .map(|id| self.update_overview(Action::Activate(id)))
                .unwrap_or_else(Task::none),
            Action::Monitor(direction) => {
                let outputs: Vec<_> = self.overview.surfaces.keys().cloned().collect();
                if outputs.is_empty() {
                    return Task::none();
                }
                let index = self
                    .overview
                    .active
                    .as_ref()
                    .and_then(|o| outputs.iter().position(|n| n == o))
                    .unwrap_or(0);
                let index = (index as i32 + direction).rem_euclid(outputs.len() as i32) as usize;
                let output = outputs[index].clone();
                self.overview.selected = self.overview_windows(&output).first().map(|w| w.id);
                Task::batch([
                    self.overview_focus(Some(output)),
                    self.scroll_overview_selection(),
                ])
            }
            Action::MonitorDirection(dx, dy) => {
                let Some(inventory) = &self.shell.inventory else {
                    return Task::none();
                };
                let Some(current) = inventory
                    .outputs
                    .iter()
                    .find(|output| Some(&output.name) == self.overview.active.as_ref())
                else {
                    return Task::none();
                };
                let output = directional_monitor(
                    current,
                    inventory
                        .outputs
                        .iter()
                        .filter(|output| self.overview.surfaces.contains_key(&output.name)),
                    dx,
                    dy,
                )
                .map(|output| output.name.clone());
                let Some(output) = output else {
                    return Task::none();
                };
                self.overview.selected = self
                    .overview_windows(&output)
                    .first()
                    .map(|window| window.id);
                self.overview.active = Some(output.clone());
                self.normalize_overview_selection();
                if let Some(id) = self.overview.selected
                    && let Some(client) = &self.overview.client
                {
                    client.priority(id);
                }
                Task::batch([
                    self.overview_focus(Some(output)),
                    self.scroll_overview_selection(),
                ])
            }
            Action::Cycle(delta) => {
                let Some(output) = &self.overview.active else {
                    return Task::none();
                };
                let windows = self.overview_windows(output);
                if windows.is_empty() {
                    return Task::none();
                }
                let index = windows
                    .iter()
                    .position(|window| Some(window.id) == self.overview.selected)
                    .unwrap_or(0);
                let index = (index as i32 + delta).rem_euclid(windows.len() as i32) as usize;
                self.update_overview(Action::Select(windows[index].id))
            }
            Action::Move(dx, dy) => {
                let Some(output) = self.overview.active.clone() else {
                    return Task::none();
                };
                let windows = self.overview_windows(&output);
                if windows.is_empty() {
                    return Task::none();
                };
                let index = windows
                    .iter()
                    .position(|w| Some(w.id) == self.overview.selected)
                    .unwrap_or(0);
                let delta = dx + dy * self.overview_columns(&output) as i32;
                let index = (index as i32 + delta).clamp(0, windows.len() as i32 - 1) as usize;
                self.update_overview(Action::Select(windows[index].id))
            }
            Action::First | Action::Last => {
                let Some(output) = &self.overview.active else {
                    return Task::none();
                };
                let windows = self.overview_windows(output);
                let entry = if matches!(action, Action::First) {
                    windows.first()
                } else {
                    windows.last()
                };
                let id = entry.map(|w| w.id);
                id.map(|id| self.update_overview(Action::Select(id)))
                    .unwrap_or_else(Task::none)
            }
        }
    }
    fn remove_overview_surfaces(&mut self) -> Task<Message> {
        let surfaces = std::mem::take(&mut self.overview.surfaces);
        Task::batch(surfaces.into_values().map(|surface| {
            self.shell.registry.remove(surface.id);
            Task::done(Message::RemoveSurface(surface.id))
        }))
    }
    fn activate_retired_overview(&mut self) -> Task<Message> {
        self.overview.retirement_timer = None;
        let Some(id) = self.overview.activation.take() else {
            return Task::none();
        };
        let Some(client) = self.overview.client.clone() else {
            return Task::none();
        };
        let generation = self.overview.generation;
        Task::perform(async move { client.activate(id).await }, move |result| {
            Message::OverviewActivated(generation, result)
        })
    }
    pub(crate) fn overview_surface_retired(&mut self, id: IcedId) -> Option<Task<Message>> {
        if !self.overview.retiring.remove(&id) {
            return None;
        }
        Some(if self.overview.retiring.is_empty() {
            self.activate_retired_overview()
        } else {
            Task::none()
        })
    }
    pub(crate) fn overview_retirement_timeout(&mut self, generation: u64) -> Task<Message> {
        if self.overview.generation != generation || self.overview.activation.is_none() {
            return Task::none();
        }
        self.compositor_action_error =
            Some("Overview surfaces did not retire before activation".into());
        self.close_overview()
    }
    pub(crate) fn close_overview(&mut self) -> Task<Message> {
        let task = self.remove_overview_surfaces();
        self.overview.client = None;
        self.overview.snapshot = capture::Snapshot::default();
        self.overview.selected = None;
        self.overview.active = None;
        self.overview.closing = false;
        self.overview.retiring.clear();
        self.overview.activation = None;
        self.overview.retirement_timer = None;
        self.overview.pointers.clear();
        self.overview.bounds.clear();
        self.overview.generation = self.overview.generation.wrapping_add(1);
        task
    }
    fn overview_focus(&mut self, output: Option<String>) -> Task<Message> {
        self.overview.active = output;
        Task::batch(self.overview.surfaces.iter().map(|(name, surface)| {
            Task::done(Message::SetToastKeyboardInteractivity(
                surface.id,
                if Some(name) == self.overview.active.as_ref() {
                    KeyboardInteractivity::Exclusive
                } else {
                    KeyboardInteractivity::None
                },
            ))
        }))
    }
    fn scroll_overview_selection(&self) -> Task<Message> {
        if !self
            .overview
            .client
            .as_ref()
            .is_some_and(|client| client.is_ready())
        {
            return Task::none();
        }
        let Some(output) = &self.overview.active else {
            return Task::none();
        };
        let Some(surface) = self.overview.surfaces.get(output) else {
            return Task::none();
        };
        let index = self
            .overview_windows(output)
            .iter()
            .position(|window| Some(window.id) == self.overview.selected)
            .unwrap_or(0);
        iced::widget::operation::scroll_to(
            format!("overview-{:?}", surface.id),
            iced::widget::scrollable::AbsoluteOffset {
                x: 0.0,
                y: (index / self.overview_columns(output)) as f32
                    * (card_height() + OXITHEME.padding_lg),
            },
        )
    }
    pub(crate) fn reconcile_overview(&mut self) -> Task<Message> {
        if self.overview.client.is_none() || self.overview.closing {
            return Task::none();
        }
        let targets: Vec<_> = self
            .shell
            .inventory
            .as_ref()
            .map(|inventory| {
                inventory
                    .outputs
                    .iter()
                    .filter(|o| o.enabled)
                    .map(|o| {
                        (
                            o.name.clone(),
                            modal_size(&self.config.snapshot.host.overview, o.logical_size),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let names: BTreeSet<_> = targets.iter().map(|(n, _)| n.clone()).collect();
        let stale: Vec<_> = self
            .overview
            .surfaces
            .keys()
            .filter(|n| !names.contains(*n))
            .cloned()
            .collect();
        let mut tasks = Vec::new();
        for name in stale {
            let surface = self.overview.surfaces.remove(&name).unwrap();
            self.shell.registry.remove(surface.id);
            tasks.push(Task::done(Message::RemoveSurface(surface.id)));
        }
        if self
            .overview
            .active
            .as_ref()
            .is_none_or(|o| !names.contains(o))
        {
            self.overview.active = targets.first().map(|(n, _)| n.clone());
        }
        for (output, size) in targets {
            if let Some(surface) = self.overview.surfaces.get_mut(&output) {
                if surface.size != size {
                    surface.size = size;
                    tasks.push(Task::done(Message::ResizeLayer(surface.id, size.0, size.1)));
                }
            } else {
                let id = IcedId::unique();
                let active = self.overview.active.as_ref() == Some(&output);
                self.register_overlay(
                    id,
                    SurfaceKind::Overview,
                    self.shell.active_bar,
                    Some(output.clone()),
                );
                self.overview
                    .surfaces
                    .insert(output.clone(), Surface { id, size });
                tasks.push(Task::done(Message::OnOutput(
                    Some(output),
                    Box::new(Message::OpenOverviewLayer(id, size, active)),
                )));
            }
        }
        if self.overview.surfaces.is_empty() {
            tasks.push(self.close_overview())
        } else {
            self.normalize_overview_selection();
            tasks.push(self.overview_focus(self.overview.active.clone()));
        }
        let visible: HashSet<_> = self
            .overview
            .surfaces
            .values()
            .map(|surface| surface.id)
            .collect();
        self.overview.pointers.retain(|id, _| visible.contains(id));
        self.overview.bounds.retain(|id, _| visible.contains(id));
        Task::batch(tasks)
    }
    pub(crate) fn overview_snapshot(
        &mut self,
        generation: u64,
        snapshot: capture::Snapshot,
    ) -> Task<Message> {
        if generation != self.overview.generation
            || self.overview.client.is_none()
            || self.overview.closing
        {
            return Task::none();
        }
        self.overview.snapshot = snapshot;
        for bounds in self.overview.bounds.values_mut() {
            bounds.retain(|id, _| {
                id.is_none_or(|id| {
                    self.overview
                        .snapshot
                        .windows
                        .iter()
                        .any(|window| window.id == id)
                })
            });
        }
        self.normalize_overview_selection();
        if let Some(id) = self.overview.selected
            && let Some(client) = &self.overview.client
        {
            client.priority(id)
        }
        Task::none()
    }
    fn normalize_overview_selection(&mut self) {
        let windows = self
            .overview
            .active
            .as_ref()
            .map(|output| self.overview_windows(output))
            .unwrap_or_default();
        if self
            .overview
            .selected
            .is_none_or(|id| !windows.iter().any(|w| w.id == id))
        {
            self.overview.selected = windows
                .iter()
                .find(|w| w.focused)
                .or(windows.first())
                .map(|w| w.id);
        }
    }
    pub(crate) fn overview_bounds(
        &mut self,
        surface: IcedId,
        target: Option<u64>,
        bounds: Rectangle,
    ) -> Task<Message> {
        if !self
            .overview
            .surfaces
            .values()
            .any(|value| value.id == surface)
            || ![bounds.x, bounds.y, bounds.width, bounds.height]
                .into_iter()
                .all(f32::is_finite)
            || target.is_some_and(|id| {
                !self
                    .overview
                    .snapshot
                    .windows
                    .iter()
                    .any(|window| window.id == id)
            })
        {
            return Task::none();
        }
        if bounds.width <= 0.0 || bounds.height <= 0.0 {
            if let Some(values) = self.overview.bounds.get_mut(&surface) {
                values.remove(&target);
            }
            return Task::none();
        }
        self.overview
            .bounds
            .entry(surface)
            .or_default()
            .insert(target, bounds);
        Task::none()
    }
    pub(crate) fn overview_pointer(
        &mut self,
        surface: IcedId,
        position: Option<Point>,
    ) -> Task<Message> {
        if self
            .overview
            .surfaces
            .values()
            .any(|value| value.id == surface)
        {
            if let Some(position) =
                position.filter(|point| point.x.is_finite() && point.y.is_finite())
            {
                self.overview.pointers.insert(surface, position);
            } else {
                self.overview.pointers.remove(&surface);
            }
        }
        Task::none()
    }
    pub(crate) fn overview_press(&mut self, surface: IcedId) -> Task<Message> {
        let Some(position) = self.overview.pointers.get(&surface).copied() else {
            return Task::none();
        };
        let Some((source, size)) = self
            .overview
            .surfaces
            .iter()
            .find(|(_, value)| value.id == surface)
        else {
            return Task::none();
        };
        if Rectangle::with_size(iced::Size::new(size.size.0 as f32, size.size.1 as f32))
            .contains(position)
        {
            return Task::none();
        }
        let Some(inventory) = &self.shell.inventory else {
            return Task::none();
        };
        let Some(output) = inventory
            .outputs
            .iter()
            .find(|output| &output.name == source)
        else {
            return Task::none();
        };
        let global = position + modal_origin(output, size.size);
        let target = self
            .overview
            .surfaces
            .iter()
            .filter(|(_, value)| value.id != surface)
            .find_map(|(name, value)| {
                let output = inventory
                    .outputs
                    .iter()
                    .find(|output| &output.name == name)?;
                let local = global - modal_origin(output, value.size);
                if !Rectangle::with_size(iced::Size::new(value.size.0 as f32, value.size.1 as f32))
                    .contains(local)
                {
                    return None;
                }
                self.overview
                    .bounds
                    .get(&value.id)?
                    .iter()
                    .find_map(|(target, bounds)| bounds.contains(local).then_some(*target))
            });
        match target {
            Some(Some(id)) => self.update_overview(Action::Activate(id)),
            Some(None) => self.close_overview(),
            None => Task::none(),
        }
    }
    pub(crate) fn overview_view(&self, id: IcedId) -> Element<'_, Message> {
        let Some((output, surface)) = self.overview.surfaces.iter().find(|(_, s)| s.id == id)
        else {
            return Space::new().into();
        };
        let windows = self.overview_windows(output);
        let columns = self.overview_columns(output);
        let width = ((surface.size.0 as f32
            - 2.0 * OXITHEME.padding_xl
            - OXITHEME.padding_sm
            - OXITHEME.padding_md
            - (columns - 1) as f32 * OXITHEME.padding_lg)
            / columns as f32)
            .max(100.0);
        let mut body = Column::new()
            .spacing(OXITHEME.padding_lg)
            .width(Length::Fill);
        if let Some(error) = &self.overview.snapshot.error {
            body = body.push(text(error).style(oxi_plugin::text_muted));
        }
        if windows.is_empty() {
            body = body.push(
                text("No windows on this monitor")
                    .size(OXITHEME.font_xl)
                    .style(oxi_plugin::text_muted),
            );
        }
        for chunk in windows.chunks(columns) {
            let mut row = Row::new().spacing(OXITHEME.padding_lg);
            for window in chunk.iter().copied() {
                let selected = if Some(window.id) == self.overview.selected {
                    1.0
                } else {
                    0.0
                };
                let motion = oxiced::theme::snapshot::current_snapshot();
                let card: Element<'_, Message> =
                    if motion.reduced_motion || motion.animation_ms == 0 {
                        window_card(window, width, selected)
                    } else {
                        iced_anim::AnimationBuilder::new(selected, move |selection| {
                            window_card(window, width, selection)
                        })
                        .animation(iced_anim::Easing::EASE_OUT.with_duration(motion.duration()))
                        .into()
                    };
                let window_id = window.id;
                row = row.push(
                    oxiced::widgets::oxi_drag_handle::capture_visible_bounds(
                        card,
                        move |bounds, _| Message::OverviewBounds(id, Some(window_id), bounds),
                    )
                    .with_identity(window_id.to_string()),
                );
            }
            body = body.push(row);
        }
        let header = Row::new()
            .push(
                Column::new()
                    .push(text("Window overview").size(OXITHEME.font_xxl))
                    .push(
                        text(output.clone())
                            .size(OXITHEME.font_lg)
                            .style(oxi_plugin::text_muted),
                    )
                    .spacing(OXITHEME.padding_sm)
                    .width(Length::Fill),
            )
            .push(oxiced::widgets::oxi_drag_handle::capture_bounds(
                oxi_plugin::surface_button(text("Close"), false)
                    .on_press(Message::Overview(Action::Close)),
                move |bounds, _| Message::OverviewBounds(id, None, bounds),
            ))
            .align_y(Alignment::Center);
        container(
            Column::new()
                .push(header)
                .push(oxi_plugin::scroll_panel(body).id(format!("overview-{id:?}")))
                .push(
                    text(format!(
                        "{}: window · {}/{}/{}/{}: monitor · {}: open · {}: close",
                        self.shortcut_label("overview.next_window"),
                        self.shortcut_label("overview.monitor_left"),
                        self.shortcut_label("overview.monitor_down"),
                        self.shortcut_label("overview.monitor_up"),
                        self.shortcut_label("overview.monitor_right"),
                        self.shortcut_label("overview.activate"),
                        self.shortcut_label("close")
                    ))
                    .size(OXITHEME.font_md)
                    .style(oxi_plugin::text_muted),
                )
                .spacing(OXITHEME.padding_xl)
                .padding(OXITHEME.padding_xl)
                .height(Length::Fill),
        )
        .style(oxi_plugin::compact_card_style(OXITHEME.mantle))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}
pub(crate) fn mouse_event(
    event: iced::Event,
    _: iced::event::Status,
    id: IcedId,
) -> Option<Message> {
    match event {
        iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
            Some(Message::OverviewPointer(id, Some(position)))
        }
        iced::Event::Mouse(iced::mouse::Event::CursorEntered | iced::mouse::Event::CursorLeft) => {
            Some(Message::OverviewPointer(id, None))
        }
        iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)) => {
            Some(Message::OverviewPress(id))
        }
        _ => None,
    }
}
fn card_height() -> f32 {
    180.0
        + OXITHEME.font_md * 3.0
        + OXITHEME.font_sm * 5.0
        + OXITHEME.padding_sm * 3.0
        + OXITHEME.padding_md * 2.0
}
fn window_card(window: &capture::Window, width: f32, selection: f32) -> Element<'_, Message> {
    let preview: Element<'_, Message> = if let Some(handle) = &window.image {
        image(handle.clone())
            .content_fit(iced::ContentFit::Contain)
            .width(Length::Fill)
            .height(180)
            .into()
    } else {
        container(
            text(
                window
                    .error
                    .as_deref()
                    .unwrap_or("Waiting for window content…"),
            )
            .style(oxi_plugin::text_muted),
        )
        .center_x(Length::Fill)
        .center_y(180)
        .into()
    };
    let content = Column::new()
        .push(preview)
        .push(
            container(
                text(window.title.chars().take(90).collect::<String>()).size(OXITHEME.font_md),
            )
            .height(OXITHEME.font_md * 3.0)
            .clip(true),
        )
        .push(
            container(
                text(window.app_id.clone())
                    .size(OXITHEME.font_sm)
                    .style(oxi_plugin::text_muted),
            )
            .height(OXITHEME.font_sm * 2.0)
            .clip(true),
        )
        .push(
            container(
                text(if window.image.is_some() {
                    window.error.as_deref().unwrap_or("")
                } else {
                    ""
                })
                .size(OXITHEME.font_sm)
                .style(oxi_plugin::text_muted),
            )
            .height(OXITHEME.font_sm * 3.0)
            .clip(true),
        )
        .spacing(OXITHEME.padding_sm);
    oxi_plugin::selectable_row(content, false)
        .padding(OXITHEME.padding_md)
        .width(width)
        .height(card_height())
        .style(move |theme, status| {
            let mut style = oxi_plugin::selectable_row_style(false)(theme, status);
            let selected = oxi_plugin::selectable_row_style(true)(theme, status);
            let factor = selection.clamp(0.0, 1.0);
            if let (Some(iced::Background::Color(a)), Some(iced::Background::Color(b))) =
                (style.background, selected.background)
            {
                style.background = Some(
                    iced::Color {
                        r: a.r + (b.r - a.r) * factor,
                        g: a.g + (b.g - a.g) * factor,
                        b: a.b + (b.b - a.b) * factor,
                        a: a.a + (b.a - a.a) * factor,
                    }
                    .into(),
                );
            }
            style.border.color = OXITHEME.primary.scale_alpha(factor);
            style.border.width = 1.0;
            style
        })
        .on_press_maybe(
            window
                .activatable
                .then_some(Message::Overview(Action::Activate(window.id))),
        )
        .into()
}
fn modal_size(config: &Config, output: (u32, u32)) -> (u32, u32) {
    (
        config.width.min(output.0.saturating_mul(9) / 10).max(1),
        config.height.min(output.1.saturating_mul(9) / 10).max(1),
    )
}

fn modal_origin(output: &crate::monitor::Output, size: (u32, u32)) -> iced::Vector {
    iced::Vector::new(
        output.position.0 as f32 + output.logical_size.0.saturating_sub(size.0) as f32 / 2.0,
        output.position.1 as f32 + output.logical_size.1.saturating_sub(size.1) as f32 / 2.0,
    )
}

fn directional_monitor<'a>(
    current: &crate::monitor::Output,
    outputs: impl Iterator<Item = &'a crate::monitor::Output>,
    dx: i32,
    dy: i32,
) -> Option<&'a crate::monitor::Output> {
    let center = |output: &crate::monitor::Output| {
        (
            output.position.0 as f64 + output.logical_size.0 as f64 / 2.0,
            output.position.1 as f64 + output.logical_size.1 as f64 / 2.0,
        )
    };
    let (x, y) = center(current);
    outputs
        .filter_map(|output| {
            let (ox, oy) = center(output);
            let forward = (ox - x) * dx as f64 + (oy - y) * dy as f64;
            let sideways = (ox - x) * dy as f64 - (oy - y) * dx as f64;
            (output.enabled && output.name != current.name && forward > 0.0)
                .then_some((output, forward * forward + 4.0 * sideways * sideways))
        })
        .min_by(|(a, sa), (b, sb)| sa.total_cmp(sb).then_with(|| a.name.cmp(&b.name)))
        .map(|(output, _)| output)
}
pub(crate) fn subscription(
    input: &(u64, Arc<capture::Client>),
) -> iced::futures::stream::BoxStream<'static, Message> {
    use iced::futures::StreamExt;
    let generation = input.0;
    capture::events(input.1.clone())
        .map(move |snapshot| Message::OverviewSnapshot(generation, snapshot))
        .boxed()
}
