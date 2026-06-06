//! System tray plugin using the StatusNotifierItem watcher protocol.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use iced::{
    Alignment, Background, Border, Color, ContentFit, Element, Length, Point, Rectangle, Shadow,
    Size, Task, Vector,
    advanced::{
        Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer,
        widget::{Operation, Tree},
    },
    futures::Stream,
    stream,
    widget::{Column, Row, Space, button, container, image, mouse_area, scrollable, svg, text},
};
use oxibar_plugin_api::{
    ABI_VERSION, HOST_REQUEST_TOGGLE_POPUP, PluginMetadata, PluginModel, PluginMsg,
    PluginPopupMetrics, PluginStream, drain_model_errors, plugin_model, toml::Table,
    with_model_read, with_model_write,
};
use oxiced::theme::theme_impl::OXITHEME;
use oxiced::widgets::oxi_plugin;
use zbus::{blocking::Connection, fdo::RequestNameFlags, interface, message::Header};

mod system;

use system::{
    TrayAction, TrayItem, TrayMenuAction, TrayMenuItem, call_item_action, call_menu_item,
    normalize_item_address, query_item, query_item_menu,
};

const WATCHER_BUS_NAME: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";
const POPUP_WIDTH: u32 = 300;
const POPUP_MAX_HEIGHT: u32 = 420;
const POPUP_MIN_HEIGHT: u32 = 48;
const POPUP_VERTICAL_PADDING: u32 = 24;
const TRAY_ROW_HEIGHT: u32 = 30;
const TRAY_ROW_SPACING: u32 = 6;
const CONTEXT_MENU_WIDTH: f32 = 240.0;
const CONTEXT_MENU_MAX_HEIGHT: f32 = 420.0;
const CONTEXT_MENU_ITEM_SPACING: f32 = 2.0;
const CONTEXT_MENU_LIMIT_EXTRA: f32 = 12.0;
const CONTEXT_MENU_OFFSET: f32 = 8.0;
const CONTEXT_MENU_DEPTH_INDENT: usize = 14;
const CONTEXT_MENU_BORDER_WIDTH: f32 = 1.0;
const CONTEXT_MENU_SHADOW_ALPHA: f32 = 0.35;
const CONTEXT_MENU_SHADOW_OFFSET_Y: f32 = 8.0;
const CONTEXT_MENU_SHADOW_BLUR: f32 = 18.0;
const TRAY_FALLBACK_ICON_WIDTH: u32 = 22;
const TRAY_ICON_SIZE: u32 = 18;

#[derive(Debug, Default)]
pub struct Model {
    items: BTreeMap<String, TrayItem>,
    hovered_item: Option<String>,
    open_menu: Option<String>,
    cursor_positions: BTreeMap<String, Point>,
    menus: BTreeMap<String, Vec<TrayMenuItem>>,
    errors: Vec<String>,
}

impl Model {
    fn new(_global_config: Table) -> Self {
        Self::default()
    }
}

#[derive(Clone, Debug)]
pub enum Message {
    TogglePopup,
    ItemsChanged(Vec<TrayItem>),
    HoverItem(Option<String>),
    CursorMoved {
        key: String,
        position: Point,
    },
    Activate {
        key: String,
        action: TrayAction,
    },
    ToggleContextMenu(String),
    MenuLoaded {
        key: String,
        result: Result<Vec<TrayMenuItem>, String>,
    },
    MenuAction {
        key: String,
        action: TrayMenuAction,
    },
    Error(String),
}

fn msg(m: Message) -> PluginMsg {
    Arc::new(m)
}

#[unsafe(no_mangle)]
pub extern "Rust" fn abi_version() -> u32 {
    ABI_VERSION
}

#[unsafe(no_mangle)]
pub extern "Rust" fn name() -> &'static str {
    "Tray"
}

#[unsafe(no_mangle)]
pub extern "Rust" fn metadata() -> PluginMetadata {
    PluginMetadata {
        popup_size: Some((POPUP_WIDTH, 180)),
        popup_input_size: Some((POPUP_WIDTH, POPUP_MAX_HEIGHT)),
    }
}

