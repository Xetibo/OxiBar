//! Clock plugin.
//!
//! Renders the current local time in a button. Format string and tick
//! interval come from the `[plugins.clock]` block in `config.toml`:
//!
//! ```toml
//! [plugins.clock]
//! format = "%H:%M"      # strftime, default "%H:%M"
//! tick_seconds = 60     # how often the real clock is polled, default 60
//! ```
//!
//! `tick_seconds` is the real-clock poll interval. If `format` shows seconds
//! (or finer), the plugin still emits a tick every second, but derives those
//! intermediate ticks from a monotonic anchor instead of re-reading the system
//! clock, only re-syncing to `Local::now()` every `tick_seconds`.
//!
//! Clicking the time toggles a host-owned calendar popup.

use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use chrono::{DateTime, Datelike, Duration as ChronoDuration, Local, NaiveDate};
use iced::{
    Alignment, Background, Border, Color, Element, Font, Length, Shadow, Task,
    border::Radius,
    font::Weight,
    futures::Stream,
    stream,
    widget::{Column, Row, button, container, text, tooltip},
};
use oxibar_plugin_api::{
    ABI_VERSION, HOST_REQUEST_TOGGLE_POPUP, PluginMetadata, PluginModel, PluginMsg, PluginStream,
    drain_model_errors, plugin_model, toml::Table, with_model_read, with_model_write,
};
use oxiced::theme::theme_impl::OXITHEME;
use oxiced::widgets::oxi_plugin;

mod caldav;

use caldav::{CaldavConfig, CalendarEvent, load_events};

const DEFAULT_FORMAT: &str = "%H:%M";
const DEFAULT_TICK_SECONDS: u64 = 60;
/// Floor for how often the display refreshes when the format shows no seconds.
const MINUTE_DISPLAY_SECONDS: u64 = 60;
const DEFAULT_FONT_SIZE: f32 = 14.0;
const DEFAULT_BOLD: bool = false;
const CALENDAR_POPUP_SIZE: (u32, u32) = (360, 320);
const CALENDAR_NAV_TEXT_SIZE: f32 = 20.0;
const CALENDAR_NAV_HEIGHT: u32 = 28;
const CALENDAR_DAY_CELL_HEIGHT: u32 = 30;
const CALENDAR_TOOLTIP_WIDTH: u32 = 240;
const ADJACENT_MONTH_ALPHA: f32 = 0.45;
const TOOLTIP_BORDER_WIDTH: f32 = 1.0;
const TOOLTIP_SHADOW_ALPHA: f32 = 0.35;
const TOOLTIP_SHADOW_OFFSET_Y: f32 = 8.0;
const TOOLTIP_SHADOW_BLUR: f32 = 18.0;

/// Tick interval shared with `subscription()`. Set during `model()` so the
/// subscription thread can read it without the model being passed in.
static TICK: OnceLock<Duration> = OnceLock::new();
/// How often the display rolls over, derived from the format string (1s when
/// seconds are shown, otherwise atomic with the minute). The subscription emits
/// a `Tick` on this cadence and only re-reads the real clock every [`TICK`],
/// simulating the intermediate ticks so finer-than-poll formats stay accurate.
static DISPLAY_INTERVAL: OnceLock<Duration> = OnceLock::new();
static CALDAV_REFRESH: OnceLock<Duration> = OnceLock::new();

#[derive(Debug)]
pub struct Model {
    format: String,
    font_size: f32,
    bold: bool,
    /// Resolved font family name from `[bar] font` (or `None` to use iced's
    /// default). Leaked to `&'static str` because [`iced::Font`] requires it.
    font_family: Option<&'static str>,
    calendar_command: Option<String>,
    caldav_config: Option<CaldavConfig>,
    calendar_events: Vec<CalendarEvent>,
    calendar_pending: bool,
    calendar_sync_error: Option<String>,
    calendar_sync_count: Option<usize>,
    now: DateTime<Local>,
    calendar_month: NaiveDate,
    calendar_open: bool,
    errors: Vec<String>,
}

