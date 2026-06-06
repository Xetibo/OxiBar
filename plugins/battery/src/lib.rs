//! Battery plugin backed by Linux power-supply sysfs.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use iced::{
    Alignment, Background, Border, Color, Element, Length, Shadow, Task,
    futures::Stream,
    stream,
    widget::{Column, Row, container, text, tooltip},
};
use oxibar_plugin_api::{
    ABI_VERSION, PluginAvailability, PluginModel, PluginMsg, PluginStream, drain_model_errors,
    plugin_model, toml::Table, with_model_read, with_model_write,
};
use oxiced::{
    theme::theme_impl::OXITHEME,
    widgets::{oxi_plugin, oxi_plugin::text_muted, oxi_plugin::text_primary},
};

mod system;

use system::{BatterySnapshot, BatteryState, read_battery};

const DEFAULT_POLL_SECONDS: u64 = 30;
const CHARGING_ICON: &str = "󰂄";
const DRAINING_ICON: &str = "󰁹";
const UNKNOWN_PERCENTAGE: &str = "?";
const BATTERY_TOOLTIP_WIDTH: u32 = 220;
const TOOLTIP_BORDER_WIDTH: f32 = 1.0;
const TOOLTIP_SHADOW_ALPHA: f32 = 0.35;
const TOOLTIP_SHADOW_OFFSET_Y: f32 = 8.0;
const TOOLTIP_SHADOW_BLUR: f32 = 18.0;

static POLL_INTERVAL: OnceLock<Duration> = OnceLock::new();

#[derive(Debug, Default)]
struct Model {
    snapshot: Option<BatterySnapshot>,
    errors: Vec<String>,
}

impl Model {
    fn new(global_config: Table) -> Self {
        let poll_seconds = read_poll_interval(&global_config);
        let _ = POLL_INTERVAL.set(Duration::from_secs(poll_seconds));
        Self::default()
    }
}

#[derive(Clone, Debug)]
enum Message {
    Refresh,
    Snapshot(Result<BatterySnapshot, String>),
}

fn msg(m: Message) -> PluginMsg {
    Arc::new(m)
}

#[unsafe(no_mangle)]
pub extern "Rust" fn abi_version() -> u32 {
    ABI_VERSION
}

#[unsafe(no_mangle)]
pub extern "Rust" fn name() -> &'static str {
    "Battery"
}

#[unsafe(no_mangle)]
pub extern "Rust" fn availability(_global_config: Table) -> PluginAvailability {
    if system::has_battery() {
        PluginAvailability::Available
    } else {
        PluginAvailability::Unavailable("no battery found")
    }
}

#[unsafe(no_mangle)]
pub extern "Rust" fn model(global_config: Table) -> (PluginModel, Option<Task<PluginMsg>>) {
    (
        plugin_model(Model::new(global_config)),
        Some(Task::done(msg(Message::Refresh))),
    )
}

#[unsafe(no_mangle)]
pub extern "Rust" fn update(model: PluginModel, msg_in: PluginMsg) -> Option<Task<PluginMsg>> {
    let m = msg_in.downcast_ref::<Message>()?.clone();
    with_model_write::<Model, _>(&model, |model| match m {
        Message::Refresh => Some(Task::perform(async { read_battery() }, |result| {
            msg(Message::Snapshot(result))
        })),
        Message::Snapshot(result) => {
            match result {
                Ok(snapshot) => model.snapshot = Some(snapshot),
                Err(error) => model.errors.push(error),
            }
            None
        }
    })
    .flatten()
}

#[unsafe(no_mangle)]
pub extern "Rust" fn launch(_focused_index: usize, _model: PluginModel) -> Option<Task<PluginMsg>> {
    Some(Task::done(msg(Message::Refresh)))
}

#[unsafe(no_mangle)]
pub extern "Rust" fn errors(model: PluginModel) -> Vec<String> {
    drain_model_errors::<Model>(&model, |model| &mut model.errors)
}

#[unsafe(no_mangle)]
pub extern "Rust" fn view(
    model: PluginModel,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    with_model_read::<Model, _>(&model, |model| {
        let bar = battery_bar_content(model.snapshot.as_ref());
        let button = oxi_plugin::bar_button(bar).on_press(msg(Message::Refresh));
        let element: Element<'static, PluginMsg> = tooltip(
            button,
            battery_tooltip(model.snapshot.as_ref()),
            tooltip::Position::FollowCursor,
        )
        .gap(OXITHEME.padding_sm)
        .into();
        vec![element]
    })
}

