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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BarDimensions {
    pub width: u32,
    pub height: u32,
}

pub const DEFAULT_BAR_WIDTH: u32 = 3440;
pub const DEFAULT_BAR_HEIGHT: u32 = 31;
pub const DEFAULT_BAR_SIZE: BarDimensions = BarDimensions {
    width: DEFAULT_BAR_WIDTH,
    height: DEFAULT_BAR_HEIGHT,
};
pub const SCALE_FACTOR: f32 = 1.0;
pub const WINDOW_MARGINS: (i32, i32, i32, i32) = (0, 0, 0, 0);
pub const WINDOW_KEYBOARD_MODE: KeyboardInteractivity = KeyboardInteractivity::None;
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
const AUTO_LAYER_WIDTH: u32 = 0;

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

pub(crate) fn layer_shell_settings(config: &Table) -> Settings {
    let width = bar_dimension_from_config(config, "width").unwrap_or(AUTO_LAYER_WIDTH);
    let height = bar_dimension_from_config(config, "height").unwrap_or(DEFAULT_BAR_HEIGHT);
    Settings {
        layer_settings: LayerShellSettings {
            size: Some((width, height.saturating_add(POPUP_MAX_HEIGHT))),
            exclusive_zone: height as i32,
            anchor: Anchor::Top | Anchor::Left | Anchor::Right,
            layer: Layer::Top,
            margin: WINDOW_MARGINS,
            keyboard_interactivity: WINDOW_KEYBOARD_MODE,
            ..Default::default()
        },
        ..Default::default()
    }
}

pub(crate) fn bar_size_from_config(config: &Table) -> BarDimensions {
    BarDimensions {
        width: bar_dimension_from_config(config, "width").unwrap_or(DEFAULT_BAR_WIDTH),
        height: bar_dimension_from_config(config, "height").unwrap_or(DEFAULT_BAR_HEIGHT),
    }
}

pub(crate) fn bar_size_from_layer_surface(
    current: BarDimensions,
    layer_width: u32,
    layer_height: u32,
) -> BarDimensions {
    let width = positive(layer_width).unwrap_or(current.width);
    let height = layer_height
        .checked_sub(POPUP_MAX_HEIGHT)
        .and_then(positive)
        .unwrap_or(current.height);
    BarDimensions { width, height }
}

pub(crate) fn popup_x(bar_width: u32, section: BarSection, connector_width: u32) -> i32 {
    let x = match section {
        BarSection::Start => 0,
        BarSection::Center => bar_width.saturating_sub(connector_width) / 2,
        BarSection::End => bar_width.saturating_sub(connector_width),
    };
    i32::try_from(x).unwrap_or(i32::MAX)
}

fn positive(value: u32) -> Option<u32> {
    (value > 0).then_some(value)
}

fn bar_dimension_from_config(config: &Table, key: &str) -> Option<u32> {
    config
        .get("bar")
        .and_then(|v| v.as_table())
        .and_then(|t| t.get(key))
        .and_then(|v| v.as_integer())
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value > 0 && *value <= i32::MAX as u32)
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
        assert_eq!(popup_x(DEFAULT_BAR_WIDTH, BarSection::Start, 440), 0);
        assert_eq!(popup_x(DEFAULT_BAR_WIDTH, BarSection::Center, 440), 1500);
        assert_eq!(popup_x(DEFAULT_BAR_WIDTH, BarSection::End, 440), 3000);
    }

    #[test]
    fn layer_settings_request_auto_width_unless_configured() {
        let settings = layer_shell_settings(&Table::new());
        assert_eq!(settings.layer_settings.size, Some((0, 451)));
        assert_eq!(settings.layer_settings.exclusive_zone, 31);
        assert_eq!(
            settings.layer_settings.anchor,
            Anchor::Top | Anchor::Left | Anchor::Right
        );

        let mut bar = Table::new();
        bar.insert("width".to_owned(), toml::Value::Integer(1920));
        bar.insert("height".to_owned(), toml::Value::Integer(36));
        let mut config = Table::new();
        config.insert("bar".to_owned(), toml::Value::Table(bar));

        let settings = layer_shell_settings(&config);
        assert_eq!(settings.layer_settings.size, Some((1920, 456)));
        assert_eq!(settings.layer_settings.exclusive_zone, 36);
    }

    #[test]
    fn bar_size_reads_config_and_updates_from_layer_surface() {
        let mut bar = Table::new();
        bar.insert("width".to_owned(), toml::Value::Integer(2560));
        bar.insert("height".to_owned(), toml::Value::Integer(34));
        let mut config = Table::new();
        config.insert("bar".to_owned(), toml::Value::Table(bar));

        assert_eq!(
            bar_size_from_config(&config),
            BarDimensions {
                width: 2560,
                height: 34,
            }
        );

        let current = DEFAULT_BAR_SIZE;
        assert_eq!(
            bar_size_from_layer_surface(current, 1920, 451),
            BarDimensions {
                width: 1920,
                height: 31,
            }
        );
        assert_eq!(bar_size_from_layer_surface(current, 0, 0), current);
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
