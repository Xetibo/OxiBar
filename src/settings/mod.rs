use std::collections::BTreeMap;

use iced::{
    Alignment, Element, Length,
    widget::{Column, Row, Space, container, scrollable, text, text_editor},
};
use iced_layershell::reexport::IcedId;
use oxibar_plugin_api::{SettingDescriptor, SettingKind};
use oxiced::{
    theme::theme_impl::OXITHEME,
    widgets::{oxi_checkbox, oxi_picklist, oxi_plugin, oxi_text_input},
};
use toml::Value;

use crate::{
    app::OxiBar,
    config::{ConfigSnapshot, store::get_path},
    messages::Message,
};

pub(crate) mod runtime;
pub use runtime::ConfigOutcome;
pub(crate) use runtime::ConfigRuntime;

pub(crate) const SEARCH_ID: &str = "oxibar-settings-search";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Appearance,
    Bar,
    Launcher,
    Plugins,
    Advanced,
}

impl Category {
    const ALL: [Self; 5] = [
        Self::Appearance,
        Self::Bar,
        Self::Launcher,
        Self::Plugins,
        Self::Advanced,
    ];
    fn label(self) -> &'static str {
        match self {
            Self::Appearance => "Appearance",
            Self::Bar => "Bar / layout",
            Self::Launcher => "Launcher",
            Self::Plugins => "Plugins",
            Self::Advanced => "Advanced",
        }
    }
    fn accepts(self, path: &str) -> bool {
        match self {
            Self::Appearance => path.starts_with("bar.font") || path == "bar.transparent",
            Self::Bar => path.starts_with("bar.") && !Self::Appearance.accepts(path),
            Self::Launcher => path.starts_with("launcher.") || path.starts_with("applications."),
            Self::Plugins => {
                ![
                    "bar.",
                    "launcher.",
                    "applications.",
                    "settings.",
                    "instance.",
                    "ipc.",
                    "startup.",
                ]
                .iter()
                .any(|prefix| path.starts_with(prefix))
                    && path != "plugins"
            }
            Self::Advanced => ["settings.", "instance.", "ipc.", "startup."]
                .iter()
                .any(|prefix| path.starts_with(prefix)),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Action {
    Open,
    Toggle,
    Close,
    Category(Category),
    Search(String),
    Edit(String, String),
    Apply(String),
    Set(String, Value),
    Reset(String),
    ResetAll,
    Reload,
    SelectWidget(String, String),
    AddWidget(String),
    RemoveWidget(String, usize),
    MoveWidget(String, usize, bool),
    EnablePlugin(String, bool),
    Advanced(text_editor::Action),
    SaveAdvanced,
}

#[derive(Debug, Clone)]
pub(crate) struct Draft {
    pub value: String,
    pub revision: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct PluginStatus {
    pub filename: String,
    pub label: String,
    pub state: String,
}

pub(crate) struct SettingsUi {
    pub window_id: Option<IcedId>,
    pub awaiting_focus: bool,
    pub category: Category,
    pub search: String,
    pub drafts: BTreeMap<String, Draft>,
    pub widget_choice: BTreeMap<String, String>,
    pub editor: text_editor::Content,
    pub editor_revision: u64,
    pub editor_dirty: bool,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub catalog: Vec<PluginStatus>,
}

impl SettingsUi {
    pub(crate) fn new(snapshot: &ConfigSnapshot) -> Self {
        Self {
            window_id: None,
            awaiting_focus: false,
            category: Category::Appearance,
            search: String::new(),
            drafts: BTreeMap::new(),
            widget_choice: BTreeMap::new(),
            editor: text_editor::Content::with_text(
                &toml::to_string_pretty(&snapshot.overrides).unwrap_or_default(),
            ),
            editor_revision: snapshot.revision,
            editor_dirty: false,
            error: None,
            notice: None,
            catalog: Vec::new(),
        }
    }

    pub(crate) fn sync_editor(&mut self, snapshot: &ConfigSnapshot) {
        if !self.editor_dirty {
            self.editor = text_editor::Content::with_text(
                &toml::to_string_pretty(&snapshot.overrides).unwrap_or_default(),
            );
            self.editor_revision = snapshot.revision;
        }
    }
}

impl OxiBar {
    pub(crate) fn settings_view(&self) -> Element<'_, Message> {
        let snapshot = &self.config.snapshot;
        let busy = self.config.jobs > 0;
        let search =
            oxi_text_input::text_input("Search settings", &self.settings.search, |value| {
                Message::Settings(Action::Search(value))
            })
            .id(SEARCH_ID)
            .padding(OXITHEME.padding_sm)
            .size(OXITHEME.font_md);
        let header = Row::new()
            .push(
                text("Settings")
                    .size(OXITHEME.font_lg)
                    .style(oxi_plugin::text_accent),
            )
            .push(Space::new().width(Length::Fill))
            .push(action_button("Reload", Action::Reload, busy))
            .push(action_button("Close", Action::Close, false))
            .spacing(OXITHEME.padding_sm)
            .align_y(Alignment::Center);
        let mut tabs = Row::new().spacing(OXITHEME.padding_sm);
        for category in Category::ALL {
            tabs = tabs.push(
                oxi_plugin::selectable_row(
                    text(category.label()).size(OXITHEME.font_md),
                    self.settings.category == category,
                )
                .on_press(Message::Settings(Action::Category(category))),
            );
        }
        let mut body = Column::new()
            .spacing(OXITHEME.padding_md)
            .width(Length::Fill);
        let query = self.settings.search.to_lowercase();
        if self.settings.category == Category::Bar && query.is_empty() {
            body = body.push(self.widget_layout_view(busy));
        }
        if self.settings.category == Category::Plugins && query.is_empty() {
            body = body.push(
                text("Installed libraries · lifecycle changes require restart")
                    .size(OXITHEME.font_md)
                    .style(oxi_plugin::text_muted),
            );
            let enabled = crate::config::get_allowed_plugins(&snapshot.effective);
            for plugin in &self.settings.catalog {
                let file = plugin.filename.clone();
                let mut control = oxi_checkbox::checkbox(
                    format!("{} · {}", plugin.label, plugin.filename),
                    enabled.contains(&file.as_str()),
                    move |enabled| Message::Settings(Action::EnablePlugin(file.clone(), enabled)),
                )
                .width(Length::Fill);
                if busy {
                    control = control.on_toggle_maybe(None::<fn(bool) -> Message>);
                }
                body = body.push(oxi_plugin::settings_card(
                    Column::new().push(control).push(
                        text(&plugin.state)
                            .size(OXITHEME.font_sm)
                            .style(oxi_plugin::text_muted),
                    ),
                ));
            }
        }
        for field in &snapshot.schema {
            let searchable =
                format!("{} {} {}", field.path, field.label, field.description).to_lowercase();
            let show = if query.is_empty() {
                self.settings.category.accepts(&field.path)
            } else {
                searchable.contains(&query)
            };
            if show && !["bar.start", "bar.center", "bar.end"].contains(&field.path.as_str()) {
                body = body.push(self.setting_field_view(field, busy));
            }
        }
        if self.settings.category == Category::Advanced && query.is_empty() {
            let editor = text_editor(&self.settings.editor)
                .height(Length::Fixed(crate::layout::SETTINGS_EDITOR_HEIGHT))
                .size(OXITHEME.font_md)
                .padding(OXITHEME.padding_md)
                .style(oxiced::widgets::oxi_text_input::editor_style)
                .on_action(|action| Message::Settings(Action::Advanced(action)));
            body = body.push(text("Sparse GUI overrides (TOML)").size(OXITHEME.font_md).style(oxi_plugin::text_accent))
                .push(text("Unknown plugin settings are preserved. Base/Nix configuration is read-only here.").size(OXITHEME.font_sm).style(oxi_plugin::text_muted))
                .push(editor).push(action_button("Save overrides", Action::SaveAdvanced, busy));
        }
        let mut footer = Column::new().spacing(OXITHEME.padding_xs);
        if let Some(error) = self.settings.error.as_ref() {
            footer = footer.push(
                text(error)
                    .size(OXITHEME.font_sm)
                    .style(oxi_plugin::text_accent),
            );
        }
        if let Some(notice) = self.settings.notice.as_ref() {
            footer = footer.push(
                text(notice)
                    .size(OXITHEME.font_sm)
                    .style(oxi_plugin::text_muted),
            );
        }
        let restart = self.restart_fields();
        if !restart.is_empty() {
            footer = footer.push(
                text(format!("Restart pending: {}", restart.join(", ")))
                    .size(OXITHEME.font_sm)
                    .style(oxi_plugin::text_muted),
            );
        }
        footer = footer.push(
            Row::new()
                .push(
                    text(if busy {
                        "Saving / reloading…".to_owned()
                    } else {
                        format!(
                            "Revision {} · {}",
                            snapshot.revision,
                            self.config.directory.display()
                        )
                    })
                    .size(OXITHEME.font_sm)
                    .style(oxi_plugin::text_muted),
                )
                .push(Space::new().width(Length::Fill))
                .push(action_button(
                    "Reset all GUI overrides",
                    Action::ResetAll,
                    busy,
                ))
                .align_y(Alignment::Center),
        );
        container(
            Column::new()
                .push(header)
                .push(search)
                .push(tabs)
                .push(scrollable(body).height(Length::Fill))
                .push(footer)
                .padding(OXITHEME.padding_lg)
                .spacing(OXITHEME.padding_md)
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .style(oxi_plugin::compact_card_style(OXITHEME.mantle))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn setting_field_view<'a>(
        &'a self,
        field: &'a SettingDescriptor,
        busy: bool,
    ) -> Element<'a, Message> {
        let snapshot = &self.config.snapshot;
        let current = get_path(&snapshot.effective, &field.path);
        let header = Row::new()
            .push(
                text(&field.label)
                    .size(OXITHEME.font_md)
                    .style(oxi_plugin::text_primary),
            )
            .push(Space::new().width(Length::Fill))
            .push(
                text(format!(
                    "{} · {}",
                    snapshot.source(&field.path).label(),
                    field.apply.label()
                ))
                .size(OXITHEME.font_sm)
                .style(oxi_plugin::text_muted),
            )
            .push(action_button(
                "Reset",
                Action::Reset(field.path.clone()),
                busy || get_path(&snapshot.overrides, &field.path).is_none(),
            ))
            .spacing(OXITHEME.padding_sm)
            .align_y(Alignment::Center);
        let path = field.path.clone();
        let control: Element<'_, Message> = match &field.kind {
            SettingKind::Boolean => {
                let mut widget = oxi_checkbox::checkbox(
                    "Enabled",
                    current.and_then(Value::as_bool).unwrap_or(false),
                    move |value| {
                        Message::Settings(Action::Set(path.clone(), Value::Boolean(value)))
                    },
                )
                .width(Length::Fill);
                if busy {
                    widget = widget.on_toggle_maybe(None::<fn(bool) -> Message>);
                }
                widget.into()
            }
            SettingKind::Choice(options) => oxi_picklist::pick_list(
                options.clone(),
                current.and_then(Value::as_str).map(str::to_owned),
                move |value| Message::Settings(Action::Set(path.clone(), Value::String(value))),
            )
            .width(Length::Fill)
            .padding(OXITHEME.padding_sm)
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
                    .padding(OXITHEME.padding_sm)
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
                    .spacing(OXITHEME.padding_sm)
                    .align_y(Alignment::Center)
                    .into()
            }
        };
        let base = snapshot.base_value(&field.path);
        let mut body = Column::new()
            .push(header)
            .push(control)
            .spacing(OXITHEME.padding_sm);
        if !field.description.is_empty() {
            body = body.push(
                text(&field.description)
                    .size(OXITHEME.font_sm)
                    .style(oxi_plugin::text_muted),
            );
        }
        body = body.push(
            text(format!(
                "{} · base/default: {}",
                field.path,
                display_value(base.as_ref())
            ))
            .size(OXITHEME.font_sm)
            .style(oxi_plugin::text_muted),
        );
        oxi_plugin::settings_card(body).into()
    }

    fn widget_layout_view(&self, busy: bool) -> Element<'_, Message> {
        let available: Vec<String> = std::iter::once("settings".to_owned())
            .chain(self.launcher.enabled.then_some("launcher".to_owned()))
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
        let mut columns = Row::new().spacing(OXITHEME.padding_md);
        for (section, entries) in [
            ("start", &self.start_widgets),
            ("center", &self.center_widgets),
            ("end", &self.end_widgets),
        ] {
            let mut column = Column::new()
                .push(
                    text(section)
                        .size(OXITHEME.font_md)
                        .style(oxi_plugin::text_accent),
                )
                .spacing(OXITHEME.padding_sm)
                .width(Length::Fill);
            for (index, name) in entries.iter().enumerate() {
                column = column.push(
                    Column::new()
                        .push(
                            text(name)
                                .size(OXITHEME.font_md)
                                .style(oxi_plugin::text_primary),
                        )
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
                                .push(action_button(
                                    "Remove",
                                    Action::RemoveWidget(section.to_owned(), index),
                                    busy,
                                ))
                                .spacing(OXITHEME.padding_xs),
                        ),
                );
            }
            let section_key = section.to_owned();
            column = column
                .push(
                    oxi_picklist::pick_list(
                        available.clone(),
                        self.settings.widget_choice.get(section).cloned(),
                        move |value| {
                            Message::Settings(Action::SelectWidget(section_key.clone(), value))
                        },
                    )
                    .placeholder("Add widget")
                    .width(Length::Fill)
                    .padding(OXITHEME.padding_sm),
                )
                .push(action_button(
                    "Add",
                    Action::AddWidget(section.to_owned()),
                    busy || !self.settings.widget_choice.contains_key(section),
                ));
            columns = columns.push(oxi_plugin::settings_card(column));
        }
        columns.into()
    }
}

