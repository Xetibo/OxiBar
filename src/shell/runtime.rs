use iced::{Task, widget::Space};
use iced_layershell::reexport::IcedId;

use super::{BarInstance, SurfaceKind};
use crate::{app::OxiBar, layout::BarSection, messages::Message, monitor::OutputSnapshot};

impl OxiBar {
    pub(crate) fn outputs_changed(
        &mut self,
        result: Result<OutputSnapshot, String>,
    ) -> Task<Message> {
        let mut tasks = Vec::new();
        match result {
            Ok(inventory) => {
                let missing: Vec<_> = self
                    .shell
                    .registry
                    .iter()
                    .filter(|(_, record)| {
                        !matches!(record.kind, SurfaceKind::Bar(_))
                            && record.output.as_ref().is_some_and(|name| {
                                !inventory
                                    .outputs
                                    .iter()
                                    .any(|output| output.enabled && &output.name == name)
                            })
                    })
                    .map(|(id, _)| *id)
                    .collect();
                for id in missing {
                    tasks.push(self.forget_surface(id));
                    tasks.push(Task::done(Message::RemoveSurface(id)));
                }
                self.shell.inventory = Some(inventory);
                self.shell.inventory_error = None;
            }
            Err(error) => {
                self.shell.inventory_error = Some(error);
                return Task::none();
            }
        }
        tasks.push(self.reconcile_bars());
        tasks.push(self.reconcile_docks());
        tasks.push(self.reconcile_desktop());
        tasks.push(self.reconcile_overview());
        Task::batch(tasks)
    }

    pub(crate) fn reconcile_bars(&mut self) -> Task<Message> {
        let desired = self
            .shell
            .desired(&self.config.snapshot, &self.plugin_order);
        let mut tasks = Vec::new();
        let removed: Vec<_> = self
            .shell
            .bars
            .keys()
            .filter(|key| !desired.contains_key(*key))
            .cloned()
            .collect();
        for key in removed {
            let bar = self.shell.bars.remove(&key).expect("existing bar");
            tasks.push(self.close_owned_surfaces(bar.id, bar.output.as_deref()));
            self.shell.registry.remove(bar.id);
            self.shell.plugin_origins.retain(|_, id| *id != bar.id);
            tasks.push(Task::done(Message::RemoveSurface(bar.id)));
        }
        for (key, (output, config, size)) in desired {
            if let Some(bar) = self.shell.bars.get_mut(&key) {
                let changed = bar.config != config;
                let geometry = bar.config.width != config.width
                    || bar.config.popup_height != config.popup_height
                    || bar.config.height != config.height
                    || bar.config.position != config.position;
                let reservation = geometry || bar.config.auto_hide != config.auto_hide;
                if bar.config.auto_hide != config.auto_hide {
                    bar.revealed = false;
                }
                bar.config = config;
                bar.anchors
                    .retain(|key, (provider, _)| bar.config.anchor_matches(key, provider));
                if bar
                    .pressed_anchor
                    .as_ref()
                    .is_some_and(|key| !bar.anchors.contains_key(key))
                {
                    bar.pressed_anchor = None;
                }
                bar.size.height = size.height;
                if bar.config.width.is_some() {
                    bar.size.width = size.width;
                }
                let id = bar.id;
                if geometry {
                    tasks.push(Task::done(Message::ConfigureBar(id, bar.config.clone())));
                }
                if reservation {
                    tasks.push(Task::done(Message::SetExclusiveZone(
                        id,
                        if bar.config.auto_hide {
                            0
                        } else {
                            bar.config.height as i32
                        },
                    )));
                }
                if changed {
                    tasks.push(self.bar_input_region_task(id));
                }
            } else {
                let id = IcedId::unique();
                self.shell.registry.register(
                    id,
                    SurfaceKind::Bar(key.clone()),
                    output.clone(),
                    None,
                );
                self.shell.bars.insert(
                    key,
                    BarInstance {
                        id,
                        output: output.clone(),
                        config: config.clone(),
                        size,
                        ready: false,
                        revealed: false,
                        anchors: Default::default(),
                        pressed_anchor: None,
                    },
                );
                tasks.push(Task::done(Message::OpenBarLayer(id, output, config)));
            }
        }
        if self
            .bar_window_id
            .is_none_or(|id| self.shell.bar(id).is_none())
        {
            self.bar_window_id = self.shell.bars.values().next().map(|bar| bar.id);
        }
        if self
            .shell
            .active_bar
            .is_none_or(|id| self.shell.bar(id).is_none())
        {
            self.shell.active_bar = self.bar_window_id;
        }
        if let Some(bar) = self.bar_window_id.and_then(|id| self.shell.bar(id)) {
            self.bar_size = bar.size;
        }
        Task::batch(tasks)
    }

