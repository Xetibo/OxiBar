use super::{
    OutputSnapshot,
    configuration::{DisplayConfig, ModeChoice},
    controller::{Controller, Event, Preview},
};
use crate::{app::OxiBar, config::store::Mutation, messages::Message, settings::ConfigOutcome};
use iced::futures::StreamExt;
use iced::{
    Element, Length, Task,
    widget::{Column, Row, Space},
};
use oxiced::{
    theme::theme_impl::OXITHEME,
    widgets::{
        oxi_picklist,
        oxi_plugin::{self, text},
        oxi_text_input, oxi_toggler,
    },
};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Debug, Clone)]
pub enum Action {
    Refresh,
    Edit(String, String, String),
    Position(String, (i32, i32)),
    DiagramBounds(iced::Rectangle),
    Mode(String, ModeChoice),
    Rotation(String, String),
    Enable(String, bool),
    Vrr(String, bool),
    Preview,
    Keep,
    Revert,
    ApplySaved,
}
#[derive(Default)]
pub(crate) struct Ui {
    pub controller: Option<Controller>,
    pub inventory: Option<OutputSnapshot>,
    pub draft: Vec<DisplayConfig>,
    pub fields: BTreeMap<(String, String), String>,
    pub revision: u64,
    pub preview: Option<u64>,
    pub busy: bool,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub diagram_bounds: Option<iced::Rectangle>,
}

