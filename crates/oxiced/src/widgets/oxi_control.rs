//! Focusable adapters for controls whose native Iced widgets only handle pointers.
use iced::advanced::{
    Clipboard, Layout, Shell, Widget, layout, overlay, renderer,
    widget::{Operation, Tree, operation::Focusable, tree},
};
use iced::{
    Element, Event, Length, Padding, Pixels, Rectangle, Size, Theme, Vector, keyboard, mouse,
};

use super::oxi_navigation::{control_focus, key_event};
use crate::theme::theme_impl::OXITHEME;

#[derive(Debug, Default)]
pub(crate) struct Focus(pub bool);

impl Focusable for Focus {
    fn is_focused(&self) -> bool {
        self.0
    }
    fn focus(&mut self) {
        self.0 = true;
    }
    fn unfocus(&mut self) {
        self.0 = false;
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Action,
    Toggle(bool),
    Slider,
}

pub struct Control<'a, W, M> {
    inner: W,
    enabled: bool,
    kind: Kind,
    on_release: Option<M>,
    context_menu: Option<M>,
    activate: Option<Box<dyn Fn() -> M + 'a>>,
}

fn control<'a, W, M>(inner: W, enabled: bool, kind: Kind) -> Control<'a, W, M> {
    Control {
        inner,
        enabled,
        kind,
        on_release: None,
        context_menu: None,
        activate: None,
    }
}

pub type Button<'a, M> = Control<'a, iced::widget::Button<'a, M>, M>;
pub type Toggler<'a, M> = Control<'a, iced::widget::Toggler<'a, M>, M>;
pub type Slider<'a, V, M> = Control<'a, iced::widget::Slider<'a, V, M>, M>;

pub mod button {
    pub use iced::widget::button::{Status, Style};
    pub type Button<'a, M> = super::Button<'a, M>;
}

pub fn button<'a, M>(content: impl Into<Element<'a, M>>) -> Button<'a, M> {
    control(iced::widget::button(content), false, Kind::Action)
}

pub fn toggler<'a, M>(value: bool) -> Toggler<'a, M> {
    control(iced::widget::toggler(value), false, Kind::Toggle(value))
}

pub fn slider<'a, V, M>(
    range: std::ops::RangeInclusive<V>,
    value: V,
    on_change: impl Fn(V) -> M + 'a,
) -> Slider<'a, V, M>
where
    V: Copy + From<u8> + PartialOrd,
    M: Clone,
{
    control(
        iced::widget::slider(range, value, on_change),
        true,
        Kind::Slider,
    )
}

macro_rules! forward {
    ($name:ident, $ty:ty) => {
        pub fn $name(mut self, value: $ty) -> Self {
            self.inner = self.inner.$name(value);
            self
        }
    };
}

impl<'a, M> Button<'a, M> {
    forward!(width, impl Into<Length>);
    forward!(height, impl Into<Length>);
    forward!(padding, impl Into<Padding>);
    forward!(clip, bool);
    forward!(
        style,
        impl Fn(&Theme, iced::widget::button::Status) -> iced::widget::button::Style + 'a
    );
    pub fn on_press(self, message: M) -> Self
    where
        M: Clone + 'a,
    {
        self.on_press_maybe(Some(message))
    }
    pub fn on_press_maybe(mut self, message: Option<M>) -> Self
    where
        M: Clone + 'a,
    {
        self.enabled = message.is_some();
        self.activate = message
            .clone()
            .map(|message| Box::new(move || message.clone()) as Box<dyn Fn() -> M>);
        self.inner = self.inner.on_press_maybe(message);
        self
    }
    pub fn on_press_with(mut self, message: impl Fn() -> M + 'a) -> Self {
        self.enabled = true;
        let message = std::rc::Rc::new(message);
        let callback = message.clone();
        self.activate = Some(Box::new(move || callback()));
        self.inner = self.inner.on_press_with(move || message());
        self
    }
    pub fn on_context_menu(mut self, message: M) -> Self {
        self.context_menu = Some(message);
        self
    }
}