    pub(crate) fn bar_input_region_task(&self, id: IcedId) -> Task<Message> {
        let Some(bar) = self.shell.bar(id) else {
            return Task::none();
        };
        let open = self.popup_open && self.shell.popup_bar == Some(id);
        let metrics = self
            .popup_plugin
            .as_deref()
            .map(|plugin| self.popup_input_metrics(plugin));
        Task::done(Message::SetBarInputRegion {
            id,
            open,
            section: self
                .popup_plugin
                .as_deref()
                .map(|plugin| self.plugin_section_on(id, plugin))
                .unwrap_or(BarSection::End),
            width: metrics
                .map(|metrics| {
                    metrics.connector_width.min(
                        bar.size.width.saturating_sub(
                            self.popup_plugin
                                .as_deref()
                                .map(|plugin| self.dropdown_x(id, plugin).max(0) as u32)
                                .unwrap_or(0),
                        ),
                    )
                })
                .unwrap_or(0),
            height: metrics.map(|metrics| metrics.height).unwrap_or(0),
            bar_size: bar.size,
            bottom: bar.config.position == crate::config::bars::Position::Bottom,
            reserve: bar.config.popup_height,
            edge_trigger: (!self.bar_is_visible(id)).then_some(bar.config.reveal_zone),
            x: self
                .popup_plugin
                .as_deref()
                .map(|plugin| self.dropdown_x(id, plugin)),
        })
    }

    pub(crate) fn bar_is_visible(&self, id: IcedId) -> bool {
        self.shell.bar(id).is_some_and(|bar| {
            !bar.config.auto_hide
                || bar.revealed
                || (self.popup_open && self.shell.popup_bar == Some(id))
        })
    }

    pub(crate) fn bar_pointer(
        &mut self,
        id: IcedId,
        position: Option<iced::Point>,
    ) -> Task<Message> {
        let Some(bar) = self.shell.bar_mut(id).filter(|bar| bar.config.auto_hide) else {
            return Task::none();
        };
        let y = if bar.config.position == crate::config::bars::Position::Bottom {
            bar.config.popup_height as f32
        } else {
            0.0
        };
        let revealed = position.is_some_and(|point| {
            iced::Rectangle {
                x: 0.0,
                y,
                width: bar.size.width as f32,
                height: bar.size.height as f32,
            }
            .contains(point)
        });
        if bar.revealed == revealed {
            return Task::none();
        }
        bar.revealed = revealed;
        self.bar_input_region_task(id)
    }

    pub(crate) fn dropdown_x(&self, id: IcedId, plugin: &str) -> i32 {
        let Some(bar) = self.shell.bar(id) else {
            return 0;
        };
        let matches = |name: &str| {
            if plugin == "@control-center" {
                name.eq_ignore_ascii_case("Clock") || name.eq_ignore_ascii_case("control-center")
            } else if plugin == "@session" {
                name.eq_ignore_ascii_case("session")
            } else {
                name.eq_ignore_ascii_case(plugin)
            }
        };
        let bounds = bar
            .pressed_anchor
            .as_ref()
            .and_then(|key| bar.anchors.get(key))
            .filter(|(name, _)| matches(name))
            .or_else(|| bar.anchors.values().find(|(name, _)| matches(name)))
            .map(|(_, bounds)| *bounds);
        let width = self.popup_metrics(plugin).connector_width;
        bounds
            .map(|bounds| crate::layout::button_popup_x(bar.size.width, width, bounds))
            .unwrap_or_else(|| {
                crate::layout::popup_x(bar.size.width, self.plugin_section_on(id, plugin), width)
            })
    }

