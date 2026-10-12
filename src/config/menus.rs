use toml::{Table, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenusConfig {
    pub controls_dropdown: bool,
    pub attach_to_clock: bool,
    pub notifications_dropdown: bool,
    pub notification_size: (u32, u32),
    pub session_enabled: bool,
    pub session_actions: Vec<String>,
}

impl MenusConfig {
    pub fn from_validated(table: &Table) -> Self {
        let boolean = |path| {
            super::store::get_path(table, path)
                .and_then(Value::as_bool)
                .unwrap_or(true)
        };
        let dimension = |path, default| {
            super::store::get_path(table, path)
                .and_then(Value::as_integer)
                .unwrap_or(default) as u32
        };
        Self {
            controls_dropdown: super::store::get_path(table, "control_center.presentation")
                .and_then(Value::as_str)
                != Some("window"),
            attach_to_clock: boolean("control_center.attach_to_clock"),
            notifications_dropdown: boolean("menus.notifications_dropdown"),
            notification_size: (
                dimension("menus.notification_width", 600),
                dimension("menus.notification_height", 640),
            ),
            session_enabled: boolean("menus.session_enabled"),
            session_actions: super::store::get_path(table, "menus.session_actions")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
        }
    }

    pub fn popup_height(&self, controls_enabled: bool, controls_height: u32) -> u32 {
        let mut height = crate::layout::POPUP_MAX_HEIGHT;
        if controls_enabled && self.controls_dropdown {
            height = height.max(controls_height);
        }
        if self.notifications_dropdown {
            height = height.max(self.notification_size.1);
        }
        height
    }
}