#[unsafe(no_mangle)]
pub extern "Rust" fn popup_metrics(model: PluginModel) -> PluginPopupMetrics {
    with_model_read::<Model, _>(&model, |model| {
        let visible_height = tray_popup_height(model.items.len());
        let input_height = if model.open_menu.is_some() {
            POPUP_MAX_HEIGHT
        } else {
            visible_height
        };
        PluginPopupMetrics {
            popup_size: Some((POPUP_WIDTH, visible_height)),
            popup_input_size: Some((POPUP_WIDTH, input_height)),
        }
    })
    .unwrap_or(PluginPopupMetrics {
        popup_size: Some((POPUP_WIDTH, POPUP_MIN_HEIGHT)),
        popup_input_size: Some((POPUP_WIDTH, POPUP_MIN_HEIGHT)),
    })
}

#[unsafe(no_mangle)]
pub extern "Rust" fn model(global_config: Table) -> (PluginModel, Option<Task<PluginMsg>>) {
    (plugin_model(Model::new(global_config)), None)
}

#[unsafe(no_mangle)]
pub extern "Rust" fn update(model: PluginModel, msg_in: PluginMsg) -> Option<Task<PluginMsg>> {
    let m = msg_in.downcast_ref::<Message>()?.clone();
    with_model_write::<Model, _>(&model, |model| match m {
        Message::TogglePopup => {
            let request: PluginMsg = Arc::new(HOST_REQUEST_TOGGLE_POPUP.to_owned());
            Some(Task::done(request))
        }
        Message::ItemsChanged(items) => {
            model.items = items
                .into_iter()
                .map(|item| (item.key.clone(), item))
                .collect();
            let keys = model.items.keys().cloned().collect::<Vec<_>>();
            model.menus.retain(|key, _| keys.contains(key));
            model.cursor_positions.retain(|key, _| keys.contains(key));
            None
        }
        Message::HoverItem(key) => {
            model.hovered_item = key;
            None
        }
        Message::CursorMoved { key, position } => {
            model.cursor_positions.insert(key, position);
            None
        }
        Message::Activate { key, action } => {
            let item = model.items.get(&key).cloned()?;
            model.open_menu = None;
            std::thread::spawn(move || {
                if let Err(e) = call_item_action(&item, action) {
                    tracing::warn!(item = item.key, "tray action failed: {e}");
                }
            });
            None
        }
        Message::ToggleContextMenu(key) => {
            if model.open_menu.as_deref() == Some(key.as_str()) {
                model.open_menu = None;
                None
            } else {
                let item = model.items.get(&key).cloned()?;
                model.open_menu = Some(key.clone());
                if item.menu_path.is_none() {
                    model.menus.insert(key, fallback_context_menu());
                    None
                } else if model.menus.contains_key(&key) {
                    None
                } else {
                    Some(Task::perform(
                        async move { query_item_menu(&item).map_err(|error| error.to_string()) },
                        move |result| msg(Message::MenuLoaded { key, result }),
                    ))
                }
            }
        }
        Message::MenuLoaded { key, result } => {
            match result {
                Ok(items) if !items.is_empty() => {
                    model.menus.insert(key, items);
                }
                Ok(_) => {
                    model.menus.insert(key, fallback_context_menu());
                }
                Err(error) => {
                    model
                        .errors
                        .push(format!("tray: menu query failed: {error}"));
                    model.menus.insert(key, fallback_context_menu());
                }
            }
            None
        }
        Message::MenuAction { key, action } => {
            let item = model.items.get(&key).cloned()?;
            model.open_menu = None;
            tracing::debug!(item = item.key, ?action, "tray menu action selected");
            std::thread::spawn(move || {
                let result = match action {
                    TrayMenuAction::DbusMenu(id) => call_menu_item(&item, id),
                    TrayMenuAction::Activate => call_item_action(&item, TrayAction::Activate),
                    TrayMenuAction::SecondaryActivate => {
                        call_item_action(&item, TrayAction::SecondaryActivate)
                    }
                    TrayMenuAction::NativeContextMenu => {
                        call_item_action(&item, TrayAction::ContextMenu)
                    }
                };
                if let Err(e) = result {
                    tracing::warn!(item = item.key, "tray menu action failed: {e}");
                }
            });
            None
        }
        Message::Error(error) => {
            model.errors.push(error);
            None
        }
    })
    .flatten()
}