    pub(crate) fn bar_anchor(
        &mut self,
        id: IcedId,
        key: String,
        provider: String,
        bounds: iced::Rectangle,
        pressed: bool,
    ) -> Task<Message> {
        let Some(bar) = self.shell.bar_mut(id) else {
            return Task::none();
        };
        if ![bounds.x, bounds.y, bounds.width, bounds.height]
            .iter()
            .all(|value| value.is_finite())
            || bounds.width <= 0.0
            || bounds.height <= 0.0
            || !bar.config.anchor_matches(&key, &provider)
        {
            return Task::none();
        }
        if bar.anchors.len() >= 256 && !bar.anchors.contains_key(&key) {
            return Task::none();
        }
        bar.anchors.insert(key.clone(), (provider, bounds));
        if pressed {
            bar.pressed_anchor = Some(key);
        }
        if self.popup_open && self.shell.popup_bar == Some(id) {
            self.bar_input_region_task(id)
        } else {
            Task::none()
        }
    }

    pub(crate) fn plugin_section_on(&self, id: IcedId, plugin: &str) -> BarSection {
        let plugin = if plugin == "@control-center" {
            if self.shell.bar(id).is_some_and(|bar| {
                bar.config
                    .start
                    .iter()
                    .chain(&bar.config.center)
                    .chain(&bar.config.end)
                    .any(|name| name.eq_ignore_ascii_case("Clock"))
            }) {
                "Clock"
            } else {
                "control-center"
            }
        } else if plugin == "@session" {
            "session"
        } else {
            plugin
        };
        if let Some(bar) = self.shell.bar(id) {
            if bar
                .config
                .start
                .iter()
                .any(|name| name.eq_ignore_ascii_case(plugin))
            {
                return BarSection::Start;
            }
            if bar
                .config
                .center
                .iter()
                .any(|name| name.eq_ignore_ascii_case(plugin))
            {
                return BarSection::Center;
            }
            return BarSection::End;
        }
        self.plugin_section(plugin)
    }

    pub(crate) fn output_for_bar(&self, id: Option<IcedId>) -> Option<String> {
        id.and_then(|id| self.shell.bar(id))
            .and_then(|bar| bar.output.clone())
    }

    pub(crate) fn dropdown_bar(&self) -> Option<IcedId> {
        let output = self.overlay_output();
        self.shell
            .bars
            .values()
            .find(|bar| bar.output == output)
            .map(|bar| bar.id)
            .or_else(|| {
                self.shell
                    .active_bar
                    .filter(|id| self.shell.bar(*id).is_some())
            })
            .or(self.bar_window_id)
    }

    pub(crate) fn close_bar_dropdown(&mut self) -> Task<Message> {
        if self.control_center.attached_to.take().is_some() {
            self.control_center.window_id = None;
        }
        self.popup_open = false;
        Task::batch([
            self.close_session(),
            self.current_input_region_task(),
            self.shell
                .popup_bar
                .map(|id| {
                    Task::done(Message::SetToastKeyboardInteractivity(
                        id,
                        iced_layershell::reexport::KeyboardInteractivity::None,
                    ))
                })
                .unwrap_or_else(Task::none),
        ])
    }

    pub(crate) fn overlay_output(&self) -> Option<String> {
        if let Some(output) = &self.shell.command_output {
            return output.clone();
        }
        self.shell
            .request_source
            .and_then(|id| self.shell.registry.get(id))
            .and_then(|record| record.output.clone())
            .or_else(crate::monitor::active_output_name)
            .or_else(|| {
                self.shell
                    .inventory
                    .as_ref()
                    .and_then(OutputSnapshot::focused)
                    .map(|output| output.name.clone())
            })
            .or_else(|| self.output_for_bar(self.shell.active_bar))
    }

    pub(crate) fn register_overlay(
        &mut self,
        id: IcedId,
        kind: SurfaceKind,
        owner: Option<IcedId>,
        output: Option<String>,
    ) {
        let owner = owner
            .filter(|id| self.output_for_bar(Some(*id)) == output)
            .or_else(|| {
                self.shell
                    .bars
                    .values()
                    .find(|bar| bar.output == output)
                    .map(|bar| bar.id)
            });
        self.shell.registry.register(id, kind, output, owner);
    }

