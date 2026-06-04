//! Oxibar entry point and the iced application driving the bar.
//!
//! TODO (architectural):
//! - `WINDOW_SIZE` is hardcoded for one monitor. Should be derived from the
//!   active wayland output (`wl_output` advertise width) or a config knob.
//! - Replace the raw `*mut Stream` ABI with an `extern "C"` callback channel
//!   (see `oxibar-plugin-api`).

use std::pin::Pin;

use iced::widget::Container;
use iced::{
    Alignment, Element, Font, Length, Point, Rectangle, Subscription, Task, Theme,
    theme::Style,
    widget::{Column, Row, Space, Stack, canvas},
};
use iced_layershell::{
    Settings,
    actions::{ActionCallback, LayershellCustomAction, LayershellCustomActionWithId},
    reexport::{Anchor, IcedId, KeyboardInteractivity, Layer},
    settings::LayerShellSettings,
};
use once_cell::sync::Lazy;
use oxibar_plugin_api::{HOST_REQUEST_TOGGLE_POPUP, PluginMsg, PluginStream, SubscriptionFn};
use oxiced::theme::theme_impl::OXITHEME;
use oxiced::{theme::theme_impl::get_derived_iced_theme, widgets::oxi_layer::layer_theme};
use toml::Table;
use tracing::error;

use crate::config::get_config;
use crate::plugins::{
    PluginMap, dispatch_update, drain_errors, load_plugins, render_plugin, render_plugin_popup,
};

pub mod config;
pub mod plugins;

static CONFIG: Lazy<Table> = Lazy::new(get_config);

const WINDOW_SIZE: (u32, u32) = (3440, 25);
const SCALE_FACTOR: f32 = 1.0;
const WINDOW_MARGINS: (i32, i32, i32, i32) = (0, 0, 0, 0);
const WINDOW_KEYBOARD_MODE: KeyboardInteractivity = KeyboardInteractivity::OnDemand;
const EXCLUSIVE_ZONE: i32 = 25;
const DEFAULT_FONT: &str = "Adwaita Sans";
const POPUP_SIZE: (u32, u32) = (320, 300);
const POPUP_CONNECTOR_WIDTH: u32 = 352;
const POPUP_CONNECTOR_HEIGHT: u32 = 0;

/// Read `[bar] font` from the global config, falling back to [`DEFAULT_FONT`]
/// when unset. Exposed as a leaked `&'static str` because both
/// `Font::with_name` and `iced::application::default_font` want a `'static`
/// reference and the value lives for the whole program.
fn bar_font() -> &'static str {
    CONFIG
        .get("bar")
        .and_then(|v| v.as_table())
        .and_then(|t| t.get("font"))
        .and_then(|v| v.as_str())
        .map(|s| &*Box::leak(s.to_owned().into_boxed_str()))
        .unwrap_or(DEFAULT_FONT)
}

/// Read `[bar] font_file` and load the file's bytes into a leaked `'static`
/// slice so iced can register it. Returning `Some` forces iced/cosmic-text
/// to add the font to its database regardless of whether fontdb's directory
/// scan / fontconfig-parser would have found it on disk — this is the
/// reliable path for nix / home-manager font setups whose paths
/// (`~/.nix-profile/share/fonts/...`, home-manager's `share/fonts/...`)
/// aren't in fontdb's hardcoded scan list.
fn bar_font_bytes() -> Option<&'static [u8]> {
    let path = CONFIG
        .get("bar")
        .and_then(|v| v.as_table())
        .and_then(|t| t.get("font_file"))
        .and_then(|v| v.as_str())?;
    match std::fs::read(path) {
        Ok(bytes) => Some(Box::leak(bytes.into_boxed_slice())),
        Err(e) => {
            error!("could not read [bar] font_file `{path}`: {e}");
            None
        }
    }
}

