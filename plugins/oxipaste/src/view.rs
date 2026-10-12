use iced::{
    Element, Length,
    widget::{Column, Row, Space, image},
};
use oxibar_plugin_api::{PluginModel, PluginMsg, with_model_read};
use oxiced::{
    theme::theme_impl::OXITHEME,
    widgets::{
        oxi_plugin::{self, text},
        oxi_text_input,
    },
};

use crate::{
    Message, Model,
    backend::{Action, Item},
    msg,
};

pub fn bar(
    model: PluginModel,
    labels: bool,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    with_model_read::<Model, _>(&model, |model| {
        let mut row = Row::new().push(oxi_plugin::icon(oxi_plugin::ShellIcon::Clipboard));
        if labels {
            row = row.push(text(format!("Clipboard {}", model.items.len())).size(OXITHEME.font_md));
        }
        vec![
            oxi_plugin::bar_button(
                row.spacing(OXITHEME.padding_sm)
                    .align_y(iced::Alignment::Center),
            )
            .on_press(msg(Message::Open))
            .into(),
        ]
    })
}

fn preview(item: &Item) -> Element<'static, PluginMsg> {
    let mut column = Column::new()
        .spacing(OXITHEME.padding_sm)
        .width(Length::Fill);
    if item.mime.starts_with("image/") {
        column = column.push(
            image(image::Handle::from_bytes(item.bytes.clone()))
                .height(88)
                .width(Length::Fill)
                .content_fit(iced::ContentFit::Contain),
        );
    }
    column
        .push(
            text(item.preview())
                .size(OXITHEME.font_md)
                .style(oxi_plugin::text_primary),
        )
        .into()
}

pub(super) fn body(model: &Model, scroll: bool) -> Element<'static, PluginMsg> {
    let mut refresh = oxi_plugin::surface_button(text("Refresh"), false);
    let mut clear = oxi_plugin::surface_button(text("Clear all"), false);
    if !model.busy {
        refresh = refresh.on_press(msg(Message::Refresh));
        clear = clear.on_press(msg(Message::Act(Action::Clear, false)));
    }
    let mut body = Column::new()
        .spacing(OXITHEME.padding_md)
        .width(Length::Fill)
        .push(
            Row::new()
                .spacing(OXITHEME.padding_sm)
                .align_y(iced::Alignment::Center)
                .push(
                    text("Clipboard history")
                        .size(OXITHEME.font_lg)
                        .style(oxi_plugin::text_primary),
                )
                .push(Space::new().width(Length::Fill))
                .push(refresh)
                .push(clear),
        )
        .push(
            oxi_text_input::text_input("Search clipboard…", &model.search, |query| {
                msg(Message::Search(query))
            })
            .id("oxibar-oxipaste-search"),
        )
        .push(
            text("Click an item to paste · Copy keeps this view open")
                .size(OXITHEME.font_sm)
                .style(oxi_plugin::text_muted),
        );
    if let Some(status) = model.health.as_ref().or(model.status.as_ref()) {
        body = body.push(
            text(status.clone())
                .size(OXITHEME.font_sm)
                .style(oxi_plugin::text_muted),
        );
    }
    let query = model.search.to_lowercase();
    let mut items = Column::new()
        .spacing(OXITHEME.padding_sm)
        .width(Length::Fill);
    let mut count = 0;
    for item in model.items.iter().filter(|item| item.matches(&query)) {
        count += 1;
        let mut paste = oxi_plugin::surface_button(preview(item), true).width(Length::Fill);
        let mut copy = oxi_plugin::surface_button(text("Copy"), false);
        let mut delete = oxi_plugin::surface_button(text("Delete"), false);
        if !model.busy {
            paste = paste.on_press(msg(Message::Act(Action::Copy(item.clone()), true)));
            copy = copy.on_press(msg(Message::Act(Action::Copy(item.clone()), false)));
            delete = delete.on_press(msg(Message::Act(Action::Delete(item.clone()), false)));
        }
        items = items.push(
            Row::new()
                .spacing(OXITHEME.padding_sm)
                .align_y(iced::Alignment::Center)
                .push(paste)
                .push(
                    Column::new()
                        .spacing(OXITHEME.padding_sm)
                        .push(copy)
                        .push(delete),
                ),
        );
    }
    if count == 0 {
        items = items.push(text("No matching clipboard items").style(oxi_plugin::text_muted));
    }
    if scroll {
        body = body
            .push(oxi_plugin::scroll_panel(items))
            .height(Length::Fill);
    } else {
        body = body.push(items);
    }
    body.into()
}
