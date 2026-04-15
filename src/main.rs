use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::thread;

use hyprland::data::Workspace;
use hyprland::dispatch;
use hyprland::dispatch::DispatchType::*;
use hyprland::dispatch::{Corner, Dispatch, FullscreenType, WorkspaceIdentifierWithSpecial};
use hyprland::event_listener::AsyncEventListener;
use hyprland::prelude::async_closure;
use hyprland::shared::{HyprData, HyprDataActive, WorkspaceId};
use iced::advanced::Widget;
use iced::border::Radius;
use iced::widget::{Container, container, row};
use iced::{Alignment, Border, Font, Padding, Shadow, Size};
use iced::{
    Element, Length, Subscription, Task, Theme,
    advanced::graphics::futures::{Runtime, backend::native::tokio},
    event,
    futures::{
        self, SinkExt, Stream, StreamExt,
        channel::mpsc::{self, Sender},
    },
    keyboard::key::Named,
    stream,
    theme::Style,
    widget::{Column, Row, text},
};
use iced_anim::AnimationBuilder;
use iced_layershell::{
    Settings,
    reexport::{Anchor, KeyboardInteractivity, Layer},
    settings::LayerShellSettings,
};
use oxiced::theme::theme_impl::OXITHEME;
use oxiced::{
    theme::theme_impl::get_derived_iced_theme,
    widgets::{
        oxi_button,
        oxi_layer::{layer_theme, rounded_layer},
    },
};
use wayland_protocols::ext::workspace::{
    self, v1::client::ext_workspace_handle_v1::ExtWorkspaceHandleV1,
};

const MEDIUM_SPACING: u32 = 20;
const WINDOW_SIZE: (u32, u32) = (3440, 25);
const SCALE_FACTOR: f32 = 1.0;
const WINDOW_LAYER: Layer = Layer::Overlay;
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
    workspaces: HashMap<String, hyprland::data::Workspace>,
    hyprland_listener: AsyncEventListener,
    active_workspace_name: String,
}

impl Default for OxiBar {
    fn default() -> Self {
        Self {
            theme: get_derived_iced_theme(),
            workspaces: HashMap::new(),
            active_workspace_name: Workspace::get_active()
                .map(|workspace| workspace.name)
                .unwrap_or(String::from("")),
            hyprland_listener: AsyncEventListener::new(),
        }
    }
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
enum Message {
    Exit,
    WaylandClientMessage(WaylandClientMessage),
    ActiveWorkspaceChanged(String),
    WorkspaceAdded(hyprland::data::Workspace),
    WorkspaceRemoved(String),
}

#[derive(Debug, Clone)]
enum WaylandClientMessage {
    WorkspaceAdded(HashMap<String, hyprland::data::Workspace>),
    WorkspaceActivated(WorkspaceId),
    Exit,
}

impl OxiBar {
    fn new() -> (Self, Task<Message>) {
        let mut content = Self {
            ..Default::default()
        };
        (content, Task::none())
    }

    fn namespace() -> String {
        String::from("OxiBar")
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Exit => std::process::exit(0),
            Message::WaylandClientMessage(WaylandClientMessage::WorkspaceAdded(workspaces)) => {
                dbg!(&workspaces);
                self.workspaces = workspaces;
                Task::none()
            }
            Message::WaylandClientMessage(WaylandClientMessage::WorkspaceActivated(id)) => {
                Task::future(async move {
                    dispatch!(async; Workspace, WorkspaceIdentifierWithSpecial::Id(id)).await;
                })
                .discard()
            }
            Message::WorkspaceAdded(workspace) => {
                self.workspaces.insert(workspace.name.clone(), workspace);
                Task::none()
            }
            Message::WorkspaceRemoved(name) => {
                self.workspaces.remove(&name);
                Task::none()
            }
            Message::ActiveWorkspaceChanged(name) => {
                self.active_workspace_name = name;
                Task::none()
            }
            _ => Task::none(),
        }
    }