fn action_button<'a>(
    label: &'a str,
    action: Action,
    disabled: bool,
) -> iced::widget::Button<'a, Message> {
    oxi_plugin::bar_button(text(label).size(OXITHEME.font_md))
        .on_press_maybe((!disabled).then_some(Message::Settings(action)))
}

pub(crate) fn edit_value(value: Option<&Value>, kind: &SettingKind) -> String {
    match (value, kind) {
        (Some(Value::String(value)), SettingKind::Text | SettingKind::Arguments) => value.clone(),
        (Some(value), _) => value.to_string(),
        _ => String::new(),
    }
}

fn display_value(value: Option<&Value>) -> String {
    value
        .map(Value::to_string)
        .unwrap_or_else(|| "Automatic / unset".to_owned())
}

pub(crate) fn draft_value(field: &SettingDescriptor, value: &str) -> Result<Value, String> {
    let value = if matches!(field.kind, SettingKind::Text | SettingKind::Arguments) {
        Value::String(value.to_owned())
    } else {
        crate::ipc::parse_value(value)?
    };
    crate::config::schema::validate_value(field, &value)?;
    Ok(value)
}

pub(crate) fn key_event(
    event: iced::Event,
    status: iced::event::Status,
    id: IcedId,
) -> Option<Message> {
    use iced::keyboard::{Event, Key, key::Named};
    let iced::Event::Keyboard(Event::KeyPressed { key, modifiers, .. }) = event else {
        return None;
    };
    match key {
        Key::Named(Named::Escape) => Some(Message::SettingsEscape(id)),
        Key::Named(Named::Tab) if status == iced::event::Status::Ignored => {
            Some(Message::SettingsFocus(id, !modifiers.shift()))
        }
        Key::Character(key) if modifiers.control() && key.eq_ignore_ascii_case("f") => {
            Some(Message::SettingsSearchFocus(id))
        }
        Key::Character(key) if modifiers.alt() => key
            .parse::<usize>()
            .ok()
            .and_then(|index| index.checked_sub(1))
            .and_then(|index| Category::ALL.get(index))
            .map(|category| Message::SettingsCategory(id, *category)),
        _ => None,
    }
}