impl<'a, M> Toggler<'a, M> {
    forward!(width, impl Into<Length>);
    forward!(size, impl Into<Pixels>);
    forward!(spacing, impl Into<Pixels>);
    forward!(font, impl Into<iced::Font>);
    forward!(text_size, impl Into<Pixels>);
    forward!(label, impl iced::widget::text::IntoFragment<'a>);
    forward!(
        style,
        impl Fn(&Theme, iced::widget::toggler::Status) -> iced::widget::toggler::Style + 'a
    );
    pub fn on_toggle(mut self, callback: impl Fn(bool) -> M + 'a) -> Self {
        self.enabled = true;
        let Kind::Toggle(value) = self.kind else {
            unreachable!()
        };
        let callback = std::rc::Rc::new(callback);
        let activate = callback.clone();
        self.activate = Some(Box::new(move || activate(!value)));
        self.inner = self.inner.on_toggle(move |value| callback(value));
        self
    }
    pub fn on_toggle_maybe(mut self, callback: Option<impl Fn(bool) -> M + 'a>) -> Self
    where
        M: 'a,
    {
        if let Some(callback) = callback {
            self.on_toggle(callback)
        } else {
            self.enabled = false;
            self.activate = None;
            self.inner = self.inner.on_toggle_maybe(None::<fn(bool) -> M>);
            self
        }
    }
}

impl<'a, V, M> Slider<'a, V, M>
where
    V: Copy + From<u8> + PartialOrd,
    M: Clone,
{
    forward!(width, impl Into<Length>);
    forward!(height, impl Into<Pixels>);
    forward!(step, impl Into<V>);
    forward!(shift_step, impl Into<V>);
    forward!(default, impl Into<V>);
    pub fn on_release(mut self, message: M) -> Self {
        self.on_release = Some(message.clone());
        self.inner = self.inner.on_release(message);
        self
    }
    forward!(
        style,
        impl Fn(&Theme, iced::widget::slider::Status) -> iced::widget::slider::Style + 'a
    );
}

pub(crate) fn draw_focus<R: iced::advanced::Renderer>(renderer: &mut R, bounds: Rectangle) {
    renderer.fill_quad(
        renderer::Quad {
            bounds,
            border: iced::Border {
                color: OXITHEME.primary,
                width: 2.,
                radius: OXITHEME.border_radius.into(),
            },
            ..Default::default()
        },
        iced::Color::TRANSPARENT,
    );
}

impl<M: Clone, W: Widget<M, Theme, iced::Renderer>> Widget<M, Theme, iced::Renderer>
    for Control<'_, W, M>
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Focus>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(Focus::default())
    }
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(
            &self.inner as &dyn Widget<M, Theme, iced::Renderer>,
        )]
    }
    fn diff(&self, tree: &mut Tree) {
        tree.children[0].diff(&self.inner as &dyn Widget<M, Theme, iced::Renderer>);
        if !self.enabled {
            tree.state.downcast_mut::<Focus>().0 = false;
        }
    }
    fn size(&self) -> Size<Length> {
        self.inner.size()
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let child = self.inner.layout(&mut tree.children[0], renderer, limits);
        layout::Node::with_children(child.size(), vec![child])
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        if self.enabled {
            control_focus(
                operation,
                layout.bounds(),
                tree.state.downcast_mut::<Focus>(),
            );
        }
        self.inner.operate(
            &mut tree.children[0],
            layout.children().next().unwrap(),
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
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        let child = layout.children().next().unwrap();
        let focused = self.enabled && tree.state.downcast_ref::<Focus>().0;
        if focused && let Event::Keyboard(keyboard::Event::KeyPressed { key, repeat, .. }) = event {
            use keyboard::{Key, key::Named};
            let pointer = mouse::Cursor::Available(child.bounds().center());
            match (self.kind, key.as_ref()) {
                (_, Key::Named(Named::ContextMenu)) if self.context_menu.is_some() => {
                    if !repeat {
                        shell.publish(self.context_menu.clone().unwrap());
                    }
                    shell.capture_event();
                    return;
                }
                (Kind::Action | Kind::Toggle(_), Key::Named(Named::Enter | Named::Space)) => {
                    if !repeat && let Some(activate) = &self.activate {
                        shell.publish(activate());
                    }
                    shell.capture_event();
                    shell.request_redraw();
                    return;
                }
                (Kind::Slider, Key::Named(key @ (Named::ArrowLeft | Named::ArrowRight))) => {
                    let event = key_event(if key == Named::ArrowLeft {
                        Named::ArrowDown
                    } else {
                        Named::ArrowUp
                    });
                    self.inner.update(
                        &mut tree.children[0],
                        &event,
                        child,
                        pointer,
                        renderer,
                        clipboard,
                        shell,
                        viewport,
                    );
                    if let Some(message) = self.on_release.clone() {
                        shell.publish(message);
                    }
                    shell.capture_event();
                    return;
                }
                _ => {}
            }
        }
        self.inner.update(
            &mut tree.children[0],
            event,
            child,
            if matches!(event, Event::Keyboard(_)) {
                mouse::Cursor::Unavailable
            } else {
                cursor
            },
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }
    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.inner.draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout.children().next().unwrap(),
            cursor,
            viewport,
        );
        if self.enabled && tree.state.downcast_ref::<Focus>().0 {
            draw_focus(renderer, layout.bounds());
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
        self.inner.mouse_interaction(
            &tree.children[0],
            layout.children().next().unwrap(),
            cursor,
            viewport,
            renderer,
        )
    }
    fn overlay<'a>(
        &'a mut self,
        tree: &'a mut Tree,
        layout: Layout<'a>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'a, M, Theme, iced::Renderer>> {
        self.inner.overlay(
            &mut tree.children[0],
            layout.children().next().unwrap(),
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, M: Clone + 'a, W: Widget<M, Theme, iced::Renderer> + 'a> From<Control<'a, W, M>>
    for Element<'a, M>
{
    fn from(control: Control<'a, W, M>) -> Self {
        Self::new(control)
    }
}
