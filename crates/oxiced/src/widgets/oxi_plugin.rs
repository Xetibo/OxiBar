use iced::{
    Background, Border, Color, Element, Length, Shadow, Theme,
    border::Radius,
    widget::{Button, Container, Row, Scrollable, button, container, scrollable, svg},
};

use crate::theme::theme_impl::OXITHEME;

pub const BAR_CONTROL_HEIGHT: f32 = 22.5;

#[derive(Debug, Clone, Copy)]
pub enum ShellIcon {
    Settings,
    Search,
    Controls,
    Calendar,
    Power,
    Reboot,
    Sleep,
    Hibernate,
    Lock,
    Logout,
    Brightness,
    Stats,
    Application,
    Overview,
    Clipboard,
    Copy,
    Delete,
}

pub fn icon<'a>(kind: ShellIcon) -> iced::widget::Svg<'a> {
    let path = match kind {
        ShellIcon::Settings => {
            "<path d='M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8Z'/><path d='m9 3-1 3-3 1-2 3 2 2-1 3 3 2 3-1 2 2 3-2 1-3 3-1v-4l-3-1-1-3-4-1Z'/>"
        }
        ShellIcon::Search => "<circle cx='10' cy='10' r='6'/><path d='m15 15 6 6'/>",
        ShellIcon::Controls => {
            "<path d='M4 6h16M4 12h16M4 18h16'/><circle cx='8' cy='6' r='2'/><circle cx='16' cy='12' r='2'/><circle cx='10' cy='18' r='2'/>"
        }
        ShellIcon::Calendar => {
            "<rect x='3' y='5' width='18' height='16' rx='2'/><path d='M7 3v4M17 3v4M3 11h18M7 15h2M12 15h2M7 18h2'/>"
        }
        ShellIcon::Power => "<path d='M12 2v10M6 5a9 9 0 1 0 12 0'/>",
        ShellIcon::Reboot => "<path d='M3 10a9 9 0 1 1 2 8M3 4v6h6'/>",
        ShellIcon::Sleep => {
            "<path d='M20 15A9 9 0 0 1 9 4a9 9 0 1 0 11 11Z'/><path d='m17 3 .6 1.4L19 5l-1.4.6L17 7l-.6-1.4L15 5l1.4-.6Z'/>"
        }
        ShellIcon::Hibernate => {
            "<path d='M12 2v20M3.3 7l17.4 10M3.3 17 20.7 7M9 4l3 3 3-3M9 20l3-3 3 3'/>"
        }
        ShellIcon::Lock => {
            "<rect x='5' y='10' width='14' height='11' rx='2'/><path d='M8 10V6a4 4 0 0 1 8 0v4M12 14v3'/>"
        }
        ShellIcon::Logout => "<path d='M10 3H4v18h6M10 12h11m-4-4 4 4-4 4'/>",
        ShellIcon::Brightness => {
            "<circle cx='12' cy='12' r='4'/><path d='M12 2v2M12 20v2M2 12h2M20 12h2M5 5l1.5 1.5M17.5 17.5 19 19M5 19l1.5-1.5M17.5 6.5 19 5'/>"
        }
        ShellIcon::Stats => {
            "<rect x='5' y='5' width='14' height='14' rx='2'/><path d='M9 9h6v6H9zM8 2v3M12 2v3M16 2v3M8 19v3M12 19v3M16 19v3M2 8h3M2 12h3M2 16h3M19 8h3M19 12h3M19 16h3'/>"
        }
        ShellIcon::Application => {
            "<rect x='3' y='3' width='18' height='18' rx='3'/><path d='M3 8h18M7 5.5h.1M10 5.5h.1M7 12h10M7 16h6'/>"
        }
        ShellIcon::Overview => {
            "<rect x='3' y='3' width='8' height='8' rx='1'/><rect x='14' y='3' width='7' height='12' rx='1'/><rect x='3' y='14' width='8' height='7' rx='1'/><path d='M14 18h7'/>"
        }
        ShellIcon::Clipboard => {
            "<rect x='5' y='4' width='14' height='17' rx='2'/><rect x='9' y='2' width='6' height='4' rx='1'/><path d='M9 10h6M9 14h6M9 18h4'/>"
        }
        ShellIcon::Copy => {
            "<rect x='8' y='8' width='13' height='13' rx='2'/><path d='M16 8V3H3v13h5'/>"
        }
        ShellIcon::Delete => "<path d='M3 6h18M9 6V3h6v3M6 6l1 15h10l1-15M10 10v7M14 10v7'/>",
    };
    svg(svg::Handle::from_memory(format!(
        "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='white' stroke-width='1.8' stroke-linecap='round' stroke-linejoin='round'>{path}</svg>"
    ).into_bytes()))
    .width(OXITHEME.font_lg)
    .height(OXITHEME.font_lg)
    .style(|_, _| svg::Style { color: Some(OXITHEME.primary) })
}

