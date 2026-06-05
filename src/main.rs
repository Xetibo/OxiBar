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
use iced_anim::{AnimationBuilder, Motion};
use iced_layershell::{
    Settings,
    actions::{ActionCallback, LayershellCustomAction, LayershellCustomActionWithId},
    reexport::{Anchor, IcedId, KeyboardInteractivity, Layer, NewLayerShellSettings, OutputOption},
    settings::LayerShellSettings,
};
use once_cell::sync::Lazy;
use oxibar_plugin_api::{
    HOST_REQUEST_CLOSE_MODAL, HOST_REQUEST_OPEN_MODAL, HOST_REQUEST_TOGGLE_PANEL,
    HOST_REQUEST_TOGGLE_POPUP, PluginMsg, PluginStream, SubscriptionFn,
};
use oxiced::theme::theme_impl::OXITHEME;
use oxiced::{theme::theme_impl::get_derived_iced_theme, widgets::oxi_layer::layer_theme};
use toml::Table;
use tracing::error;

use crate::config::get_config;
use crate::plugins::{
    PluginMap, dispatch_update, drain_errors, load_plugins, render_plugin, render_plugin_modal,
    render_plugin_panel, render_plugin_popup,
};

pub mod config;
pub mod plugins;

static CONFIG: Lazy<Table> = Lazy::new(get_config);

const WINDOW_SIZE: (u32, u32) = (3440, 31);
const SCALE_FACTOR: f32 = 1.0;
const WINDOW_MARGINS: (i32, i32, i32, i32) = (0, 0, 0, 0);
const WINDOW_KEYBOARD_MODE: KeyboardInteractivity = KeyboardInteractivity::None;
const EXCLUSIVE_ZONE: i32 = WINDOW_SIZE.1 as i32;
const DEFAULT_FONT: &str = "Adwaita Sans";
const DEFAULT_POPUP_SIZE: (u32, u32) = (320, 300);
const LARGE_POPUP_SIZE: (u32, u32) = (460, 420);
const POPUP_MAX_HEIGHT: u32 = LARGE_POPUP_SIZE.1;
const POPUP_CONNECTOR_PADDING: u32 = 32;
const POPUP_CONNECTOR_HEIGHT: u32 = 0;
const MODAL_SIZE: (u32, u32) = (460, 300);
const PANEL_WIDTH: u32 = 420;

/// Read `[bar] font` from the global config, falling back to [`DEFAULT_FONT`]
/// when unset. Exposed as a leaked `&'static str` because both
/// `Font::with_name` and `iced::application::default_font` want a `'static`
/// reference and the value lives for the whole program.
fn configured_bar_font() -> &'static str {
    CONFIG
        .get("bar")
        .and_then(|v| v.as_table())
        .and_then(|t| t.get("font"))
        .and_then(|v| v.as_str())
        .map(|s| &*Box::leak(s.to_owned().into_boxed_str()))
        .unwrap_or(DEFAULT_FONT)
}

fn bar_font() -> &'static str {
    let configured = configured_bar_font();
    fontconfig_value(configured, "%{family}")
        .and_then(|family| {
            family
                .split(',')
                .next()
                .map(str::trim)
                .filter(|family| !family.is_empty())
                .map(ToOwned::to_owned)
        })
        .map(|family| &*Box::leak(family.into_boxed_str()))
        .unwrap_or(configured)
}

/// Read `[bar] font_file` and load the file's bytes into a leaked `'static`
/// slice so iced can register it. Returning `Some` forces iced/cosmic-text
/// to add the font to its database regardless of whether fontdb's directory
/// scan / fontconfig-parser would have found it on disk — this is the
/// reliable path for nix / home-manager font setups whose paths
/// (`~/.nix-profile/share/fonts/...`, home-manager's `share/fonts/...`)
/// aren't in fontdb's hardcoded scan list.
fn bar_font_bytes() -> Option<&'static [u8]> {
    let configured_path = CONFIG
        .get("bar")
        .and_then(|v| v.as_table())
        .and_then(|t| t.get("font_file"))
        .and_then(|v| v.as_str());
    let path = configured_path
        .map(ToOwned::to_owned)
        .or_else(|| fontconfig_value(configured_bar_font(), "%{file}"))?;
    read_font_bytes(&path)
}