#[unsafe(no_mangle)]
pub extern "Rust" fn launch(_focused_index: usize, _model: PluginModel) -> Option<Task<PluginMsg>> {
    Some(Task::done(msg(Message::TogglePopup)))
}

#[unsafe(no_mangle)]
pub extern "Rust" fn errors(model: PluginModel) -> Vec<String> {
    drain_model_errors::<Model>(&model, |model| &mut model.errors)
}

#[unsafe(no_mangle)]
pub extern "Rust" fn view(
    model: PluginModel,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    with_model_read::<Model, _>(&model, |model| {
        let count = model.items.len();
        let content: Element<'static, PluginMsg> = if count == 0 {
            text("󰀻")
                .size(OXITHEME.font_md)
                .align_y(Alignment::Center)
                .align_x(Alignment::Center)
                .into()
        } else {
            Row::new()
                .push(text("󰀻").size(OXITHEME.font_md).align_y(Alignment::Center))
                .push(
                    text(count.to_string())
                        .size(OXITHEME.font_md)
                        .align_y(Alignment::Center),
                )
                .spacing(OXITHEME.padding_md)
                .height(Length::Fill)
                .align_y(Alignment::Center)
                .into()
        };

        let btn = oxi_plugin::bar_button(content).on_press(msg(Message::TogglePopup));

        vec![btn.into()]
    })
}

#[unsafe(no_mangle)]
pub extern "Rust" fn popup_view(
    model: PluginModel,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    with_model_read::<Model, _>(&model, |model| {
        let mut list = Column::new()
            .spacing(OXITHEME.padding_sm)
            .padding([OXITHEME.padding_md, OXITHEME.padding_lg])
            .width(Length::Fill);

        if model.items.is_empty() {
            list = list.push(text("No tray items").size(OXITHEME.font_md).style(|_| {
                iced::widget::text::Style {
                    color: Some(OXITHEME.primary),
                }
            }));
        }

        for item in model.items.values() {
            let menu = if model.open_menu.as_deref() == Some(item.key.as_str()) {
                Some(model.menus.get(&item.key).map(Vec::as_slice).unwrap_or(&[]))
            } else {
                None
            };
            let position = model
                .cursor_positions
                .get(&item.key)
                .copied()
                .unwrap_or_else(|| Point::new(OXITHEME.padding_md, OXITHEME.padding_md));
            list = list.push(tray_item_row(
                item,
                model.hovered_item.as_deref() == Some(&item.key),
                menu,
                position,
            ));
        }

        if tray_popup_height(model.items.len()) >= POPUP_MAX_HEIGHT {
            vec![scrollable(list).height(Length::Fill).into()]
        } else {
            vec![list.into()]
        }
    })
}

fn tray_popup_height(item_count: usize) -> u32 {
    if item_count == 0 {
        return POPUP_MIN_HEIGHT;
    }

    let count = item_count as u32;
    let rows = count * TRAY_ROW_HEIGHT;
    let spacing = count.saturating_sub(1) * TRAY_ROW_SPACING;
    (POPUP_VERTICAL_PADDING + rows + spacing).clamp(POPUP_MIN_HEIGHT, POPUP_MAX_HEIGHT)
}