impl Model {
    fn new(global_config: Table) -> Self {
        let cfg = read_config(&global_config);
        // First setter wins; if the dylib is reloaded in-process the old
        // value sticks, which is fine — interval changes need a restart.
        let _ = TICK.set(Duration::from_secs(cfg.tick_seconds));
        let _ = DISPLAY_INTERVAL.set(Duration::from_secs(display_interval(
            cfg.tick_seconds,
            &cfg.format,
        )));
        if let Some(caldav) = cfg.caldav_config.as_ref() {
            let _ = CALDAV_REFRESH.set(Duration::from_secs(caldav.refresh_minutes * 60));
        }
        // Read the bar-wide font so bold rendering doesn't lose the family.
        let font_family = global_config
            .get("bar")
            .and_then(|v| v.as_table())
            .and_then(|t| t.get("font"))
            .and_then(|v| v.as_str())
            .map(resolve_font_family)
            .map(|s| &*Box::leak(s.to_owned().into_boxed_str()));
        Self {
            format: cfg.format,
            font_size: cfg.font_size,
            bold: cfg.bold,
            font_family,
            calendar_command: cfg.calendar_command,
            caldav_config: cfg.caldav_config,
            calendar_events: Vec::new(),
            calendar_pending: false,
            calendar_sync_error: cfg.caldav_config_error,
            calendar_sync_count: None,
            now: Local::now(),
            calendar_month: current_month_start(),
            calendar_open: false,
            errors: Vec::new(),
        }
    }
}

struct ClockConfig {
    format: String,
    tick_seconds: u64,
    font_size: f32,
    bold: bool,
    calendar_command: Option<String>,
    caldav_config: Option<CaldavConfig>,
    caldav_config_error: Option<String>,
}

fn read_config(global: &Table) -> ClockConfig {
    // `[plugins.clock]` is the canonical location. Note: at the top level
    // `plugins` is *also* a `Vec<String>` of allowed plugins (see
    // `config::get_allowed_plugins`). To avoid that collision while staying
    // compatible, prefer a top-level `[clock]` table; fall back to a nested
    // `plugins.clock` table only if `plugins` happens to be a `Table` rather
    // than an `Array`.
    let table = global.get("clock").and_then(|v| v.as_table()).or_else(|| {
        global
            .get("plugins")
            .and_then(|v| v.as_table())
            .and_then(|t| t.get("clock"))
            .and_then(|v| v.as_table())
    });
    let Some(plugins) = table else {
        return ClockConfig {
            format: DEFAULT_FORMAT.to_owned(),
            tick_seconds: DEFAULT_TICK_SECONDS,
            font_size: DEFAULT_FONT_SIZE,
            bold: DEFAULT_BOLD,
            calendar_command: None,
            caldav_config: None,
            caldav_config_error: None,
        };
    };

    let format = plugins
        .get("format")
        .and_then(|v| v.as_str())
        .unwrap_or(DEFAULT_FORMAT)
        .to_owned();
    let tick_seconds = plugins
        .get("tick_seconds")
        .and_then(|v| v.as_integer())
        .filter(|n| *n > 0)
        .map(|n| n as u64)
        .unwrap_or(DEFAULT_TICK_SECONDS);
    let font_size = plugins
        .get("font_size")
        .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|n| n as f64)))
        .filter(|n| *n > 0.0)
        .map(|n| n as f32)
        .unwrap_or(DEFAULT_FONT_SIZE);
    let bold = plugins
        .get("bold")
        .and_then(|v| v.as_bool())
        .unwrap_or(DEFAULT_BOLD);
    let calendar_command = plugins
        .get("calendar_command")
        .and_then(|v| v.as_str())
        .map(ToOwned::to_owned)
        .or_else(
            || match plugins.get("calendar_app").and_then(|v| v.as_str()) {
                Some("thunderbird") => Some("thunderbird --calendar".to_owned()),
                _ => None,
            },
        );
    let caldav_config = caldav::read_config(table);
    let caldav_config_error = caldav_config_missing_error(plugins, caldav_config.is_some());
    ClockConfig {
        format,
        tick_seconds,
        font_size,
        bold,
        calendar_command,
        caldav_config,
        caldav_config_error,
    }
}

fn caldav_config_missing_error(clock: &Table, configured: bool) -> Option<String> {
    if configured {
        return None;
    }
    let caldav = clock.get("caldav")?.as_table()?;
    if caldav.get("enabled").and_then(|value| value.as_bool()) == Some(false) {
        return None;
    }
    Some("clock: [clock.caldav] requires url and username".to_owned())
}