fn read_font_bytes(path: &str) -> Option<&'static [u8]> {
    match std::fs::read(path) {
        Ok(bytes) => Some(Box::leak(bytes.into_boxed_slice())),
        Err(e) => {
            error!("could not read bar font file `{path}`: {e}");
            None
        }
    }
}

fn fontconfig_value(family: &str, format: &str) -> Option<String> {
    let output = std::process::Command::new("fc-match")
        .args(["-f", format, family])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if value.is_empty() { None } else { Some(value) }
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
            size: Some((WINDOW_SIZE.0, WINDOW_SIZE.1 + POPUP_MAX_HEIGHT)),
            exclusive_zone: EXCLUSIVE_ZONE,
            anchor: Anchor::Top,
            layer: Layer::Top,
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
    popup_open: bool,
    modal_plugin: Option<String>,
    modal_window_id: Option<IcedId>,
    panel_plugin: Option<String>,
    panel_window_id: Option<IcedId>,
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
            Message::SetPopupInputRegion(open, section, width, height) => {
                Ok(LayershellCustomActionWithId::new(
                    None,
                    LayershellCustomAction::SetInputRegion(ActionCallback::new(move |region| {
                        region.add(0, 0, WINDOW_SIZE.0 as i32, WINDOW_SIZE.1 as i32);
                        if open {
                            region.add(
                                popup_x(section, width),
                                WINDOW_SIZE.1 as i32,
                                width as i32,
                                height as i32,
                            );
                        }
                    })),
                ))
            }
            Message::OpenModalLayer(id) => Ok(LayershellCustomActionWithId::new(
                None,
                LayershellCustomAction::NewLayerShell {
                    settings: NewLayerShellSettings {
                        size: Some(MODAL_SIZE),
                        layer: Layer::Overlay,
                        anchor: Anchor::empty(),
                        exclusive_zone: None,
                        margin: Some((0, 0, 0, 0)),
                        keyboard_interactivity: KeyboardInteractivity::Exclusive,
                        output_option: OutputOption::LastOutput,
                        events_transparent: false,
                        namespace: Some("OxiBar modal".to_owned()),
                    },
                    id,
                },
            )),
            Message::CloseModalLayer(id) => Ok(LayershellCustomActionWithId::new(
                Some(id),
                LayershellCustomAction::RemoveWindow,
            )),
            Message::OpenPanelLayer(id) => Ok(LayershellCustomActionWithId::new(
                None,
                LayershellCustomAction::NewLayerShell {
                    settings: NewLayerShellSettings {
                        size: Some((PANEL_WIDTH, 0)),
                        layer: Layer::Overlay,
                        anchor: Anchor::Top | Anchor::Bottom | Anchor::Right,
                        exclusive_zone: None,
                        margin: Some((0, 0, 0, 0)),
                        keyboard_interactivity: KeyboardInteractivity::None,
                        output_option: OutputOption::LastOutput,
                        events_transparent: false,
                        namespace: Some("OxiBar notification panel".to_owned()),
                    },
                    id,
                },
            )),
            Message::ClosePanelLayer(id) => Ok(LayershellCustomActionWithId::new(
                Some(id),
                LayershellCustomAction::RemoveWindow,
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
    TogglePluginPanel(String),
    OpenPluginModal(String),
    ClosePluginModal(String),
    SetPopupPlugin(Option<String>),
    SetPopupOpen(bool),
    SetPopupInputRegion(bool, BarSection, u32, u32),
    OpenModalLayer(IcedId),
    CloseModalLayer(IcedId),
    OpenPanelLayer(IcedId),
    ClosePanelLayer(IcedId),
}

fn initial_input_region_task() -> Task<Message> {
    Task::perform(
        async {
            std::thread::sleep(std::time::Duration::from_millis(50));
        },
        |_| Message::SetPopupInputRegion(false, BarSection::End, 0, 0),
    )
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
            popup_open: false,
            modal_plugin: None,
            modal_window_id: None,
            panel_plugin: None,
            panel_window_id: None,
        };
        let mut tasks = plugin_tasks;
        tasks.push(initial_input_region_task());
        (bar, Task::batch(tasks))
    }

    fn namespace() -> String {
        String::from("OxiBar")
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Exit => std::process::exit(0),
            Message::SetPopupInputRegion(_, _, _, _)
            | Message::OpenModalLayer(_)
            | Message::CloseModalLayer(_)
            | Message::OpenPanelLayer(_)
            | Message::ClosePanelLayer(_) => Task::none(),
            Message::SetPopupPlugin(plugin_id) => {
                self.popup_plugin = plugin_id;
                Task::none()
            }
            Message::SetPopupOpen(open) => {
                self.popup_open = open;
                Task::none()
            }
            Message::TogglePluginPopup(plugin_id) => self.toggle_plugin_popup(plugin_id),
            Message::TogglePluginPanel(plugin_id) => self.toggle_plugin_panel(plugin_id),
            Message::OpenPluginModal(plugin_id) => self.open_plugin_modal(plugin_id),
            Message::ClosePluginModal(plugin_id) => self.close_plugin_modal(plugin_id),
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

    fn view(&self, id: IcedId) -> Element<'_, Message> {
        if self.modal_window_id == Some(id) {
            return self.modal_surface_view();
        }
        if self.panel_window_id == Some(id) {
            return self.panel_surface_view();
        }
        self.bar_surface_view()
    }

    fn bar_surface_view(&self) -> Element<'_, Message> {
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
            .padding([3, 0])
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .width(Length::Fill)
            .height(WINDOW_SIZE.1 as f32);

        Container::new(
            Column::new()
                .push(bar)
                .push(self.animated_popup_row())
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn modal_surface_view(&self) -> Element<'_, Message> {
        let Some(plugin_id) = self.modal_plugin.as_deref() else {
            return Space::new()
                .width(MODAL_SIZE.0 as f32)
                .height(MODAL_SIZE.1 as f32)
                .into();
        };
        let Some((model, funcs)) = self.plugins.get(plugin_id) else {
            return Space::new()
                .width(MODAL_SIZE.0 as f32)
                .height(MODAL_SIZE.1 as f32)
                .into();
        };

        let modal_elements: Vec<Element<'static, Message>> = render_plugin_modal(funcs, model)
            .into_iter()
            .map(|el| {
                let id = plugin_id.to_owned();
                el.map(move |msg| map_plugin_message(id.clone(), msg))
            })
            .collect();
        if modal_elements.is_empty() {
            tracing::warn!(plugin = plugin_id, "plugin modal_view returned no elements");
        }

        let body = Column::from_vec(modal_elements)
            .width(Length::Fill)
            .height(Length::Fill);
        Container::new(body)
            .style(modal_surface_style)
            .padding(16)
            .width(MODAL_SIZE.0 as f32)
            .height(MODAL_SIZE.1 as f32)
            .into()
    }

    fn panel_surface_view(&self) -> Element<'_, Message> {
        let Some(plugin_id) = self.panel_plugin.as_deref() else {
            return Space::new().width(PANEL_WIDTH as f32).into();
        };
        let Some((model, funcs)) = self.plugins.get(plugin_id) else {
            return Space::new().width(PANEL_WIDTH as f32).into();
        };

        let panel_elements: Vec<Element<'static, Message>> = render_plugin_panel(funcs, model)
            .into_iter()
            .map(|el| {
                let id = plugin_id.to_owned();
                el.map(move |msg| map_plugin_message(id.clone(), msg))
            })
            .collect();
        if panel_elements.is_empty() {
            tracing::warn!(plugin = plugin_id, "plugin panel_view returned no elements");
        }

        let body = Column::from_vec(panel_elements)
            .width(Length::Fill)
            .height(Length::Fill);
        Container::new(body)
            .style(panel_surface_style)
            .width(PANEL_WIDTH as f32)
            .height(Length::Fill)
            .into()
    }

    fn animated_popup_row(&self) -> Element<'_, Message> {
        let target = if self.popup_open {
            self.popup_plugin
                .as_deref()
                .map(popup_metrics)
                .unwrap_or_default()
                .height as f32
        } else {
            0.0
        };
        AnimationBuilder::new(target, |height| {
            Container::new(self.popup_row(height))
                .width(Length::Fill)
                .height(height)
                .clip(true)
                .into()
        })
        .animation(Motion::SMOOTH)
        .animates_layout(true)
        .into()
    }

    fn popup_row(&self, height: f32) -> Element<'_, Message> {
        let empty = || Space::new().into();
        let Some(plugin_id) = self.popup_plugin.as_deref() else {
            return Space::new().height(height.max(1.0)).into();
        };
        let (start_content, center_content, end_content): (
            Element<'_, Message>,
            Element<'_, Message>,
            Element<'_, Message>,
        ) = match self.plugin_section(plugin_id) {
            BarSection::Start => (self.popup_view(plugin_id, height), empty(), empty()),
            BarSection::Center => (empty(), self.popup_view(plugin_id, height), empty()),
            BarSection::End => (empty(), empty(), self.popup_view(plugin_id, height)),
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
            .height(height.max(1.0))
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
        if self.popup_open && self.popup_plugin.as_deref() == Some(plugin_id.as_str()) {
            Task::done(Message::SetPopupOpen(false)).chain(Task::done(
                Message::SetPopupInputRegion(false, section, 0, 0),
            ))
        } else {
            let metrics = popup_metrics(&plugin_id);
            Task::done(Message::SetPopupPlugin(Some(plugin_id)))
                .chain(Task::done(Message::SetPopupInputRegion(
                    true,
                    section,
                    metrics.connector_width,
                    metrics.height,
                )))
                .chain(Task::done(Message::SetPopupOpen(true)))
        }
    }

    fn open_plugin_modal(&mut self, plugin_id: String) -> Task<Message> {
        let Some((_model, funcs)) = self.plugins.get(&plugin_id) else {
            tracing::warn!(plugin = plugin_id, "unknown plugin requested modal");
            return Task::none();
        };
        if funcs.modal_view.is_none() {
            tracing::warn!(
                plugin = plugin_id,
                "plugin requested modal but has no modal_view"
            );
            return Task::none();
        }
        if self.modal_plugin.as_deref() == Some(plugin_id.as_str()) {
            return Task::none();
        }

        let id = IcedId::unique();
        let close_existing = self.modal_window_id.take();
        self.modal_plugin = Some(plugin_id);
        self.modal_window_id = Some(id);

        if let Some(existing) = close_existing {
            Task::done(Message::CloseModalLayer(existing))
                .chain(Task::done(Message::OpenModalLayer(id)))
        } else {
            Task::done(Message::OpenModalLayer(id))
        }
    }

    fn toggle_plugin_panel(&mut self, plugin_id: String) -> Task<Message> {
        let Some((_model, funcs)) = self.plugins.get(&plugin_id) else {
            tracing::warn!(plugin = plugin_id, "unknown plugin requested panel");
            return Task::none();
        };
        if funcs.panel_view.is_none() {
            tracing::warn!(
                plugin = plugin_id,
                "plugin requested panel but has no panel_view"
            );
            return Task::none();
        }
        if self.panel_plugin.as_deref() == Some(plugin_id.as_str()) {
            return self.close_panel();
        }

        let id = IcedId::unique();
        let close_existing = self.panel_window_id.take();
        self.panel_plugin = Some(plugin_id);
        self.panel_window_id = Some(id);

        if let Some(existing) = close_existing {
            Task::done(Message::ClosePanelLayer(existing))
                .chain(Task::done(Message::OpenPanelLayer(id)))
        } else {
            Task::done(Message::OpenPanelLayer(id))
        }
    }

    fn close_panel(&mut self) -> Task<Message> {
        self.panel_plugin = None;
        let Some(id) = self.panel_window_id.take() else {
            return Task::none();
        };
        Task::done(Message::ClosePanelLayer(id))
    }

    fn close_plugin_modal(&mut self, plugin_id: String) -> Task<Message> {
        if self.modal_plugin.as_deref() != Some(plugin_id.as_str()) {
            return Task::none();
        }
        self.close_modal()
    }

    fn close_modal(&mut self) -> Task<Message> {
        self.modal_plugin = None;
        let Some(id) = self.modal_window_id.take() else {
            return Task::none();
        };
        Task::done(Message::CloseModalLayer(id))
    }

    fn popup_view(&self, plugin_id: &str, height: f32) -> Element<'_, Message> {
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

        let metrics = popup_metrics(plugin_id);
        let popup_height = height.max(1.0);
        let body_height = (popup_height - POPUP_CONNECTOR_HEIGHT as f32).max(0.0);
        let background = canvas(PopupBackground {
            width: metrics.connector_width as f32,
            body_width: metrics.body_width as f32,
            height: popup_height,
        })
        .width(metrics.connector_width as f32)
        .height(popup_height);
        let content = Container::new(
            Column::new()
                .push(Space::new().height(POPUP_CONNECTOR_HEIGHT as f32))
                .push(
                    Container::new(popup_content)
                        .width(metrics.body_width as f32)
                        .height(body_height),
                )
                .align_x(Alignment::Center),
        )
        .width(metrics.connector_width as f32)
        .height(popup_height)
        .align_x(Alignment::Center)
        .align_y(Alignment::Start);

        Stack::new()
            .push(background)
            .push(content)
            .width(metrics.connector_width as f32)
            .height(popup_height)
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
    if let Some(request) = msg.downcast_ref::<String>() {
        match request.as_str() {
            HOST_REQUEST_TOGGLE_POPUP => return Message::TogglePluginPopup(plugin_id),
            HOST_REQUEST_OPEN_MODAL => return Message::OpenPluginModal(plugin_id),
            HOST_REQUEST_CLOSE_MODAL => return Message::ClosePluginModal(plugin_id),
            HOST_REQUEST_TOGGLE_PANEL => return Message::TogglePluginPanel(plugin_id),
            _ => {}
        }
    }
    Message::PluginSubMsg(plugin_id, msg)
}