fn tray_item_row(
    item: &TrayItem,
    _hovered: bool,
    menu: Option<&[TrayMenuItem]>,
    position: Point,
) -> Element<'static, PluginMsg> {
    let key = item.key.clone();
    let label = item.label();
    let icon = tray_icon(item.icon_path.as_deref());

    let item_button = button(
        Row::new()
            .push(icon)
            .push(text(label).size(OXITHEME.font_md).width(Length::Fill))
            .spacing(OXITHEME.padding_sm)
            .align_y(Alignment::Center),
    )
    .on_press(msg(Message::Activate {
        key: key.clone(),
        action: TrayAction::Activate,
    }))
    .style(tray_item_button_style)
    .padding([OXITHEME.padding_xs, OXITHEME.padding_sm])
    .width(Length::Fill);

    let row = Row::new().push(item_button).spacing(OXITHEME.padding_sm);

    let row = mouse_area(row)
        .on_enter(msg(Message::HoverItem(Some(item.key.clone()))))
        .on_move({
            let key = item.key.clone();
            move |position| {
                msg(Message::CursorMoved {
                    key: key.clone(),
                    position,
                })
            }
        })
        .on_exit(msg(Message::HoverItem(None)))
        .on_right_press(msg(Message::ToggleContextMenu(item.key.clone())));

    if let Some(menu) = menu {
        ContextMenuOverlay::new(row.into(), tray_context_menu(&item.key, menu), position).into()
    } else {
        row.into()
    }
}

fn tray_context_menu(key: &str, items: &[TrayMenuItem]) -> Element<'static, PluginMsg> {
    let mut menu = Column::new()
        .spacing(CONTEXT_MENU_ITEM_SPACING)
        .width(CONTEXT_MENU_WIDTH);
    if items.is_empty() {
        menu = menu.push(text("Loading menu...").size(OXITHEME.font_sm).style(|_| {
            iced::widget::text::Style {
                color: Some(OXITHEME.text_muted),
            }
        }));
    } else {
        for item in items {
            if item.separator {
                menu = menu.push(
                    text("────────")
                        .size(OXITHEME.font_sm)
                        .style(|_| iced::widget::text::Style {
                            color: Some(OXITHEME.text_muted),
                        })
                        .width(CONTEXT_MENU_WIDTH),
                );
                continue;
            }

            let label = Row::new()
                .push(
                    Space::new().width(item.depth.saturating_mul(CONTEXT_MENU_DEPTH_INDENT) as u32),
                )
                .push(
                    text(item.label.clone())
                        .size(OXITHEME.font_md)
                        .style(menu_item_text_style(item.enabled))
                        .width(Length::Fill),
                )
                .align_y(Alignment::Center);
            let mut button = button(label)
                .style(tray_menu_button_style)
                .padding([OXITHEME.padding_xs, OXITHEME.padding_sm])
                .width(Length::Fill);
            if item.enabled
                && let Some(action) = item.action.clone()
            {
                button = button.on_press(msg(Message::MenuAction {
                    key: key.to_owned(),
                    action,
                }));
            }
            menu = menu.push(button);
        }
    }

    container(menu)
        .padding([OXITHEME.padding_xs, OXITHEME.padding_sm])
        .width(CONTEXT_MENU_WIDTH)
        .style(tray_context_menu_style)
        .into()
}

struct ContextMenuOverlay<'a> {
    content: Element<'a, PluginMsg>,
    menu: Element<'a, PluginMsg>,
    position: Point,
}

impl<'a> ContextMenuOverlay<'a> {
    fn new(content: Element<'a, PluginMsg>, menu: Element<'a, PluginMsg>, position: Point) -> Self {
        Self {
            content,
            menu,
            position,
        }
    }
}

impl<'a> Widget<PluginMsg, iced::Theme, iced::Renderer> for ContextMenuOverlay<'a> {
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content), Tree::new(&self.menu)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[&self.content, &self.menu]);
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &iced::Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, PluginMsg>,
        viewport: &Rectangle,
    ) {
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        _renderer: &iced::Renderer,
        _viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, PluginMsg, iced::Theme, iced::Renderer>> {
        let position =
            layout.position() + translation + Vector::new(self.position.x, self.position.y);
        Some(overlay::Element::new(Box::new(ContextMenuOverlayLayer {
            menu: &mut self.menu,
            tree: &mut tree.children[1],
            position,
        })))
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }
}

impl<'a> From<ContextMenuOverlay<'a>> for Element<'a, PluginMsg> {
    fn from(value: ContextMenuOverlay<'a>) -> Self {
        Element::new(value)
    }
}

struct ContextMenuOverlayLayer<'a, 'b> {
    menu: &'b mut Element<'a, PluginMsg>,
    tree: &'b mut Tree,
    position: Point,
}

