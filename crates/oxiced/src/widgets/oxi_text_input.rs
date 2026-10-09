use iced::{
    Border, Theme,
    border::Radius,
    widget::text_input::{Status, Style},
};

use crate::theme::theme_impl::OXITHEME;

pub fn text_input_style(_: &Theme, status: Status) -> Style {
    let palette = &OXITHEME;
    let mut style = Style {
        background: iced::Background::Color(palette.mantle),
        border: Border {
            color: palette.secondary_bg,
            width: 1.0,
            radius: Radius::from(palette.border_radius),
        },
        icon: palette.text,
        placeholder: palette.text_muted,
        value: palette.text,
        selection: palette.primary,
    };
    match status {
        Status::Active => style,
        Status::Hovered => {
            style.background = iced::Background::Color(palette.mantle_hover);
            style.border.color = palette.primary;
            style
        }
        Status::Focused { is_hovered: _ } => {
            style.background = iced::Background::Color(palette.mantle);
            style.border.color = palette.primary;
            style
        }
        Status::Disabled => {
            style.value = palette.text_muted;
            style
        }
    }
}

pub fn editor_style(
    theme: &Theme,
    status: iced::widget::text_editor::Status,
) -> iced::widget::text_editor::Style {
    use iced::widget::text_editor;
    let status = match status {
        text_editor::Status::Active => Status::Active,
        text_editor::Status::Hovered => Status::Hovered,
        text_editor::Status::Focused { is_hovered } => Status::Focused { is_hovered },
        text_editor::Status::Disabled => Status::Disabled,
    };
    let style = text_input_style(theme, status);
    text_editor::Style {
        background: style.background,
        border: style.border,
        placeholder: style.placeholder,
        value: style.value,
        selection: style.selection,
    }
}

pub fn text_input<'a, M>(
    placeholder: &str,
    value: &str,
    on_text_changed: impl Fn(String) -> M + 'a,
) -> iced::widget::TextInput<'a, M>
where
    M: Clone,
{
    iced::widget::text_input(placeholder, value)
        .padding(OXITHEME.padding_lg)
        .on_input(on_text_changed)
        .style(text_input_style)
}