    fn view(&self) -> Element<Message> {
        let palette = &OXITHEME;
        let mut sorted_entries: Vec<hyprland::data::Workspace> =
            self.workspaces.clone().into_values().collect();
        sorted_entries.sort_by(|first, second| first.id.cmp(&second.id));
        let workspace_entries: Vec<Element<Message>> = sorted_entries
            .into_iter()
            .map(|workspace| {
                let is_active = self.active_workspace_name == workspace.name;
                let bg_color = if is_active {
                    iced::Background::Color(palette.primary)
                } else {
                    iced::Background::Color(palette.primary_bg)
                };
                let fg_color = if is_active {
                    palette.primary_contrast
                } else {
                    palette.primary
                };
                let font = Font {
                    family: iced::font::Family::SansSerif,
                    weight: iced::font::Weight::Semibold,
                    stretch: iced::font::Stretch::Normal,
                    style: iced::font::Style::Normal,
                };

                AnimationBuilder::new((bg_color, fg_color), move |(bg_color, fg_color)| {
                    let base = iced::widget::button::Style {
                        background: Some(bg_color),
                        text_color: fg_color,
                        border: Border {
                            color: iced::Color::TRANSPARENT,
                            width: 0.0,
                            radius: Radius::new(360 / 4),
                        },
                        shadow: Shadow {
                            // color: p,
                            // offset: Vector { x: 0.2, y: 0.2 },
                            blur_radius: 2.0,
                            ..Shadow::default()
                        },
                        snap: false,
                    };
                    let style =
                        |base: iced::widget::button::Style,
                         status: iced::widget::button::Status| {
                            match status {
                                iced::widget::button::Status::Active => base,
                                iced::widget::button::Status::Pressed => {
                                    iced::widget::button::Style {
                                        background: Some(iced::Background::Color(
                                            palette.primary_active,
                                        )),
                                        ..base
                                    }
                                }
                                iced::widget::button::Status::Hovered => {
                                    iced::widget::button::Style {
                                        background: Some(iced::Background::Color(
                                            palette.primary_hover,
                                        )),
                                        ..base
                                    }
                                }
                                iced::widget::button::Status::Disabled => base, //disabled(base),
                            }
                        };
                    oxi_button::button(
                        text(format!("{}", workspace.id))
                            .size(13)
                            .font(font)
                            .align_y(Alignment::Center)
                            .align_x(Alignment::Center),
                        oxi_button::ButtonVariant::PrimaryBg,
                    )
                    .on_press(Message::WaylandClientMessage(
                        WaylandClientMessage::WorkspaceActivated(workspace.id),
                    ))
                    .style(move |&_, status| (style)(base, status))
                    .padding(0)
                    .height(22.5)
                    .width(22.5)
                    .into()
                })
                .into()
            })
            .collect();

        let workspace_row = Row::from_vec(workspace_entries)
            .align_y(Alignment::Center)
            .spacing(5);
        // .spacing(palette.padding_sm)
        // .padding(palette.padding_sm);

        let row = Row::from_vec(vec![workspace_row.into()]).width(Length::Fill);
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
            ..container::rounded_box(theme)
        }
    }

    fn theme(&self) -> Theme {
        self.theme.clone()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            event::listen_with(move |event, _status, _id| match event {
                iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    modifiers: modifier,
                    key: iced::keyboard::key::Key::Named(key),
                    modified_key: _,
                    physical_key: _,
                    location: _,
                    text: _,
                    repeat: _,
                }) => match key {
                    Named::Escape => Some(Message::Exit),
                    _ => None,
                },
                _ => None,
            }),
            Subscription::run(OxiBar::client_worker),
        ])
    }

    fn client_worker() -> impl Stream<Item = Message> {
        stream::channel(100, async |mut output| {
            let workspaces_res = hyprland::data::Workspaces::get_async().await;
            if let Ok(workspaces) = workspaces_res {
                output
                    .send(Message::WaylandClientMessage(
                        WaylandClientMessage::WorkspaceAdded(
                            workspaces
                                .into_iter()
                                .map(|workspace| (workspace.name.clone(), workspace))
                                .collect(),
                        ),
                    ))
                    .await;
            }
            let mut content = AsyncEventListener::new();
            content.add_workspace_moved_handler(|workspace| Box::pin(async move {}));
            let mut output_clone = output.clone();
            content.add_workspace_added_handler(move |workspace| {
                let mut output_clone = output_clone.clone();
                Box::pin(async move {
                    let workspaces_res = hyprland::data::Workspaces::get_async().await;
                    if let Ok(workspaces) = workspaces_res {
                        let filtered_workspaces: Vec<hyprland::data::Workspace> = workspaces
                            .into_iter()
                            .filter(|other| workspace.name.to_string() == other.name.to_string())
                            .collect();
                        if let Some(workspace) = filtered_workspaces.first() {
                            output_clone.send(Message::WorkspaceAdded(workspace.clone())).await;
                        }
                    }
                })
            });
            let mut output_clone = output.clone();
            content.add_workspace_deleted_handler(move |workspace| {
                let mut output_clone = output_clone.clone();
                Box::pin(async move {
                    output_clone
                        .send(Message::WorkspaceRemoved(workspace.name.to_string()))
                        .await;
                })
            });
            let mut output_clone = output.clone();
            content.add_workspace_changed_handler(move |workspace| {
                let mut output_clone = output_clone.clone();
                Box::pin(async move {
                    output_clone
                        .send(Message::ActiveWorkspaceChanged(workspace.name.to_string()))
                        .await;
                })
            });
            let mut output_clone = output.clone();
            content.add_active_monitor_changed_handler(move |monitor| {
                let mut output_clone = output_clone.clone();
                Box::pin(async move {
                    output_clone
                        .send(Message::ActiveWorkspaceChanged(
                            monitor
                                .workspace_name
                                .clone()
                                .map(|what| what.to_string())
                                .unwrap_or(String::from("")),
                        ))
                        .await;
                })
            });
            let gg = hyprland::default_instance();
            if let Ok(gg2) = gg {
                content.instance_start_listener_async(gg2).await;
            }
        })
    }

    // remove the annoying background color
    fn style(&self, _: &Theme) -> Style {
        layer_theme()
    }

    fn scale_factor(&self) -> f32 {
        SCALE_FACTOR
    }
}
