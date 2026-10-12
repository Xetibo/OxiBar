use iced::advanced::{
    Clipboard, Layout, Shell, Widget, layout, overlay, renderer,
    widget::{Operation, Tree},
};
use iced::{Element, Event, Length, Point, Rectangle, Size, Vector, mouse};

/// Capture the press position in the widget's own event pass, before an
/// asynchronous cursor subscription can replace it with another position.
pub fn drag_handle<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    on_press: impl Fn(Point) -> Message + 'a,
) -> DragHandle<'a, Message> {
    DragHandle {
        content: content.into(),
        on_press: Box::new(on_press),
        release: false,
        enabled: true,
        bounds: None,
        clip_bounds: false,
        identity: None,
    }
}

pub fn drop_zone<'a, Message: Clone + 'a>(
    content: impl Into<Element<'a, Message>>,
    enabled: bool,
    on_drop: Message,
) -> DragHandle<'a, Message> {
    DragHandle {
        content: content.into(),
        on_press: Box::new(move |_| on_drop.clone()),
        release: true,
        enabled,
        bounds: None,
        clip_bounds: false,
        identity: None,
    }
}

pub struct DragHandle<'a, Message> {
    content: Element<'a, Message>,
    on_press: Box<dyn Fn(Point) -> Message + 'a>,
    release: bool,
    enabled: bool,
    bounds: Option<Box<dyn Fn(Rectangle, bool) -> Message + 'a>>,
    clip_bounds: bool,
    identity: Option<String>,
}

impl<Message> DragHandle<'_, Message> {
    /// Republish bounds when a different logical item reuses the same widget tree.
    pub fn with_identity(mut self, identity: impl Into<String>) -> Self {
        self.identity = Some(identity.into());
        self
    }
}

/// Report actual rendered bounds without consuming the child's button events.
pub fn capture_bounds<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    on_bounds: impl Fn(Rectangle, bool) -> Message + 'a,
) -> DragHandle<'a, Message> {
    DragHandle {
        content: content.into(),
        on_press: Box::new(|_| unreachable!("bounds capture does not handle drag presses")),
        release: false,
        enabled: false,
        bounds: Some(Box::new(on_bounds)),
        clip_bounds: false,
        identity: None,
    }
}

/// Report only the visible hit region, including an empty region when scrolled out.
pub fn capture_visible_bounds<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    on_bounds: impl Fn(Rectangle, bool) -> Message + 'a,
) -> DragHandle<'a, Message> {
    let mut handle = capture_bounds(content, on_bounds);
    handle.clip_bounds = true;
    handle
}

#[derive(Default)]
struct BoundsState {
    last: Option<Rectangle>,
    identity: Option<String>,
}

impl<Message> Widget<Message, iced::Theme, iced::Renderer> for DragHandle<'_, Message> {
    fn tag(&self) -> iced::advanced::widget::tree::Tag {
        iced::advanced::widget::tree::Tag::of::<BoundsState>()
    }
    fn state(&self) -> iced::advanced::widget::tree::State {
        iced::advanced::widget::tree::State::new(BoundsState {
            identity: self.identity.clone(),
            ..BoundsState::default()
        })
    }
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }
    fn diff(&self, tree: &mut Tree) {
        let state = tree.state.downcast_mut::<BoundsState>();
        if state.identity != self.identity {
            state.identity.clone_from(&self.identity);
            state.last = None;
        }
        tree.diff_children(std::slice::from_ref(&self.content));
    }
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
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
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        if let Some(on_bounds) = &self.bounds {
            let bounds = if self.clip_bounds {
                layout.bounds().intersection(viewport).unwrap_or_default()
            } else {
                layout.bounds()
            };
            let pressed = matches!(
                event,
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            ) && cursor
                .position()
                .is_some_and(|point| bounds.contains(point) && viewport.contains(point));
            let state = tree.state.downcast_mut::<BoundsState>();
            if (self.clip_bounds || bounds.intersects(viewport))
                && (state.last != Some(bounds) || pressed)
            {
                state.last = Some(bounds);
                shell.publish(on_bounds(bounds, pressed));
            }
        }
        let press = match (self.enabled, self.release, event) {
            (true, false, Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)))
            | (true, true, Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))) => {
                cursor.position()
            }
            _ => None,
        };
        if let Some(point) =
            press.filter(|point| layout.bounds().contains(*point) && viewport.contains(*point))
        {
            shell.publish((self.on_press)(point));
            shell.capture_event();
            return;
        }
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
        if self.enabled && !self.release && cursor.is_over(layout.bounds()) {
            mouse::Interaction::Grab
        } else {
            self.content.as_widget().mouse_interaction(
                &tree.children[0],
                layout,
                cursor,
                viewport,
                renderer,
            )
        }
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
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, Message: 'a> From<DragHandle<'a, Message>> for Element<'a, Message> {
    fn from(handle: DragHandle<'a, Message>) -> Self {
        Element::new(handle)
    }
}

/// A draw-only preview follows the pointer without intercepting drop targets.
pub fn drag_preview<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    preview: Option<Element<'a, Message>>,
) -> DragPreview<'a, Message> {
    DragPreview {
        content: content.into(),
        preview,
    }
}