impl overlay::Overlay<PluginMsg, iced::Theme, iced::Renderer> for ContextMenuOverlayLayer<'_, '_> {
    fn layout(&mut self, renderer: &iced::Renderer, bounds: Size) -> layout::Node {
        let max_size = Size::new(
            CONTEXT_MENU_WIDTH + CONTEXT_MENU_LIMIT_EXTRA,
            CONTEXT_MENU_MAX_HEIGHT,
        );
        let menu_layout = self.menu.as_widget_mut().layout(
            self.tree,
            renderer,
            &layout::Limits::new(Size::ZERO, max_size),
        );
        let size = menu_layout.size();
        let x = (self.position.x + CONTEXT_MENU_OFFSET)
            .clamp(0.0, (bounds.width - size.width).max(0.0));
        let y = (self.position.y + CONTEXT_MENU_OFFSET)
            .clamp(0.0, (bounds.height - size.height).max(0.0));
        menu_layout.move_to(Point::new(x, y))
    }

    fn draw(
        &self,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        self.menu.as_widget().draw(
            self.tree,
            renderer,
            theme,
            style,
            layout,
            cursor,
            &Rectangle::with_size(Size::INFINITE),
        );
    }

    fn update(
        &mut self,
        event: &iced::Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, PluginMsg>,
    ) {
        self.menu.as_widget_mut().update(
            self.tree,
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            &Rectangle::with_size(Size::INFINITE),
        );
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.menu.as_widget().mouse_interaction(
            self.tree,
            layout,
            cursor,
            &Rectangle::with_size(Size::INFINITE),
            renderer,
        )
    }
}

fn fallback_context_menu() -> Vec<TrayMenuItem> {
    vec![
        TrayMenuItem {
            label: "Activate".to_owned(),
            enabled: true,
            depth: 0,
            action: Some(TrayMenuAction::Activate),
            separator: false,
        },
        TrayMenuItem {
            label: "Secondary activate".to_owned(),
            enabled: true,
            depth: 0,
            action: Some(TrayMenuAction::SecondaryActivate),
            separator: false,
        },
        TrayMenuItem {
            label: "Show native menu".to_owned(),
            enabled: true,
            depth: 0,
            action: Some(TrayMenuAction::NativeContextMenu),
            separator: false,
        },
    ]
}

fn menu_item_text_style(enabled: bool) -> impl Fn(&iced::Theme) -> iced::widget::text::Style {
    move |_| iced::widget::text::Style {
        color: Some(if enabled {
            OXITHEME.text
        } else {
            OXITHEME.text_muted
        }),
    }
}

fn tray_context_menu_style(_: &iced::Theme) -> container::Style {
    let palette = &OXITHEME;
    container::Style {
        background: Some(Background::Color(palette.mantle)),
        text_color: Some(palette.text),
        border: Border {
            color: palette.primary_bg_hover,
            width: CONTEXT_MENU_BORDER_WIDTH,
            radius: palette.border_radius.into(),
        },
        shadow: Shadow {
            color: Color::BLACK.scale_alpha(CONTEXT_MENU_SHADOW_ALPHA),
            offset: iced::Vector::new(0.0, CONTEXT_MENU_SHADOW_OFFSET_Y),
            blur_radius: CONTEXT_MENU_SHADOW_BLUR,
        },
        ..Default::default()
    }
}

fn tray_menu_button_style(_: &iced::Theme, status: button::Status) -> button::Style {
    let palette = &OXITHEME;
    let base = button::Style {
        background: None,
        text_color: palette.text,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: palette.border_radius.into(),
        },
        shadow: Shadow::default(),
        snap: false,
    };
    match status {
        button::Status::Hovered => button::Style {
            background: Some(Background::Color(palette.primary_bg_hover)),
            ..base
        },
        button::Status::Pressed => button::Style {
            background: Some(Background::Color(palette.primary_bg_active)),
            ..base
        },
        button::Status::Active | button::Status::Disabled => base,
    }
}

