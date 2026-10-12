use iced_layershell::reexport::Anchor;
use oxibar_plugin_api::{ApplyMode, SettingDescriptor, SettingKind};
use toml::{Table, Value};

use super::store::get_path;
use crate::layout;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    pub anchor: Anchor,
    pub margin: (i32, i32, i32, i32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToastConfig {
    pub position: String,
    pub margin: u32,
    pub spacing: u32,
}

impl ToastConfig {
    pub(crate) fn from_validated(table: &Table) -> Self {
        Self {
            position: get_path(table, "toasts.position")
                .and_then(Value::as_str)
                .expect("validated toast position")
                .to_owned(),
            margin: get_path(table, "toasts.margin")
                .and_then(Value::as_integer)
                .expect("validated toast margin") as u32,
            spacing: get_path(table, "toasts.spacing")
                .and_then(Value::as_integer)
                .expect("validated toast spacing") as u32,
        }
    }

    pub fn placement(&self, offset: i32) -> Placement {
        let bottom = self.position.starts_with("bottom-");
        let mut anchor = if bottom { Anchor::Bottom } else { Anchor::Top };
        let mut margin = if bottom {
            (0, 0, offset, 0)
        } else {
            (offset, 0, 0, 0)
        };
        if self.position.ends_with("-left") {
            anchor |= Anchor::Left;
            margin.3 = self.margin as i32;
        } else if self.position.ends_with("-right") {
            anchor |= Anchor::Right;
            margin.1 = self.margin as i32;
        }
        Placement { anchor, margin }
    }
}

pub(crate) fn schema() -> Vec<SettingDescriptor> {
    let distance = SettingKind::Integer {
        min: 0,
        max: layout::MAX_HOST_SURFACE_DIMENSION,
    };
    vec![
        SettingDescriptor::new(
            "toasts.position",
            "Notification toast position",
            SettingKind::Choice(
                [
                    "top-left",
                    "top-center",
                    "top-right",
                    "bottom-left",
                    "bottom-center",
                    "bottom-right",
                ]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            ),
            Some(Value::String("top-right".into())),
            ApplyMode::Surface,
        )
        .description("Notifications and music share this placement and stack on each output."),
        SettingDescriptor::new(
            "toasts.margin",
            "Toast edge margin",
            distance.clone(),
            Some(Value::Integer(layout::TOAST_MARGIN_TOP.into())),
            ApplyMode::Surface,
        ),
        SettingDescriptor::new(
            "toasts.spacing",
            "Toast spacing",
            distance,
            Some(Value::Integer(layout::TOAST_SPACING.into())),
            ApplyMode::Surface,
        ),
    ]
}
