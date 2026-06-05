use iced_layershell::{
    Settings,
    reexport::{Anchor, KeyboardInteractivity, Layer},
    settings::LayerShellSettings,
};
use toml::Table;

#[derive(Clone, Copy, Debug)]
pub enum BarSection {
    Start,
    Center,
    End,
}

pub const WINDOW_SIZE: (u32, u32) = (3440, 31);
pub const SCALE_FACTOR: f32 = 1.0;
pub const WINDOW_MARGINS: (i32, i32, i32, i32) = (0, 0, 0, 0);
pub const WINDOW_KEYBOARD_MODE: KeyboardInteractivity = KeyboardInteractivity::None;
pub const EXCLUSIVE_ZONE: i32 = WINDOW_SIZE.1 as i32;
pub const DEFAULT_POPUP_SIZE: (u32, u32) = (320, 300);
pub const LARGE_POPUP_SIZE: (u32, u32) = (460, 420);
pub const POPUP_MAX_HEIGHT: u32 = LARGE_POPUP_SIZE.1;
pub const POPUP_CONNECTOR_PADDING: u32 = 32;
pub const POPUP_CONNECTOR_HEIGHT: u32 = 0;
pub const MODAL_SIZE: (u32, u32) = (460, 300);
pub const PANEL_WIDTH: u32 = 420;
pub const TOAST_MARGIN_TOP: i32 = 16;
pub const TOAST_MARGIN_RIGHT: i32 = 16;
pub const TOAST_SPACING: i32 = 12;

#[derive(Clone, Copy)]
pub(crate) struct PopupMetrics {
    pub body_width: u32,
    pub connector_width: u32,
    pub height: u32,
}

impl Default for PopupMetrics {
    fn default() -> Self {
        Self::new(DEFAULT_POPUP_SIZE)
    }
}

impl PopupMetrics {
    pub(crate) fn new((body_width, height): (u32, u32)) -> Self {
        Self {
            body_width,
            connector_width: body_width + POPUP_CONNECTOR_PADDING,
            height,
        }
    }
}

pub(crate) fn layer_shell_settings() -> Settings {
    Settings {
        layer_settings: LayerShellSettings {
            size: Some((WINDOW_SIZE.0, WINDOW_SIZE.1 + POPUP_MAX_HEIGHT)),
            exclusive_zone: EXCLUSIVE_ZONE,
            anchor: Anchor::Top,
            layer: Layer::Top,
            margin: WINDOW_MARGINS,
            keyboard_interactivity: WINDOW_KEYBOARD_MODE,
            ..Default::default()
        },
        ..Default::default()
    }
}

pub(crate) fn popup_x(section: BarSection, connector_width: u32) -> i32 {
    match section {
        BarSection::Start => 0,
        BarSection::Center => (WINDOW_SIZE.0.saturating_sub(connector_width) / 2) as i32,
        BarSection::End => WINDOW_SIZE.0.saturating_sub(connector_width) as i32,
    }
}

pub(crate) fn popup_size_from_config(config: &Table, plugin_id: &str) -> Option<(u32, u32)> {
    let sizes = config
        .get("bar")
        .and_then(|v| v.as_table())
        .and_then(|t| t.get("popup_sizes"))
        .and_then(|v| v.as_table())?;
    sizes
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(plugin_id))
        .and_then(|(_, value)| popup_size_from_value(value))
}

fn popup_size_from_value(value: &toml::Value) -> Option<(u32, u32)> {
    let arr = value.as_array()?;
    let [width, height] = arr.as_slice() else {
        return None;
    };
    let width = width.as_integer().filter(|n| *n > 0)? as u32;
    let height = height.as_integer().filter(|n| *n > 0)? as u32;
    Some((width, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn popup_geometry_matches_bar_sections() {
        assert_eq!(popup_x(BarSection::Start, 440), 0);
        assert_eq!(popup_x(BarSection::Center, 440), 1500);
        assert_eq!(popup_x(BarSection::End, 440), 3000);
    }

    #[test]
    fn popup_metrics_use_configured_size_values() {
        let metrics = PopupMetrics::new((460, 420));
        assert_eq!(metrics.body_width, 460);
        assert_eq!(metrics.height, 420);
        assert_eq!(metrics.connector_width, 460 + POPUP_CONNECTOR_PADDING);

        let mut popup_sizes = Table::new();
        popup_sizes.insert(
            "Audio".to_owned(),
            toml::Value::Array(vec![toml::Value::Integer(500), toml::Value::Integer(360)]),
        );
        let mut bar = Table::new();
        bar.insert("popup_sizes".to_owned(), toml::Value::Table(popup_sizes));
        let mut config = Table::new();
        config.insert("bar".to_owned(), toml::Value::Table(bar));

        assert_eq!(popup_size_from_config(&config, "audio"), Some((500, 360)));
        assert_eq!(popup_size_from_config(&config, "clock"), None);
    }
}