fn tray_icon(path: Option<&str>) -> Element<'static, PluginMsg> {
    let Some(path) = path else {
        return text("󰀻")
            .size(OXITHEME.font_md)
            .align_x(Alignment::Center)
            .width(TRAY_FALLBACK_ICON_WIDTH)
            .into();
    };
    if path.ends_with(".svg") || path.ends_with(".svgz") {
        svg(path)
            .width(TRAY_ICON_SIZE)
            .height(TRAY_ICON_SIZE)
            .into()
    } else {
        image(path)
            .width(TRAY_ICON_SIZE)
            .height(TRAY_ICON_SIZE)
            .content_fit(ContentFit::Contain)
            .into()
    }
}

fn tray_item_button_style(_: &iced::Theme, status: button::Status) -> button::Style {
    let palette = &OXITHEME;
    let base = button::Style {
        background: Some(Background::Color(palette.primary_bg)),
        text_color: palette.primary,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: palette.border_radius.into(),
        },
        shadow: Shadow::default(),
        snap: false,
    };
    match status {
        button::Status::Hovered => button::Style {
            background: Some(Background::Color(palette.primary_bg_hover)),
            ..base
        },
        button::Status::Pressed => button::Style {
            background: Some(Background::Color(palette.primary_bg_active)),
            ..base
        },
        button::Status::Active | button::Status::Disabled => base,
    }
}

#[derive(Clone)]
struct Watcher {
    items: Arc<Mutex<BTreeMap<String, TrayItem>>>,
    output: Arc<Mutex<iced::futures::channel::mpsc::Sender<PluginMsg>>>,
}

#[interface(name = "org.kde.StatusNotifierWatcher")]
impl Watcher {
    #[zbus(name = "RegisterStatusNotifierItem")]
    fn register_status_notifier_item(
        &self,
        service_or_path: String,
        #[zbus(header)] header: Header<'_>,
    ) {
        let sender = header
            .sender()
            .map(|name| name.to_string())
            .unwrap_or_default();
        let (service, path) = normalize_item_address(&sender, &service_or_path);
        if service.is_empty() || path.is_empty() {
            return;
        }
        let item = TrayItem::new_fallback(service.clone(), path.clone(), service_or_path);
        self.items.lock().unwrap().insert(item.key.clone(), item);
        publish_snapshot(&self.items, &self.output);

        let items = self.items.clone();
        let output = self.output.clone();
        std::thread::spawn(move || {
            if let Some(item) = query_item(&service, &path) {
                items.lock().unwrap().insert(item.key.clone(), item);
                publish_snapshot(&items, &output);
            }
        });
    }

    #[zbus(name = "RegisterStatusNotifierHost")]
    fn register_status_notifier_host(&self, _service: String) {}

    #[zbus(property, name = "RegisteredStatusNotifierItems")]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.items.lock().unwrap().keys().cloned().collect()
    }

    #[zbus(property, name = "IsStatusNotifierHostRegistered")]
    fn is_status_notifier_host_registered(&self) -> bool {
        true
    }

    #[zbus(property, name = "ProtocolVersion")]
    fn protocol_version(&self) -> i32 {
        0
    }
}

#[unsafe(no_mangle)]
pub extern "Rust" fn subscription() -> *mut PluginStream {
    let s = stream::channel(
        32,
        move |output: iced::futures::channel::mpsc::Sender<PluginMsg>| async move {
            let output = Arc::new(Mutex::new(output));
            let watcher_output = output.clone();
            std::thread::spawn(move || run_watcher(watcher_output));
            std::future::pending::<()>().await;
        },
    );

    Box::into_raw(Box::new(s)) as *mut PluginStream
}