pub struct DragPreview<'a, Message> {
    content: Element<'a, Message>,
    preview: Option<Element<'a, Message>>,
}

struct DragPreviewState;

impl<Message> Widget<Message, iced::Theme, iced::Renderer> for DragPreview<'_, Message> {
    fn tag(&self) -> iced::advanced::widget::tree::Tag {
        iced::advanced::widget::tree::Tag::of::<DragPreviewState>()
    }

    fn state(&self) -> iced::advanced::widget::tree::State {
        iced::advanced::widget::tree::State::new(DragPreviewState)
    }

    fn children(&self) -> Vec<Tree> {
        std::iter::once(Tree::new(&self.content))
            .chain(self.preview.as_ref().map(Tree::new))
            .collect()
    }

    fn diff(&self, tree: &mut Tree) {
        let children: Vec<_> = std::iter::once(self.content.as_widget())
            .chain(self.preview.as_ref().map(|preview| preview.as_widget()))
            .collect();
        tree.diff_children(&children);
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let content = self
            .content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits);
        let size = content.size();
        let mut children = vec![content];
        if let Some(preview) = &mut self.preview {
            children.push(preview.as_widget_mut().layout(
                &mut tree.children[1],
                renderer,
                &layout::Limits::new(Size::ZERO, limits.max()),
            ));
        }
        layout::Node::with_children(size, children)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content.as_widget_mut().operate(
            &mut tree.children[0],
            layout.children().next().expect("content layout"),
            renderer,
            operation,
        );
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout.children().next().expect("content layout"),
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
        if self.preview.is_some() && matches!(event, Event::Mouse(mouse::Event::CursorMoved { .. }))
        {
            shell.request_redraw();
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        if self.preview.is_some() {
            mouse::Interaction::Grabbing
        } else {
            self.content.as_widget().mouse_interaction(
                &tree.children[0],
                layout.children().next().expect("content layout"),
                cursor,
                viewport,
                renderer,
            )
        }
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
        use iced::advanced::Renderer as _;
        let mut children = layout.children();
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            children.next().expect("content layout"),
            cursor,
            viewport,
        );
        if let (Some(preview), Some(point), Some(layout)) =
            (&self.preview, cursor.position(), children.next())
        {
            let bounds = layout.bounds();
            let gap = crate::theme::theme_impl::OXITHEME.padding_lg;
            let x = (point.x + gap)
                .min(viewport.x + viewport.width - bounds.width)
                .max(viewport.x);
            let y = (point.y + gap)
                .min(viewport.y + viewport.height - bounds.height)
                .max(viewport.y);
            let offset = Vector::new(x - bounds.x, y - bounds.y);
            let translated_viewport = Rectangle {
                x: viewport.x - offset.x,
                y: viewport.y - offset.y,
                ..*viewport
            };
            renderer.with_layer(*viewport, |renderer| {
                renderer.with_translation(offset, |renderer| {
                    preview.as_widget().draw(
                        &tree.children[1],
                        renderer,
                        theme,
                        style,
                        layout,
                        mouse::Cursor::Unavailable,
                        &translated_viewport,
                    );
                })
            });
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout.children().next().expect("content layout"),
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, Message: 'a> From<DragPreview<'a, Message>> for Element<'a, Message> {
    fn from(preview: DragPreview<'a, Message>) -> Self {
        Element::new(preview)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::widget::{Space, text, text_input};

    #[test]
    fn equal_geometry_republishes_bounds_for_a_replaced_logical_item() {
        let captured = |identity| -> Element<'_, ()> {
            capture_bounds(Space::new().width(34).height(22), |_, _| ())
                .with_identity(identity)
                .into()
        };
        let launcher = captured("start:0:launcher");
        let settings = captured("start:0:settings");
        let mut tree = Tree::new(&launcher);
        tree.diff(&launcher);
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(34., 22.));
        tree.state.downcast_mut::<BoundsState>().last = Some(bounds);
        tree.diff(&launcher);
        assert_eq!(tree.state.downcast_ref::<BoundsState>().last, Some(bounds));
        tree.diff(&settings);
        assert_eq!(tree.state.downcast_ref::<BoundsState>().last, None);
    }

    #[test]
    fn drag_preview_reconciles_empty_surfaces_and_preview_transitions() {
        let content = || text_input::<(), iced::Theme, iced::Renderer>("Search", "kept");
        let without: Element<'_, ()> = drag_preview(content(), None).into();
        let with: Element<'_, ()> =
            drag_preview(content(), Some(text("Dragged card").into())).into();
        let empty: Element<'_, ()> = Space::new().into();
        let mut tree = Tree::new(&empty);

        tree.diff(&without);
        assert_eq!(tree.children.len(), 1);
        let input_tag = tree.children[0].tag;
        assert_ne!(tree.tag, input_tag);
        tree.diff(&with);
        assert_eq!(tree.children.len(), 2);
        assert_eq!(tree.children[0].tag, input_tag);
        tree.diff(&without);
        assert_eq!(tree.children.len(), 1);
        tree.diff(&empty);
        tree.diff(&with);
        assert_eq!(tree.children.len(), 2);

        let mut empty_tree = Tree::empty();
        with.as_widget().diff(&mut empty_tree);
        assert_eq!(empty_tree.children.len(), 2);
    }
}
