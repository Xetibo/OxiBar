//! Clock plugin.
//!
//! Renders the current local time in a button. Format string and tick
//! interval come from the `[plugins.clock]` block in `config.toml`:
//!
//! ```toml
//! [plugins.clock]
//! format = "%H:%M"      # strftime, default "%H:%M"
//! tick_seconds = 60     # how often to refresh, default 60
//! ```
//!
//! Clicking the time toggles a host-owned calendar popup.

use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::Duration;

use chrono::{DateTime, Datelike, Duration as ChronoDuration, Local, NaiveDate};
use iced::{
    Alignment, Background, Border, Color, Element, Font, Length, Shadow, Task,
    border::Radius,
    font::Weight,
    futures::Stream,
    stream,
    widget::{Column, Row, button, text},
};
use oxibar_plugin_api::{
    ABI_VERSION, HOST_REQUEST_TOGGLE_POPUP, OxiAny, PluginModel, PluginMsg, PluginStream,
    toml::Table,
};
use oxiced::theme::theme_impl::OXITHEME;

const DEFAULT_FORMAT: &str = "%H:%M";
const DEFAULT_TICK_SECONDS: u64 = 60;
const DEFAULT_FONT_SIZE: f32 = 14.0;
const DEFAULT_BOLD: bool = false;

/// Tick interval shared with `subscription()`. Set during `model()` so the
/// subscription thread can read it without the model being passed in.
static TICK: OnceLock<Duration> = OnceLock::new();

#[derive(Debug)]
pub struct Model {
    format: String,
    font_size: f32,
    bold: bool,
    /// Resolved font family name from `[bar] font` (or `None` to use iced's
    /// default). Leaked to `&'static str` because [`iced::Font`] requires it.
    font_family: Option<&'static str>,
    calendar_command: Option<String>,
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
        .map(ToOwned::to_owned);
    ClockConfig {
        format,
        tick_seconds,
        font_size,
        bold,
        calendar_command,
    }
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
pub extern "Rust" fn model(global_config: Table) -> (PluginModel, Option<Task<PluginMsg>>) {
    let m: Box<dyn OxiAny> = Box::new(Model::new(global_config));
    // Seed with an immediate tick so the bar shows the time before the
    // first subscription wakeup.
    let task = Task::done(msg(Message::Tick(Local::now())));
    (Arc::new(RwLock::new(m)), Some(task))
}

#[unsafe(no_mangle)]
pub extern "Rust" fn update(model: PluginModel, msg_in: PluginMsg) -> Option<Task<PluginMsg>> {
    let mut guard = model.try_write().ok()?;
    let model = guard.downcast_mut::<Model>()?;
    let m = msg_in.downcast_ref::<Message>()?.clone();
    match m {
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
            Some(Task::done(request))
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
        Message::CalendarOpenResult(result) => {
            if let Err(error) = result {
                model.errors.push(error);
            }
            None
        }
    }
}

#[unsafe(no_mangle)]
pub extern "Rust" fn launch(_focused_index: usize, _model: PluginModel) -> Option<Task<PluginMsg>> {
    // No keyboard-launch semantics for the clock yet. A future binding
    // could map this to "toggle calendar".
    None
}

#[unsafe(no_mangle)]
pub extern "Rust" fn errors(model: PluginModel) -> Vec<String> {
    let Ok(mut guard) = model.try_write() else {
        return Vec::new();
    };
    let Some(m) = guard.downcast_mut::<Model>() else {
        return Vec::new();
    };
    std::mem::take(&mut m.errors)
}

#[unsafe(no_mangle)]
pub extern "Rust" fn view(
    model: PluginModel,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    let lock = model.try_read().map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::WouldBlock, "model is write-locked")
    })?;
    let model = lock.downcast_ref::<Model>().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "model has wrong type")
    })?;

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

    // Transparent button: text in the primary accent color, no background
    // until hovered. Hover paints a subtle `primary_bg_hover` so the user
    // knows it's clickable. Pressed darkens slightly via `primary_bg_active`.
    let style = |_: &iced::Theme, status: button::Status| -> button::Style {
        let palette = &OXITHEME;
        let base = button::Style {
            background: None,
            text_color: palette.primary,
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: Radius::from(palette.border_radius),
            },
            shadow: Shadow::default(),
            snap: false,
        };
        match status {
            button::Status::Active | button::Status::Disabled => base,
            button::Status::Hovered => button::Style {
                background: Some(Background::Color(palette.primary_bg_hover)),
                ..base
            },
            button::Status::Pressed => button::Style {
                background: Some(Background::Color(palette.primary_bg_active)),
                ..base
            },
        }
    };

    let btn = button(label_text)
        .on_press(msg(Message::ToggleCalendar))
        .style(style)
        .padding([0, 8])
        .height(22.5)
        .width(Length::Shrink);

    Ok(vec![btn.into()])
}