fn resolve_font_family(configured: &str) -> String {
    let Ok(output) = std::process::Command::new("fc-match")
        .args(["-f", "%{family}", configured])
        .output()
    else {
        return configured.to_owned();
    };
    if !output.status.success() {
        return configured.to_owned();
    }
    let family = String::from_utf8_lossy(&output.stdout);
    family
        .split(',')
        .next()
        .map(str::trim)
        .filter(|family| !family.is_empty())
        .unwrap_or(configured)
        .to_owned()
}

#[derive(Clone, Debug)]
pub enum Message {
    Tick(DateTime<Local>),
    ToggleCalendar,
    PreviousMonth,
    NextMonth,
    OpenDay(NaiveDate),
    RefreshCalendar,
    CalendarLoaded(Result<Vec<CalendarEvent>, String>),
    CalendarOpenResult(Result<(), String>),
}

fn msg(m: Message) -> PluginMsg {
    Arc::new(m)
}

// ---------------- Plugin ABI ----------------

#[unsafe(no_mangle)]
pub extern "Rust" fn abi_version() -> u32 {
    ABI_VERSION
}

#[unsafe(no_mangle)]
pub extern "Rust" fn name() -> &'static str {
    "Clock"
}

#[unsafe(no_mangle)]
pub extern "Rust" fn metadata() -> PluginMetadata {
    PluginMetadata {
        popup_size: Some(CALENDAR_POPUP_SIZE),
        ..PluginMetadata::default()
    }
}

#[unsafe(no_mangle)]
pub extern "Rust" fn model(global_config: Table) -> (PluginModel, Option<Task<PluginMsg>>) {
    // Seed with an immediate tick so the bar shows the time before the
    // first subscription wakeup.
    let model = Model::new(global_config);
    let mut tasks = vec![Task::done(msg(Message::Tick(Local::now())))];
    if model.caldav_config.is_some() {
        tasks.push(Task::done(msg(Message::RefreshCalendar)));
    }
    (plugin_model(model), Some(Task::batch(tasks)))
}

#[unsafe(no_mangle)]
pub extern "Rust" fn update(model: PluginModel, msg_in: PluginMsg) -> Option<Task<PluginMsg>> {
    let m = msg_in.downcast_ref::<Message>()?.clone();
    with_model_write::<Model, _>(&model, |model| match m {
        Message::Tick(now) => {
            model.now = now;
            None
        }
        Message::ToggleCalendar => {
            model.calendar_open = !model.calendar_open;
            if model.calendar_open {
                model.calendar_month = current_month_start();
            }
            let request: PluginMsg = Arc::new(HOST_REQUEST_TOGGLE_POPUP.to_owned());
            let request = Task::done(request);
            if model.calendar_open {
                if let Some(refresh) = start_caldav_refresh(model) {
                    Some(Task::batch(vec![request, refresh]))
                } else {
                    Some(request)
                }
            } else {
                Some(request)
            }
        }
        Message::PreviousMonth => {
            model.calendar_month = add_months(model.calendar_month, -1);
            None
        }
        Message::NextMonth => {
            model.calendar_month = add_months(model.calendar_month, 1);
            None
        }
        Message::OpenDay(date) => {
            let command = model.calendar_command.clone();
            Some(Task::perform(
                async move { open_calendar_day(date, command) },
                |result| msg(Message::CalendarOpenResult(result)),
            ))
        }
        Message::RefreshCalendar => start_caldav_refresh(model),
        Message::CalendarLoaded(result) => {
            model.calendar_pending = false;
            match result {
                Ok(events) => {
                    model.calendar_sync_count = Some(events.len());
                    model.calendar_sync_error = None;
                    model.calendar_events = events;
                }
                Err(error) => {
                    model.calendar_sync_error = Some(error.clone());
                    model.errors.push(error);
                }
            }
            None
        }
        Message::CalendarOpenResult(result) => {
            if let Err(error) = result {
                model.errors.push(error);
            }
            None
        }
    })
    .flatten()
}

fn start_caldav_refresh(model: &mut Model) -> Option<Task<PluginMsg>> {
    if model.calendar_pending {
        return None;
    }
    let config = model.caldav_config.clone()?;
    model.calendar_pending = true;
    Some(Task::perform(
        async move { load_events(config) },
        |result| msg(Message::CalendarLoaded(result)),
    ))
}