#[unsafe(no_mangle)]
pub extern "Rust" fn subscription() -> *mut PluginStream {
    let interval = *POLL_INTERVAL
        .get()
        .unwrap_or(&Duration::from_secs(DEFAULT_POLL_SECONDS));
    let s = stream::channel(
        16,
        move |mut output: iced::futures::channel::mpsc::Sender<PluginMsg>| async move {
            std::thread::spawn(move || {
                loop {
                    std::thread::sleep(interval);
                    let _ = output.try_send(msg(Message::Refresh));
                }
            });
            std::future::pending::<()>().await;
        },
    );

    Box::into_raw(Box::new(s)) as *mut PluginStream
}

fn battery_bar_content(snapshot: Option<&BatterySnapshot>) -> Element<'static, PluginMsg> {
    let icon = snapshot
        .map(|snapshot| state_icon(snapshot.state))
        .unwrap_or(DRAINING_ICON);
    let percentage = snapshot
        .map(|snapshot| percentage_label(snapshot.percentage))
        .unwrap_or_else(|| UNKNOWN_PERCENTAGE.to_owned());

    Row::new()
        .push(text(icon).size(OXITHEME.font_md).align_y(Alignment::Center))
        .push(
            text(percentage)
                .size(OXITHEME.font_md)
                .align_y(Alignment::Center),
        )
        .spacing(OXITHEME.padding_xs)
        .height(Length::Fill)
        .align_y(Alignment::Center)
        .into()
}

fn battery_tooltip(snapshot: Option<&BatterySnapshot>) -> Element<'static, PluginMsg> {
    let content = if let Some(snapshot) = snapshot {
        Column::new()
            .spacing(OXITHEME.padding_xs)
            .width(BATTERY_TOOLTIP_WIDTH)
            .push(
                text(state_label(snapshot.state))
                    .size(OXITHEME.font_md)
                    .style(text_primary),
            )
            .push(
                text(format!(
                    "Exact: {}",
                    exact_percentage_label(snapshot.percentage)
                ))
                .size(OXITHEME.font_sm)
                .style(text_muted),
            )
            .push(
                text(time_summary(snapshot))
                    .size(OXITHEME.font_sm)
                    .style(text_muted),
            )
    } else {
        Column::new()
            .spacing(OXITHEME.padding_xs)
            .width(BATTERY_TOOLTIP_WIDTH)
            .push(
                text("Battery unavailable")
                    .size(OXITHEME.font_md)
                    .style(text_primary),
            )
            .push(
                text("Waiting for /sys/class/power_supply data")
                    .size(OXITHEME.font_sm)
                    .style(text_muted),
            )
    };

    container(content)
        .padding([OXITHEME.padding_sm, OXITHEME.padding_md])
        .style(battery_tooltip_style)
        .into()
}

fn state_icon(state: BatteryState) -> &'static str {
    match state {
        BatteryState::Charging => CHARGING_ICON,
        BatteryState::Draining => DRAINING_ICON,
    }
}

fn state_label(state: BatteryState) -> &'static str {
    match state {
        BatteryState::Charging => "Charging",
        BatteryState::Draining => "Draining",
    }
}

fn percentage_label(percentage: f32) -> String {
    format!("{percentage:.0}%")
}

fn exact_percentage_label(percentage: f32) -> String {
    format!("{percentage:.1}%")
}

fn time_summary(snapshot: &BatterySnapshot) -> String {
    match snapshot.state {
        BatteryState::Charging if snapshot.percentage >= 100.0 => {
            "To 100%: fully charged".to_owned()
        }
        BatteryState::Charging => snapshot
            .estimate
            .map(|duration| format!("To 100%: {}", duration_label(duration)))
            .unwrap_or_else(|| "To 100%: unknown".to_owned()),
        BatteryState::Draining => snapshot
            .estimate
            .map(|duration| format!("Remaining: {}", duration_label(duration)))
            .unwrap_or_else(|| "Remaining: unknown".to_owned()),
    }
}