impl OxiBar {
    pub(crate) fn update_monitors(&mut self, action: Action) -> Task<Message> {
        if let Action::DiagramBounds(bounds) = action {
            if [bounds.x, bounds.y, bounds.width, bounds.height]
                .into_iter()
                .all(f32::is_finite)
            {
                self.settings.monitors.diagram_bounds = Some(bounds);
            }
            return Task::none();
        }
        if self.settings.monitors.busy {
            return Task::none();
        }
        if self.settings.monitors.controller.is_none() {
            match Controller::new() {
                Ok(controller) => self.settings.monitors.controller = Some(controller),
                Err(error) => {
                    self.settings.monitors.error = Some(error);
                    return Task::none();
                }
            }
        }
        let controller = self
            .settings
            .monitors
            .controller
            .as_ref()
            .expect("display controller")
            .clone();
        match action {
            Action::DiagramBounds(_) => {
                unreachable!("diagram bounds handled before the controller")
            }
            Action::Refresh => {
                if self.settings.monitors.preview.is_some() {
                    return Task::none();
                }
                self.settings.monitors.busy = true;
                Task::perform(controller.discover(), Message::MonitorsLoaded)
            }
            Action::ApplySaved => {
                if self.settings.monitors.preview.is_some() {
                    return Task::none();
                }
                match super::configuration::parse(
                    self.config
                        .snapshot
                        .effective
                        .get("monitor")
                        .and_then(|value| value.get("profiles")),
                ) {
                    Ok(config) if !config.is_empty() => {
                        self.settings.monitors.busy = true;
                        Task::perform(controller.saved(config), Message::MonitorsLoaded)
                    }
                    Ok(_) => Task::none(),
                    Err(error) => {
                        self.settings.monitors.error = Some(error);
                        Task::none()
                    }
                }
            }
            Action::Preview => {
                if self.settings.monitors.preview.is_some() {
                    return Task::none();
                }
                if let Err(error) = self.validate_monitor_fields() {
                    self.settings.monitors.error = Some(error);
                    return Task::none();
                }
                self.settings.monitors.busy = true;
                self.settings.monitors.error = None;
                Task::perform(
                    controller.preview(self.settings.monitors.draft.clone()),
                    Message::MonitorsPreviewed,
                )
            }
            Action::Revert => {
                let Some(id) = self.settings.monitors.preview.take() else {
                    return Task::none();
                };
                self.settings.monitors.busy = true;
                Task::perform(controller.revert(id), Message::MonitorsReverted)
            }
            Action::Keep => {
                let Some(id) = self.settings.monitors.preview else {
                    return Task::none();
                };
                if self.config.jobs > 0 {
                    return Task::none();
                }
                let store = self.config.store.clone();
                let revision = self.settings.monitors.revision;
                let mut profiles = Vec::new();
                let previous = self
                    .config
                    .snapshot
                    .effective
                    .get("monitor")
                    .and_then(|value| value.get("profiles"))
                    .and_then(toml::Value::as_array);
                for output in &self.settings.monitors.draft {
                    let mut table = previous
                        .and_then(|values| {
                            values.iter().find(|value| {
                                value.get("name").and_then(toml::Value::as_str)
                                    == Some(&output.name)
                            })
                        })
                        .and_then(toml::Value::as_table)
                        .cloned()
                        .unwrap_or_default();
                    if output.vrr.is_none() {
                        table.remove("vrr");
                    }
                    table.extend(
                        output
                            .value()
                            .as_table()
                            .expect("display configuration")
                            .clone(),
                    );
                    profiles.push(toml::Value::Table(table));
                }
                self.settings.monitors.busy = true;
                self.config.jobs += 1;
                Task::perform(
                    controller.keep(
                        id,
                        Box::new(move || {
                            store
                                .lock()
                                .map_err(|_| "Configuration lock poisoned".to_owned())?
                                .transact(
                                    Mutation::Set(
                                        "monitor.profiles".into(),
                                        toml::Value::Array(profiles),
                                    ),
                                    Some(revision),
                                )
                        }),
                    ),
                    Message::MonitorsSaved,
                )
            }
            action => {
                if self.settings.monitors.preview.is_some() {
                    return Task::none();
                }
                let name = match &action {
                    Action::Edit(name, ..)
                    | Action::Position(name, _)
                    | Action::Mode(name, _)
                    | Action::Rotation(name, _)
                    | Action::Enable(name, _)
                    | Action::Vrr(name, _) => name,
                    _ => return Task::none(),
                };
                let Some(output) = self
                    .settings
                    .monitors
                    .draft
                    .iter_mut()
                    .find(|output| &output.name == name)
                else {
                    return Task::none();
                };
                match action {
                    Action::Edit(name, field, text) => {
                        match field.as_str() {
                            "scale" => {
                                if let Ok(value) = text.parse::<f64>()
                                    && value.is_finite()
                                    && (0.25..=8.0).contains(&value)
                                {
                                    output.scale = value;
                                }
                            }
                            "x" => {
                                if let Ok(value) = text.parse::<i32>() {
                                    output.position.0 = value;
                                }
                            }
                            "y" => {
                                if let Ok(value) = text.parse::<i32>() {
                                    output.position.1 = value;
                                }
                            }
                            _ => {}
                        }
                        self.settings.monitors.fields.insert((name, field), text);
                    }
                    Action::Mode(_, choice) => output.mode = choice.0,
                    Action::Position(name, position) => {
                        output.position = position;
                        self.settings
                            .monitors
                            .fields
                            .insert((name.clone(), "x".into()), position.0.to_string());
                        self.settings
                            .monitors
                            .fields
                            .insert((name, "y".into()), position.1.to_string());
                    }
                    Action::Rotation(_, rotation) => {
                        output.transform = ROTATIONS
                            .iter()
                            .position(|value| *value == rotation)
                            .unwrap_or(0) as u32
                    }
                    Action::Enable(_, enabled) => output.enabled = enabled,
                    Action::Vrr(_, vrr) => output.vrr = Some(vrr),
                    _ => {}
                }
                Task::none()
            }
        }
    }
    fn validate_monitor_fields(&mut self) -> Result<(), String> {
        for output in &mut self.settings.monitors.draft {
            for field in ["scale", "x", "y"] {
                if let Some(text) = self
                    .settings
                    .monitors
                    .fields
                    .get(&(output.name.clone(), field.into()))
                {
                    match field {
                        "scale" => {
                            output.scale = text
                                .parse()
                                .map_err(|_| format!("{}: enter a valid scale", output.name))?
                        }
                        "x" => {
                            output.position.0 = text.parse().map_err(|_| {
                                format!("{}: enter an integer X position", output.name)
                            })?
                        }
                        _ => {
                            output.position.1 = text.parse().map_err(|_| {
                                format!("{}: enter an integer Y position", output.name)
                            })?
                        }
                    }
                }
            }
        }
        super::configuration::validate(&self.settings.monitors.draft)
    }
    pub(crate) fn monitors_loaded(
        &mut self,
        result: Result<OutputSnapshot, String>,
    ) -> Task<Message> {
        let ui = &mut self.settings.monitors;
        ui.busy = false;
        match result {
            Ok(inventory) => {
                ui.draft = inventory.outputs.iter().map(DisplayConfig::from).collect();
                ui.inventory = Some(inventory.clone());
                ui.fields.clear();
                ui.revision = self.config.snapshot.revision;
                ui.error = None;
                self.outputs_changed(Ok(inventory))
            }
            Err(error) => {
                ui.error = Some(error);
                Task::none()
            }
        }
    }
    pub(crate) fn monitors_previewed(&mut self, result: Result<Preview, String>) -> Task<Message> {
        self.settings.monitors.busy = false;
        match result {
            Ok(preview) => {
                self.settings.monitors.preview = Some(preview.id);
                self.settings.monitors.notice = Some(format!(
                    "Keep this layout? It automatically reverts after {} seconds, even if settings closes.",
                    preview.seconds
                ));
                self.outputs_changed(Ok(preview.inventory))
            }
            Err(error) => {
                self.settings.monitors.error = Some(error);
                Task::none()
            }
        }
    }
    pub(crate) fn monitors_reverted(
        &mut self,
        result: Result<OutputSnapshot, String>,
    ) -> Task<Message> {
        self.settings.monitors.preview = None;
        self.settings.monitors.notice = Some("Previous display layout restored.".into());
        self.monitors_loaded(result)
    }
    pub(crate) fn monitors_expired(&mut self, event: Event) -> Task<Message> {
        if self.settings.monitors.preview != Some(event.id) {
            return Task::none();
        }
        self.monitors_reverted(event.result)
    }
    pub(crate) fn monitors_saved(
        &mut self,
        result: Result<Arc<crate::config::ConfigSnapshot>, String>,
    ) -> Task<Message> {
        self.settings.monitors.preview = None;
        self.settings.monitors.busy = false;
        self.settings.monitors.notice = result
            .as_ref()
            .ok()
            .map(|_| "Display layout saved as a sparse GUI override.".into());
        self.settings.monitors.error = result.as_ref().err().cloned();
        let outcome = ConfigOutcome {
            snapshot: result
                .clone()
                .unwrap_or_else(|_| self.config.snapshot.clone()),
            error: result.err(),
            reply: None,
            edited_path: Some("monitor.profiles".into()),
            clear_editor: false,
        };
        self.config_updated(&outcome)
    }
    pub(crate) fn monitor_controls_view(&self) -> Element<'_, Message> {
        let ui = &self.settings.monitors;
        let disabled = ui.busy || ui.preview.is_some();
        let mut body = Column::new().spacing(OXITHEME.padding_lg);
        let action = |label: &str, action, enabled: bool| {
            let key = match &action {
                Action::Preview => "preview",
                Action::Keep => "keep",
                Action::Revert => "revert",
                Action::Refresh => "refresh",
                Action::ApplySaved => "apply-saved",
                _ => "action",
            };
            crate::settings::inspected(
                format!("@monitor/{key}"),
                oxi_plugin::surface_button(text(label.to_owned()), false)
                    .on_press_maybe(enabled.then_some(Message::Monitor(action))),
            )
        };
        let mut toolbar = Row::new()
            .push(
                text(
                    ui.inventory
                        .as_ref()
                        .map(|inventory| inventory.backend.as_str())
                        .unwrap_or("Display configuration"),
                )
                .size(OXITHEME.font_lg),
            )
            .push(Space::new().width(Length::Fill))
            .spacing(OXITHEME.padding_md);
        if ui.preview.is_some() {
            toolbar = toolbar
                .push(action("Revert", Action::Revert, !ui.busy))
                .push(action(
                    "Keep & save",
                    Action::Keep,
                    !ui.busy && self.config.jobs == 0,
                ));
        } else {
            toolbar = toolbar
                .push(action("Refresh", Action::Refresh, !ui.busy))
                .push(action("Apply saved", Action::ApplySaved, !ui.busy))
                .push(action(
                    "Test layout",
                    Action::Preview,
                    !ui.busy && !ui.draft.is_empty(),
                ));
        }
        body = body.push(oxi_plugin::settings_card(toolbar));
        if !ui.draft.is_empty() {
            body = body.push(oxi_plugin::settings_card(
                Column::new()
                    .push(
                        text("Drag to snap nearby edges and align displays. Overlapping drops restore the original position; numeric coordinates stay exact.")
                            .style(oxi_plugin::text_muted),
                    )
                    .push(
                        oxiced::widgets::oxi_drag_handle::capture_visible_bounds(iced::widget::Canvas::new(super::diagram::Diagram {
                            outputs: &ui.draft,
                            enabled: !disabled,
                        })
                        .width(Length::Fill)
                        .height(220), |bounds, _| Message::Monitor(Action::DiagramBounds(bounds))),
                    )
                    .spacing(OXITHEME.padding_md),
            ));
        }
        if let Some(notice) = &ui.notice {
            body = body.push(text(notice).style(oxi_plugin::text_accent));
        }
        if let Some(error) = &ui.error {
            body = body.push(text(error).style(oxi_plugin::text_muted));
        }
        for output in &ui.draft {
            let name = output.name.clone();
            let current = ui.inventory.as_ref().and_then(|inventory| {
                inventory
                    .outputs
                    .iter()
                    .find(|current| current.name == name)
            });
            let mut modes: Vec<_> = current
                .map(|current| current.modes.iter().cloned().map(ModeChoice).collect())
                .unwrap_or_default();
            if !modes.iter().any(|mode| mode.0 == output.mode) {
                modes.push(ModeChoice(output.mode.clone()));
            }
            let mut controls = Column::new()
                .spacing(OXITHEME.padding_lg)
                .push(
                    Row::new()
                        .push(text(&output.name).size(OXITHEME.font_lg))
                        .push(Space::new().width(Length::Fill))
                        .push(
                            oxi_toggler::toggler(output.enabled)
                                .label("Enabled")
                                .on_toggle_maybe((!disabled).then_some(move |enabled| {
                                    Message::Monitor(Action::Enable(name.clone(), enabled))
                                })),
                        )
                        .spacing(OXITHEME.padding_md),
                )
                .push(
                    text(
                        current
                            .map(|current| current.description.as_str())
                            .unwrap_or_default(),
                    )
                    .size(OXITHEME.font_md)
                    .style(oxi_plugin::text_muted),
                );
            let name = output.name.clone();
            controls = controls.push(
                oxi_picklist::pick_list(
                    modes,
                    Some(ModeChoice(output.mode.clone())),
                    move |choice| {
                        if disabled {
                            Message::Ignore
                        } else {
                            Message::Monitor(Action::Mode(name.clone(), choice))
                        }
                    },
                )
                .width(Length::Fill),
            );
            let name = output.name.clone();
            controls = controls.push(
                oxi_picklist::pick_list(
                    ROTATIONS.map(str::to_owned),
                    Some(ROTATIONS[output.transform.min(7) as usize].to_owned()),
                    move |rotation| {
                        if disabled {
                            Message::Ignore
                        } else {
                            Message::Monitor(Action::Rotation(name.clone(), rotation))
                        }
                    },
                )
                .width(Length::Fill),
            );
            let mut coordinates = Row::new().spacing(OXITHEME.padding_lg);
            for (field, default) in [
                ("scale", output.scale.to_string()),
                ("x", output.position.0.to_string()),
                ("y", output.position.1.to_string()),
            ] {
                let name = output.name.clone();
                let value = ui
                    .fields
                    .get(&(name.clone(), field.into()))
                    .cloned()
                    .unwrap_or(default);
                let input = oxi_text_input::text_input(field, &value, |_| Message::Ignore)
                    .on_input_maybe((!disabled).then_some(move |text| {
                        Message::Monitor(Action::Edit(name.clone(), field.into(), text))
                    }));
                coordinates = coordinates.push(
                    Column::new()
                        .push(
                            text(if field == "scale" {
                                "Scale"
                            } else if field == "x" {
                                "X position"
                            } else {
                                "Y position"
                            })
                            .size(OXITHEME.font_md),
                        )
                        .push(input)
                        .spacing(OXITHEME.padding_sm)
                        .width(Length::Fill),
                );
            }
            controls = controls.push(coordinates);
            if let Some(vrr) = output.vrr {
                let name = output.name.clone();
                controls = controls.push(
                    oxi_toggler::toggler(vrr)
                        .label("Adaptive sync / VRR")
                        .on_toggle_maybe((!disabled).then_some(move |enabled| {
                            Message::Monitor(Action::Vrr(name.clone(), enabled))
                        })),
                );
            }
            body = body.push(oxi_plugin::settings_card(controls));
        }
        body.into()
    }

    pub(crate) fn monitor_diagram_status(&self) -> serde_json::Value {
        let ui = &self.settings.monitors;
        serde_json::json!(ui.diagram_bounds.filter(|bounds| bounds.width > 0.0 && bounds.height > 0.0)
            .map(|bounds| super::diagram::rectangles(&ui.draft, bounds).into_iter().map(|(name, rectangle)| {
                serde_json::json!({"name":name,"x":rectangle.x,"y":rectangle.y,"width":rectangle.width,"height":rectangle.height})
            }).collect::<Vec<_>>()).unwrap_or_default())
    }
}
const ROTATIONS: [&str; 8] = [
    "Normal",
    "90°",
    "180°",
    "270°",
    "Flipped",
    "Flipped 90°",
    "Flipped 180°",
    "Flipped 270°",
];
pub(crate) fn events(
    controller: &Controller,
) -> iced::futures::stream::BoxStream<'static, Message> {
    controller.events().map(Message::MonitorsExpired).boxed()
}