#[unsafe(no_mangle)]
pub extern "Rust" fn launch(_focused_index: usize, _model: PluginModel) -> Option<Task<PluginMsg>> {
    // No keyboard-launch semantics for the clock yet. A future binding
    // could map this to "toggle calendar".
    None
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
        // chrono's `format` panics on invalid specifiers at render time, not
        // parse time. Guard with `format_with_items` would be cleaner, but
        // catching errors per-frame and surfacing once is good enough.
        let label = format_time(&model.now, &model.format);
        let font_size = model.font_size;
        let label_text = text(label)
            .size(font_size)
            .align_y(Alignment::Center)
            .align_x(Alignment::Center);
        let label_text = if model.bold && model.font_family.is_none() {
            label_text.font(Font {
                weight: Weight::Bold,
                ..Font::DEFAULT
            })
        } else {
            label_text
        };

        let btn = oxi_plugin::bar_button(label_text).on_press(msg(Message::ToggleCalendar));

        vec![btn.into()]
    })
}

#[unsafe(no_mangle)]
pub extern "Rust" fn popup_view(
    model: PluginModel,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    with_model_read::<Model, _>(&model, |model| {
        let now = Local::now().date_naive();
        let month_start = model.calendar_month;
        let title = month_start.format("%B %Y").to_string();
        let palette = &OXITHEME;

        let header = Row::new()
            .push(calendar_nav_button("‹", Message::PreviousMonth))
            .push(
                text(title)
                    .size(OXITHEME.font_lg)
                    .style(move |_| iced::widget::text::Style {
                        color: Some(palette.primary),
                    })
                    .align_x(Alignment::Center)
                    .width(Length::Fill),
            )
            .push(calendar_nav_button("›", Message::NextMonth))
            .align_y(Alignment::Center)
            .width(Length::Fill);

        let mut column = Column::new()
            .spacing(OXITHEME.padding_sm)
            .padding([OXITHEME.padding_md, OXITHEME.padding_lg])
            .push(header);

        let mut weekdays = Row::new().spacing(OXITHEME.padding_xs).width(Length::Fill);
        for day in ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"] {
            weekdays = weekdays.push(
                text(day)
                    .size(OXITHEME.font_sm)
                    .style(move |_| iced::widget::text::Style {
                        color: Some(palette.primary),
                    })
                    .align_x(Alignment::Center)
                    .width(Length::Fill),
            );
        }
        column = column.push(weekdays);

        let first_offset = month_start.weekday().num_days_from_monday() as i64;
        let grid_start = month_start - ChronoDuration::days(first_offset);
        for week in 0..6 {
            let mut row = Row::new().spacing(OXITHEME.padding_xs).width(Length::Fill);
            for day in 0..7 {
                let date = grid_start + ChronoDuration::days(week * 7 + day);
                let is_current_month =
                    date.year() == month_start.year() && date.month() == month_start.month();
                let is_today = date == now;
                let events = model
                    .calendar_events
                    .iter()
                    .filter(|event| event.date == date)
                    .cloned()
                    .collect::<Vec<_>>();
                row = row.push(day_cell(date, is_current_month, is_today, events));
            }
            column = column.push(row);
        }

        vec![column.width(Length::Fill).height(Length::Fill).into()]
    })
}

fn calendar_nav_button(label: &'static str, message: Message) -> Element<'static, PluginMsg> {
    button(
        text(label)
            .size(CALENDAR_NAV_TEXT_SIZE)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center),
    )
    .on_press(msg(message))
    .style(oxi_plugin::bar_button_style)
    .padding([OXITHEME.padding_xs, OXITHEME.padding_md])
    .height(CALENDAR_NAV_HEIGHT)
    .into()
}

