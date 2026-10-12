use std::collections::BTreeMap;

use iced::widget::text_editor;
use iced_layershell::reexport::IcedId;
use oxibar_plugin_api::SettingKind;
use toml::Value;

use crate::config::ConfigSnapshot;

mod navigation;
pub(crate) mod runtime;
mod view;
pub use runtime::ConfigOutcome;
pub(crate) use runtime::ConfigRuntime;
pub(crate) use view::inspected;

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
    Dock,
    Desktop,
    Theme,
    Shortcuts,
}

impl Category {
    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::Appearance => "appearance",
            Self::Bar => "bar",
            Self::Launcher => "launcher",
            Self::Plugins => "plugins",
            Self::Advanced => "advanced",
            Self::Monitors => "monitors",
            Self::ControlCenter => "controls",
            Self::Dock => "dock",
            Self::Desktop => "desktop",
            Self::Theme => "theme",
            Self::Shortcuts => "shortcuts",
        }
    }

    pub(crate) const ALL: [Self; 11] = [
        Self::Appearance,
        Self::Bar,
        Self::Launcher,
        Self::Plugins,
        Self::Advanced,
        Self::Monitors,
        Self::ControlCenter,
        Self::Dock,
        Self::Desktop,
        Self::Theme,
        Self::Shortcuts,
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
            Self::Dock => "Dock",
            Self::Desktop => "Desktop widgets",
            Self::Theme => "Theme",
            Self::Shortcuts => "Shortcuts",
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
            Self::Dock => "Pinned applications and running windows.",
            Self::Desktop => "Place independent widget instances on your displays.",
            Self::Theme => "Live palette, typography, spacing and motion from oxiced.",
            Self::Shortcuts => {
                "Keyboard bindings inside Oxibar. Global open/toggle bindings belong to your compositor and can invoke oxibarctl."
            }
        }
    }

    fn accepts(self, path: &str) -> bool {
        match self {
            Self::Appearance => {
                path.starts_with("bar.font")
                    || path == "bar.transparent"
                    || path.starts_with("osd.")
                    || path.starts_with("toasts.")
            }
            Self::Bar => {
                (path.starts_with("bar.") && !Self::Appearance.accepts(path))
                    || path.starts_with("menus.")
            }
            Self::Launcher => {
                path.starts_with("launcher.")
                    || path.starts_with("applications.")
                    || path.starts_with("overview.")
            }
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
                    "menus.",
                    "osd.",
                    "toasts.",
                    "dock.",
                    "desktop.",
                    "theme.",
                    "overview.",
                    "shortcuts.",
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
            Self::Dock => path.starts_with("dock."),
            Self::Desktop => path.starts_with("desktop."),
            Self::Theme => path.starts_with("theme."),
            Self::Shortcuts => path.starts_with("shortcuts."),
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
    Page(String),
    TogglePage(String),
    NavigationBounds(String, iced::Rectangle),
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
    BeginWidgetDrag {
        section: String,
        index: usize,
        name: String,
        revision: u64,
    },
    DropWidget(String, usize),
    CancelWidgetDrag,
    EnableDefaultProviders(bool),
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
pub(crate) struct WidgetDrag {
    pub section: String,
    pub index: usize,
    pub name: String,
    pub revision: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct PluginStatus {
    pub filename: String,
    pub label: String,
    pub state: String,
    pub abi: Option<u32>,
    pub capabilities: Vec<String>,
    pub enabled: bool,
}

pub(crate) struct SettingsUi {
    pub window_id: Option<IcedId>,
    pub awaiting_focus: bool,
    pub category: Category,
    pub navigation: navigation::Navigation,
    pub navigation_bounds: BTreeMap<String, iced::Rectangle>,
    pub search: String,
    pub drafts: BTreeMap<String, Draft>,
    pub widget_choice: BTreeMap<String, String>,
    pub widget_drag: Option<WidgetDrag>,
    pub editor: text_editor::Content,
    pub editor_revision: u64,
    pub editor_dirty: bool,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub catalog: Vec<PluginStatus>,
    pub monitors: crate::monitor::runtime::Ui,
}

impl SettingsUi {
    pub(crate) fn new(snapshot: &ConfigSnapshot) -> Self {
        Self {
            window_id: None,
            awaiting_focus: false,
            category: Category::Appearance,
            navigation: navigation::Navigation::new(&snapshot.schema),
            navigation_bounds: BTreeMap::new(),
            search: String::new(),
            drafts: BTreeMap::new(),
            widget_choice: BTreeMap::new(),
            widget_drag: None,
            editor: text_editor::Content::with_text(
                &toml::to_string_pretty(&snapshot.overrides).unwrap_or_default(),
            ),
            editor_revision: snapshot.revision,
            editor_dirty: false,
            error: None,
            notice: None,
            catalog: Vec::new(),
            monitors: crate::monitor::runtime::Ui::default(),
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
        (
            Some(Value::String(value)),
            SettingKind::Text | SettingKind::Color | SettingKind::Arguments,
        ) => value.clone(),
        (Some(value), _) => value.to_string(),
        _ => String::new(),
    }
}

pub(crate) fn draft_value(
    field: &oxibar_plugin_api::SettingDescriptor,
    value: &str,
) -> Result<Value, String> {
    let value = if matches!(
        field.kind,
        SettingKind::Text | SettingKind::Color | SettingKind::Arguments
    ) {
        Value::String(value.to_owned())
    } else {
        crate::ipc::parse_value(value)?
    };
    crate::config::schema::validate_value(field, &value)?;
    Ok(value)
}
