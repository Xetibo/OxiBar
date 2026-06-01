//! Oxibar entry point and the iced application driving the bar.
//!
//! TODO (architectural):
//! - `WINDOW_SIZE` is hardcoded for one monitor. Should be derived from the
//!   active wayland output (`wl_output` advertise width) or a config knob.
//! - `view()` currently concatenates every plugin into a single `Row`.
//!   Once plugins declare a `Slot`, split into left/center/right containers.
//! - Replace the raw `*mut Stream` ABI with an `extern "C"` callback channel
//!   (see `oxibar-plugin-api`).

use std::pin::Pin;

use iced::widget::Container;
use iced::{
    Alignment, Element, Font, Length, Subscription, Task, Theme,
    theme::Style,
    widget::Row,
};
use iced_layershell::{
    Settings,
    reexport::{Anchor, KeyboardInteractivity, Layer},
    settings::LayerShellSettings,
};
use once_cell::sync::Lazy;
use oxibar_plugin_api::{PluginMsg, PluginStream, SubscriptionFn};
use oxiced::theme::theme_impl::OXITHEME;
use oxiced::{theme::theme_impl::get_derived_iced_theme, widgets::oxi_layer::layer_theme};
use toml::Table;
use tracing::error;

use crate::config::get_config;
use crate::plugins::{PluginMap, dispatch_update, drain_errors, load_plugins, render_plugin};

pub mod config;
pub mod plugins;

static CONFIG: Lazy<Table> = Lazy::new(get_config);

const WINDOW_SIZE: (u32, u32) = (3440, 25);
const SCALE_FACTOR: f32 = 1.0;
const WINDOW_MARGINS: (i32, i32, i32, i32) = (0, 0, 0, 0);
const WINDOW_KEYBOARD_MODE: KeyboardInteractivity = KeyboardInteractivity::OnDemand;
const EXCLUSIVE_ZONE: i32 = 25;

pub fn main() -> Result<(), iced_layershell::Error> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let settings = Settings {
        layer_settings: LayerShellSettings {
            size: Some(WINDOW_SIZE),
            exclusive_zone: EXCLUSIVE_ZONE,
            anchor: Anchor::Top,
            layer: Layer::Background,
            margin: WINDOW_MARGINS,
            keyboard_interactivity: WINDOW_KEYBOARD_MODE,
            ..Default::default()
        },
        ..Default::default()
    };
    iced_layershell::application(OxiBar::new, OxiBar::namespace, OxiBar::update, OxiBar::view)
        .subscription(OxiBar::subscription)
        .settings(settings)
        .theme(OxiBar::theme)
        .default_font(Font::with_name("Adwaita Sans"))
        .style(OxiBar::style)
        .scale_factor(OxiBar::scale_factor)
        .run()
}

struct OxiBar {
    theme: Theme,
    plugins: PluginMap,
}

impl TryInto<iced_layershell::actions::LayershellCustomActionWithId> for Message {
    type Error = Self;
    fn try_into(
        self,
    ) -> Result<iced_layershell::actions::LayershellCustomActionWithId, Self::Error> {
        Err(self)
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Exit,
    PluginSubMsg(String, PluginMsg),
}

impl OxiBar {
    fn new() -> (Self, Task<Message>) {
        let (plugins, plugin_tasks) = load_plugins(&CONFIG);
        let bar = Self {
            plugins,
            theme: get_derived_iced_theme(),
        };
        (bar, Task::batch(plugin_tasks))
    }

    fn namespace() -> String {
        String::from("OxiBar")
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Exit => std::process::exit(0),
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
                        task.map(move |msg| Message::PluginSubMsg(id.clone(), msg))
                    }
                    None => Task::none(),
                }
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let plugin_views: Vec<Element<Message>> = self
            .plugins
            .iter()
            .flat_map(|(plugin_id, (model, funcs))| {
                let id = plugin_id.clone();
                render_plugin(funcs, model)
                    .into_iter()
                    .map(move |el| {
                        let id = id.clone();
                        el.map(move |msg| Message::PluginSubMsg(id.clone(), msg))
                    })
                    .collect::<Vec<_>>()
            })
            .collect();

        let row = Row::from_vec(plugin_views).width(Length::Fill);
        Container::new(row)
            .style(OxiBar::box_style)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn box_style(theme: &Theme) -> iced::widget::container::Style {
        let palette = &OXITHEME;
        iced::widget::container::Style {
            background: Some(iced::Background::Color(palette.mantle)),
            ..iced::widget::container::rounded_box(theme)
        }
    }

    fn theme(&self) -> Theme {
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
                Subscription::run_with(
                    (plugin_id.clone(), sub_fn_addr),
                    build_plugin_stream,
                )
                .with(plugin_id.clone())
                .map(|(id, msg): (String, PluginMsg)| Message::PluginSubMsg(id, msg))
            })
            .collect();
        Subscription::batch(subs)
    }

    fn style(&self, _: &Theme) -> Style {
        layer_theme()
    }

    fn scale_factor(&self) -> f32 {
        SCALE_FACTOR
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

