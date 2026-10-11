use iced::widget::Container;
use iced::{
    Alignment, Element, Length, Padding, Point, Rectangle, Theme,
    widget::{Column, Row, Space, Stack, canvas},
};
use iced_anim::{AnimationBuilder, transition::Easing};
use iced_layershell::reexport::IcedId;
use oxiced::theme::theme_impl::OXITHEME;
use oxiced::widgets::oxi_plugin;

use crate::{
    app::OxiBar,
    layout::{MODAL_SIZE, PANEL_WIDTH, POPUP_CONNECTOR_HEIGHT, POPUP_CONNECTOR_PADDING},
    messages::{Message, map_plugin_message},
    plugins::{
        render_plugin, render_plugin_modal, render_plugin_panel, render_plugin_popup,
        render_plugin_toast,
    },
    shell::SurfaceKind,
};

const PANEL_BODY_INSET: u32 = POPUP_CONNECTOR_PADDING / 2;
const PANEL_RADIUS: f32 = 20.0;
const MODAL_SURFACE_RADIUS: f32 = 18.0;
const SURFACE_BORDER_WIDTH: f32 = 1.0;
const SURFACE_SHADOW_ALPHA: f32 = 0.35;
const MODAL_SHADOW_OFFSET_Y: f32 = 10.0;
const MODAL_SHADOW_BLUR: f32 = 24.0;
const POPUP_SURFACE_RADIUS: f32 = PANEL_RADIUS;

pub(crate) fn overlay_mouse_event(
    event: iced::Event,
    _: iced::event::Status,
    id: IcedId,
) -> Option<Message> {
    use iced::mouse::{Button, Event};
    match event {
        iced::Event::Mouse(Event::CursorMoved { position }) => {
            Some(Message::OverlayPointer(id, Some(position)))
        }
        iced::Event::Mouse(Event::CursorEntered | Event::CursorLeft) => {
            Some(Message::OverlayPointer(id, None))
        }
        iced::Event::Mouse(Event::ButtonPressed(Button::Left)) => Some(Message::OverlayPress(id)),
        _ => None,
    }
}

pub(crate) fn modal_key_event(
    event: iced::Event,
    status: iced::event::Status,
    id: IcedId,
) -> Option<Message> {
    use iced::keyboard::{Event, Key, key::Named};
    let iced::Event::Keyboard(Event::KeyPressed { key, modifiers, .. }) = event else {
        return None;
    };
    match key {
        Key::Named(Named::Escape) => Some(Message::ModalEscape(id)),
        Key::Named(Named::Tab) if status == iced::event::Status::Ignored => {
            Some(Message::ModalFocus(id, !modifiers.shift()))
        }
        _ => None,
    }
}

impl OxiBar {
    pub(crate) fn overlay_press(&mut self, id: IcedId) -> iced::Task<Message> {
        let Some(record) = self.shell.registry.get(id) else {
            return iced::Task::none();
        };
        let Some(pointer) = record.pointer else {
            return iced::Task::none();
        };
        let fallback = match &record.kind {
            SurfaceKind::Settings => self.config.snapshot.host.settings_size,
            SurfaceKind::Launcher => self.launcher.size,
            SurfaceKind::ControlCenter => self.config.snapshot.host.control_center_size,
            SurfaceKind::Modal(_) => MODAL_SIZE,
            _ => return iced::Task::none(),
        };
        let (width, height) = record.size.unwrap_or(fallback);
        if pointer.x >= 0.0
            && pointer.y >= 0.0
            && pointer.x < width as f32
            && pointer.y < height as f32
        {
            return iced::Task::none();
        }
        match &record.kind {
            SurfaceKind::Settings => self.close_settings(),
            SurfaceKind::Launcher => self.close_launcher(),
            SurfaceKind::ControlCenter => self.close_control_center(),
            SurfaceKind::Modal(_) => self.close_modal(),
            _ => iced::Task::none(),
        }
    }