fn day_cell(
    date: NaiveDate,
    is_current_month: bool,
    is_today: bool,
    events: Vec<CalendarEvent>,
) -> Element<'static, PluginMsg> {
    let palette = &OXITHEME;
    let has_event = !events.is_empty();
    let text_color = if is_current_month {
        palette.primary
    } else {
        Color {
            a: ADJACENT_MONTH_ALPHA,
            ..palette.primary
        }
    };
    let background = if is_today {
        Some(Background::Color(palette.primary_bg_hover))
    } else if has_event && is_current_month {
        Some(Background::Color(palette.primary_bg))
    } else {
        None
    };

    let cell = button(
        text(date.day().to_string())
            .size(OXITHEME.font_md)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .style(move |_| iced::widget::text::Style {
                color: Some(text_color),
            }),
    )
    .on_press(msg(Message::OpenDay(date)))
    .style(move |_, status| {
        let background = match status {
            button::Status::Hovered => Some(Background::Color(palette.primary_bg_hover)),
            button::Status::Pressed => Some(Background::Color(palette.primary_bg_active)),
            button::Status::Active | button::Status::Disabled => background,
        };
        button::Style {
            background,
            text_color,
            border: Border {
                radius: Radius::from(OXITHEME.border_radius),
                ..Default::default()
            },
            shadow: Shadow::default(),
            snap: false,
        }
    })
    .padding(0)
    .width(Length::Fill)
    .height(CALENDAR_DAY_CELL_HEIGHT);

    if events.is_empty() {
        cell.into()
    } else {
        tooltip(
            cell,
            calendar_event_tooltip(date, &events),
            tooltip::Position::FollowCursor,
        )
        .gap(OXITHEME.padding_sm)
        .into()
    }
}

fn calendar_event_tooltip(
    date: NaiveDate,
    events: &[CalendarEvent],
) -> Element<'static, PluginMsg> {
    let palette = &OXITHEME;
    let mut content = Column::new()
        .spacing(OXITHEME.padding_sm)
        .width(CALENDAR_TOOLTIP_WIDTH)
        .push(
            text(date.format("%A, %Y-%m-%d").to_string())
                .size(OXITHEME.font_sm)
                .style(move |_| iced::widget::text::Style {
                    color: Some(palette.primary),
                }),
        );

    for event in events {
        let mut item = Column::new()
            .spacing(OXITHEME.padding_xs)
            .width(Length::Fill)
            .push(
                text(event.summary.clone())
                    .size(OXITHEME.font_sm)
                    .style(move |_| iced::widget::text::Style {
                        color: Some(palette.text),
                    })
                    .wrapping(iced::widget::text::Wrapping::Word),
            );
        if let Some(location) = event.location.as_ref() {
            item = item.push(
                text(location.clone())
                    .size(OXITHEME.font_sm)
                    .style(move |_| iced::widget::text::Style {
                        color: Some(palette.text_muted),
                    })
                    .wrapping(iced::widget::text::Wrapping::Word),
            );
        }
        if let Some(description) = event.description.as_ref() {
            item = item.push(
                text(description.clone())
                    .size(OXITHEME.font_sm)
                    .style(move |_| iced::widget::text::Style {
                        color: Some(palette.text_muted),
                    })
                    .wrapping(iced::widget::text::Wrapping::Word),
            );
        }
        content = content.push(item);
    }

    container(content)
        .padding([OXITHEME.padding_sm, OXITHEME.padding_md])
        .style(calendar_tooltip_style)
        .into()
}