pub fn bar_action<'a, M: 'a>(kind: ShellIcon, label: &'a str) -> Button<'a, M> {
    bar_action_with_label(kind, label, true)
}

pub fn bar_action_with_label<'a, M: 'a>(
    kind: ShellIcon,
    label: &'a str,
    show_label: bool,
) -> Button<'a, M> {
    let mut row = Row::new().push(icon(kind));
    if show_label {
        row = row.push(text(label).size(OXITHEME.font_md));
    }
    bar_button(
        row.spacing(OXITHEME.padding_sm)
            .align_y(iced::Alignment::Center),
    )
}

pub fn application_icon<'a, M: 'a>(path: Option<&std::path::Path>, size: u32) -> Element<'a, M> {
    match path {
        Some(path)
            if path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("svg")) =>
        {
            svg(svg::Handle::from_path(path))
                .width(size)
                .height(size)
                .into()
        }
        Some(path) => iced::widget::image(iced::widget::image::Handle::from_path(path))
            .width(size)
            .height(size)
            .content_fit(iced::ContentFit::Contain)
            .into(),
        None => icon(ShellIcon::Application).width(size).height(size).into(),
    }
}

pub fn scroll_panel<'a, M: 'a>(content: impl Into<Element<'a, M>>) -> Scrollable<'a, M> {
    scrollable(content)
        .direction(scrollable::Direction::Vertical(
            scrollable::Scrollbar::new()
                .width(OXITHEME.padding_sm)
                .scroller_width(OXITHEME.padding_sm)
                .spacing(OXITHEME.padding_md),
        ))
        .width(Length::Fill)
        .height(Length::Fill)
}

pub mod text {
    pub use iced::widget::text::*;
}

pub fn text<'a>(content: impl text::IntoFragment<'a>) -> iced::widget::Text<'a> {
    iced::widget::text(content).font(crate::theme::snapshot::current_font())
}

pub fn text_primary(_: &Theme) -> text::Style {
    text::Style {
        color: Some(OXITHEME.text),
    }
}

pub fn text_muted(_: &Theme) -> text::Style {
    text::Style {
        color: Some(OXITHEME.text_muted),
    }
}

pub fn text_accent(_: &Theme) -> text::Style {
    text::Style {
        color: Some(accent_text_color()),
    }
}

fn accent_text_color() -> Color {
    crate::theme::contrast::readable(
        OXITHEME.primary,
        &[
            OXITHEME.mantle,
            OXITHEME.mantle_hover,
            OXITHEME.mantle_active,
            OXITHEME.primary_bg,
            OXITHEME.primary_bg_hover,
            OXITHEME.primary_bg_active,
        ],
        OXITHEME.text,
    )
}

pub fn level_meter<'a, M: 'a>(value: f32, maximum: f32) -> Element<'a, M> {
    let maximum = maximum.max(1.0);
    iced::widget::progress_bar(0.0..=maximum, value.clamp(0.0, maximum))
        .girth(OXITHEME.padding_sm)
        .style(|_| iced::widget::progress_bar::Style {
            background: OXITHEME.secondary_bg.into(),
            bar: OXITHEME.primary.into(),
            border: iced::Border {
                radius: OXITHEME.border_radius.into(),
                ..Default::default()
            },
        })
        .into()
}

pub fn osd_content<'a, M: 'a>(
    icon: &'static str,
    title: impl Into<String>,
    value: impl Into<String>,
    level: Option<(f32, f32)>,
) -> Element<'a, M> {
    let labels = iced::widget::Column::new()
        .push(text(title.into()).size(OXITHEME.font_md).style(text_muted))
        .push(
            text(value.into())
                .size(OXITHEME.font_lg)
                .style(text_primary),
        )
        .spacing(OXITHEME.padding_xs)
        .width(Length::Fill);
    let mut body = iced::widget::Column::new()
        .spacing(OXITHEME.padding_md)
        .push(
            iced::widget::Row::new()
                .push(text(icon).size(OXITHEME.font_xl).style(text_accent))
                .push(labels)
                .spacing(OXITHEME.padding_lg)
                .align_y(iced::Alignment::Center),
        );
    if let Some((value, maximum)) = level {
        body = body.push(level_meter(value, maximum));
    }
    body.into()
}

pub fn bar_button_style(_: &Theme, status: button::Status) -> button::Style {
    let base = button::Style {
        background: None,
        text_color: accent_text_color(),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: Radius::from(8.0),
        },
        shadow: Shadow::default(),
        snap: false,
    };
    match status {
        button::Status::Pressed => button::Style {
            background: Some(Background::Color(OXITHEME.primary_bg_active)),
            ..base
        },
        button::Status::Active | button::Status::Hovered | button::Status::Disabled => base,
    }
}

pub fn flat_primary_bg_button_style(_: &Theme, status: button::Status) -> button::Style {
    flat_bg_button_style(status, OXITHEME.primary_bg, accent_text_color())
}

pub fn flat_secondary_bg_button_style(_: &Theme, status: button::Status) -> button::Style {
    flat_bg_button_style(status, OXITHEME.secondary_bg, OXITHEME.text)
}