    pub(crate) fn view(&self, id: IcedId) -> Element<'_, Message> {
        let content = match self.shell.registry.get(id).map(|record| &record.kind) {
            Some(SurfaceKind::Settings) => self.settings_view(),
            Some(SurfaceKind::Osd) => self.osd_view(id),
            Some(SurfaceKind::Dock) => self.dock_view(id),
            Some(SurfaceKind::DesktopWidget(_)) => self.desktop_view(id),
            Some(SurfaceKind::DesktopEditor) => self.desktop_editor_view(id),
            Some(SurfaceKind::ControlCenter) => self.control_center_view(),
            Some(SurfaceKind::Launcher) => self.launcher.view(),
            Some(SurfaceKind::Modal(_)) => self.modal_surface_view(),
            Some(SurfaceKind::Panel(_)) => self.panel_surface_view(),
            Some(SurfaceKind::Toast(_, _)) => self.toast_surface_view(id),
            Some(SurfaceKind::Bar(_)) => self.bar_surface_view(id),
            None => return self.empty_surface(),
        };
        content.map(move |message| Message::FromSurface(id, Box::new(message)))
    }

    fn bar_surface_view(&self, id: IcedId) -> Element<'_, Message> {
        let Some(instance) = self.shell.bar(id) else {
            return self.empty_surface();
        };
        let start = Container::new(Row::from_vec(self.render_section(
            id,
            "start",
            &instance.config.start,
        )))
        .align_x(Alignment::Start)
        .align_y(Alignment::Center)
        .width(Length::Fill)
        .height(Length::Fill);
        let center = Container::new(Row::from_vec(self.render_section(
            id,
            "center",
            &instance.config.center,
        )))
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .width(Length::Fill)
        .height(Length::Fill);
        let end = Container::new(Row::from_vec(self.render_section(
            id,
            "end",
            &instance.config.end,
        )))
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
        let transparent = instance.config.transparent;

        let bar = Container::new(row)
            .style(move |theme: &Theme| Self::box_style(theme, transparent))
            .padding([OXITHEME.padding_xs, OXITHEME.padding_sm])
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .width(Length::Fill)
            .height(instance.size.height as f32);

        let popup = self.animated_popup_row(id);
        let column = if instance.config.position == crate::config::bars::Position::Bottom {
            Column::new()
                .push(Space::new().height(Length::Fill))
                .push(popup)
                .push(bar)
        } else {
            Column::new().push(bar).push(popup)
        };
        Container::new(column.width(Length::Fill).height(Length::Fill))
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
            .padding(OXITHEME.padding_lg)
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
        let background = canvas(PanelBackground {
            width: PANEL_WIDTH as f32,
        })
        .width(PANEL_WIDTH as f32)
        .height(Length::Fill);
        let content = Container::new(body)
            .padding(Padding::ZERO.left(PANEL_BODY_INSET))
            .width(PANEL_WIDTH as f32)
            .height(Length::Fill);

        Stack::new()
            .push(background)
            .push(content)
            .width(PANEL_WIDTH as f32)
            .height(Length::Fill)
            .into()
    }

    fn toast_surface_view(&self, id: IcedId) -> Element<'_, Message> {
        let Some(toast) = self.toasts.iter().find(|toast| toast.window_id == id) else {
            return Space::new().into();
        };
        let Some((model, funcs)) = self.plugins.get(&toast.plugin_id) else {
            return Space::new()
                .width(toast.width as f32)
                .height(toast.height as f32)
                .into();
        };

        let toast_elements: Vec<Element<'static, Message>> =
            render_plugin_toast(funcs, model, &toast.toast_id)
                .into_iter()
                .map(|el| {
                    let id = toast.plugin_id.clone();
                    el.map(move |msg| map_plugin_message(id.clone(), msg))
                })
                .collect();
        if toast_elements.is_empty() {
            tracing::warn!(
                plugin = toast.plugin_id,
                toast_id = toast.toast_id,
                "plugin toast_view returned no elements"
            );
        }

        Column::from_vec(toast_elements)
            .width(toast.width as f32)
            .height(toast.height as f32)
            .into()
    }

    fn animated_popup_row(&self, id: IcedId) -> Element<'_, Message> {
        let target = if self.popup_open && self.shell.popup_bar == Some(id) {
            self.popup_plugin
                .as_deref()
                .map(|plugin_id| self.popup_metrics(plugin_id))
                .unwrap_or_default()
                .height as f32
        } else {
            0.0
        };
        let input_target = self
            .popup_plugin
            .as_deref()
            .map(|plugin_id| self.popup_input_metrics(plugin_id).height as f32)
            .unwrap_or(target);
        let has_detached_overlay = input_target > target;
        let motion = oxiced::theme::snapshot::current_snapshot();
        if motion.reduced_motion || motion.animation_ms == 0 {
            let height = if target > 0.0 { input_target } else { 0.0 };
            return Container::new(self.popup_row(id, target, height))
                .width(Length::Fill)
                .height(height.max(1.0))
                .clip(!has_detached_overlay)
                .into();
        }
        AnimationBuilder::new(target, move |height| {
            let input_height = if has_detached_overlay && height > 0.0 {
                input_target
            } else {
                height
            };
            Container::new(self.popup_row(id, height, input_height))
                .width(Length::Fill)
                .height(input_height.max(1.0))
                .clip(!has_detached_overlay)
                .into()
        })
        .animation(
            Easing::EASE_OUT
                .with_duration(motion.duration())
                .reversible(true),
        )
        .animates_layout(true)
        .into()
    }

    fn popup_row(
        &self,
        id: IcedId,
        visible_height: f32,
        input_height: f32,
    ) -> Element<'_, Message> {
        if self.shell.popup_bar != Some(id) {
            return Space::new().height(input_height.max(1.0)).into();
        }
        let empty = || Space::new().into();
        let Some(plugin_id) = self.popup_plugin.as_deref() else {
            return Space::new().height(input_height.max(1.0)).into();
        };
        let (start_content, center_content, end_content): (
            Element<'_, Message>,
            Element<'_, Message>,
            Element<'_, Message>,
        ) = match self.plugin_section_on(id, plugin_id) {
            crate::layout::BarSection::Start => (
                self.popup_view(plugin_id, visible_height, input_height),
                empty(),
                empty(),
            ),
            crate::layout::BarSection::Center => (
                empty(),
                self.popup_view(plugin_id, visible_height, input_height),
                empty(),
            ),
            crate::layout::BarSection::End => (
                empty(),
                empty(),
                self.popup_view(plugin_id, visible_height, input_height),
            ),
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
            .height(input_height.max(1.0))
            .into()
    }

    fn popup_view(
        &self,
        plugin_id: &str,
        visible_height: f32,
        input_height: f32,
    ) -> Element<'_, Message> {
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

        let metrics = self.popup_metrics(plugin_id);
        let input_height = input_height.max(1.0);
        let popup_height = visible_height.max(1.0);
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
        .height(input_height)
        .align_x(Alignment::Center)
        .align_y(Alignment::Start);

        Stack::new()
            .push(background)
            .push(content)
            .width(metrics.connector_width as f32)
            .height(input_height)
            .into()
    }

    fn render_section(
        &self,
        source: IcedId,
        section: &str,
        names: &[String],
    ) -> Vec<Element<'static, Message>> {
        names
            .iter()
            .enumerate()
            .flat_map(|(index, name)| {
                if name.eq_ignore_ascii_case("settings") {
                    return vec![
                        oxiced::widgets::oxi_plugin::bar_button(
                            oxi_plugin::text("Settings").size(OXITHEME.font_md),
                        )
                        .on_press(Message::Settings(crate::settings::Action::Open))
                        .into(),
                    ];
                }
                if name.eq_ignore_ascii_case("launcher") {
                    return if self.launcher.enabled {
                        vec![
                            oxiced::widgets::oxi_plugin::bar_button(
                                oxi_plugin::text("Search").size(OXITHEME.font_md),
                            )
                            .on_press(Message::Launcher(crate::launcher::Action::Toggle))
                            .into(),
                        ]
                    } else {
                        Vec::new()
                    };
                }
                if name.eq_ignore_ascii_case("control-center") {
                    return if self.config.snapshot.host.control_center_enabled {
                        vec![
                            oxiced::widgets::oxi_plugin::bar_button(
                                oxi_plugin::text("Controls").size(OXITHEME.font_md),
                            )
                            .on_press(Message::ControlCenter(
                                crate::control_center::Action::Toggle,
                            ))
                            .into(),
                        ]
                    } else {
                        Vec::new()
                    };
                }
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
                let elements = if let Some(view) = funcs.context_view {
                    let output = self.output_for_bar(Some(source));
                    let scale = self
                        .shell
                        .inventory
                        .as_ref()
                        .and_then(|inventory| {
                            inventory
                                .outputs
                                .iter()
                                .find(|item| Some(&item.name) == output.as_ref())
                        })
                        .map(|output| output.scale)
                        .unwrap_or(1.0);
                    let context = oxibar_plugin_api::PluginViewContext {
                        instance_id: format!(
                            "{}:{section}:{id}:{}",
                            output.as_deref().unwrap_or("default"),
                            names[..index]
                                .iter()
                                .filter(|prior| prior.eq_ignore_ascii_case(name))
                                .count()
                        ),
                        output_name: output,
                        output_scale: scale,
                    };
                    unsafe { view(model.clone(), context) }.unwrap_or_default()
                } else {
                    render_plugin(funcs, model)
                };
                elements
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
}

fn modal_surface_style(_: &Theme) -> iced::widget::container::Style {
    let palette = &OXITHEME;
    iced::widget::container::Style {
        background: Some(iced::Background::Color(palette.mantle)),
        border: iced::Border {
            radius: MODAL_SURFACE_RADIUS.into(),
            color: palette.primary_bg_hover,
            width: SURFACE_BORDER_WIDTH,
        },
        shadow: iced::Shadow {
            color: iced::Color::BLACK.scale_alpha(SURFACE_SHADOW_ALPHA),
            offset: iced::Vector::new(0.0, MODAL_SHADOW_OFFSET_Y),
            blur_radius: MODAL_SHADOW_BLUR,
        },
        ..Default::default()
    }
}

struct PanelBackground {
    width: f32,
}

impl<Message> canvas::Program<Message> for PanelBackground {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let palette = &OXITHEME;
        let w = self.width.max(1.0);
        let h = bounds.height.max(1.0);
        let mut frame = canvas::Frame::new(renderer, iced::Size::new(w, h));
        let inset = (PANEL_BODY_INSET as f32).min(w / 2.0);
        let shoulder_radius = PANEL_RADIUS.min(inset).min(h);
        let bottom_radius = PANEL_RADIUS.min((w - inset) / 2.0).min(h / 2.0);

        let shape = canvas::Path::new(|path| {
            path.move_to(Point::new(0.0, 0.0));
            path.line_to(Point::new(w, 0.0));
            path.line_to(Point::new(w, h));
            path.line_to(Point::new(inset + bottom_radius, h));
            path.quadratic_curve_to(Point::new(inset, h), Point::new(inset, h - bottom_radius));
            path.line_to(Point::new(inset, shoulder_radius));
            path.quadratic_curve_to(
                Point::new(inset, 0.0),
                Point::new(inset - shoulder_radius, 0.0),
            );
            path.line_to(Point::new(0.0, 0.0));
            path.close();
        });

        frame.fill(&shape, palette.mantle);
        vec![frame.into_geometry()]
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
        let radius = POPUP_SURFACE_RADIUS;
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