fn calendar_tooltip_style(_: &iced::Theme) -> container::Style {
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

fn current_month_start() -> NaiveDate {
    let now = Local::now().date_naive();
    NaiveDate::from_ymd_opt(now.year(), now.month(), 1).expect("valid current year/month")
}

fn add_months(date: NaiveDate, delta: i32) -> NaiveDate {
    let zero_based = date.year() * 12 + date.month0() as i32 + delta;
    let year = zero_based.div_euclid(12);
    let month = zero_based.rem_euclid(12) as u32 + 1;
    NaiveDate::from_ymd_opt(year, month, 1).expect("valid shifted year/month")
}

fn open_calendar_day(date: NaiveDate, command: Option<String>) -> Result<(), String> {
    let Some(command) = command else {
        return Err(
            "clock: set [clock] calendar_command, e.g. `gnome-calendar --date {date}`, or calendar_app = \"thunderbird\"".to_owned(),
        );
    };
    let date_string = date.format("%Y-%m-%d").to_string();
    let command = command
        .replace("{date}", &date_string)
        .replace("{year}", &date.year().to_string())
        .replace("{month}", &format!("{:02}", date.month()))
        .replace("{day}", &format!("{:02}", date.day()));
    let output = Command::new("sh")
        .args(["-c", &command])
        .output()
        .map_err(|e| format!("clock: failed to run calendar_command: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        if stderr.is_empty() {
            Err("clock: calendar_command failed".to_owned())
        } else {
            Err(format!("clock: calendar_command failed: {stderr}"))
        }
    }
}

/// Wrap chrono's panicking `format` so a malformed user spec doesn't take
/// the whole bar down. Falls back to RFC3339 on failure.
fn format_time(now: &DateTime<Local>, fmt: &str) -> String {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    catch_unwind(AssertUnwindSafe(|| now.format(fmt).to_string())).unwrap_or_else(|_| {
        tracing::warn!("clock: invalid format string `{fmt}`, falling back to RFC3339");
        now.to_rfc3339()
    })
}

/// Whether `fmt` contains a strftime specifier that changes within a second
/// (whole seconds or finer). Used to decide whether the display needs a
/// per-second refresh. `%%` is a literal percent and is skipped, as is a bare
/// trailing `%`.
fn format_shows_seconds(fmt: &str) -> bool {
    let bytes = fmt.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'%' {
            i += 1;
            continue;
        }
        i += 1;
        let Some(&spec) = bytes.get(i) else {
            break; // trailing bare `%`
        };
        if spec == b'%' {
            i += 1; // `%%`: literal percent, not a specifier
            continue;
        }
        match spec {
            // Whole-second level.
            b'S' | b'T' | b'r' | b'X' | b'c' | b's' => return true,
            // Fractional seconds (`%f`, `%.3f`, ...).
            b'f' => return true,
            b'.' | b'0'..=b'9' => {
                let mut j = i;
                if spec == b'.' {
                    j += 1;
                    while bytes.get(j).is_some_and(|b| b.is_ascii_digit()) {
                        j += 1;
                    }
                }
                if bytes.get(j) == Some(&b'f') {
                    return true;
                }
                i = j;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    false
}

/// How often the display emits a `Tick`. When the format shows seconds this is
/// 1s so the digits roll over; otherwise it is at most the poll interval but at
/// least once a minute to catch minute/hour/date rollovers. The real clock is
/// only re-read every `tick_seconds`; intermediate ticks are simulated.
fn display_interval(tick_seconds: u64, fmt: &str) -> u64 {
    if format_shows_seconds(fmt) {
        1
    } else {
        tick_seconds.min(MINUTE_DISPLAY_SECONDS)
    }
}

/// Advance an anchor time by the measured monotonic elapsed time, clamped to
/// non-negative so a wall-clock re-sync never overshoots the anchor.
fn simulated_now(anchor: &DateTime<Local>, elapsed: Duration) -> DateTime<Local> {
    let offset = ChronoDuration::from_std(elapsed).unwrap_or_default();
    *anchor + offset
}

/// Tick the model on a fixed interval. Iced's subscription executor isn't
/// guaranteed to run inside a tokio reactor (the `tokio` iced feature only
/// covers `Task::perform`), so we drive the timer from a plain OS thread and
/// bridge into the async stream via `try_send` — same pattern as the
/// workspaces plugin's hyprland listener.
#[unsafe(no_mangle)]
pub extern "Rust" fn subscription() -> *mut PluginStream {
    let poll_interval = TICK
        .get()
        .copied()
        .unwrap_or(Duration::from_secs(DEFAULT_TICK_SECONDS));
    let display_interval = DISPLAY_INTERVAL
        .get()
        .copied()
        .unwrap_or(Duration::from_secs(MINUTE_DISPLAY_SECONDS));

    let s = stream::channel(
        8,
        move |output: iced::futures::channel::mpsc::Sender<PluginMsg>| async move {
            let output = Arc::new(Mutex::new(output));
            let tick_output = output.clone();
            std::thread::spawn(move || {
                // We poll the real clock every `poll_interval` and *simulate*
                // the ticks in between, re-anchoring to `Local::now()` past that
                // point so monotonic drift and clock/NTP changes are corrected.
                let mut anchor = Local::now();
                let mut anchor_at = Instant::now();
                loop {
                    std::thread::sleep(display_interval.min(poll_interval));
                    let elapsed = anchor_at.elapsed();
                    let now = if elapsed >= poll_interval {
                        let now = Local::now();
                        anchor = now;
                        anchor_at = Instant::now();
                        now
                    } else {
                        simulated_now(&anchor, elapsed)
                    };
                    if tick_output
                        .lock()
                        .unwrap()
                        .try_send(msg(Message::Tick(now)))
                        .is_err()
                    {
                        // Channel full or closed; on `closed` we'd want to bail,
                        // but `try_send` on a full mpsc returns `Full` not
                        // `Closed`, so swallow and try again next tick.
                    }
                }
            });
            if let Some(interval) = CALDAV_REFRESH.get().copied() {
                let caldav_output = output.clone();
                std::thread::spawn(move || {
                    loop {
                        std::thread::sleep(interval);
                        let _ = caldav_output
                            .lock()
                            .unwrap()
                            .try_send(msg(Message::RefreshCalendar));
                    }
                });
            }
            // Keep the async task alive forever so iced doesn't drop the stream
            // (and with it the receiving end of the mpsc).
            std::future::pending::<()>().await;
        },
    );

    Box::into_raw(Box::new(s)) as *mut PluginStream
}

// Compile-time sanity check: the stream we're about to box really is a
// `PluginStream`-shaped trait object.
const _: fn() = || {
    fn assert_stream<S: Stream<Item = PluginMsg> + Send + 'static>(_: &S) {}
    let _ = |s: &iced::futures::stream::BoxStream<'static, PluginMsg>| assert_stream(s);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_clock_config_from_top_level_table() {
        let mut clock = Table::new();
        clock.insert(
            "format".to_owned(),
            oxibar_plugin_api::toml::Value::String("%S".to_owned()),
        );
        clock.insert(
            "tick_seconds".to_owned(),
            oxibar_plugin_api::toml::Value::Integer(5),
        );
        clock.insert(
            "font_size".to_owned(),
            oxibar_plugin_api::toml::Value::Float(16.5),
        );
        clock.insert(
            "bold".to_owned(),
            oxibar_plugin_api::toml::Value::Boolean(true),
        );
        clock.insert(
            "calendar_command".to_owned(),
            oxibar_plugin_api::toml::Value::String("calendar {date}".to_owned()),
        );
        let mut global = Table::new();
        global.insert(
            "clock".to_owned(),
            oxibar_plugin_api::toml::Value::Table(clock),
        );

        let config = read_config(&global);

        assert_eq!(config.format, "%S");
        assert_eq!(config.tick_seconds, 5);
        assert_eq!(config.font_size, 16.5);
        assert!(config.bold);
        assert_eq!(config.calendar_command.as_deref(), Some("calendar {date}"));
    }

    #[test]
    fn invalid_clock_config_uses_defaults() {
        let mut clock = Table::new();
        clock.insert(
            "tick_seconds".to_owned(),
            oxibar_plugin_api::toml::Value::Integer(0),
        );
        clock.insert(
            "font_size".to_owned(),
            oxibar_plugin_api::toml::Value::Float(-1.0),
        );
        let mut global = Table::new();
        global.insert(
            "clock".to_owned(),
            oxibar_plugin_api::toml::Value::Table(clock),
        );

        let config = read_config(&global);

        assert_eq!(config.tick_seconds, DEFAULT_TICK_SECONDS);
        assert_eq!(config.font_size, DEFAULT_FONT_SIZE);
    }

    #[test]
    fn format_shows_seconds_detects_second_level_specifiers() {
        assert!(format_shows_seconds("%H:%M:%S"));
        assert!(format_shows_seconds("%T"));
        assert!(format_shows_seconds("%r"));
        assert!(format_shows_seconds("%X"));
        assert!(format_shows_seconds("%c"));
        assert!(format_shows_seconds("%s"));
        assert!(format_shows_seconds("%f"));
        assert!(format_shows_seconds("%.3f"));
    }

    #[test]
    fn format_shows_seconds_ignores_minute_only_and_escaped_percent() {
        assert!(!format_shows_seconds("%H:%M"));
        assert!(!format_shows_seconds("%R"));
        assert!(!format_shows_seconds("%Y-%m-%d"));
        assert!(!format_shows_seconds("%I %p"));
        assert!(!format_shows_seconds("%%S"));
        assert!(!format_shows_seconds("%%"));
        assert!(!format_shows_seconds("100%"));
        assert!(!format_shows_seconds("time %"));
    }

    #[test]
    fn display_interval_is_per_second_for_seconds_formats() {
        assert_eq!(display_interval(60, "%H:%M:%S"), 1);
        assert_eq!(display_interval(300, "%T"), 1);
    }

    #[test]
    fn display_interval_without_seconds_is_at_most_tick_and_a_minute() {
        assert_eq!(display_interval(60, "%H:%M"), 60);
        assert_eq!(display_interval(300, "%H:%M"), 60);
        assert_eq!(display_interval(10, "%H:%M"), 10);
        assert_eq!(display_interval(60, "%Y-%m-%d"), 60);
    }

    #[test]
    fn simulated_now_advances_the_anchor_by_elapsed() {
        let anchor = Local::now();
        let advanced = simulated_now(&anchor, Duration::from_secs(90));
        assert_eq!(advanced - anchor, ChronoDuration::seconds(90));
    }

    #[test]
    fn thunderbird_calendar_app_maps_to_calendar_command() {
        let mut clock = Table::new();
        clock.insert(
            "calendar_app".to_owned(),
            oxibar_plugin_api::toml::Value::String("thunderbird".to_owned()),
        );
        let mut global = Table::new();
        global.insert(
            "clock".to_owned(),
            oxibar_plugin_api::toml::Value::Table(clock),
        );

        let config = read_config(&global);

        assert_eq!(
            config.calendar_command.as_deref(),
            Some("thunderbird --calendar")
        );
    }

    #[test]
    fn reads_caldav_config_and_schedules_initial_refresh() {
        let mut caldav = Table::new();
        caldav.insert(
            "url".to_owned(),
            oxibar_plugin_api::toml::Value::String(
                "https://cloud.example.com/remote.php/dav/calendars/alice/personal/".to_owned(),
            ),
        );
        caldav.insert(
            "username".to_owned(),
            oxibar_plugin_api::toml::Value::String("alice".to_owned()),
        );
        let mut clock = Table::new();
        clock.insert(
            "caldav".to_owned(),
            oxibar_plugin_api::toml::Value::Table(caldav),
        );
        let mut global = Table::new();
        global.insert(
            "clock".to_owned(),
            oxibar_plugin_api::toml::Value::Table(clock),
        );

        let config = read_config(&global);
        let (_plugin_model, init_task) = model(global);

        assert!(config.caldav_config.is_some());
        assert!(config.caldav_config_error.is_none());
        assert!(init_task.is_some());
    }

    #[test]
    fn incomplete_caldav_config_is_visible_state_error() {
        let mut clock = Table::new();
        clock.insert(
            "caldav".to_owned(),
            oxibar_plugin_api::toml::Value::Table(Table::new()),
        );
        let mut global = Table::new();
        global.insert(
            "clock".to_owned(),
            oxibar_plugin_api::toml::Value::Table(clock),
        );

        let (plugin_model, _init_task) = model(global);

        let guard = plugin_model.read().unwrap();
        let model = guard.downcast_ref::<Model>().unwrap();
        assert!(model.caldav_config.is_none());
        assert!(model.calendar_sync_error.is_some());
    }

    #[test]
    fn calendar_loaded_tracks_visible_sync_status() {
        let (plugin_model, _init_task) = model(Table::new());
        let event = CalendarEvent {
            date: NaiveDate::from_ymd_opt(2026, 6, 5).unwrap(),
            summary: "demo".to_owned(),
            description: None,
            location: None,
        };

        let task = update(
            plugin_model.clone(),
            msg(Message::CalendarLoaded(Ok(vec![event]))),
        );

        assert!(task.is_none());
        let guard = plugin_model.read().unwrap();
        let model = guard.downcast_ref::<Model>().unwrap();
        assert_eq!(model.calendar_sync_count, Some(1));
        assert!(model.calendar_sync_error.is_none());
        assert_eq!(model.calendar_events.len(), 1);
    }

    #[test]
    fn month_arithmetic_crosses_year_boundaries() {
        let jan = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
        let dec = NaiveDate::from_ymd_opt(2023, 12, 1).unwrap();
        let feb = NaiveDate::from_ymd_opt(2024, 2, 1).unwrap();

        assert_eq!(add_months(jan, -1), dec);
        assert_eq!(add_months(jan, 1), feb);
    }

    #[test]
    fn update_errors_are_drained() {
        let (plugin_model, init_task) = model(Table::new());
        assert!(init_task.is_some());
        assert_eq!(name(), "Clock");
        assert_eq!(abi_version(), ABI_VERSION);

        let task = update(
            plugin_model.clone(),
            msg(Message::CalendarOpenResult(Err("boom".to_owned()))),
        );
        assert!(task.is_none());

        assert_eq!(errors(plugin_model.clone()), vec!["boom"]);
        assert!(errors(plugin_model).is_empty());
    }
}
