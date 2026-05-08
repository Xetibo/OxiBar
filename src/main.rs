use std::collections::HashMap;
use std::pin::Pin;

use iced::widget::{Container, container};
use iced::{Alignment, Font};
use iced::{
    Element, Length, Subscription, Task, Theme,
    futures::Stream,
    theme::Style,
    widget::Row,
};
use iced_layershell::{
    Settings,
    reexport::{Anchor, KeyboardInteractivity, Layer},
    settings::LayerShellSettings,
};
use once_cell::sync::Lazy;
use oxiced::theme::theme_impl::OXITHEME;
use oxiced::{
    theme::theme_impl::get_derived_iced_theme,
    widgets::oxi_layer::layer_theme,
};
use toml::Table;

use crate::config::{get_allowed_plugins, get_config, get_oxirun_dir};
use crate::plugins::{PluginFuncs, PluginModel, PluginMsg, SubscriptionFn, load_plugin};

pub mod config;
pub mod plugins;

static CONFIG: Lazy<Table> = Lazy::new(get_config);

const WINDOW_SIZE: (u32, u32) = (3440, 25);
const SCALE_FACTOR: f32 = 1.0;
const WINDOW_MARGINS: (i32, i32, i32, i32) = (0, 0, 0, 0);
const WINDOW_KEYBAORD_MODE: KeyboardInteractivity = KeyboardInteractivity::OnDemand;

pub fn main() -> Result<(), iced_layershell::Error> {
    let settings = Settings {
        layer_settings: LayerShellSettings {
            size: Some(WINDOW_SIZE),
            exclusive_zone: 25,
            anchor: Anchor::Top,
            layer: Layer::Background,
            margin: WINDOW_MARGINS,
            keyboard_interactivity: WINDOW_KEYBAORD_MODE,
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
    _config: Table,
    plugins: HashMap<usize, (PluginModel, PluginFuncs)>,
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
    PluginSubMsg(usize, PluginMsg),
}

type PluginMap = HashMap<usize, (PluginModel, PluginFuncs)>;

fn get_plugins(config: &Table) -> (PluginMap, Vec<Task<Message>>) {
    let mut plugins = HashMap::new();
    let mut tasks = Vec::new();
    let plugin_dir = get_oxirun_dir().join("plugins");
    if !plugin_dir.is_dir() {
        std::fs::create_dir(&plugin_dir).expect("Could not create config dir");
    }
    let allowed_files = get_allowed_plugins(config);
    for (index, res) in plugin_dir
        .read_dir()
        .expect("Could not read plugin directory")
        .enumerate()
    {
        if let Ok(file) = res {
            if allowed_files.contains(&file.file_name().to_str().unwrap_or("")) {
                unsafe {
                    let lib = Box::leak(Box::new(
                        libloading::Library::new(file.path()).expect("Could not load library"),
                    ));
                    if let Some(plugin) = load_plugin(lib) {
                        let (model, task_opt) = (plugin.model.clone())(config.clone());
                        plugins.insert(index, (model, plugin));
                        if let Some(task) = task_opt
                            .map(|val| val.map(move |msg| Message::PluginSubMsg(index, msg)))
                        {
                            tasks.push(task)
                        }
                    }
                }
            }
        }
    }
    (plugins, tasks)
}

impl OxiBar {
    fn new() -> (Self, Task<Message>) {
        let (plugins, plugin_tasks) = get_plugins(&CONFIG);
        dbg!(&plugins);
        let content = Self {
            _config: CONFIG.to_owned(),
            plugins,
            theme: get_derived_iced_theme(),
        };
        (content, Task::batch(plugin_tasks))
    }

    fn namespace() -> String {
        String::from("OxiBar")
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Exit => std::process::exit(0),
            Message::PluginSubMsg(index, msg) => unsafe {
                let plugin = self.plugins.get_mut(&index).unwrap();
                let update_func = plugin.1.update.clone();
                let task_opt = (update_func)(String::from(""), plugin.0.clone(), msg);
                if let Some(task) = task_opt {
                    task.map(move |msg| Message::PluginSubMsg(index, msg))
                } else {
                    Task::none()
                }
            },
        }
    }

    fn view(&self) -> Element<Message> {
        let plugin_views: Vec<Element<Message>> = self
            .plugins
            .iter()
            .flat_map(|(index, (model, funcs))| {
                let view_func = funcs.view.clone();
                let view_res = unsafe { (view_func)(model.clone()) };
                match view_res {
                    Ok(view) => view
                        .into_iter()
                        .map(move |element| {
                            element.map(|msg| Message::PluginSubMsg(*index, msg.clone()))
                        })
                        .collect(),
                    Err(_) => Vec::new(),
                }
            })
            .collect::<Vec<_>>();
        let what = Row::from_vec(plugin_views);
        let row = Row::from_vec(vec![what.into()]).width(Length::Fill);
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
        let plugin_subscriptions: Vec<Subscription<Message>> = self
            .plugins
            .iter()
            .map(|(index, (_model, funcs))| {
                let subscription_fn: SubscriptionFn = unsafe { *funcs.subscription };
                let index_clone = *index;
                Subscription::run_with(
                    (index_clone, subscription_fn as usize),
                    build_plugin_stream,
                )
                // Attach the index as context so the map closure is zero-sized (non-capturing).
                .with(index_clone)
                .map(|(idx, msg)| Message::PluginSubMsg(idx, msg))
            })
            .collect();
        Subscription::batch(plugin_subscriptions)
    }

    fn style(&self, _: &Theme) -> Style {
        layer_theme()
    }

    fn scale_factor(&self) -> f32 {
        SCALE_FACTOR
    }
}

/// Non-capturing builder fn required by `Subscription::run_with`.
/// Receives (plugin_index, fn_ptr_as_usize), calls the plugin's subscription function,
/// and reconstitutes the returned raw pointer as a pinned boxed stream on the host side.
/// Only a raw `*mut dyn Stream` crosses the dylib boundary — no non-repr(C) Rust types.
fn build_plugin_stream(
    data: &(usize, usize),
) -> impl Stream<Item = PluginMsg> + use<> {
    let subscription_fn: SubscriptionFn = unsafe { std::mem::transmute(data.1) };
    unsafe { Pin::new_unchecked(Box::from_raw((subscription_fn)())) }
}
