use iced::advanced::{
    Clipboard, Layout, Shell, Widget, layout, overlay, renderer,
    widget::{
        Id, Operation, Tree,
        operation::{Focusable, Scrollable, TextInput},
    },
};
use iced::{Element, Event, Length, Rectangle, Size, Vector, mouse};

/// Reveal a fixed-layout child without shrinking and reshaping it on animation ticks.
pub fn reveal<'a, M: 'a>(
    content: impl Into<Element<'a, M>>,
    full_height: f32,
    visible_height: f32,
) -> Reveal<'a, M> {
    Reveal {
        content: content.into(),
        full: full_height.max(1.),
        visible: visible_height.max(1.),
    }
}
pub struct Reveal<'a, M> {
    content: Element<'a, M>,
    full: f32,
    visible: f32,
}

struct VisibleFocus<'a> {
    inner: &'a mut dyn Operation,
    bounds: Rectangle,
}
impl Operation for VisibleFocus<'_> {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        self.inner.traverse(&mut |inner| {
            operate(&mut VisibleFocus {
                inner,
                bounds: self.bounds,
            })
        });
    }
    fn container(&mut self, id: Option<&Id>, bounds: Rectangle) {
        self.inner.container(id, bounds);
    }
    fn focusable(&mut self, id: Option<&Id>, bounds: Rectangle, state: &mut dyn Focusable) {
        if self.bounds.contains(bounds.center()) {
            self.inner.focusable(id, bounds, state);
        } else {
            state.unfocus();
        }
    }
    fn text_input(&mut self, id: Option<&Id>, bounds: Rectangle, state: &mut dyn TextInput) {
        if self.bounds.contains(bounds.center()) {
            self.inner.text_input(id, bounds, state);
        }
    }
    fn scrollable(
        &mut self,
        id: Option<&Id>,
        bounds: Rectangle,
        content: Rectangle,
        translation: Vector,
        state: &mut dyn Scrollable,
    ) {
        self.inner
            .scrollable(id, bounds, content, translation, state);
    }
    fn text(&mut self, id: Option<&Id>, bounds: Rectangle, text: &str) {
        self.inner.text(id, bounds, text);
    }
    fn custom(&mut self, id: Option<&Id>, bounds: Rectangle, state: &mut dyn std::any::Any) {
        self.inner.custom(id, bounds, state);
    }
}
fn content_limits(limits: &layout::Limits, full: f32) -> layout::Limits {
    layout::Limits::new(Size::ZERO, Size::new(limits.max().width, full))
}
fn clipped_cursor(cursor: mouse::Cursor, viewport: Rectangle) -> mouse::Cursor {
    if cursor.is_over(viewport) {
        cursor
    } else {
        mouse::Cursor::Unavailable
    }
}
impl<M> Widget<M, iced::Theme, iced::Renderer> for Reveal<'_, M> {
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }
    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }
    fn size(&self) -> Size<Length> {
        Size::new(
            self.content.as_widget().size().width,
            Length::Fixed(self.visible),
        )
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let node = self.content.as_widget_mut().layout(
            &mut tree.children[0],
            renderer,
            &content_limits(limits, self.full),
        );
        layout::Node::with_children(
            Size::new(node.size().width, self.visible.min(limits.max().height)),
            vec![node],
        )
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        l: Layout<'_>,
        r: &iced::Renderer,
        op: &mut dyn Operation,
    ) {
        let mut visible = VisibleFocus {
            inner: op,
            bounds: l.bounds(),
        };
        let op: &mut dyn Operation = if self.visible < self.full {
            &mut visible
        } else {
            visible.inner
        };
        self.content.as_widget_mut().operate(
            &mut tree.children[0],
            l.children().next().unwrap(),
            r,
            op,
        );
    }
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        l: Layout<'_>,
        cursor: mouse::Cursor,
        r: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        if let Some(viewport) = viewport.intersection(&l.bounds()) {
            // Buttons hit-test their full layout bounds, ignoring the clipped viewport.
            self.content.as_widget_mut().update(
                &mut tree.children[0],
                event,
                l.children().next().unwrap(),
                clipped_cursor(cursor, viewport),
                r,
                clipboard,
                shell,
                &viewport,
            );
        }
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        l: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        r: &iced::Renderer,
    ) -> mouse::Interaction {
        viewport
            .intersection(&l.bounds())
            .map_or(mouse::Interaction::None, |viewport| {
                self.content.as_widget().mouse_interaction(
                    &tree.children[0],
                    l.children().next().unwrap(),
                    clipped_cursor(cursor, viewport),
                    &viewport,
                    r,
                )
            })
    }
    fn draw(
        &self,
        tree: &Tree,
        r: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        l: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        use iced::advanced::Renderer;
        if let Some(viewport) = viewport.intersection(&l.bounds()) {
            r.with_layer(viewport, |r| {
                self.content.as_widget().draw(
                    &tree.children[0],
                    r,
                    theme,
                    style,
                    l.children().next().unwrap(),
                    clipped_cursor(cursor, viewport),
                    &viewport,
                )
            });
        }
    }
    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        l: Layout<'b>,
        r: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, M, iced::Theme, iced::Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            l.children().next().unwrap(),
            r,
            viewport,
            translation,
        )
    }
}
impl<'a, M: 'a> From<Reveal<'a, M>> for Element<'a, M> {
    fn from(value: Reveal<'a, M>) -> Self {
        Element::new(value)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use iced::advanced::renderer::Headless;
    use iced::widget::{Column, Space, button};
    use iced::{Font, Pixels, Point};
    use iced_layershell::reexport::core::clipboard;

    fn renderer() -> iced::Renderer {
        iced::futures::executor::block_on(iced::Renderer::new(
            Font::default(),
            Pixels(16.),
            Some("tiny-skia"),
        ))
        .expect("headless CPU renderer")
    }

    fn child(visible: f32) -> Element<'static, &'static str> {
        reveal(
            button(Space::new())
                .width(100)
                .height(80)
                .on_press("submenu"),
            80.,
            visible,
        )
        .into()
    }

    fn pointer_event(
        widget: &mut Element<'_, &'static str>,
        tree: &mut Tree,
        node: &layout::Node,
        renderer: &iced::Renderer,
        event: mouse::Event,
        point: Point,
        viewport: &Rectangle,
    ) -> Vec<&'static str> {
        let mut messages = Vec::new();
        widget.as_widget_mut().update(
            tree,
            &Event::Mouse(event),
            Layout::new(node),
            mouse::Cursor::Available(point),
            renderer,
            &mut clipboard::Null,
            &mut Shell::new(&mut messages),
            viewport,
        );
        messages
    }

