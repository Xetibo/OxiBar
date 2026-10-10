use std::collections::BTreeMap;

use iced::widget::text_editor;
use iced_layershell::reexport::IcedId;
use oxibar_plugin_api::SettingKind;
use toml::Value;

use crate::{config::ConfigSnapshot, messages::Message};

pub(crate) mod runtime;
mod view;
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
    Monitors,
    ControlCenter,
}

impl Category {
    const ALL: [Self; 7] = [
        Self::Appearance,
        Self::Bar,
        Self::Launcher,
        Self::Plugins,
        Self::Advanced,
        Self::Monitors,
        Self::ControlCenter,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Appearance => "Appearance",
            Self::Bar => "Bar & layout",
            Self::Launcher => "Launcher",
            Self::Plugins => "Plugins",
            Self::Advanced => "Advanced",
            Self::Monitors => "Monitors",
            Self::ControlCenter => "Control center",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Appearance => "Make the shell feel at home.",
            Self::Bar => "Arrange widgets and choose where your bars appear.",
            Self::Launcher => "Quick access to your applications.",
            Self::Plugins => "Choose your plugins and tune their behavior.",
            Self::Advanced => "Shell preferences and sparse GUI overrides.",
            Self::Monitors => "Connected displays and their current geometry.",
            Self::ControlCenter => "Quick controls from your running plugins.",
        }
    }

    fn accepts(self, path: &str) -> bool {
        match self {
            Self::Appearance => {
                path.starts_with("bar.font")
                    || path == "bar.transparent"
                    || path.starts_with("osd.")
            }
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
                    "monitor.",
                    "control_center.",
                    "osd.",
                ]
                .iter()
                .any(|prefix| path.starts_with(prefix))
                    && path != "plugins"
            }
            Self::Advanced => ["settings.", "instance.", "ipc.", "startup."]
                .iter()
                .any(|prefix| path.starts_with(prefix)),
            Self::Monitors => path.starts_with("monitor."),
            Self::ControlCenter => path.starts_with("control_center."),
        }
    }
}

impl std::fmt::Display for Category {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
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

pub(crate) fn edit_value(value: Option<&Value>, kind: &SettingKind) -> String {
    match (value, kind) {
        (Some(Value::String(value)), SettingKind::Text | SettingKind::Arguments) => value.clone(),
        (Some(value), _) => value.to_string(),
        _ => String::new(),
    }
}

pub(crate) fn draft_value(
    field: &oxibar_plugin_api::SettingDescriptor,
    value: &str,
) -> Result<Value, String> {
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
