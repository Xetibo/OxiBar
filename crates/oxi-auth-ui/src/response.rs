use std::time::{Duration, Instant};

use iced::advanced::Renderer as _;
use iced::{
    Color, Element, Event, Font, Length, Pixels, Rectangle, Size,
    advanced::{
        Clipboard, Layout, Shell, Widget, layout, mouse, renderer, text,
        widget::{Operation, Tree, text as text_widget, tree},
    },
};
use oxiced::theme::{snapshot, theme_impl::OXITHEME};

const BLINK: Duration = Duration::from_millis(500);
type Paragraph = <iced::Renderer as text::Renderer>::Paragraph;

pub(crate) fn display<'a, Message: 'a>(value: String, focused: bool) -> Element<'a, Message> {
    Element::new(Response {
        value,
        focused,
        reduced_motion: snapshot::current_snapshot().reduced_motion,
        font: snapshot::current_font(),
        size: OXITHEME.font_xl,
        color: OXITHEME.text,
    })
}

struct Response {
    // This is only the mask, or text from an explicitly visible authentication prompt.
    value: String,
    focused: bool,
    reduced_motion: bool,
    font: Font,
    size: f32,
    color: Color,
}

struct State {
    paragraph: text::paragraph::Plain<Paragraph>,
    focused: bool,
    window_focused: bool,
    updated: Instant,
    frame: Instant,
}
impl State {
    fn reset(&mut self) {
        self.updated = Instant::now();
        self.frame = self.updated;
    }
}

impl<Message> Widget<Message, iced::Theme, iced::Renderer> for Response {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fixed(self.size * 1.3))
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        let now = Instant::now();
        tree::State::new(State {
            paragraph: text::paragraph::Plain::default(),
            focused: false,
            window_focused: true,
            updated: now,
            frame: now,
        })
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let state = tree.state.downcast_mut::<State>();
        if state.focused != self.focused || state.paragraph.content() != self.value {
            state.reset();
        }
        state.focused = self.focused;
        text_widget::layout(
            &mut state.paragraph,
            renderer,
            limits,
            &self.value,
            text_widget::Format {
                width: Length::Fill,
                height: Length::Fixed(self.size * 1.3),
                size: Some(Pixels(self.size)),
                font: Some(self.font),
                wrapping: text::Wrapping::None,
                ..Default::default()
            },
        )
    }

    fn operate(
        &mut self,
        _: &mut Tree,
        layout: Layout<'_>,
        _: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        operation.text(None, layout.bounds(), &self.value);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        _: Layout<'_>,
        _: mouse::Cursor,
        _: &iced::Renderer,
        _: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        match event {
            Event::Window(iced::window::Event::Unfocused) => {
                state.window_focused = false;
                shell.request_redraw();
            }
            Event::Window(iced::window::Event::Focused) => {
                state.window_focused = true;
                state.reset();
                shell.request_redraw();
            }
            Event::Window(iced::window::Event::RedrawRequested(now)) => state.frame = *now,
            Event::Keyboard(iced::keyboard::Event::KeyPressed { .. }) if self.focused => {
                state.reset();
                shell.request_redraw();
            }
            _ => {}
        }
        if self.focused && state.window_focused && !self.reduced_motion {
            let elapsed = state.frame.saturating_duration_since(state.updated);
            let offset = Duration::from_nanos((elapsed.as_nanos() % BLINK.as_nanos()) as u64);
            shell.request_redraw_at(state.frame.max(state.updated) + BLINK - offset);
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        _: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        _: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
        let Some(clip) = bounds.intersection(viewport) else {
            return;
        };
        let width = state.paragraph.min_width();
        let scroll = (width - (bounds.width - 2.0).max(0.0)).max(0.0);
        renderer.with_layer(clip, |renderer| {
            text_widget::draw(
                renderer,
                style,
                Rectangle {
                    x: bounds.x - scroll,
                    ..bounds
                },
                state.paragraph.raw(),
                text_widget::Style {
                    color: Some(self.color),
                },
                &clip,
            );
            let visible = self.reduced_motion
                || state
                    .frame
                    .saturating_duration_since(state.updated)
                    .as_millis()
                    / 500
                    % 2
                    == 0;
            if self.focused && state.window_focused && visible {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x: bounds.x + width - scroll,
                            y: bounds.y + (bounds.height - self.size) / 2.0,
                            width: 1.0,
                            height: self.size,
                        },
                        ..Default::default()
                    },
                    self.color,
                );
            }
        });
    }
}