    fn click(
        widget: &mut Element<'_, &'static str>,
        tree: &mut Tree,
        node: &layout::Node,
        renderer: &iced::Renderer,
        point: Point,
        viewport: &Rectangle,
    ) -> Vec<&'static str> {
        [
            mouse::Event::ButtonPressed(mouse::Button::Left),
            mouse::Event::ButtonReleased(mouse::Button::Left),
        ]
        .into_iter()
        .flat_map(|event| pointer_event(widget, tree, node, renderer, event, point, viewport))
        .collect()
    }

    #[test]
    fn child_layout_remains_full_size_during_reveal() {
        for height in [1., 30., 120., 760.] {
            let limits = layout::Limits::new(Size::ZERO, Size::new(960., height));
            assert_eq!(content_limits(&limits, 760.).max(), Size::new(960., 760.));
        }
    }

    #[test]
    fn hidden_submenu_does_not_intercept_clicks_on_following_rows() {
        let renderer = renderer();
        let viewport = Rectangle::new(Point::ORIGIN, Size::new(100., 200.));
        for visible in [0., 20.] {
            let mut sidebar: Element<'_, &'static str> = Column::new()
                .push(child(visible))
                .push(button(Space::new()).width(100).height(40).on_press("dock"))
                .into();
            let mut tree = Tree::new(&sidebar);
            let node = sidebar.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, viewport.size()),
            );
            let dock = Layout::new(&node).children().nth(1).unwrap().bounds();
            assert_eq!(
                click(
                    &mut sidebar,
                    &mut tree,
                    &node,
                    &renderer,
                    dock.center(),
                    &viewport
                ),
                ["dock"],
                "visible submenu height: {visible}",
            );
        }
    }

    #[test]
    fn only_visible_child_region_accepts_clicks_and_hover() {
        let renderer = renderer();
        let viewport = Rectangle::new(Point::new(0., 10.), Size::new(100., 100.));
        let mut widget = child(20.);
        let mut tree = Tree::new(&widget);
        let node = widget.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(100., 200.)),
        );
        for (y, expected) in [(5., Vec::new()), (15., vec!["submenu"]), (30., Vec::new())] {
            let point = Point::new(50., y);
            assert_eq!(
                click(&mut widget, &mut tree, &node, &renderer, point, &viewport),
                expected,
                "cursor y: {y}",
            );
            assert_eq!(
                widget.as_widget().mouse_interaction(
                    &tree,
                    Layout::new(&node),
                    mouse::Cursor::Available(point),
                    &viewport,
                    &renderer,
                ),
                if expected.is_empty() {
                    mouse::Interaction::None
                } else {
                    mouse::Interaction::Pointer
                },
                "cursor y: {y}",
            );
        }
    }

    #[test]
    fn releasing_outside_reveal_cancels_child_press() {
        let renderer = renderer();
        let viewport = Rectangle::new(Point::ORIGIN, Size::new(100., 200.));
        let mut widget = child(20.);
        let mut tree = Tree::new(&widget);
        let node = widget.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, viewport.size()),
        );
        for (event, y) in [
            (mouse::Event::ButtonPressed(mouse::Button::Left), 10.),
            (mouse::Event::ButtonReleased(mouse::Button::Left), 30.),
            (mouse::Event::ButtonReleased(mouse::Button::Left), 10.),
        ] {
            assert!(
                pointer_event(
                    &mut widget,
                    &mut tree,
                    &node,
                    &renderer,
                    event,
                    Point::new(50., y),
                    &viewport,
                )
                .is_empty()
            );
        }
        assert_eq!(
            click(
                &mut widget,
                &mut tree,
                &node,
                &renderer,
                Point::new(50., 10.),
                &viewport,
            ),
            ["submenu"],
        );
    }
}
