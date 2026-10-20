//! Explicit-location Open-Meteo weather, shared across shell surfaces.

use iced::{
    Element, Length, Task,
    futures::StreamExt,
    widget::{Column, Row},
};
use oxibar_plugin_api::{
    self as api, PluginModel, PluginMsg, PluginStream,
    toml::{Table, Value},
    with_model_read, with_model_write,
};
use oxiced::{
    theme::theme_impl::OXITHEME,
    widgets::oxi_plugin::{self, text},
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, PartialEq)]
struct Config {
    enabled: bool,
    latitude: f64,
    longitude: f64,
    location: String,
    fahrenheit: bool,
}
impl Config {
    fn read(table: &Table) -> Self {
        let table = table.get("weather").and_then(Value::as_table);
        let get = |key| table.and_then(|table| table.get(key));
        let number = |key| {
            get(key)
                .and_then(|value| {
                    value
                        .as_float()
                        .or_else(|| value.as_integer().map(|value| value as f64))
                })
                .unwrap_or(0.0)
        };
        Self {
            enabled: get("enabled").and_then(Value::as_bool).unwrap_or(false),
            latitude: number("latitude"),
            longitude: number("longitude"),
            location: get("location")
                .and_then(Value::as_str)
                .unwrap_or("Weather")
                .into(),
            fahrenheit: get("units").and_then(Value::as_str) == Some("fahrenheit"),
        }
    }
    fn valid(&self) -> bool {
        self.latitude.is_finite()
            && self.longitude.is_finite()
            && (-90.0..=90.0).contains(&self.latitude)
            && (-180.0..=180.0).contains(&self.longitude)
    }
}
#[derive(Debug, Clone)]
struct Forecast {
    date: String,
    low: f64,
    high: f64,
    code: u64,
}
#[derive(Debug, Clone)]
struct Weather {
    temperature: f64,
    feels: f64,
    humidity: f64,
    wind: f64,
    code: u64,
    forecast: Vec<Forecast>,
}
#[derive(Debug)]
struct Model {
    config: Config,
    weather: Option<Weather>,
    updated: Option<Instant>,
    pending: bool,
    generation: u64,
    error: Option<String>,
    errors: Vec<String>,
}
#[derive(Debug, Clone)]
enum Message {
    Refresh,
    Loaded(u64, Result<Weather, String>),
    Toggle,
}
fn msg(message: Message) -> PluginMsg {
    Arc::new(message)
}
api::export_theme_changed!();
#[unsafe(no_mangle)]
pub extern "Rust" fn abi_version() -> u32 {
    api::ABI_VERSION
}
#[unsafe(no_mangle)]
pub extern "Rust" fn name() -> &'static str {
    "Weather"
}
#[unsafe(no_mangle)]
pub extern "Rust" fn model(table: Table) -> (PluginModel, Option<Task<PluginMsg>>) {
    (
        api::plugin_model(Model {
            config: Config::read(&table),
            weather: None,
            updated: None,
            pending: false,
            generation: 0,
            error: None,
            errors: Vec::new(),
        }),
        Some(Task::done(msg(Message::Refresh))),
    )
}
#[unsafe(no_mangle)]
pub extern "Rust" fn update(model: PluginModel, message: PluginMsg) -> Option<Task<PluginMsg>> {
    let message = message.downcast_ref::<Message>()?.clone();
    with_model_write::<Model, _>(&model, |model| match message {
        Message::Toggle => Some(Task::done(
            Arc::new(api::HOST_REQUEST_TOGGLE_POPUP.to_owned()) as PluginMsg,
        )),
        Message::Refresh => {
            if !model.config.enabled
                || model.pending
                || model
                    .updated
                    .is_some_and(|updated| updated.elapsed() < Duration::from_secs(600))
            {
                return None;
            }
            if !model.config.valid() {
                model.error = Some("Set valid latitude and longitude in Weather settings.".into());
                return None;
            }
            model.pending = true;
            model.generation = model.generation.wrapping_add(1);
            let generation = model.generation;
            let config = model.config.clone();
            Some(Task::perform(
                api::worker::run(move || fetch(&config)),
                move |result| msg(Message::Loaded(generation, result)),
            ))
        }
        Message::Loaded(generation, result) => {
            if generation != model.generation {
                return None;
            }
            model.pending = false;
            match result {
                Ok(weather) => {
                    model.weather = Some(weather);
                    model.updated = Some(Instant::now());
                    model.error = None;
                }
                Err(error) => {
                    model.error = Some(error.clone());
                    model.errors.push(error);
                }
            }
            None
        }
    })
    .flatten()
}
#[unsafe(no_mangle)]
pub extern "Rust" fn config_changed(model: PluginModel, table: Table) -> Option<Task<PluginMsg>> {
    with_model_write::<Model, _>(&model, |model| {
        let config = Config::read(&table);
        if model.config == config {
            return None;
        }
        model.config = config;
        model.generation = model.generation.wrapping_add(1);
        model.pending = false;
        model.weather = None;
        model.updated = None;
        model.error = None;
        Some(Task::done(msg(Message::Refresh)))
    })
    .flatten()
}
#[unsafe(no_mangle)]
pub extern "Rust" fn launch(_: usize, _: PluginModel) -> Option<Task<PluginMsg>> {
    Some(Task::done(msg(Message::Toggle)))
}
#[unsafe(no_mangle)]
pub extern "Rust" fn errors(model: PluginModel) -> Vec<String> {
    api::drain_model_errors::<Model>(&model, |model| &mut model.errors)
}
#[unsafe(no_mangle)]
pub extern "Rust" fn subscription_policy() -> api::SubscriptionPolicy {
    api::SubscriptionPolicy::WhenVisible
}
#[unsafe(no_mangle)]
pub extern "Rust" fn subscription() -> *mut PluginStream {
    let stream = iced::futures::stream::once(async { msg(Message::Refresh) })
        .chain(api::worker::interval(Duration::from_secs(600)).map(|()| msg(Message::Refresh)));
    Box::into_raw(Box::new(stream)) as *mut PluginStream
}
#[unsafe(no_mangle)]
pub extern "Rust" fn view(
    model: PluginModel,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    with_model_read::<Model, _>(&model, |model| {
        let label = model
            .weather
            .as_ref()
            .map(|weather| format!("{} {:.0}°", weather_icon(weather.code), weather.temperature))
            .unwrap_or_else(|| "☁".into());
        vec![
            oxi_plugin::bar_button(text(label).size(OXITHEME.font_md))
                .on_press(msg(Message::Toggle))
                .into(),
        ]
    })
}
#[unsafe(no_mangle)]
pub extern "Rust" fn control_center_view(
    model: PluginModel,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    with_model_read::<Model, _>(&model, |model| vec![body(model, true)])
}
#[unsafe(no_mangle)]
pub extern "Rust" fn popup_view(
    model: PluginModel,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    with_model_read::<Model, _>(&model, |model| {
        vec![
            iced::widget::scrollable(
                iced::widget::container(body(model, true))
                    .padding([OXITHEME.padding_md, OXITHEME.padding_lg]),
            )
            .height(Length::Fill)
            .into(),
        ]
    })
}
#[unsafe(no_mangle)]
pub extern "Rust" fn desktop_view(
    model: PluginModel,
    _: api::PluginViewContext,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    control_center_view(model)
}
#[unsafe(no_mangle)]
pub extern "Rust" fn desktop_interactive(_: PluginModel) -> bool {
    false
}
#[unsafe(no_mangle)]
pub extern "Rust" fn subwidgets(_: PluginModel) -> Vec<api::SubwidgetDescriptor> {
    vec![api::SubwidgetDescriptor {
        id: "current".into(),
        title: "Weather".into(),
    }]
}
#[unsafe(no_mangle)]
pub extern "Rust" fn subwidget_view(
    model: PluginModel,
    id: &str,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    with_model_read::<Model, _>(&model, |model| {
        if id == "current" {
            vec![body(model, false)]
        } else {
            Vec::new()
        }
    })
}
#[unsafe(no_mangle)]
pub extern "Rust" fn popup_metrics(model: PluginModel) -> api::PluginPopupMetrics {
    let height =
        with_model_read::<Model, _>(
            &model,
            |model| if model.weather.is_some() { 410 } else { 180 },
        )
        .unwrap_or(180);
    api::PluginPopupMetrics {
        popup_size: Some((460, height)),
        ..Default::default()
    }
}
fn body(model: &Model, forecast: bool) -> Element<'static, PluginMsg> {
    let mut column = Column::new()
        .width(Length::Fill)
        .spacing(OXITHEME.padding_md)
        .push(text(model.config.location.clone()).size(OXITHEME.font_lg));
    if !model.config.enabled {
        return column.push(text("Choose latitude, longitude and location in Settings → Weather, then enable weather.").style(oxi_plugin::text_muted)).into();
    }
    if let Some(weather) = &model.weather {
        let unit = if model.config.fahrenheit {
            "°F"
        } else {
            "°C"
        };
        column = column
            .push(
                text(format!(
                    "{} {:.0}{unit}",
                    weather_icon(weather.code),
                    weather.temperature
                ))
                .size(OXITHEME.font_xxl),
            )
            .push(text(condition(weather.code)).size(OXITHEME.font_md))
            .push(
                text(format!(
                    "Feels {:.0}{unit} · Humidity {:.0}%\nWind {:.0} km/h",
                    weather.feels, weather.humidity, weather.wind
                ))
                .size(OXITHEME.font_md)
                .style(oxi_plugin::text_muted),
            );
        if forecast {
            for day in &weather.forecast {
                column = column.push(
                    Row::new()
                        .spacing(OXITHEME.padding_md)
                        .push(
                            text(format!("{} {}", day.date, weather_icon(day.code)))
                                .width(Length::Fill),
                        )
                        .push(text(format!("{:.0}° / {:.0}°", day.high, day.low))),
                );
            }
        }
    }
    if model.pending {
        column = column.push(text("Updating…").style(oxi_plugin::text_muted));
    }
    if let Some(error) = &model.error {
        column = column.push(
            text(format!(
                "{}{}",
                if model.weather.is_some() {
                    "Last known weather · "
                } else {
                    ""
                },
                error
            ))
            .style(oxi_plugin::text_muted),
        );
    }
    column
        .push(
            text("Weather data: Open-Meteo.com · CC BY 4.0")
                .size(OXITHEME.font_sm)
                .style(oxi_plugin::text_muted),
        )
        .into()
}
fn fetch(config: &Config) -> Result<Weather, String> {
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}&current=temperature_2m,apparent_temperature,relative_humidity_2m,weather_code,wind_speed_10m&daily=weather_code,temperature_2m_max,temperature_2m_min&forecast_days=3&timezone=auto&temperature_unit={}",
        config.latitude,
        config.longitude,
        if config.fahrenheit {
            "fahrenheit"
        } else {
            "celsius"
        }
    );
    let output = std::process::Command::new("curl")
        .args([
            "-fsS",
            "--connect-timeout",
            "3",
            "--max-time",
            "10",
            "--max-filesize",
            "262144",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            &url,
        ])
        .output()
        .map_err(|error| format!("Weather: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "Weather request failed: {}",
            String::from_utf8_lossy(&output.stderr)
                .chars()
                .take(300)
                .collect::<String>()
        ));
    }
    parse(&output.stdout)
}
fn parse(bytes: &[u8]) -> Result<Weather, String> {
    let root: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    let current = &root["current"];
    let number = |key: &str| {
        current[key]
            .as_f64()
            .filter(|value| value.is_finite())
            .ok_or_else(|| format!("Weather response missing {key}"))
    };
    let daily = &root["daily"];
    let forecast = (0..3)
        .filter_map(|index| {
            Some(Forecast {
                date: daily["time"][index].as_str()?.into(),
                low: daily["temperature_2m_min"][index].as_f64()?,
                high: daily["temperature_2m_max"][index].as_f64()?,
                code: daily["weather_code"][index].as_u64()?,
            })
        })
        .collect();
    Ok(Weather {
        temperature: number("temperature_2m")?,
        feels: number("apparent_temperature")?,
        humidity: number("relative_humidity_2m")?,
        wind: number("wind_speed_10m")?,
        code: current["weather_code"]
            .as_u64()
            .ok_or("Weather response missing condition")?,
        forecast,
    })
}
fn condition(code: u64) -> &'static str {
    match code {
        0 => "Clear sky",
        1..=3 => "Cloudy",
        45 | 48 => "Fog",
        51..=67 | 80..=82 => "Rain",
        71..=77 | 85 | 86 => "Snow",
        95..=99 => "Thunderstorm",
        _ => "Unknown conditions",
    }
}
fn weather_icon(code: u64) -> &'static str {
    match code {
        0 => "☀",
        1..=3 => "☁",
        45 | 48 => "≋",
        71..=77 | 85 | 86 => "❄",
        95..=99 => "ϟ",
        _ => "☂",
    }
}
#[unsafe(no_mangle)]
pub extern "Rust" fn settings_schema() -> Vec<api::SettingDescriptor> {
    use api::{ApplyMode::Live, SettingDescriptor as Field, SettingKind as Kind};
    vec![
        Field::new(
            "weather.enabled",
            "Enable weather requests",
            Kind::Boolean,
            Some(Value::Boolean(false)),
            Live,
        )
        .description("Fetch Open-Meteo weather for your explicitly configured location."),
        Field::new(
            "weather.location",
            "Location name",
            Kind::Text,
            Some(Value::String("Weather".into())),
            Live,
        ),
        Field::new(
            "weather.latitude",
            "Latitude",
            Kind::Number {
                min: -90.0,
                max: 90.0,
            },
            Some(Value::Float(0.0)),
            Live,
        ),
        Field::new(
            "weather.longitude",
            "Longitude",
            Kind::Number {
                min: -180.0,
                max: 180.0,
            },
            Some(Value::Float(0.0)),
            Live,
        ),
        Field::new(
            "weather.units",
            "Temperature units",
            Kind::Choice(vec!["celsius".into(), "fahrenheit".into()]),
            Some(Value::String("celsius".into())),
            Live,
        ),
    ]
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn response_validation_and_stale_config_results() {
        let payload = br#"{"current":{"temperature_2m":18.2,"apparent_temperature":17.0,"relative_humidity_2m":64,"wind_speed_10m":8,"weather_code":3}}"#;
        assert_eq!(parse(payload).unwrap().temperature, 18.2);
        assert!(parse(br#"{"current":{"temperature_2m":null}}"#).is_err());
        let (model, _) = model(Table::new());
        with_model_write::<Model, _>(&model, |model| {
            model.generation = 3;
            model.pending = true;
        });
        update(model.clone(), msg(Message::Loaded(2, parse(payload))));
        with_model_read::<Model, _>(&model, |model| {
            assert!(model.weather.is_none());
            assert!(model.pending);
        })
        .unwrap();
        assert!(!Config::read(&Table::new()).enabled);
    }
}
