use crate::{
    Message,
    app::OxiBar,
    config::{desktop::WidgetSpec, store::Mutation},
    messages::map_plugin_message,
    shell::SurfaceKind,
};
use iced::{
    Element, Length, Point, Task,
    widget::{Column, Row, Space, Stack, container},
};
use iced_layershell::reexport::IcedId;
use oxiced::widgets::oxi_plugin::text;
use oxiced::{
    theme::theme_impl::OXITHEME,
    widgets::{oxi_drag_handle::drag_handle, oxi_picklist, oxi_plugin},
};
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub enum Action {
    Edit,
    Save,
    Cancel,
    Select(String),
    Provider(String),
    Output(String),
    Add,
    Remove,
    Toggle,
    CycleInput,
    Reset,
    Start(String, bool),
    StartAt(String, bool, Point),
    Pointer(IcedId, Point),
    Release(IcedId),
    Nudge(IcedId, i32, i32, bool),
    Escape(IcedId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct Instance {
    pub id: IcedId,
    pub spec: WidgetSpec,
    pub output: Option<String>,
    pub rect: Rect,
    pub interactive: bool,
}

#[derive(Debug)]
pub(crate) struct Draft {
    pub revision: u64,
    pub widgets: Vec<WidgetSpec>,
    pub original: Vec<oxibar_plugin_api::toml::Value>,
    pub selected: Option<String>,
    pub provider: String,
    pub output: String,
    pub error: Option<String>,
    pub saving: bool,
}

#[derive(Debug)]
struct Gesture {
    surface: IcedId,
    widget: String,
    start: Point,
    rect: Rect,
    resize: bool,
}

#[derive(Debug, Default)]
pub(crate) struct Desktop {
    pub instances: BTreeMap<String, Instance>,
    pub editors: BTreeMap<Option<String>, (IcedId, (u32, u32))>,
    pub draft: Option<Draft>,
    cursor: BTreeMap<IcedId, Point>,
    gesture: Option<Gesture>,
}

impl Desktop {
    pub(crate) fn forget(&mut self, id: IcedId) {
        self.instances.retain(|_, instance| instance.id != id);
        self.editors.retain(|_, (editor, _)| *editor != id);
        self.cursor.remove(&id);
        if self
            .gesture
            .as_ref()
            .is_some_and(|gesture| gesture.surface == id)
        {
            self.gesture = None;
        }
    }
}

pub(crate) fn clamped(spec: &WidgetSpec, size: (u32, u32)) -> Rect {
    let width = spec.width.min(size.0.max(1));
    let height = spec.height.min(size.1.max(1));
    Rect {
        x: spec.x.min(size.0.saturating_sub(width)),
        y: spec.y.min(size.1.saturating_sub(height)),
        width,
        height,
    }
}

pub(crate) fn snapped(value: f32, grid: u32) -> u32 {
    if !value.is_finite() {
        return 0;
    }
    ((value.max(0.0) / grid.max(1) as f32).round() * grid.max(1) as f32).min(16384.0) as u32
}

impl OxiBar {
    fn widget_target(&self, spec: &WidgetSpec) -> Option<(Option<String>, (u32, u32))> {
        let targets = self.selected_outputs(
            if spec.output.is_empty() {
                &[]
            } else {
                std::slice::from_ref(&spec.output)
            },
            self.config.snapshot.host.desktop.fallback,
        );
        targets
            .into_iter()
            .next()
            .map(|(name, width, height)| (name, (width, height)))
    }

    pub(crate) fn desktop_providers(&self) -> Vec<String> {
        self.plugin_order
            .iter()
            .filter(|name| {
                self.plugins.get(*name).is_some_and(|(_, funcs)| {
                    funcs.desktop_view.is_some() || funcs.popup_view.is_some()
                })
            })
            .cloned()
            .collect()
    }

    pub(crate) fn reconcile_desktop(&mut self) -> Task<Message> {
        let mut tasks = Vec::new();
        if self.desktop.draft.is_some() {
            for (_, instance) in std::mem::take(&mut self.desktop.instances) {
                self.shell.registry.remove(instance.id);
                tasks.push(Task::done(Message::RemoveSurface(instance.id)));
            }
            let targets = self.selected_outputs(&["*".into()], true);
            let keep: Vec<_> = targets.iter().map(|(name, _, _)| name.clone()).collect();
            for (output, width, height) in targets {
                if let Some((id, size)) = self.desktop.editors.get_mut(&output) {
                    if *size != (width, height) {
                        *size = (width, height);
                        tasks.push(Task::done(Message::ConfigureDesktopLayer(
                            *id,
                            Rect {
                                x: 0,
                                y: 0,
                                width,
                                height,
                            },
                        )));
                    }
                } else {
                    let id = IcedId::unique();
                    self.register_overlay(
                        id,
                        SurfaceKind::DesktopEditor,
                        self.shell.active_bar,
                        output.clone(),
                    );
                    self.desktop
                        .editors
                        .insert(output.clone(), (id, (width, height)));
                    tasks.push(Task::done(Message::OnOutput(
                        output,
                        Box::new(Message::OpenDesktopLayer(
                            id,
                            Rect {
                                x: 0,
                                y: 0,
                                width,
                                height,
                            },
                            true,
                            true,
                        )),
                    )));
                }
            }
            let stale: Vec<_> = self
                .desktop
                .editors
                .keys()
                .filter(|name| !keep.contains(name))
                .cloned()
                .collect();
            for output in stale {
                if let Some((id, _)) = self.desktop.editors.remove(&output) {
                    self.shell.registry.remove(id);
                    tasks.push(Task::done(Message::RemoveSurface(id)));
                }
            }
            return Task::batch(tasks);
        }
        for (_, (id, _)) in std::mem::take(&mut self.desktop.editors) {
            self.shell.registry.remove(id);
            tasks.push(Task::done(Message::RemoveSurface(id)));
        }
        let mut keep = Vec::new();
        let specs = if self.config.snapshot.host.desktop.enabled {
            self.config.snapshot.host.desktop.widgets.clone()
        } else {
            Vec::new()
        };
        for spec in specs.into_iter().filter(|spec| spec.enabled) {
            let Some((output, size)) = self.widget_target(&spec) else {
                continue;
            };
            let rect = clamped(&spec, size);
            let interactive = spec.interactive.unwrap_or_else(|| {
                self.plugins
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case(&spec.plugin))
                    .map(|(_, (model, funcs))| {
                        funcs
                            .desktop_interactive
                            .map(|policy| unsafe { policy(model.clone()) })
                            .unwrap_or(true)
                    })
                    .unwrap_or(false)
            });
            keep.push(spec.id.clone());
            if let Some(instance) = self.desktop.instances.get(&spec.id)
                && (instance.output != output
                    || instance.spec.plugin != spec.plugin
                    || instance.interactive != interactive)
            {
                let id = instance.id;
                self.desktop.instances.remove(&spec.id);
                self.shell.registry.remove(id);
                tasks.push(Task::done(Message::RemoveSurface(id)));
            }
            if let Some(instance) = self.desktop.instances.get_mut(&spec.id) {
                if instance.rect != rect {
                    tasks.push(Task::done(Message::ConfigureDesktopLayer(
                        instance.id,
                        rect,
                    )));
                }
                instance.rect = rect;
                instance.spec = spec;
            } else {
                let id = IcedId::unique();
                self.register_overlay(
                    id,
                    SurfaceKind::DesktopWidget(spec.id.clone()),
                    self.shell.active_bar,
                    output.clone(),
                );
                self.desktop.instances.insert(
                    spec.id.clone(),
                    Instance {
                        id,
                        spec,
                        output: output.clone(),
                        rect,
                        interactive,
                    },
                );
                tasks.push(Task::done(Message::OnOutput(
                    output,
                    Box::new(Message::OpenDesktopLayer(id, rect, false, interactive)),
                )));
            }
        }
        let stale: Vec<_> = self
            .desktop
            .instances
            .keys()
            .filter(|id| !keep.contains(id))
            .cloned()
            .collect();
        for key in stale {
            if let Some(instance) = self.desktop.instances.remove(&key) {
                self.shell.registry.remove(instance.id);
                tasks.push(Task::done(Message::RemoveSurface(instance.id)));
            }
        }
        Task::batch(tasks)
    }

    pub(crate) fn cancel_desktop_edit(&mut self) -> Task<Message> {
        self.desktop.draft = None;
        self.desktop.gesture = None;
        self.desktop.cursor.clear();
        self.reconcile_desktop()
    }

    pub(crate) fn desktop_config_outcome(&mut self, outcome: &crate::settings::ConfigOutcome) {
        if outcome.edited_path.as_deref() != Some("desktop.widgets")
            || !self
                .desktop
                .draft
                .as_ref()
                .is_some_and(|draft| draft.saving)
        {
            return;
        }
        if let Some(error) = &outcome.error {
            if let Some(draft) = &mut self.desktop.draft {
                draft.saving = false;
                draft.error = Some(error.clone());
            }
        } else {
            self.desktop.draft = None;
            self.desktop.gesture = None;
            self.desktop.cursor.clear();
        }
    }

    pub(crate) fn update_desktop(&mut self, action: Action) -> Task<Message> {
        if matches!(action, Action::Edit) {
            if self.desktop.draft.is_none() {
                let widgets = self.config.snapshot.host.desktop.widgets.clone();
                let original = crate::config::store::get_path(
                    &self.config.snapshot.effective,
                    "desktop.widgets",
                )
                .and_then(|value| value.as_array())
                .cloned()
                .unwrap_or_default();
                let providers = self.desktop_providers();
                let provider = providers
                    .iter()
                    .find(|name| name.eq_ignore_ascii_case("Clock"))
                    .or_else(|| providers.first())
                    .cloned()
                    .unwrap_or_default();
                self.desktop.draft = Some(Draft {
                    revision: self.config.snapshot.revision,
                    selected: widgets.first().map(|spec| spec.id.clone()),
                    widgets,
                    original,
                    provider,
                    output: self.overlay_output().unwrap_or_default(),
                    error: None,
                    saving: false,
                });
            }
            let mut tasks = vec![
                self.close_settings(),
                self.close_launcher(),
                self.close_oxipaste(),
                self.close_control_center(),
                self.close_modal(),
                self.close_panel(),
            ];
            tasks.push(self.reconcile_desktop());
            return Task::batch(tasks);
        }
        if matches!(action, Action::Cancel) {
            return self.cancel_desktop_edit();
        }
        if let Action::Escape(id) = action {
            return if self
                .desktop
                .editors
                .values()
                .any(|(editor, _)| *editor == id)
            {
                self.cancel_desktop_edit()
            } else {
                Task::none()
            };
        }
        if let Action::Pointer(id, point) = action {
            if !self
                .desktop
                .editors
                .values()
                .any(|(editor, _)| *editor == id)
            {
                return Task::none();
            }
            self.desktop.cursor.insert(id, point);
            if let Some(gesture) = &self.desktop.gesture
                && gesture.surface == id
            {
                if self
                    .desktop
                    .draft
                    .as_ref()
                    .is_some_and(|draft| draft.saving)
                {
                    return Task::none();
                }
                let dx = point.x - gesture.start.x;
                let dy = point.y - gesture.start.y;
                let grid = self.config.snapshot.host.desktop.snap;
                let size = self
                    .desktop
                    .editors
                    .values()
                    .find(|(editor, _)| *editor == id)
                    .map(|(_, size)| *size)
                    .unwrap();
                if let Some(widget) = self.desktop.draft.as_mut().and_then(|draft| {
                    draft
                        .widgets
                        .iter_mut()
                        .find(|widget| widget.id == gesture.widget)
                }) {
                    if gesture.resize {
                        widget.width =
                            snapped(gesture.rect.width as f32 + dx, grid).clamp(64, size.0.max(64));
                        widget.height = snapped(gesture.rect.height as f32 + dy, grid)
                            .clamp(64, size.1.max(64));
                    } else {
                        widget.x = snapped(gesture.rect.x as f32 + dx, grid)
                            .min(size.0.saturating_sub(widget.width.min(size.0)));
                        widget.y = snapped(gesture.rect.y as f32 + dy, grid)
                            .min(size.1.saturating_sub(widget.height.min(size.1)));
                    }
                }
            }
            return Task::none();
        }
        if let Action::Release(id) = action {
            if self
                .desktop
                .gesture
                .as_ref()
                .is_some_and(|gesture| gesture.surface == id)
            {
                self.desktop.gesture = None;
            }
            return Task::none();
        }
        if let Action::StartAt(widget, resize, point) = action {
            if !point.x.is_finite() || !point.y.is_finite() {
                return Task::none();
            }
            let Some(source) = self.shell.request_source.filter(|id| {
                self.desktop
                    .editors
                    .values()
                    .any(|(editor, _)| editor == id)
            }) else {
                return Task::none();
            };
            self.desktop.cursor.insert(source, point);
            return self.update_desktop(Action::Start(widget, resize));
        }
        if let Action::Start(widget, resize) = action {
            if self.desktop.draft.as_ref().is_none_or(|draft| draft.saving) {
                return Task::none();
            }
            let Some(surface) = self.shell.request_source.filter(|id| {
                self.desktop
                    .editors
                    .values()
                    .any(|(editor, _)| editor == id)
            }) else {
                return Task::none();
            };
            let Some(spec) = self
                .desktop
                .draft
                .as_ref()
                .and_then(|draft| draft.widgets.iter().find(|spec| spec.id == widget))
                .cloned()
            else {
                return Task::none();
            };
            let size = self
                .desktop
                .editors
                .values()
                .find(|(id, _)| *id == surface)
                .map(|(_, size)| *size)
                .unwrap();
            self.desktop.gesture = Some(Gesture {
                surface,
                widget: widget.clone(),
                start: self
                    .desktop
                    .cursor
                    .get(&surface)
                    .copied()
                    .unwrap_or_default(),
                rect: clamped(&spec, size),
                resize,
            });
            if let Some(draft) = &mut self.desktop.draft {
                draft.selected = Some(widget);
            }
            return Task::none();
        }
        if let Action::Save = action {
            let Some(draft) = &mut self.desktop.draft else {
                return Task::none();
            };
            if draft.saving || self.config.jobs > 0 {
                return Task::none();
            }
            let values = draft
                .widgets
                .iter()
                .map(|spec| {
                    let mut table = draft
                        .original
                        .iter()
                        .find(|value| {
                            value.get("id").and_then(|value| value.as_str())
                                == Some(spec.id.as_str())
                        })
                        .and_then(|value| value.as_table())
                        .cloned()
                        .unwrap_or_default();
                    table.extend(spec.value().as_table().unwrap().clone());
                    if spec.interactive.is_none() {
                        table.remove("interactive");
                    }
                    oxibar_plugin_api::toml::Value::Table(table)
                })
                .collect();
            draft.saving = true;
            let revision = draft.revision;
            return self.config_job(
                Some(Mutation::Set(
                    "desktop.widgets".into(),
                    oxibar_plugin_api::toml::Value::Array(values),
                )),
                Some(revision),
                None,
                Some("desktop.widgets".into()),
                false,
            );
        }
        let providers = self.desktop_providers();
        let Some(draft) = &mut self.desktop.draft else {
            return Task::none();
        };
        if draft.saving {
            return Task::none();
        }
        match action {
            Action::Select(id) => {
                if let Some(widget) = draft.widgets.iter().find(|widget| widget.id == id) {
                    draft.output = widget.output.clone();
                    draft.selected = Some(id);
                }
            }
            Action::Provider(provider) => {
                if providers.contains(&provider) {
                    draft.provider = provider;
                }
            }
            Action::Output(output) => {
                draft.output = output.clone();
                if let Some(widget) = draft
                    .widgets
                    .iter_mut()
                    .find(|widget| Some(&widget.id) == draft.selected.as_ref())
                {
                    widget.output = output;
                }
            }
            Action::Add => {
                if !draft.provider.is_empty() && draft.widgets.len() < 32 {
                    let mut number = 1;
                    while draft
                        .widgets
                        .iter()
                        .any(|widget| widget.id == format!("widget-{number}"))
                    {
                        number += 1;
                    }
                    let id = format!("widget-{number}");
                    draft.widgets.push(WidgetSpec {
                        id: id.clone(),
                        plugin: draft.provider.clone(),
                        output: draft.output.clone(),
                        x: 32,
                        y: 128,
                        width: 360,
                        height: 360,
                        enabled: true,
                        interactive: None,
                    });
                    draft.selected = Some(id);
                }
            }
            Action::Remove => {
                draft
                    .widgets
                    .retain(|widget| Some(&widget.id) != draft.selected.as_ref());
                draft.selected = draft.widgets.first().map(|widget| widget.id.clone());
            }
            Action::Toggle | Action::CycleInput | Action::Reset | Action::Nudge(_, _, _, _) => {
                if let Some(widget) = draft
                    .widgets
                    .iter_mut()
                    .find(|widget| Some(&widget.id) == draft.selected.as_ref())
                {
                    match action {
                        Action::Toggle => widget.enabled = !widget.enabled,
                        Action::CycleInput => {
                            widget.interactive = match widget.interactive {
                                None => Some(true),
                                Some(true) => Some(false),
                                Some(false) => None,
                            }
                        }
                        Action::Reset => {
                            widget.x = 32;
                            widget.y = 128;
                            widget.width = 360;
                            widget.height = 360;
                        }
                        Action::Nudge(id, dx, dy, resize) => {
                            if self
                                .desktop
                                .editors
                                .values()
                                .any(|(editor, _)| *editor == id)
                            {
                                let step = self.config.snapshot.host.desktop.snap as i64;
                                if resize {
                                    widget.width = (i64::from(widget.width) + i64::from(dx) * step)
                                        .clamp(64, 16384)
                                        as u32;
                                    widget.height = (i64::from(widget.height)
                                        + i64::from(dy) * step)
                                        .clamp(64, 16384)
                                        as u32;
                                } else {
                                    widget.x = (i64::from(widget.x) + i64::from(dx) * step)
                                        .clamp(0, 16384)
                                        as u32;
                                    widget.y = (i64::from(widget.y) + i64::from(dy) * step)
                                        .clamp(0, 16384)
                                        as u32;
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        Task::none()
    }

    fn widget_content(
        &self,
        spec: &WidgetSpec,
        output: Option<&str>,
        small: bool,
    ) -> Element<'_, Message> {
        let Some((name, (model, funcs))) = self
            .plugins
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(&spec.plugin))
        else {
            return text(format!("{} is not running", spec.plugin))
                .style(oxi_plugin::text_muted)
                .into();
        };
        let context = oxibar_plugin_api::PluginViewContext {
            instance_id: format!("desktop:{}", spec.id),
            output_name: output.map(str::to_owned),
            output_scale: self
                .shell
                .inventory
                .as_ref()
                .and_then(|snapshot| {
                    snapshot
                        .outputs
                        .iter()
                        .find(|item| Some(item.name.as_str()) == output)
                })
                .map_or(1.0, |item| item.scale),
        };
        let elements = if small {
            unsafe { (funcs.view)(model.clone()) }
        } else if let Some(view) = funcs.desktop_view {
            unsafe { view(model.clone(), context) }
        } else if let Some(view) = funcs.popup_view {
            unsafe { view(model.clone()) }
        } else {
            unsafe { (funcs.view)(model.clone()) }
        };
        let name = name.clone();
        let mut body = Column::new().spacing(OXITHEME.padding_sm);
        match elements {
            Ok(elements) => {
                for element in elements {
                    let name = name.clone();
                    body = body.push(
                        element.map(move |message| map_plugin_message(name.clone(), message)),
                    );
                }
            }
            Err(error) => body = body.push(text(error.to_string()).style(oxi_plugin::text_muted)),
        }
        body.into()
    }

    pub(crate) fn desktop_view(&self, id: IcedId) -> Element<'_, Message> {
        let Some(instance) = self
            .desktop
            .instances
            .values()
            .find(|instance| instance.id == id)
        else {
            return Space::new().into();
        };
        widget_card(
            self.widget_content(
                &instance.spec,
                instance.output.as_deref(),
                instance.rect.width < 180,
            ),
            false,
        )
        .height(Length::Fill)
        .into()
    }

    pub(crate) fn desktop_editor_view(&self, id: IcedId) -> Element<'_, Message> {
        let Some((output, (_, size))) = self
            .desktop
            .editors
            .iter()
            .find(|(_, (editor, _))| *editor == id)
        else {
            return Space::new().into();
        };
        let Some(draft) = &self.desktop.draft else {
            return Space::new().into();
        };
        let mut stack = Stack::new().push(Space::new().width(Length::Fill).height(Length::Fill));
        for spec in &draft.widgets {
            if !spec.enabled
                || self
                    .widget_target(spec)
                    .is_none_or(|(target, _)| &target != output)
            {
                continue;
            }
            let rect = clamped(spec, *size);
            let selected = draft.selected.as_ref() == Some(&spec.id);
            let move_id = spec.id.clone();
            let move_handle = drag_handle(
                container(
                    text(format!("{} · {}", spec.id, spec.plugin))
                        .size(OXITHEME.font_md)
                        .style(oxi_plugin::text_accent),
                )
                .width(Length::Fill)
                .padding(OXITHEME.padding_sm),
                move |point| Message::Desktop(Action::StartAt(move_id.clone(), false, point)),
            );
            let resize_id = spec.id.clone();
            let resize = drag_handle(
                container(
                    text("↘")
                        .size(OXITHEME.font_lg)
                        .style(oxi_plugin::text_accent),
                )
                .padding(OXITHEME.padding_sm),
                move |point| Message::Desktop(Action::StartAt(resize_id.clone(), true, point)),
            );
            let content = self
                .widget_content(spec, output.as_deref(), rect.width < 180)
                .map(|_| Message::Ignore);
            let body = Column::new()
                .push(move_handle)
                .push(
                    container(content)
                        .height(Length::Fill)
                        .width(Length::Fill)
                        .clip(true),
                )
                .push(
                    Row::new()
                        .push(Space::new().width(Length::Fill))
                        .push(resize),
                );
            let preview = widget_card(body.into(), selected)
                .width(rect.width)
                .height(rect.height);
            stack = stack.push(
                container(preview)
                    .padding(iced::Padding {
                        top: rect.y as f32,
                        left: rect.x as f32,
                        right: 0.0,
                        bottom: 0.0,
                    })
                    .width(Length::Fill)
                    .height(Length::Fill),
            );
        }
        let providers = self.desktop_providers();
        let outputs: Vec<_> = self
            .shell
            .inventory
            .as_ref()
            .into_iter()
            .flat_map(|snapshot| &snapshot.outputs)
            .filter(|output| output.enabled)
            .map(|output| output.name.clone())
            .collect();
        let selected: Vec<_> = draft
            .widgets
            .iter()
            .map(|widget| widget.id.clone())
            .collect();
        let controls=Column::new().spacing(OXITHEME.padding_md)
            .push(Row::new().spacing(OXITHEME.padding_md).align_y(iced::Alignment::Center).push(text("Edit desktop widgets").size(OXITHEME.font_xl).style(oxi_plugin::text_primary)).push(Space::new().width(Length::Fill)).push(editor_button("Save",Action::Save,!draft.saving,true)).push(editor_button("Cancel",Action::Cancel,true,false)))
            .push(Row::new().spacing(OXITHEME.padding_md).push(oxi_picklist::pick_list(providers,Some(draft.provider.clone()),|provider| Message::Desktop(Action::Provider(provider))).width(Length::Fill))
                .push(oxi_picklist::pick_list(outputs,Some(draft.output.clone()),|output|Message::Desktop(Action::Output(output))).width(Length::Fill)).push(editor_button("Add widget",Action::Add,!draft.saving,false)))
            .push(Row::new().spacing(OXITHEME.padding_md).push(oxi_picklist::pick_list(selected,draft.selected.clone(),|id|Message::Desktop(Action::Select(id))).width(Length::Fill))
                .push(editor_button("Show / hide",Action::Toggle,draft.selected.is_some(),false)).push(editor_button("Remove",Action::Remove,draft.selected.is_some(),false)).push(editor_button("Reset placement",Action::Reset,draft.selected.is_some(),false)))
            .push(editor_button(&format!("Pointer input: {}", draft.widgets.iter().find(|widget| Some(&widget.id) == draft.selected.as_ref()).and_then(|widget| widget.interactive).map(|value| if value { "interactive" } else { "pass through" }).unwrap_or("automatic")), Action::CycleInput, draft.selected.is_some(), false))
            .push(text(draft.error.as_deref().unwrap_or("Drag the title to move, corner to resize. Arrows move; Shift + arrows resize. Escape cancels.")).size(OXITHEME.font_md).style(oxi_plugin::text_muted));
        stack
            .push(
                Column::new().push(Space::new().height(Length::Fill)).push(
                    container(widget_card(
                        oxiced::widgets::oxi_navigation::navigation(
                            controls,
                            self.config.snapshot.host.shortcuts.navigation(),
                        )
                        .focus_on_direction(false)
                        .into(),
                        false,
                    ))
                    .padding(OXITHEME.padding_lg)
                    .width(Length::Fill),
                ),
            )
            .into()
    }
}

fn widget_card<'a>(
    content: Element<'a, Message>,
    selected: bool,
) -> iced::widget::Container<'a, Message> {
    container(content)
        .padding(OXITHEME.padding_lg)
        .width(Length::Fill)
        .clip(true)
        .style(move |_| container::Style {
            background: Some(OXITHEME.mantle.into()),
            text_color: Some(OXITHEME.text),
            border: iced::Border {
                color: if selected {
                    OXITHEME.primary
                } else {
                    OXITHEME.secondary_bg
                },
                width: if selected { 2.0 } else { 0.0 },
                radius: OXITHEME.border_radius.into(),
            },
            ..Default::default()
        })
}

fn editor_button(
    label: &str,
    action: Action,
    enabled: bool,
    primary: bool,
) -> Element<'static, Message> {
    let button = oxi_plugin::surface_button(text(label.to_owned()).size(OXITHEME.font_md), primary);
    if enabled {
        button.on_press(Message::Desktop(action)).into()
    } else {
        button.into()
    }
}

pub(crate) fn event(event: iced::Event, _: iced::event::Status, id: IcedId) -> Option<Message> {
    use iced::mouse;
    let action = match event {
        iced::Event::Mouse(mouse::Event::CursorMoved { position }) => Action::Pointer(id, position),
        iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
            Action::Release(id)
        }
        _ => return None,
    };
    Some(Message::Desktop(action))
}
