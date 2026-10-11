use crate::{
    PluginModel, PluginMsg,
    iced::Task,
    toml::{Table, Value},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyMode {
    Live,
    Surface,
    Restart,
}

impl ApplyMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Live => "Live",
            Self::Surface => "Surface update",
            Self::Restart => "Restart required",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SettingKind {
    Boolean,
    Integer { min: i64, max: i64 },
    Number { min: f64, max: f64 },
    Text,
    Color,
    TextList,
    TableList,
    Choice(Vec<String>),
    Arguments,
}

/// Full dotted configuration paths preserve the existing top-level plugin tables.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingDescriptor {
    pub path: String,
    pub label: String,
    pub description: String,
    pub kind: SettingKind,
    pub default: Option<Value>,
    pub apply: ApplyMode,
}

impl SettingDescriptor {
    pub fn new(
        path: &str,
        label: &str,
        kind: SettingKind,
        default: Option<Value>,
        apply: ApplyMode,
    ) -> Self {
        Self {
            path: path.to_owned(),
            label: label.to_owned(),
            description: String::new(),
            kind,
            default,
            apply,
        }
    }

    pub fn description(mut self, description: &str) -> Self {
        self.description = description.to_owned();
        self
    }
}

/// Optional exports; additions do not change the existing ABI symbol signatures.
pub type SettingsSchemaFn = unsafe extern "Rust" fn() -> Vec<SettingDescriptor>;
/// Receives the full effective document; ignore unrelated changes and retain
/// running values for restart-only fields until the next model creation.
pub type ConfigChangedFn =
    unsafe extern "Rust" fn(model: PluginModel, config: Table) -> Option<Task<PluginMsg>>;