pub fn main() -> Result<(), iced_layershell::Error> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let settings = Settings {
        layer_settings: LayerShellSettings {
            size: Some((WINDOW_SIZE.0, WINDOW_SIZE.1 + POPUP_SIZE.1)),
            exclusive_zone: EXCLUSIVE_ZONE,
            anchor: Anchor::Top,
            layer: Layer::Background,
            margin: WINDOW_MARGINS,
            keyboard_interactivity: WINDOW_KEYBOARD_MODE,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut app =
        iced_layershell::daemon(OxiBar::new, OxiBar::namespace, OxiBar::update, OxiBar::view)
            .subscription(OxiBar::subscription)
            .settings(settings)
            .theme(OxiBar::theme)
            .default_font(Font::with_name(bar_font()))
            .style(OxiBar::style)
            .scale_factor(OxiBar::scale_factor);
    if let Some(bytes) = bar_font_bytes() {
        app = app.font(bytes);
    }
    app.run()
}

struct OxiBar {
    theme: Theme,
    plugins: PluginMap,
    /// When true, the bar's container background is fully transparent so
    /// only plugin widgets and the compositor's wallpaper show through.
    /// Read once at startup from `[bar] transparent = true` in `config.toml`.
    transparent: bool,
    /// Plugin names (in order) to render in the start (left) section.
    /// Read once at startup from `[bar] start = [...]`.
    start_widgets: Vec<String>,
    /// Plugin names (in order) to render in the center section.
    /// Read once at startup from `[bar] center = [...]`.
    center_widgets: Vec<String>,
    /// Plugin names (in order) to render in the end (right) section.
    /// Read once at startup from `[bar] end = [...]`.
    end_widgets: Vec<String>,
    popup_plugin: Option<String>,
}

#[derive(Clone, Copy, Debug)]
pub enum BarSection {
    Start,
    Center,
    End,
}

impl TryInto<iced_layershell::actions::LayershellCustomActionWithId> for Message {
    type Error = Self;
    fn try_into(
        self,
    ) -> Result<iced_layershell::actions::LayershellCustomActionWithId, Self::Error> {
        match self {
            Message::SetPopupInputRegion(open, section) => Ok(LayershellCustomActionWithId::new(
                None,
                LayershellCustomAction::SetInputRegion(ActionCallback::new(move |region| {
                    region.add(0, 0, WINDOW_SIZE.0 as i32, WINDOW_SIZE.1 as i32);
                    if open {
                        region.add(
                            popup_x(section),
                            WINDOW_SIZE.1 as i32,
                            POPUP_CONNECTOR_WIDTH as i32,
                            POPUP_SIZE.1 as i32,
                        );
                    }
                })),
            )),
            message => Err(message),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Exit,
    PluginSubMsg(String, PluginMsg),
    TogglePluginPopup(String),
    SetPopupPlugin(Option<String>),
    SetPopupInputRegion(bool, BarSection),
}

impl OxiBar {
    fn new() -> (Self, Task<Message>) {
        let (plugins, plugin_tasks) = load_plugins(&CONFIG);
        let bar_table = CONFIG.get("bar").and_then(|v| v.as_table());
        let transparent = bar_table
            .and_then(|t| t.get("transparent"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let read_section = |key: &str| -> Vec<String> {
            bar_table
                .and_then(|t| t.get(key))
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_owned()))
                        .collect()
                })
                .unwrap_or_default()
        };
        let start_widgets = read_section("start");
        let center_widgets = read_section("center");
        let end_widgets = read_section("end");
        // Backward-compat: if no sections were configured at all, place every
        // loaded plugin in the start section (the previous single-row layout).
        let start_widgets =
            if start_widgets.is_empty() && center_widgets.is_empty() && end_widgets.is_empty() {
                plugins.keys().cloned().collect()
            } else {
                start_widgets
            };
        let bar = Self {
            plugins,
            theme: get_derived_iced_theme(),
            transparent,
            start_widgets,
            center_widgets,
            end_widgets,
            popup_plugin: None,
        };
        let mut tasks = plugin_tasks;
        tasks.push(Task::done(Message::SetPopupInputRegion(
            false,
            BarSection::End,
        )));
        (bar, Task::batch(tasks))
    }

    fn namespace() -> String {
        String::from("OxiBar")
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Exit => std::process::exit(0),
            Message::SetPopupInputRegion(_, _) => Task::none(),
            Message::SetPopupPlugin(plugin_id) => {
                self.popup_plugin = plugin_id;
                Task::none()
            }
            Message::TogglePluginPopup(plugin_id) => self.toggle_plugin_popup(plugin_id),
            Message::PluginSubMsg(plugin_id, msg) => {
                let Some((model, funcs)) = self.plugins.get(&plugin_id) else {
                    error!("message for unknown plugin `{plugin_id}`");
                    return Task::none();
                };
                let task = dispatch_update(funcs, model.clone(), msg);
                drain_errors(funcs, model);
                match task {
                    Some(task) => {
                        let id = plugin_id.clone();
                        task.map(move |msg| map_plugin_message(id.clone(), msg))
                    }
                    None => Task::none(),
                }
            }
        }
    }

    fn view(&self, _: IcedId) -> Element<'_, Message> {
        let start = Container::new(Row::from_vec(self.render_section(&self.start_widgets)))
            .align_x(Alignment::Start)
            .align_y(Alignment::Center)
            .width(Length::Fill)
            .height(Length::Fill);
        let center = Container::new(Row::from_vec(self.render_section(&self.center_widgets)))
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .width(Length::Fill)
            .height(Length::Fill);
        let end = Container::new(Row::from_vec(self.render_section(&self.end_widgets)))
            .align_x(Alignment::End)
            .align_y(Alignment::Center)
            .width(Length::Fill)
            .height(Length::Fill);

        let row = Row::new()
            .push(start)
            .push(center)
            .push(end)
            .width(Length::Fill)
            .height(Length::Fill);
        let transparent = self.transparent;

        let bar = Container::new(row)
            .style(move |theme: &Theme| Self::box_style(theme, transparent))
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .width(Length::Fill)
            .height(WINDOW_SIZE.1 as f32);

        if self.popup_plugin.is_none() {
            return bar.into();
        }

        Container::new(
            Column::new()
                .push(bar)
                .push(self.popup_row())
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn popup_row(&self) -> Element<'_, Message> {
        let empty = || Space::new().into();
        let Some(plugin_id) = self.popup_plugin.as_deref() else {
            return Space::new().height(POPUP_SIZE.1 as f32).into();
        };
        let (start_content, center_content, end_content): (
            Element<'_, Message>,
            Element<'_, Message>,
            Element<'_, Message>,
        ) = match self.plugin_section(plugin_id) {
            BarSection::Start => (self.popup_view(plugin_id), empty(), empty()),
            BarSection::Center => (empty(), self.popup_view(plugin_id), empty()),
            BarSection::End => (empty(), empty(), self.popup_view(plugin_id)),
        };

        let start = Container::new(start_content)
            .align_x(Alignment::Start)
            .align_y(Alignment::Start)
            .width(Length::Fill)
            .height(Length::Fill);
        let center = Container::new(center_content)
            .align_x(Alignment::Center)
            .align_y(Alignment::Start)
            .width(Length::Fill)
            .height(Length::Fill);
        let end = Container::new(end_content)
            .align_x(Alignment::End)
            .align_y(Alignment::Start)
            .width(Length::Fill)
            .height(Length::Fill);

        Row::new()
            .push(start)
            .push(center)
            .push(end)
            .width(Length::Fill)
            .height(POPUP_SIZE.1 as f32)
            .into()
    }

    fn plugin_section(&self, plugin_id: &str) -> BarSection {
        if self
            .start_widgets
            .iter()
            .any(|name| name.eq_ignore_ascii_case(plugin_id))
        {
            BarSection::Start
        } else if self
            .center_widgets
            .iter()
            .any(|name| name.eq_ignore_ascii_case(plugin_id))
        {
            BarSection::Center
        } else {
            BarSection::End
        }
    }

    fn toggle_plugin_popup(&mut self, plugin_id: String) -> Task<Message> {
        let Some((_model, funcs)) = self.plugins.get(&plugin_id) else {
            tracing::warn!(plugin = plugin_id, "unknown plugin requested popup");
            return Task::none();
        };
        if funcs.popup_view.is_none() {
            tracing::warn!(
                plugin = plugin_id,
                "plugin requested popup but has no popup_view"
            );
            return Task::none();
        }

        let section = self.plugin_section(&plugin_id);
        if self.popup_plugin.as_deref() == Some(plugin_id.as_str()) {
            Task::done(Message::SetPopupPlugin(None))
                .chain(Task::done(Message::SetPopupInputRegion(false, section)))
        } else {
            Task::done(Message::SetPopupInputRegion(true, section))
                .chain(Task::done(Message::SetPopupPlugin(Some(plugin_id))))
        }
    }

    fn popup_view(&self, plugin_id: &str) -> Element<'_, Message> {
        let Some((model, funcs)) = self.plugins.get(plugin_id) else {
            return Space::new().into();
        };
        let popup_elements: Vec<Element<'static, Message>> = render_plugin_popup(funcs, model)
            .into_iter()
            .map(|el| {
                let id = plugin_id.to_owned();
                el.map(move |msg| map_plugin_message(id.clone(), msg))
            })
            .collect();
        if popup_elements.is_empty() {
            tracing::warn!(plugin = plugin_id, "plugin popup_view returned no elements");
        }
        let popup_content = Column::from_vec(popup_elements)
            .width(Length::Fill)
            .height(Length::Fill);

        let body_height = POPUP_SIZE.1 - POPUP_CONNECTOR_HEIGHT;
        let background = canvas(PopupBackground)
            .width(POPUP_CONNECTOR_WIDTH as f32)
            .height(POPUP_SIZE.1 as f32);
        let content = Container::new(
            Column::new()
                .push(Space::new().height(POPUP_CONNECTOR_HEIGHT as f32))
                .push(
                    Container::new(popup_content)
                        .width(POPUP_SIZE.0 as f32)
                        .height(body_height as f32),
                )
                .align_x(Alignment::Center),
        )
        .width(POPUP_CONNECTOR_WIDTH as f32)
        .height(POPUP_SIZE.1 as f32)
        .align_x(Alignment::Center)
        .align_y(Alignment::Start);

        Stack::new()
            .push(background)
            .push(content)
            .width(POPUP_CONNECTOR_WIDTH as f32)
            .height(POPUP_SIZE.1 as f32)
            .into()
    }

    /// Render every plugin named in `names` (in order), skipping any names
    /// that don't correspond to a loaded plugin. Used by `view()` to build
    /// each of the start / center / end sections. Name matching is
    /// case-insensitive so users can write `"clock"` even though the plugin
    /// reports itself as `"Clock"`.
    fn render_section(&self, names: &[String]) -> Vec<Element<'static, Message>> {
        names
            .iter()
            .flat_map(|name| {
                let Some((key, (model, funcs))) = self
                    .plugins
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case(name))
                else {
                    tracing::warn!(
                        "bar section references unknown plugin `{name}` \
                         (loaded plugins: {:?})",
                        self.plugins.keys().collect::<Vec<_>>()
                    );
                    return Vec::new();
                };
                let id = key.clone();
                render_plugin(funcs, model)
                    .into_iter()
                    .map(move |el| {
                        let id = id.clone();
                        el.map(move |msg| Message::PluginSubMsg(id.clone(), msg))
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    fn box_style(theme: &Theme, transparent: bool) -> iced::widget::container::Style {
        if transparent {
            // Fully transparent surface — let the wallpaper / compositor
            // background show through. Border + shadow zeroed so there's no
            // residual chrome.
            return iced::widget::container::Style {
                background: Some(iced::Background::Color(iced::Color::TRANSPARENT)),
                ..Default::default()
            };
        }
        let palette = &OXITHEME;
        iced::widget::container::Style {
            background: Some(iced::Background::Color(palette.mantle)),
            ..iced::widget::container::rounded_box(theme)
        }
    }

    fn theme(&self, _: IcedId) -> Theme {
        self.theme.clone()
    }

    fn subscription(&self) -> Subscription<Message> {
        // `Subscription::run_with` requires a bare `fn` pointer (so it can be
        // hashed for identity) and `Subscription::map` requires a
        // *non-capturing* closure. We can't capture the plugin id directly,
        // so we attach it via `.with(id)` (which makes the items `(id, msg)`
        // tuples) and then map a non-capturing closure that destructures.
        //
        // The plugin's `subscription` fn pointer is re-encoded as `usize`
        // because raw fn pointers aren't `Hash`. Soundness: the backing
        // `Library` is owned by `Arc<PluginFuncs>` in `self.plugins` and
        // dropped only at shutdown, so the fn pointer remains valid for as
        // long as iced may invoke this builder.
        let subs: Vec<Subscription<Message>> = self
            .plugins
            .iter()
            .map(|(plugin_id, (_model, funcs))| {
                let sub_fn_addr = funcs.subscription as usize;
                Subscription::run_with((plugin_id.clone(), sub_fn_addr), build_plugin_stream)
                    .with(plugin_id.clone())
                    .map(|(id, msg): (String, PluginMsg)| map_plugin_message(id, msg))
            })
            .collect();
        Subscription::batch(subs)
    }

    fn style(&self, _: &Theme) -> Style {
        layer_theme()
    }

    fn scale_factor(&self, _: IcedId) -> f32 {
        SCALE_FACTOR
    }
}

fn map_plugin_message(plugin_id: String, msg: PluginMsg) -> Message {
    if msg
        .downcast_ref::<String>()
        .is_some_and(|request| request == HOST_REQUEST_TOGGLE_POPUP)
    {
        Message::TogglePluginPopup(plugin_id)
    } else {
        Message::PluginSubMsg(plugin_id, msg)
    }
}

struct PopupBackground;

impl<Message> canvas::Program<Message> for PopupBackground {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &Theme,
        _bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let palette = &OXITHEME;
        let mut frame = canvas::Frame::new(
            renderer,
            iced::Size::new(POPUP_CONNECTOR_WIDTH as f32, POPUP_SIZE.1 as f32),
        );

        let w = POPUP_CONNECTOR_WIDTH as f32;
        let h = POPUP_SIZE.1 as f32;
        let body_w = POPUP_SIZE.0 as f32;
        let ch = POPUP_CONNECTOR_HEIGHT as f32;
        let inset = (w - body_w) / 2.0;
        let body_left = inset;
        let body_right = body_left + body_w;
        let radius = 20.0 as f32;//palette.border_radius as f32;
        let top_radius = radius.min(ch / 2.0);
        let shoulder_radius = inset.min(radius).max(0.0);
        let bottom_radius = radius.min(body_w / 2.0);

        let shape = canvas::Path::new(|path| {
            path.move_to(Point::new(top_radius, 0.0));
            path.line_to(Point::new(w - top_radius, 0.0));
            path.quadratic_curve_to(Point::new(w, 0.0), Point::new(w, top_radius));
            path.line_to(Point::new(w, ch));
            path.line_to(Point::new(body_right + shoulder_radius, ch));
            path.quadratic_curve_to(
                Point::new(body_right, ch),
                Point::new(body_right, ch + shoulder_radius),
            );
            path.line_to(Point::new(body_right, h - bottom_radius));
            path.quadratic_curve_to(
                Point::new(body_right, h),
                Point::new(body_right - bottom_radius, h),
            );
            path.line_to(Point::new(body_left + bottom_radius, h));
            path.quadratic_curve_to(
                Point::new(body_left, h),
                Point::new(body_left, h - bottom_radius),
            );
            path.line_to(Point::new(body_left, ch + shoulder_radius));
            path.quadratic_curve_to(
                Point::new(body_left, ch),
                Point::new(body_left - shoulder_radius, ch),
            );
            path.line_to(Point::new(0.0, ch));
            path.line_to(Point::new(0.0, top_radius));
            path.quadratic_curve_to(Point::new(0.0, 0.0), Point::new(top_radius, 0.0));
            path.close();
        });

        frame.fill(&shape, palette.mantle);
        vec![frame.into_geometry()]
    }
}

fn popup_x(section: BarSection) -> i32 {
    match section {
        BarSection::Start => 0,
        BarSection::Center => (WINDOW_SIZE.0.saturating_sub(POPUP_CONNECTOR_WIDTH) / 2) as i32,
        BarSection::End => WINDOW_SIZE.0.saturating_sub(POPUP_CONNECTOR_WIDTH) as i32,
    }
}

/// Reconstitute the plugin's boxed stream into a pinned `Box`. Required to
/// be a bare `fn` (no captures) by `Subscription::run_with`.
fn build_plugin_stream(data: &(String, usize)) -> Pin<Box<PluginStream>> {
    // SAFETY: `data.1` was produced by casting a `SubscriptionFn` (an
    // `unsafe extern "Rust" fn`) to `usize` in `subscription()`. The
    // backing dylib outlives this call because `OxiBar.plugins` holds the
    // `Arc<Library>` and is dropped only on shutdown. The returned raw
    // pointer is a `Box<PluginStream>` from the plugin's allocator (same
    // global allocator as the host, since both are built with the same
    // toolchain and link the same `oxibar-plugin-api`).
    let sub_fn: SubscriptionFn = unsafe { std::mem::transmute(data.1) };
    let raw = unsafe { sub_fn() };
    unsafe { Pin::new_unchecked(Box::from_raw(raw)) }
}
