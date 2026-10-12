use iced::{
    Alignment, Element, Length, Task,
    widget::{Column, Row, Space, container},
};
use iced_layershell::reexport::IcedId;
use oxiced::{
    theme::theme_impl::OXITHEME,
    widgets::oxi_plugin::{self, text},
};

use crate::{
    app::OxiBar,
    ipc::WindowCommand,
    messages::{Message, map_plugin_message},
    plugins::render_plugin_popup,
    shell::SurfaceKind,
};

pub(crate) const SEARCH_ID: &str = "oxibar-oxipaste-search";

impl OxiBar {
    pub(crate) fn close_oxipaste(&mut self) -> Task<Message> {
        self.shell
            .oxipaste_window
            .take()
            .map(|id| {
                self.shell.registry.remove(id);
                Task::done(Message::RemoveSurface(id))
            })
            .unwrap_or_else(Task::none)
    }

    pub(crate) fn update_oxipaste(
        &mut self,
        command: WindowCommand,
    ) -> Result<Task<Message>, String> {
        match command {
            WindowCommand::Close => return Ok(self.close_oxipaste()),
            WindowCommand::Toggle if self.shell.oxipaste_window.is_some() => {
                return Ok(self.close_oxipaste());
            }
            WindowCommand::Open if self.shell.oxipaste_window.is_some() => return Ok(Task::none()),
            _ => {}
        }
        let (name, (_, funcs)) = self
            .plugins
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("OxiPaste"))
            .ok_or("OxiPaste provider is not enabled; install liboxipaste.so and enable it in Settings → Plugins")?;
        if funcs.popup_view.is_none() {
            return Err("OxiPaste provider has no clipboard view".into());
        }
        let size = crate::layout::popup_size_from_config(&self.config.snapshot.effective, name)
            .or(funcs.metadata.popup_size)
            .unwrap_or(crate::layout::DEFAULT_POPUP_SIZE);
        let output = self.overlay_output();
        let size = self
            .shell
            .inventory
            .as_ref()
            .and_then(|inventory| {
                inventory
                    .outputs
                    .iter()
                    .find(|item| item.enabled && Some(&item.name) == output.as_ref())
            })
            .map(|output| {
                (
                    size.0.min(output.logical_size.0 * 9 / 10).max(1),
                    size.1.min(output.logical_size.1 * 9 / 10).max(1),
                )
            })
            .unwrap_or(size);
        let dismiss = Task::batch([
            self.close_launcher(),
            self.close_settings(),
            self.close_control_center(),
            self.close_bar_dropdown(),
            self.close_modal(),
            self.close_panel(),
            self.close_overview(),
            if self.desktop.draft.is_some() {
                self.cancel_desktop_edit()
            } else {
                Task::none()
            },
        ]);
        let id = IcedId::unique();
        self.register_overlay(
            id,
            SurfaceKind::OxiPaste,
            self.shell.active_bar,
            output.clone(),
        );
        self.shell
            .registry
            .get_mut(id)
            .expect("registered clipboard")
            .size = Some(size);
        self.shell.oxipaste_window = Some(id);
        Ok(dismiss.chain(Task::done(Message::OnOutput(
            output,
            Box::new(Message::OpenOxiPasteLayer(id, size)),
        ))))
    }

    pub(crate) fn oxipaste_view(&self, id: IcedId) -> Element<'_, Message> {
        let Some((name, (model, funcs))) = self
            .plugins
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("OxiPaste"))
        else {
            return self.empty_surface();
        };
        let content = Column::from_vec(
            render_plugin_popup(funcs, model)
                .into_iter()
                .map(|element| {
                    let name = name.clone();
                    element.map(move |message| map_plugin_message(name.clone(), message))
                })
                .collect(),
        )
        .width(Length::Fill)
        .height(Length::Fill);
        let footer = Row::new()
            .push(
                text(format!("{}: close", self.shortcut_label("close")))
                    .size(OXITHEME.font_sm)
                    .style(oxi_plugin::text_muted),
            )
            .push(Space::new().width(Length::Fill))
            .push(
                oxi_plugin::surface_button(text("Close"), false)
                    .on_press(Message::OxiPasteEscape(id)),
            )
            .spacing(OXITHEME.padding_md)
            .align_y(Alignment::Center);
        container(
            Column::new()
                .push(content)
                .push(footer)
                .spacing(OXITHEME.padding_md)
                .padding(OXITHEME.padding_lg)
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .style(oxi_plugin::compact_card_style(OXITHEME.mantle))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}
