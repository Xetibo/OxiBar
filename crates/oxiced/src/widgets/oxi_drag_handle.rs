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
    }
}

pub struct DragHandle<'a, Message> {
    content: Element<'a, Message>,
    on_press: Box<dyn Fn(Point) -> Message + 'a>,
}

impl<Message> Widget<Message, iced::Theme, iced::Renderer> for DragHandle<'_, Message> {
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }
    fn diff(&self, tree: &mut Tree) {
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
        let press = match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => cursor.position(),
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
        if cursor.is_over(layout.bounds()) {
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
