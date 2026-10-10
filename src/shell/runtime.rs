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
                    || bar.config.height != config.height
                    || bar.config.position != config.position;
                bar.config = config;
                bar.size.height = size.height;
                if bar.config.width.is_some() {
                    bar.size.width = size.width;
                }
                let id = bar.id;
                if geometry {
                    tasks.push(Task::done(Message::ConfigureBar(id, bar.config.clone())));
                    tasks.push(Task::done(Message::SetExclusiveZone(
                        id,
                        bar.config.height as i32,
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
            width: metrics.map(|metrics| metrics.connector_width).unwrap_or(0),
            height: metrics.map(|metrics| metrics.height).unwrap_or(0),
            bar_size: bar.size,
            bottom: bar.config.position == crate::config::bars::Position::Bottom,
        })
    }

    pub(crate) fn plugin_section_on(&self, id: IcedId, plugin: &str) -> BarSection {
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

    pub(crate) fn overlay_output(&self) -> Option<String> {
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
            self.popup_open = false;
            self.shell.popup_bar = None;
        }
        Task::batch(tasks)
    }

    pub(crate) fn forget_surface(&mut self, id: IcedId) -> Task<Message> {
        let mut callback = Task::none();
        self.shell.registry.remove(id);
        if self.launcher.window_id == Some(id) {
            self.launcher.window_id = None;
        }
        if self.settings.window_id == Some(id) {
            self.settings.window_id = None;
        }
        if self.control_center.window_id == Some(id) {
            self.control_center.window_id = None;
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
        let is_bar = self.shell.bar(id).is_some();
        let callback = self.forget_surface(id);
        if is_bar {
            self.shell.bars.retain(|_, bar| bar.id != id);
            self.bar_window_id = None;
            return Task::batch([callback, self.reconcile_bars()]);
        }
        callback
    }
}