fn run_watcher(output: Arc<Mutex<iced::futures::channel::mpsc::Sender<PluginMsg>>>) {
    let items = Arc::new(Mutex::new(BTreeMap::new()));
    let watcher = Watcher {
        items: items.clone(),
        output: output.clone(),
    };

    let connection = match Connection::session() {
        Ok(connection) => connection,
        Err(e) => {
            let _ = output.lock().unwrap().try_send(msg(Message::Error(format!(
                "tray: could not connect to session bus: {e}"
            ))));
            return;
        }
    };

    if let Err(e) = connection.object_server().at(WATCHER_PATH, watcher) {
        let _ = output.lock().unwrap().try_send(msg(Message::Error(format!(
            "tray: could not register watcher object: {e}"
        ))));
        return;
    }

    let flags = RequestNameFlags::ReplaceExisting | RequestNameFlags::DoNotQueue;
    if let Err(e) = connection.request_name_with_flags(WATCHER_BUS_NAME, flags) {
        let _ = output.lock().unwrap().try_send(msg(Message::Error(format!(
            "tray: could not own {WATCHER_BUS_NAME}: {e}"
        ))));
        return;
    }

    loop {
        std::thread::sleep(Duration::from_secs(30));
        publish_snapshot(&items, &output);
    }
}

fn publish_snapshot(
    items: &Arc<Mutex<BTreeMap<String, TrayItem>>>,
    output: &Arc<Mutex<iced::futures::channel::mpsc::Sender<PluginMsg>>>,
) {
    let snapshot = items.lock().unwrap().values().cloned().collect::<Vec<_>>();
    let _ = output
        .lock()
        .unwrap()
        .try_send(msg(Message::ItemsChanged(snapshot)));
}

const _: fn() = || {
    fn assert_stream<S: Stream<Item = PluginMsg> + Send + 'static>(_: &S) {}
    let _ = |s: &iced::futures::stream::BoxStream<'static, PluginMsg>| assert_stream(s);
};

#[cfg(test)]
mod tests {
    use super::*;

    fn tray_item(key: &str, title: &str) -> TrayItem {
        TrayItem {
            key: key.to_owned(),
            service: "org.example.App".to_owned(),
            path: "/StatusNotifierItem".to_owned(),
            id: "example-id".to_owned(),
            title: title.to_owned(),
            icon_path: None,
            menu_path: None,
        }
    }

    #[test]
    fn model_views_and_error_drain_are_deterministic() {
        let (plugin_model, init_task) = model(Table::new());
        assert!(init_task.is_none());
        assert_eq!(name(), "Tray");
        assert_eq!(abi_version(), ABI_VERSION);
        assert_eq!(view(plugin_model.clone()).unwrap().len(), 1);
        assert_eq!(popup_view(plugin_model.clone()).unwrap().len(), 1);
        assert_eq!(
            popup_metrics(plugin_model.clone()).popup_size,
            Some((POPUP_WIDTH, POPUP_MIN_HEIGHT))
        );

        let _ = update(
            plugin_model.clone(),
            msg(Message::ItemsChanged(vec![tray_item("k", "Title")])),
        );
        let _ = update(
            plugin_model.clone(),
            msg(Message::HoverItem(Some("k".to_owned()))),
        );
        {
            let guard = plugin_model.read().unwrap();
            let model = guard.downcast_ref::<Model>().unwrap();
            assert_eq!(model.items.len(), 1);
            assert_eq!(model.hovered_item.as_deref(), Some("k"));
        }

        let task = update(
            plugin_model.clone(),
            msg(Message::ToggleContextMenu("k".to_owned())),
        );
        assert!(task.is_none());
        {
            let guard = plugin_model.read().unwrap();
            let model = guard.downcast_ref::<Model>().unwrap();
            assert_eq!(model.open_menu.as_deref(), Some("k"));
            assert_eq!(model.menus.get("k").unwrap().len(), 3);
        }
        assert_eq!(
            popup_metrics(plugin_model.clone()).popup_input_size,
            Some((POPUP_WIDTH, POPUP_MAX_HEIGHT))
        );

        let _ = update(
            plugin_model.clone(),
            msg(Message::Error("watcher failed".to_owned())),
        );
        assert_eq!(errors(plugin_model.clone()), vec!["watcher failed"]);
        assert!(errors(plugin_model).is_empty());
    }

    #[test]
    fn tray_popup_height_tracks_item_count_with_cap() {
        assert_eq!(tray_popup_height(0), POPUP_MIN_HEIGHT);
        assert_eq!(tray_popup_height(1), 54);
        assert_eq!(tray_popup_height(5), 198);
        assert_eq!(tray_popup_height(100), POPUP_MAX_HEIGHT);
    }
}
