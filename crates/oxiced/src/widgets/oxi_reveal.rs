use iced::advanced::{
    Clipboard, Layout, Shell, Widget, layout, overlay, renderer,
    widget::{Operation, Tree},
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
fn content_limits(limits: &layout::Limits, full: f32) -> layout::Limits {
    layout::Limits::new(Size::ZERO, Size::new(limits.max().width, full))
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
            self.content.as_widget_mut().update(
                &mut tree.children[0],
                event,
                l.children().next().unwrap(),
                cursor,
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
                    cursor,
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
                    cursor,
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
    #[test]
    fn child_layout_remains_full_size_during_reveal() {
        for height in [1., 30., 120., 760.] {
            let limits = layout::Limits::new(Size::ZERO, Size::new(960., height));
            assert_eq!(content_limits(&limits, 760.).max(), Size::new(960., 760.));
        }
    }
}