#[unsafe(no_mangle)]
pub extern "Rust" fn popup_view(
    model: PluginModel,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    let lock = model.try_read().map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::WouldBlock, "model is write-locked")
    })?;
    let model = lock.downcast_ref::<Model>().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "model has wrong type")
    })?;
    let now = Local::now().date_naive();
    let month_start = model.calendar_month;
    let title = month_start.format("%B %Y").to_string();
    let palette = &OXITHEME;

    let header = Row::new()
        .push(calendar_nav_button("‹", Message::PreviousMonth))
        .push(
            text(title)
                .size(18)
                .style(move |_| iced::widget::text::Style {
                    color: Some(palette.primary),
                })
                .align_x(Alignment::Center)
                .width(Length::Fill),
        )
        .push(calendar_nav_button("›", Message::NextMonth))
        .align_y(Alignment::Center)
        .width(Length::Fill);

    let mut column = Column::new().spacing(8).padding([12, 14]).push(header);

    let mut weekdays = Row::new().spacing(4).width(Length::Fill);
    for day in ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"] {
        weekdays = weekdays.push(
            text(day)
                .size(12)
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
        let mut row = Row::new().spacing(4).width(Length::Fill);
        for day in 0..7 {
            let date = grid_start + ChronoDuration::days(week * 7 + day);
            let is_current_month = date.month() == now.month();
            let is_today = date == now;
            row = row.push(day_cell(date, is_current_month, is_today));
        }
        column = column.push(row);
    }

    Ok(vec![column.width(Length::Fill).height(Length::Fill).into()])
}

fn calendar_nav_button(label: &'static str, message: Message) -> Element<'static, PluginMsg> {
    button(
        text(label)
            .size(20)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center),
    )
    .on_press(msg(message))
    .style(clock_button_style)
    .padding([2, 10])
    .height(28)
    .into()
}

fn day_cell(
    date: NaiveDate,
    is_current_month: bool,
    is_today: bool,
) -> Element<'static, PluginMsg> {
    let palette = &OXITHEME;
    let text_color = if is_current_month {
        palette.primary
    } else {
        Color {
            a: 0.45,
            ..palette.primary
        }
    };
    let background = if is_today {
        Some(Background::Color(palette.primary_bg_hover))
    } else {
        None
    };

    button(
        text(date.day().to_string())
            .size(13)
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
                radius: Radius::from(8.0),
                ..Default::default()
            },
            shadow: Shadow::default(),
            snap: false,
        }
    })
    .padding(0)
    .width(Length::Fill)
    .height(30)
    .into()
}

fn clock_button_style(_: &iced::Theme, status: button::Status) -> button::Style {
    let base = button::Style {
        background: None,
        text_color: OXITHEME.primary,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 8.0.into(),
        },
        shadow: Shadow::default(),
        snap: false,
    };
    match status {
        button::Status::Hovered => button::Style {
            background: Some(Background::Color(OXITHEME.primary_bg_hover)),
            ..base
        },
        button::Status::Pressed => button::Style {
            background: Some(Background::Color(OXITHEME.primary_bg_active)),
            ..base
        },
        button::Status::Active | button::Status::Disabled => base,
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
            "clock: set [clock] calendar_command, e.g. `gnome-calendar --date {date}`".to_owned(),
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

/// Tick the model on a fixed interval. Iced's subscription executor isn't
/// guaranteed to run inside a tokio reactor (the `tokio` iced feature only
/// covers `Task::perform`), so we drive the timer from a plain OS thread and
/// bridge into the async stream via `try_send` — same pattern as the
/// workspaces plugin's hyprland listener.
#[unsafe(no_mangle)]
pub extern "Rust" fn subscription() -> *mut PluginStream {
    let interval = TICK
        .get()
        .copied()
        .unwrap_or(Duration::from_secs(DEFAULT_TICK_SECONDS));

    let s = stream::channel(
        8,
        move |output: iced::futures::channel::mpsc::Sender<PluginMsg>| async move {
            let output = Arc::new(Mutex::new(output));
            std::thread::spawn(move || {
                loop {
                    let now = Local::now();
                    if output
                        .lock()
                        .unwrap()
                        .try_send(msg(Message::Tick(now)))
                        .is_err()
                    {
                        // Channel full or closed; on `closed` we'd want to bail,
                        // but `try_send` on a full mpsc returns `Full` not
                        // `Closed`, so swallow and try again next tick.
                    }
                    std::thread::sleep(interval);
                }
            });
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