fn duration_label(duration: Duration) -> String {
    let minutes = duration.as_secs().div_ceil(60);
    if minutes == 0 {
        return "<1m".to_owned();
    }
    let hours = minutes / 60;
    let minutes = minutes % 60;
    match (hours, minutes) {
        (0, minutes) => format!("{minutes}m"),
        (hours, 0) => format!("{hours}h"),
        (hours, minutes) => format!("{hours}h {minutes}m"),
    }
}

fn battery_tooltip_style(_: &iced::Theme) -> container::Style {
    let palette = &OXITHEME;
    container::Style {
        background: Some(Background::Color(palette.mantle)),
        text_color: Some(palette.text),
        border: Border {
            color: palette.primary_bg_hover,
            width: TOOLTIP_BORDER_WIDTH,
            radius: OXITHEME.border_radius.into(),
        },
        shadow: Shadow {
            color: Color::BLACK.scale_alpha(TOOLTIP_SHADOW_ALPHA),
            offset: iced::Vector::new(0.0, TOOLTIP_SHADOW_OFFSET_Y),
            blur_radius: TOOLTIP_SHADOW_BLUR,
        },
        ..Default::default()
    }
}

fn read_poll_interval(global: &Table) -> u64 {
    global
        .get("battery")
        .and_then(|v| v.as_table())
        .and_then(|t| t.get("poll_seconds"))
        .and_then(|v| v.as_integer())
        .filter(|n| *n > 0)
        .map(|n| n as u64)
        .unwrap_or(DEFAULT_POLL_SECONDS)
}

const _: fn() = || {
    fn assert_stream<S: Stream<Item = PluginMsg> + Send + 'static>(_: &S) {}
    let _ = |s: &iced::futures::stream::BoxStream<'static, PluginMsg>| assert_stream(s);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_poll_interval_from_config() {
        let mut battery = Table::new();
        battery.insert(
            "poll_seconds".to_owned(),
            oxibar_plugin_api::toml::Value::Integer(12),
        );
        let mut global = Table::new();
        global.insert(
            "battery".to_owned(),
            oxibar_plugin_api::toml::Value::Table(battery),
        );

        assert_eq!(read_poll_interval(&global), 12);
        assert_eq!(read_poll_interval(&Table::new()), DEFAULT_POLL_SECONDS);
    }

    #[test]
    fn duration_labels_are_compact() {
        assert_eq!(duration_label(Duration::ZERO), "<1m");
        assert_eq!(duration_label(Duration::from_secs(60)), "1m");
        assert_eq!(duration_label(Duration::from_secs(2 * 60 * 60)), "2h");
        assert_eq!(
            duration_label(Duration::from_secs((2 * 60 + 5) * 60)),
            "2h 5m"
        );
    }

    #[test]
    fn labels_match_state_and_percentage() {
        let charging = BatterySnapshot {
            percentage: 83.4,
            state: BatteryState::Charging,
            estimate: Some(Duration::from_secs(80 * 60)),
        };
        let draining = BatterySnapshot {
            percentage: 24.2,
            state: BatteryState::Draining,
            estimate: None,
        };

        assert_eq!(state_icon(charging.state), CHARGING_ICON);
        assert_eq!(state_icon(draining.state), DRAINING_ICON);
        assert_eq!(percentage_label(charging.percentage), "83%");
        assert_eq!(exact_percentage_label(draining.percentage), "24.2%");
        assert_eq!(time_summary(&charging), "To 100%: 1h 20m");
        assert_eq!(time_summary(&draining), "Remaining: unknown");
    }

    #[test]
    fn model_view_and_error_drain_are_deterministic() {
        let (plugin_model, init_task) = model(Table::new());
        assert!(init_task.is_some());
        assert_eq!(name(), "Battery");
        assert_eq!(abi_version(), ABI_VERSION);
        assert_eq!(view(plugin_model.clone()).unwrap().len(), 1);

        let task = update(
            plugin_model.clone(),
            msg(Message::Snapshot(Err("battery failed".to_owned()))),
        );
        assert!(task.is_none());
        assert_eq!(errors(plugin_model.clone()), vec!["battery failed"]);
        assert!(errors(plugin_model).is_empty());
    }
}
