use std::collections::BTreeMap;

use iced::{
    Element, Length, Task,
    widget::{Column, container},
};
use iced_layershell::reexport::IcedId;
use oxibar_plugin_api::HostOsdRequest;
use oxiced::theme::theme_impl::OXITHEME;

use crate::{app::OxiBar, messages::Message, shell::SurfaceKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OsdConfig {
    pub enabled: bool,
    pub size: (u32, u32),
    pub bottom: bool,
    pub margin: u32,
    pub timeout_ms: u32,
}

#[derive(Debug)]
pub(crate) struct Instance {
    pub id: IcedId,
    pub plugin: String,
    pub topic: String,
    pub generation: u64,
    pub timer: Option<iced::task::Handle>,
}

#[derive(Debug)]
pub(crate) struct Osds {
    pub instances: BTreeMap<Option<String>, Instance>,
    pub timers_enabled: bool,
    generation: u64,
}

impl Default for Osds {
    fn default() -> Self {
        Self {
            instances: BTreeMap::new(),
            timers_enabled: true,
            generation: 0,
        }
    }
}

impl Osds {
    pub(crate) fn untimed() -> Self {
        Self {
            timers_enabled: false,
            ..Self::default()
        }
    }
}

impl OxiBar {
    pub(crate) fn request_osd(
        &mut self,
        plugin: String,
        request: HostOsdRequest,
    ) -> Result<Task<Message>, String> {
        let plugin = self
            .plugins
            .keys()
            .find(|name| name.eq_ignore_ascii_case(&plugin))
            .cloned()
            .ok_or("Unknown OSD provider")?;
        let (model, funcs) = &self.plugins[&plugin];
        let view = funcs.osd_view.ok_or("Provider has no OSD view")?;
        if request.topic.is_empty() || request.topic.len() > 128 {
            return Err("Invalid OSD topic".to_owned());
        }
        if unsafe { view(model.clone(), &request.topic) }
            .map_err(|error| error.to_string())?
            .is_empty()
        {
            return Err("OSD content is unavailable".to_owned());
        }
        if !self.config.snapshot.host.osd.enabled {
            return Ok(Task::none());
        }
        let output = if request.follow_focus {
            let source = self.shell.request_source.take();
            let command = self.shell.command_output.take();
            let output = self.overlay_output();
            self.shell.request_source = source;
            self.shell.command_output = command;
            output
        } else {
            self.overlay_output()
        };
        if output.as_ref().is_some_and(|name| {
            self.shell.inventory.as_ref().is_some_and(|inventory| {
                !inventory
                    .outputs
                    .iter()
                    .any(|item| item.enabled && &item.name == name)
            })
        }) {
            return Ok(Task::none());
        }
        self.osds.generation = self.osds.generation.wrapping_add(1);
        let generation = self.osds.generation;
        let mut tasks = Vec::new();
        let id = if let Some(instance) = self.osds.instances.get_mut(&output) {
            drop(instance.timer.take());
            instance.plugin = plugin;
            instance.topic = request.topic;
            instance.generation = generation;
            instance.id
        } else {
            let id = IcedId::unique();
            self.register_overlay(id, SurfaceKind::Osd, self.shell.active_bar, output.clone());
            self.osds.instances.insert(
                output.clone(),
                Instance {
                    id,
                    plugin,
                    topic: request.topic,
                    generation,
                    timer: None,
                },
            );
            tasks.push(Task::done(Message::OnOutput(
                output.clone(),
                Box::new(Message::OpenOsdLayer(
                    id,
                    self.config.snapshot.host.osd.clone(),
                )),
            )));
            id
        };
        let (task, timer) = self.osd_timer(id, generation);
        self.osds.instances.get_mut(&output).unwrap().timer = timer;
        tasks.push(task);
        Ok(Task::batch(tasks))
    }

    fn osd_timer(
        &self,
        id: IcedId,
        generation: u64,
    ) -> (Task<Message>, Option<iced::task::Handle>) {
        if !self.osds.timers_enabled {
            return (Task::none(), None);
        }
        let duration =
            std::time::Duration::from_millis(self.config.snapshot.host.osd.timeout_ms.into());
        let (task, handle) = Task::perform(
            async move {
                tokio::time::sleep(duration).await;
            },
            move |_| Message::ExpireOsd(id, generation),
        )
        .abortable();
        (task, Some(handle.abort_on_drop()))
    }

    pub(crate) fn expire_osd(&mut self, id: IcedId, generation: u64) -> Task<Message> {
        if !self
            .osds
            .instances
            .values()
            .any(|item| item.id == id && item.generation == generation)
        {
            return Task::none();
        }
        self.shell.registry.remove(id);
        self.osds.instances.retain(|_, item| item.id != id);
        Task::done(Message::RemoveSurface(id))
    }

    pub(crate) fn close_osds(&mut self) -> Task<Message> {
        let ids: Vec<_> = self.osds.instances.values().map(|item| item.id).collect();
        self.osds.instances.clear();
        Task::batch(ids.into_iter().map(|id| {
            self.shell.registry.remove(id);
            Task::done(Message::RemoveSurface(id))
        }))
    }

    pub(crate) fn apply_osd_config(&mut self) -> Task<Message> {
        if !self.config.snapshot.host.osd.enabled {
            return self.close_osds();
        }
        let ids: Vec<_> = self
            .osds
            .instances
            .iter()
            .map(|(output, item)| (output.clone(), item.id))
            .collect();
        let mut tasks = Vec::new();
        for (output, id) in ids {
            self.osds.generation = self.osds.generation.wrapping_add(1);
            let generation = self.osds.generation;
            let (task, timer) = self.osd_timer(id, generation);
            let item = self.osds.instances.get_mut(&output).unwrap();
            item.timer = timer;
            item.generation = generation;
            let config = self.config.snapshot.host.osd.clone();
            tasks.extend([
                task,
                Task::done(Message::ConfigureOsdLayer(id, config.clone())),
                Task::done(Message::SetLayerMargin(
                    id,
                    crate::messages::osd_margin(&config),
                )),
            ]);
        }
        Task::batch(tasks)
    }

    pub(crate) fn osd_view(&self, id: IcedId) -> Element<'_, Message> {
        let Some(instance) = self.osds.instances.values().find(|item| item.id == id) else {
            return self.empty_surface();
        };
        let Some((model, funcs)) = self.plugins.get(&instance.plugin) else {
            return self.empty_surface();
        };
        let Some(view) = funcs.osd_view else {
            return self.empty_surface();
        };
        let content = unsafe { view(model.clone(), &instance.topic) }
            .unwrap_or_default()
            .into_iter()
            .map(|element| element.map(|_| Message::Ignore));
        container(
            Column::with_children(content)
                .spacing(OXITHEME.padding_sm)
                .width(Length::Fill),
        )
        .padding(OXITHEME.padding_lg)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_y(Length::Fill)
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