pub fn selectable_row_style(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        flat_bg_button_style(
            status,
            if selected {
                OXITHEME.primary_bg
            } else {
                OXITHEME.mantle_hover
            },
            OXITHEME.text,
        )
    }
}

pub fn accent_selection_style(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let mut style = bar_button_style(theme, status);
        if selected {
            style.background = Some(
                match status {
                    button::Status::Hovered => OXITHEME.primary_hover,
                    button::Status::Pressed => OXITHEME.primary_active,
                    button::Status::Active | button::Status::Disabled => OXITHEME.primary,
                }
                .into(),
            );
            style.text_color = OXITHEME.primary_contrast;
        }
        style.border.radius = OXITHEME.border_radius.into();
        style
    }
}

pub fn selectable_row<'a, M>(content: impl Into<Element<'a, M>>, selected: bool) -> Button<'a, M> {
    button(content)
        .style(selectable_row_style(selected))
        .padding(OXITHEME.padding_sm)
        .width(Length::Fill)
}

pub fn navigation_row<'a, M>(content: impl Into<Element<'a, M>>, selected: bool) -> Button<'a, M> {
    button(content)
        .style(move |theme, status| {
            let mut style = bar_button_style(theme, status);
            style.text_color = if selected {
                accent_text_color()
            } else {
                OXITHEME.text
            };
            if selected && status == button::Status::Active {
                style.background = Some(OXITHEME.primary_bg.into());
            } else if status == button::Status::Hovered {
                style.background = Some(OXITHEME.primary_bg_hover.into());
            }
            style.border.radius = OXITHEME.border_radius.into();
            style
        })
        .padding(OXITHEME.padding_lg)
        .width(Length::Fill)
}

pub fn settings_card<'a, M>(content: impl Into<Element<'a, M>>) -> Container<'a, M> {
    Container::new(content)
        .style(compact_card_style(OXITHEME.primary_bg))
        .padding(OXITHEME.padding_xl)
        .width(Length::Fill)
}

pub fn surface_button<'a, M>(content: impl Into<Element<'a, M>>, primary: bool) -> Button<'a, M> {
    button(content)
        .style(if primary {
            flat_primary_bg_button_style
        } else {
            flat_secondary_bg_button_style
        })
        .padding([OXITHEME.padding_md, OXITHEME.padding_lg])
}

pub fn badge<'a, M>(label: impl Into<String>, accent: bool) -> Container<'a, M> {
    container(text(label.into()).size(OXITHEME.font_md).style(if accent {
        text_accent
    } else {
        text_muted
    }))
    .style(compact_card_style(if accent {
        OXITHEME.primary_bg
    } else {
        OXITHEME.secondary_bg
    }))
    .padding([OXITHEME.padding_xs, OXITHEME.padding_sm])
}

fn flat_bg_button_style(status: button::Status, bg: Color, text: Color) -> button::Style {
    let background = match status {
        button::Status::Hovered => OXITHEME.primary_bg_hover,
        button::Status::Pressed => OXITHEME.primary_bg_active,
        button::Status::Active | button::Status::Disabled => bg,
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color: text,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: Radius::from(OXITHEME.border_radius),
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

pub fn bar_button<'a, M: 'a>(content: impl Into<Element<'a, M>>) -> Button<'a, M> {
    button(
        container(content)
            .height(Length::Fill)
            .center_y(Length::Fill),
    )
    .style(bar_button_style)
    .padding([0.0, OXITHEME.padding_sm])
    .height(BAR_CONTROL_HEIGHT)
    .width(Length::Shrink)
}

pub fn compact_card_style(bg: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(bg)),
        border: Border {
            radius: Radius::from(OXITHEME.border_radius),
            color: Color::TRANSPARENT,
            width: 0.0,
        },
        shadow: Shadow::default(),
        ..Default::default()
    }
}

pub fn compact_card<'a, M>(content: impl Into<Element<'a, M>>, bg: Color) -> Container<'a, M> {
    Container::new(content)
        .style(compact_card_style(bg))
        .padding(10)
        .width(Length::Fill)
}

#[cfg(test)]
mod tests {
    use iced::widget::button;

    use super::*;

    #[test]
    fn bar_button_style_stays_flat_on_hover() {
        let theme = Theme::Dark;
        let active = bar_button_style(&theme, button::Status::Active);
        let hovered = bar_button_style(&theme, button::Status::Hovered);

        assert!(active.background.is_none());
        assert_eq!(hovered, active);
        assert_eq!(active.shadow, Shadow::default());
    }

    #[test]
    fn text_helpers_use_theme_roles() {
        let theme = Theme::Dark;

        assert_eq!(text_primary(&theme).color, Some(OXITHEME.text));
        assert_eq!(text_muted(&theme).color, Some(OXITHEME.text_muted));
        let accent = text_accent(&theme).color.unwrap();
        for background in [
            OXITHEME.mantle,
            OXITHEME.primary_bg,
            OXITHEME.primary_bg_hover,
            OXITHEME.primary_bg_active,
        ] {
            assert!(crate::theme::contrast::ratio(accent, background) >= 4.5);
        }
    }
}