fn modal_surface_style(_: &Theme) -> iced::widget::container::Style {
    let palette = &OXITHEME;
    iced::widget::container::Style {
        background: Some(iced::Background::Color(palette.mantle)),
        border: iced::Border {
            radius: 18.0.into(),
            color: palette.primary_bg_hover,
            width: 1.0,
        },
        shadow: iced::Shadow {
            color: iced::Color::BLACK.scale_alpha(0.35),
            offset: iced::Vector::new(0.0, 10.0),
            blur_radius: 24.0,
        },
        ..Default::default()
    }
}

fn panel_surface_style(_: &Theme) -> iced::widget::container::Style {
    let palette = &OXITHEME;
    iced::widget::container::Style {
        background: Some(iced::Background::Color(palette.mantle)),
        border: iced::Border {
            radius: 0.0.into(),
            color: palette.primary_bg_hover,
            width: 1.0,
        },
        shadow: iced::Shadow {
            color: iced::Color::BLACK.scale_alpha(0.35),
            offset: iced::Vector::new(-10.0, 0.0),
            blur_radius: 24.0,
        },
        ..Default::default()
    }
}

struct PopupBackground {
    width: f32,
    body_width: f32,
    height: f32,
}

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
            iced::Size::new(self.width.max(1.0), self.height.max(0.0)),
        );

        let w = self.width.max(1.0);
        let h = self.height.max(0.0);
        if h <= 0.0 {
            return vec![frame.into_geometry()];
        }
        let body_w = self.body_width.min(w);
        let ch = (POPUP_CONNECTOR_HEIGHT as f32).min(h);
        let inset = (w - body_w) / 2.0;
        let body_left = inset;
        let body_right = body_left + body_w;
        let radius = 20.0 as f32; //palette.border_radius as f32;
        let top_radius = radius.min(ch / 2.0);
        let shoulder_radius = inset.min(radius).min((h - ch).max(0.0)).max(0.0);
        let bottom_radius = radius.min(body_w / 2.0).min((h - ch).max(0.0) / 2.0);

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

fn popup_x(section: BarSection, connector_width: u32) -> i32 {
    match section {
        BarSection::Start => 0,
        BarSection::Center => (WINDOW_SIZE.0.saturating_sub(connector_width) / 2) as i32,
        BarSection::End => WINDOW_SIZE.0.saturating_sub(connector_width) as i32,
    }
}

#[derive(Clone, Copy)]
struct PopupMetrics {
    body_width: u32,
    connector_width: u32,
    height: u32,
}

impl Default for PopupMetrics {
    fn default() -> Self {
        Self::new(DEFAULT_POPUP_SIZE)
    }
}

impl PopupMetrics {
    fn new((body_width, height): (u32, u32)) -> Self {
        Self {
            body_width,
            connector_width: body_width + POPUP_CONNECTOR_PADDING,
            height,
        }
    }
}

fn popup_metrics(plugin_id: &str) -> PopupMetrics {
    if plugin_id.eq_ignore_ascii_case("audio")
        || plugin_id.eq_ignore_ascii_case("network")
        || plugin_id.eq_ignore_ascii_case("bluetooth")
    {
        PopupMetrics::new(LARGE_POPUP_SIZE)
    } else {
        PopupMetrics::default()
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