    fn close_owned_surfaces(&mut self, owner: IcedId, output: Option<&str>) -> Task<Message> {
        let output_present = self.shell.inventory.as_ref().is_none_or(|inventory| {
            output.is_none()
                || inventory
                    .outputs
                    .iter()
                    .any(|item| item.enabled && Some(item.name.as_str()) == output)
        });
        if output_present {
            self.shell.registry.detach_owner(owner);
            if self.shell.popup_bar == Some(owner) {
                if self.control_center.attached_to == Some(owner) {
                    self.control_center.attached_to = None;
                    self.control_center.window_id = None;
                }
                self.popup_open = false;
                self.shell.popup_bar = None;
            }
            return Task::none();
        }
        let ids: Vec<_> = self
            .shell
            .registry
            .iter()
            .filter(|(_, record)| {
                !matches!(record.kind, SurfaceKind::Bar(_))
                    && (record.owner_bar == Some(owner)
                        || output.is_some() && record.output.as_deref() == output)
            })
            .map(|(id, _)| *id)
            .collect();
        let tasks: Vec<_> = ids
            .into_iter()
            .map(|id| {
                Task::batch([
                    self.forget_surface(id),
                    Task::done(Message::RemoveSurface(id)),
                ])
            })
            .collect();
        if self.shell.popup_bar == Some(owner) {
            if self.control_center.attached_to == Some(owner) {
                self.control_center.attached_to = None;
                self.control_center.window_id = None;
            }
            self.popup_open = false;
            self.shell.popup_bar = None;
        }
        Task::batch(tasks)
    }

    pub(crate) fn forget_surface(&mut self, id: IcedId) -> Task<Message> {
        let mut callback = Task::none();
        self.shell.registry.remove(id);
        if self.shell.session_window == Some(id) {
            self.shell.session_window = None;
        }
        if self.launcher.window_id == Some(id) {
            self.launcher.window_id = None;
        }
        if self.settings.window_id == Some(id) {
            self.settings.window_id = None;
        }
        if self.control_center.window_id == Some(id) {
            self.control_center.window_id = None;
            self.control_center.attached_to = None;
        }
        if self.modal_window_id == Some(id) {
            self.modal_window_id = None;
            let plugin = self.modal_plugin.take();
            callback = self.modal_close_callback(plugin.as_deref(), id);
        }
        if self.panel_window_id == Some(id) {
            self.panel_window_id = None;
            self.panel_plugin = None;
        }
        self.toasts.retain(|toast| toast.window_id != id);
        self.osds.instances.retain(|_, instance| instance.id != id);
        self.docks.instances.retain(|_, instance| instance.id != id);
        self.desktop.forget(id);
        self.overview.surfaces.retain(|_, surface| surface.id != id);
        callback
    }

    pub(crate) fn source_message(&mut self, source: IcedId, message: Message) -> Task<Message> {
        let Some(record) = self.shell.registry.get(source) else {
            return Task::none();
        };
        let owner = record
            .owner_bar
            .or_else(|| matches!(record.kind, SurfaceKind::Bar(_)).then_some(source));
        if let Some(owner) = owner {
            self.shell.active_bar = Some(owner);
        }
        if let Message::PluginSubMsg(plugin, _) = &message
            && let Some(owner) = owner
        {
            self.shell.plugin_origins.insert(plugin.clone(), owner);
        }
        let previous = self.shell.request_source.replace(source);
        let task = self.update(message);
        self.shell.request_source = previous;
        task
    }

    pub(crate) fn empty_surface(&self) -> iced::Element<'_, Message> {
        Space::new().into()
    }

    pub(crate) fn surface_closed(&mut self, id: IcedId) -> Task<Message> {
        if let Some(task) = self.overview_surface_retired(id) {
            return task;
        }
        let kind = self
            .shell
            .registry
            .get(id)
            .map(|record| record.kind.clone());
        let is_bar = self.shell.bar(id).is_some();
        let callback = self.forget_surface(id);
        if is_bar {
            self.shell.bars.retain(|_, bar| bar.id != id);
            self.bar_window_id = None;
            return Task::batch([callback, self.reconcile_bars()]);
        }
        match kind {
            Some(SurfaceKind::Overview) => Task::batch([callback, self.close_overview()]),
            Some(SurfaceKind::DesktopEditor) => Task::batch([callback, self.cancel_desktop_edit()]),
            Some(SurfaceKind::DesktopWidget(_)) => {
                Task::batch([callback, self.reconcile_desktop()])
            }
            Some(SurfaceKind::Dock) => Task::batch([callback, self.reconcile_docks()]),
            _ => callback,
        }
    }
}
