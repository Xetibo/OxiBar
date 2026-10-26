//! Surface-local keyboard policy supplied once by the host, including plugin overlays.
use iced::advanced::{
    Clipboard, Layout, Overlay, Shell, Widget, layout, overlay, renderer,
    widget::{
        Id, Operation, Tree,
        operation::{Focusable, Scrollable, scrollable::AbsoluteOffset},
        tree,
    },
};
use iced::{
    Element, Event, Length, Rectangle, Size, Theme, Vector,
    keyboard::{self, Key, Modifiers, key::Named},
    mouse,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    FocusNext,
    FocusPrevious,
    Next,
    Previous,
    Left,
    Right,
    First,
    Last,
    Activate,
    ContextMenu,
    Cancel,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bindings(pub Vec<(Key, Modifiers, Action)>);

impl Bindings {
    fn action(&self, event: &Event) -> Option<Action> {
        let Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) = event else {
            return None;
        };
        let key = match key {
            Key::Character(key) => Key::Character(key.to_lowercase().into()),
            key => key.clone(),
        };
        self.0
            .iter()
            .find(|(k, m, _)| *k == key && m == modifiers)
            .map(|(_, _, a)| *a)
    }
}

pub(crate) fn key_event(key: Named) -> Event {
    Event::Keyboard(keyboard::Event::KeyPressed {
        key: Key::Named(key),
        modified_key: Key::Named(key),
        physical_key: keyboard::key::Physical::Unidentified(
            keyboard::key::NativeCode::Unidentified,
        ),
        location: keyboard::Location::Standard,
        modifiers: Modifiers::empty(),
        text: None,
        repeat: false,
    })
}

const CONTROL_ROLE: &str = "oxiced.navigation.control";

pub(crate) fn control_focus(
    operation: &mut dyn Operation,
    bounds: Rectangle,
    state: &mut dyn Focusable,
) {
    // Standard operation IDs survive independent plugin dylib dependency graphs.
    operation.container(Some(&Id::new(CONTROL_ROLE)), bounds);
    operation.focusable(None, bounds, state);
}

fn translated(event: &Event, action: Option<Action>) -> Event {
    let named = match action {
        Some(Action::Next | Action::FocusNext) => Named::ArrowDown,
        Some(Action::Previous | Action::FocusPrevious) => Named::ArrowUp,
        Some(Action::Left) => Named::ArrowLeft,
        Some(Action::Right) => Named::ArrowRight,
        Some(Action::First) => Named::Home,
        Some(Action::Last) => Named::End,
        Some(Action::Activate) => Named::Enter,
        Some(Action::ContextMenu) => Named::ContextMenu,
        Some(Action::Cancel) => Named::Escape,
        None => {
            // Unbound native control keys must not bypass a disabled or remapped shortcut.
            if let Event::Keyboard(keyboard::Event::KeyPressed {
                key:
                    Key::Named(
                        Named::Enter
                        | Named::Space
                        | Named::ArrowLeft
                        | Named::ArrowRight
                        | Named::ArrowUp
                        | Named::ArrowDown
                        | Named::Home
                        | Named::End
                        | Named::Tab
                        | Named::Escape
                        | Named::ContextMenu,
                    ),
                ..
            }) = event
            {
                let mut event = event.clone();
                if let Event::Keyboard(keyboard::Event::KeyPressed {
                    key,
                    modified_key,
                    text,
                    ..
                }) = &mut event
                {
                    *key = Key::Unidentified;
                    *modified_key = Key::Unidentified;
                    *text = None;
                }
                return event;
            }
            return event.clone();
        }
    };
    let mut translated = key_event(named);
    if let (
        Event::Keyboard(keyboard::Event::KeyPressed { repeat, .. }),
        Event::Keyboard(keyboard::Event::KeyPressed { repeat: new, .. }),
    ) = (event, &mut translated)
    {
        *new = *repeat;
    }
    translated
}

#[derive(Clone, Copy)]
struct Scroll {
    index: usize,
    bounds: Rectangle,
    translation: Vector,
}
struct Entry {
    bounds: Rectangle,
    focused: bool,
    text: bool,
    scrolls: Vec<Scroll>,
}

#[derive(Default)]
pub(crate) struct FocusScan {
    entries: Vec<Entry>,
    control: Option<Rectangle>,
    scrolls: Vec<Scroll>,
    pending: Option<Scroll>,
    count: usize,
}
impl FocusScan {
    pub(crate) fn focused(&self) -> Option<usize> {
        self.entries.iter().position(|e| e.focused)
    }
    fn editing(&self) -> bool {
        self.focused().is_some_and(|i| self.entries[i].text)
    }
    fn target(&self, action: Option<Action>) -> Option<usize> {
        let count = self.entries.len();
        if count == 0 {
            return None;
        }
        match action {
            Some(Action::Next | Action::FocusNext | Action::Right) => {
                Some(self.focused().map_or(0, |i| (i + 1) % count))
            }
            Some(Action::Previous | Action::FocusPrevious | Action::Left) => Some(
                self.focused()
                    .map_or(count - 1, |i| (i + count - 1) % count),
            ),
            Some(Action::First) => Some(0),
            Some(Action::Last) => Some(count - 1),
            _ => None,
        }
    }
    fn pointed(&self, cursor: mouse::Cursor, viewport: &Rectangle) -> Option<usize> {
        self.entries.iter().rposition(|entry| {
            let bounds = entry
                .scrolls
                .iter()
                .fold(entry.bounds, |bounds, scroll| bounds - scroll.translation);
            cursor.is_over(bounds)
                && cursor.is_over(*viewport)
                && entry
                    .scrolls
                    .iter()
                    .all(|scroll| cursor.is_over(scroll.bounds))
        })
    }
    fn scrolling(&self, target: usize) -> Vec<(usize, AbsoluteOffset)> {
        let entry = &self.entries[target];
        let mut bounds = entry.bounds;
        entry
            .scrolls
            .iter()
            .rev()
            .map(|scroll| {
                bounds = bounds - scroll.translation;
                let delta = |start: f32, end: f32, low: f32, high: f32| {
                    if start < low {
                        start - low
                    } else if end > high {
                        (end - high).min(start - low)
                    } else {
                        0.
                    }
                };
                let offset = AbsoluteOffset {
                    x: delta(
                        bounds.x,
                        bounds.x + bounds.width,
                        scroll.bounds.x,
                        scroll.bounds.x + scroll.bounds.width,
                    ),
                    y: delta(
                        bounds.y,
                        bounds.y + bounds.height,
                        scroll.bounds.y,
                        scroll.bounds.y + scroll.bounds.height,
                    ),
                };
                bounds = bounds - Vector::new(offset.x, offset.y);
                (scroll.index, offset)
            })
            .collect()
    }
}

pub(crate) fn reveal_focused(mut operate: impl FnMut(&mut dyn Operation)) {
    let mut scan = FocusScan::default();
    operate(&mut scan);
    if let Some(target) = scan.focused() {
        operate(&mut ScrollToFocus {
            offsets: scan.scrolling(target),
            index: 0,
        });
    }
}
impl Operation for FocusScan {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        let scroll = self.pending.take();
        if let Some(scroll) = scroll {
            self.scrolls.push(scroll);
        }
        operate(self);
        if scroll.is_some() {
            self.scrolls.pop();
        }
    }
    fn container(&mut self, id: Option<&Id>, bounds: Rectangle) {
        if id == Some(&Id::new(CONTROL_ROLE)) {
            self.control = Some(bounds);
        }
    }
    fn focusable(&mut self, _: Option<&Id>, bounds: Rectangle, state: &mut dyn Focusable) {
        self.entries.push(Entry {
            bounds,
            focused: state.is_focused(),
            // Unmarked native focusables include both single- and multiline editors.
            text: self.control.take() != Some(bounds),
            scrolls: self.scrolls.clone(),
        });
    }
    fn scrollable(
        &mut self,
        _: Option<&Id>,
        bounds: Rectangle,
        _: Rectangle,
        translation: Vector,
        _: &mut dyn Scrollable,
    ) {
        self.pending = Some(Scroll {
            index: self.count,
            bounds,
            translation,
        });
        self.count += 1;
    }
}

struct SetFocus {
    target: Option<usize>,
    index: usize,
}
impl Operation for SetFocus {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }
    fn focusable(&mut self, _: Option<&Id>, _: Rectangle, state: &mut dyn Focusable) {
        if self.target == Some(self.index) {
            state.focus();
        } else {
            state.unfocus();
        }
        self.index += 1;
    }
}
struct ScrollToFocus {
    offsets: Vec<(usize, AbsoluteOffset)>,
    index: usize,
}
impl Operation for ScrollToFocus {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }
    fn scrollable(
        &mut self,
        _: Option<&Id>,
        bounds: Rectangle,
        content: Rectangle,
        _: Vector,
        state: &mut dyn Scrollable,
    ) {
        if let Some((_, offset)) = self.offsets.iter().find(|(i, _)| *i == self.index) {
            state.scroll_by(*offset, bounds, content);
        }
        self.index += 1;
    }
}

pub fn navigation<'a, M>(
    content: impl Into<Element<'a, M>>,
    bindings: Bindings,
) -> Navigation<'a, M> {
    Navigation {
        content: content.into(),
        bindings,
        initial_focus: None,
        focus_on_direction: true,
    }
}
pub struct Navigation<'a, M> {
    content: Element<'a, M>,
    bindings: Bindings,
    initial_focus: Option<usize>,
    focus_on_direction: bool,
}
impl<M> Navigation<'_, M> {
    pub fn initial_focus(mut self, index: usize) -> Self {
        self.initial_focus = Some(index);
        self
    }
    pub fn focus_on_direction(mut self, enabled: bool) -> Self {
        self.focus_on_direction = enabled;
        self
    }
}

#[derive(Default)]
struct State {
    initialized: bool,
}
impl<M> Widget<M, Theme, iced::Renderer> for Navigation<'_, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }
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
        let child = self
            .content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits);
        let node = layout::Node::with_children(child.size(), vec![child]);
        let state = tree.state.downcast_mut::<State>();
        if !state.initialized {
            state.initialized = true;
            if let Some(target) = self.initial_focus {
                self.content.as_widget_mut().operate(
                    &mut tree.children[0],
                    Layout::new(&node).children().next().unwrap(),
                    renderer,
                    &mut SetFocus {
                        target: Some(target),
                        index: 0,
                    },
                );
            }
        }
        node
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        op: &mut dyn Operation,
    ) {
        self.content.as_widget_mut().operate(
            &mut tree.children[0],
            layout.children().next().unwrap(),
            renderer,
            op,
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
        let layout = layout.children().next().unwrap();
        let tree = &mut tree.children[0];
        let child = self.content.as_widget_mut();
        let pointer_press = matches!(
            event,
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                | Event::Touch(iced::touch::Event::FingerPressed { .. })
        );
        if pointer_press {
            child.operate(
                tree,
                layout,
                renderer,
                &mut SetFocus {
                    target: None,
                    index: 0,
                },
            );
        }
        let action = self.bindings.action(event);
        // Escape reaches the host even when an editor would consume it. An open overlay gets first refusal.
        if action == Some(Action::Cancel) {
            return;
        }
        let mut scan = FocusScan::default();
        child.operate(tree, layout, renderer, &mut scan);
        if !self.focus_on_direction
            && scan.focused().is_none()
            && matches!(
                action,
                Some(Action::Next | Action::Previous | Action::Left | Action::Right)
            )
        {
            return;
        }
        let traversal = matches!(action, Some(Action::FocusNext | Action::FocusPrevious));
        if scan.editing() && !traversal {
            child.update(
                tree, event, layout, cursor, renderer, clipboard, shell, viewport,
            );
            return;
        }
        let horizontal = matches!(action, Some(Action::Left | Action::Right));
        let target = if horizontal {
            None
        } else {
            scan.target(action)
        };
        if let Some(target) = target {
            child.operate(
                tree,
                layout,
                renderer,
                &mut SetFocus {
                    target: Some(target),
                    index: 0,
                },
            );
            child.operate(
                tree,
                layout,
                renderer,
                &mut ScrollToFocus {
                    offsets: scan.scrolling(target),
                    index: 0,
                },
            );
            shell.capture_event();
            shell.request_redraw();
        } else {
            child.update(
                tree,
                &translated(event, action),
                layout,
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            );
            if pointer_press && shell.is_event_captured() {
                let mut scan = FocusScan::default();
                child.operate(tree, layout, renderer, &mut scan);
                if scan.focused().is_none()
                    && let Some(target) = scan.pointed(cursor, viewport)
                {
                    child.operate(
                        tree,
                        layout,
                        renderer,
                        &mut SetFocus {
                            target: Some(target),
                            index: 0,
                        },
                    );
                }
            }
            if action.is_none() && translated(event, None) != *event {
                // Keep legacy host result shortcuts confined to an active search editor.
                shell.capture_event();
            }
            if horizontal
                && !shell.is_event_captured()
                && let Some(target) = scan.target(action)
            {
                child.operate(
                    tree,
                    layout,
                    renderer,
                    &mut SetFocus {
                        target: Some(target),
                        index: 0,
                    },
                );
                child.operate(
                    tree,
                    layout,
                    renderer,
                    &mut ScrollToFocus {
                        offsets: scan.scrolling(target),
                        index: 0,
                    },
                );
                shell.capture_event();
                shell.request_redraw();
            }
        }
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
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout.children().next().unwrap(),
            cursor,
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
        let bindings = self.bindings.clone();
        self.content
            .as_widget_mut()
            .overlay(
                &mut tree.children[0],
                layout.children().next().unwrap(),
                renderer,
                viewport,
                translation,
            )
            .map(|content| overlay::Element::new(Box::new(NavigationOverlay { content, bindings })))
    }
}
impl<'a, M: 'a> From<Navigation<'a, M>> for Element<'a, M> {
    fn from(nav: Navigation<'a, M>) -> Self {
        Self::new(nav)
    }
}

struct NavigationOverlay<'a, M> {
    content: overlay::Element<'a, M, Theme, iced::Renderer>,
    bindings: Bindings,
}
impl<M> Overlay<M, Theme, iced::Renderer> for NavigationOverlay<'_, M> {
    fn layout(&mut self, renderer: &iced::Renderer, bounds: Size) -> layout::Node {
        self.content.as_overlay_mut().layout(renderer, bounds)
    }
    fn operate(
        &mut self,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .as_overlay_mut()
            .operate(layout, renderer, operation);
    }
    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
    ) {
        let content = self.content.as_overlay_mut();
        let action = self.bindings.action(event);
        let mut scan = FocusScan::default();
        content.operate(layout, renderer, &mut scan);
        let traversal = matches!(action, Some(Action::FocusNext | Action::FocusPrevious));
        if scan.editing() && !traversal && action != Some(Action::Cancel) {
            content.update(event, layout, cursor, renderer, clipboard, shell);
            return;
        }
        if let Some(target) = scan.target(action) {
            content.operate(
                layout,
                renderer,
                &mut SetFocus {
                    target: Some(target),
                    index: 0,
                },
            );
            content.operate(
                layout,
                renderer,
                &mut ScrollToFocus {
                    offsets: scan.scrolling(target),
                    index: 0,
                },
            );
            shell.capture_event();
            shell.request_redraw();
        } else {
            if matches!(action, Some(Action::Activate | Action::ContextMenu))
                && scan.focused().is_none()
                && !scan.entries.is_empty()
            {
                content.operate(
                    layout,
                    renderer,
                    &mut SetFocus {
                        target: Some(0),
                        index: 0,
                    },
                );
            }
            content.update(
                &translated(event, action),
                layout,
                cursor,
                renderer,
                clipboard,
                shell,
            );
        }
    }
    fn draw(
        &self,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        self.content
            .as_overlay()
            .draw(renderer, theme, style, layout, cursor);
    }
    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.content
            .as_overlay()
            .mouse_interaction(layout, cursor, renderer)
    }
    fn overlay<'a>(
        &'a mut self,
        layout: Layout<'a>,
        renderer: &iced::Renderer,
    ) -> Option<overlay::Element<'a, M, Theme, iced::Renderer>> {
        let bindings = self.bindings.clone();
        self.content
            .as_overlay_mut()
            .overlay(layout, renderer)
            .map(|content| overlay::Element::new(Box::new(NavigationOverlay { content, bindings })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::{oxi_control, oxi_picklist, oxi_reveal};
    use iced::advanced::renderer::Headless;
    use iced::widget::{Column, Space, scrollable, text_input};
    use iced::{Font, Pixels, Point};
    use iced_layershell::reexport::core::clipboard;

    #[derive(Debug, Clone, PartialEq)]
    enum Message {
        Action(usize),
        Text(String),
        Toggle(bool),
        Value(f32),
        Commit,
        Selected(usize),
        Closed,
        Edit(iced::widget::text_editor::Action),
    }

    fn bindings() -> Bindings {
        Bindings(vec![
            (
                Key::Named(Named::Tab),
                Modifiers::empty(),
                Action::FocusNext,
            ),
            (
                Key::Named(Named::Tab),
                Modifiers::SHIFT,
                Action::FocusPrevious,
            ),
            (Key::Character("j".into()), Modifiers::empty(), Action::Next),
            (
                Key::Character("k".into()),
                Modifiers::empty(),
                Action::Previous,
            ),
            (Key::Character("h".into()), Modifiers::empty(), Action::Left),
            (
                Key::Character("l".into()),
                Modifiers::empty(),
                Action::Right,
            ),
            (Key::Named(Named::Home), Modifiers::empty(), Action::First),
            (Key::Named(Named::End), Modifiers::empty(), Action::Last),
            (
                Key::Named(Named::Enter),
                Modifiers::empty(),
                Action::Activate,
            ),
            (
                Key::Named(Named::Escape),
                Modifiers::empty(),
                Action::Cancel,
            ),
            (
                Key::Named(Named::ContextMenu),
                Modifiers::empty(),
                Action::ContextMenu,
            ),
        ])
    }

    fn character(value: &str) -> Event {
        let mut event = key_event(Named::Enter);
        if let Event::Keyboard(keyboard::Event::KeyPressed {
            key,
            modified_key,
            text,
            ..
        }) = &mut event
        {
            *key = Key::Character(value.into());
            *modified_key = key.clone();
            *text = Some(value.into());
        }
        event
    }

    fn button(index: usize) -> oxi_control::Button<'static, Message> {
        oxi_control::button(Space::new())
            .width(100)
            .height(30)
            .on_press(Message::Action(index))
    }

    struct Harness {
        widget: Element<'static, Message>,
        tree: Tree,
        node: layout::Node,
        renderer: iced::Renderer,
        viewport: Rectangle,
    }
    impl Harness {
        fn new(content: impl Into<Element<'static, Message>>, bindings: Bindings) -> Self {
            let renderer = iced::futures::executor::block_on(iced::Renderer::new(
                Font::default(),
                Pixels(16.),
                Some("tiny-skia"),
            ))
            .expect("headless CPU renderer");
            let mut widget: Element<'_, Message> = navigation(content, bindings).into();
            let mut tree = Tree::new(&widget);
            let viewport = Rectangle::new(Point::ORIGIN, Size::new(300., 160.));
            let node = widget.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, viewport.size()),
            );
            Self {
                widget,
                tree,
                node,
                renderer,
                viewport,
            }
        }
        fn scan(&mut self) -> FocusScan {
            let mut scan = FocusScan::default();
            self.widget.as_widget_mut().operate(
                &mut self.tree,
                Layout::new(&self.node),
                &self.renderer,
                &mut scan,
            );
            scan
        }
        fn event(&mut self, event: Event) -> (Vec<Message>, bool) {
            self.event_at(event, mouse::Cursor::Unavailable)
        }
        fn event_at(&mut self, event: Event, cursor: mouse::Cursor) -> (Vec<Message>, bool) {
            let mut messages = Vec::new();
            let mut shell = Shell::new(&mut messages);
            self.widget.as_widget_mut().update(
                &mut self.tree,
                &event,
                Layout::new(&self.node),
                cursor,
                &self.renderer,
                &mut clipboard::Null,
                &mut shell,
                &self.viewport,
            );
            let captured = shell.is_event_captured();
            (messages, captured)
        }
        fn overlay_event(&mut self, event: Event) -> (Vec<Message>, bool, FocusScan) {
            let mut overlay = self
                .widget
                .as_widget_mut()
                .overlay(
                    &mut self.tree,
                    Layout::new(&self.node),
                    &self.renderer,
                    &self.viewport,
                    Vector::ZERO,
                )
                .expect("open overlay");
            let node = overlay
                .as_overlay_mut()
                .layout(&self.renderer, self.viewport.size());
            let mut messages = Vec::new();
            let mut shell = Shell::new(&mut messages);
            overlay.as_overlay_mut().update(
                &event,
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &self.renderer,
                &mut clipboard::Null,
                &mut shell,
            );
            let captured = shell.is_event_captured();
            let mut scan = FocusScan::default();
            overlay
                .as_overlay_mut()
                .operate(Layout::new(&node), &self.renderer, &mut scan);
            (messages, captured, scan)
        }
    }

    #[test]
    fn traversal_wraps_skips_disabled_and_stays_in_its_surface() {
        let mut first = Harness::new(
            Column::new()
                .push(button(0))
                .push(oxi_control::button(Space::new()))
                .push(button(1)),
            bindings(),
        );
        let mut other = Harness::new(button(2), bindings());
        for expected in [0, 1, 0] {
            assert!(first.event(key_event(Named::Tab)).1);
            assert_eq!(first.scan().focused(), Some(expected));
            first.event(Event::Window(iced::window::Event::Unfocused));
            first.event(Event::Window(iced::window::Event::Focused));
            assert_eq!(first.scan().focused(), Some(expected));
        }
        assert_eq!(first.event(key_event(Named::Enter)).0, [Message::Action(0)]);
        assert_eq!(other.scan().focused(), None);
        first.event(character("k"));
        assert_eq!(first.event(key_event(Named::Enter)).0, [Message::Action(1)]);
    }

    #[test]
    fn remapping_and_disabling_do_not_leave_native_key_backdoors() {
        let mut keys = bindings();
        keys.0
            .retain(|(_, _, action)| !matches!(action, Action::Next | Action::Activate));
        keys.0
            .push((Key::Character("n".into()), Modifiers::empty(), Action::Next));
        keys.0.push((
            Key::Character("x".into()),
            Modifiers::empty(),
            Action::Activate,
        ));
        let mut harness = Harness::new(Column::new().push(button(0)).push(button(1)), keys);
        harness.event(character("j"));
        assert_eq!(harness.scan().focused(), None);
        harness.event(character("n"));
        assert_eq!(harness.scan().focused(), Some(0));
        assert!(harness.event(key_event(Named::Enter)).0.is_empty());
        assert_eq!(harness.event(character("x")).0, [Message::Action(0)]);
        let mut disabled = Harness::new(button(0), Bindings::default());
        disabled.event(key_event(Named::Tab));
        assert_eq!(disabled.scan().focused(), None);
    }

    #[test]
    fn text_editing_keeps_vim_characters_submit_and_surface_escape() {
        let mut harness = Harness::new(
            Column::new()
                .push(
                    text_input("Search", "")
                        .on_input(Message::Text)
                        .on_submit(Message::Commit),
                )
                .push(button(0)),
            bindings(),
        );
        harness.event(key_event(Named::Tab));
        assert!(harness.scan().editing());
        assert_eq!(harness.event(character("j")).0, [Message::Text("j".into())]);
        assert_eq!(harness.event(key_event(Named::Enter)).0, [Message::Commit]);
        assert!(
            !harness.event(key_event(Named::Escape)).1,
            "host must receive surface close"
        );
        harness.event(key_event(Named::Tab));
        assert_eq!(
            harness.event(key_event(Named::Enter)).0,
            [Message::Action(0)]
        );
    }

    #[test]
    fn multiline_editors_keep_vim_characters_and_tab_leaves_synchronously() {
        let content = Box::leak(Box::new(iced::widget::text_editor::Content::new()));
        let mut harness = Harness::new(
            Column::new()
                .push(iced::widget::text_editor(content).on_action(Message::Edit))
                .push(button(0)),
            bindings(),
        );
        harness.event(key_event(Named::Tab));
        assert!(harness.scan().editing());
        assert!(matches!(
            harness.event(character("j")).0.as_slice(),
            [Message::Edit(_)]
        ));
        assert_eq!(harness.scan().focused(), Some(0));
        harness.event(key_event(Named::Tab));
        assert_eq!(
            harness.event(key_event(Named::Enter)).0,
            [Message::Action(0)]
        );
    }

    #[test]
    fn nested_actions_toggle_slider_and_context_menu_have_distinct_targets() {
        let card = oxi_control::button(button(1))
            .padding(8)
            .on_press(Message::Action(0))
            .on_context_menu(Message::Action(2));
        let mut harness = Harness::new(
            Column::new()
                .push(card)
                .push(oxi_control::toggler(false).on_toggle(Message::Toggle))
                .push(
                    oxi_control::slider(0.0..=10.0, 5.0, Message::Value)
                        .on_release(Message::Commit),
                ),
            bindings(),
        );
        harness.event(key_event(Named::Tab));
        assert_eq!(
            harness.event(key_event(Named::Enter)).0,
            [Message::Action(0)]
        );
        assert_eq!(
            harness.event(key_event(Named::ContextMenu)).0,
            [Message::Action(2)]
        );
        harness.event(character("j"));
        assert_eq!(
            harness.event(key_event(Named::Enter)).0,
            [Message::Action(1)]
        );
        harness.event(character("j"));
        assert_eq!(
            harness.event(key_event(Named::Enter)).0,
            [Message::Toggle(true)]
        );
        harness.event(character("j"));
        assert_eq!(
            harness.event(character("l")).0,
            [Message::Value(6.0), Message::Commit]
        );
        assert_eq!(harness.scan().focused(), Some(3));
        harness.event(key_event(Named::Home));
        let slider = harness.scan().entries[3].bounds;
        assert!(
            harness
                .event_at(
                    key_event(Named::ArrowUp),
                    mouse::Cursor::Available(slider.center())
                )
                .0
                .is_empty(),
            "hovered, unfocused sliders must not consume editor keys"
        );
        let child = harness.scan().entries[1].bounds.center();
        for event in [
            mouse::Event::ButtonPressed(mouse::Button::Left),
            mouse::Event::ButtonReleased(mouse::Button::Left),
        ] {
            harness.event_at(Event::Mouse(event), mouse::Cursor::Available(child));
        }
        assert_eq!(harness.scan().focused(), Some(1));
        assert_eq!(
            harness.event(key_event(Named::Enter)).0,
            [Message::Action(1)],
            "pointer focus must select the nested child, not its card"
        );
    }

    #[test]
    fn focusing_offscreen_controls_scrolls_and_reveal_hides_collapsed_controls() {
        let rows = (0..20).fold(Column::new(), |rows, i| rows.push(button(i)));
        let mut harness = Harness::new(scrollable(rows).height(90), bindings());
        harness.event(key_event(Named::End));
        let scan = harness.scan();
        assert_eq!(scan.focused(), Some(19));
        assert!(scan.entries[19].scrolls[0].translation.y > 0.);
        assert_eq!(scan.scrolling(19)[0].1.y, 0., "focused row must be visible");
        harness.event(key_event(Named::Home));
        assert_eq!(harness.scan().entries[0].scrolls[0].translation.y, 0.);
        let mut collapsed = Harness::new(
            Column::new()
                .push(oxi_reveal::reveal(button(0), 30., 1.))
                .push(button(1)),
            bindings(),
        );
        collapsed.event(key_event(Named::Tab));
        assert_eq!(collapsed.scan().entries.len(), 1);
        assert_eq!(
            collapsed.event(key_event(Named::Enter)).0,
            [Message::Action(1)]
        );
    }

    #[test]
    fn picklist_overlay_inherits_remaps_scrolls_and_consumes_cancel() {
        let picker =
            oxi_picklist::pick_list((0..30).collect::<Vec<_>>(), Some(25), Message::Selected)
                .on_close(Message::Closed);
        let mut keys = bindings();
        keys.0.retain(|(_, _, action)| *action != Action::Next);
        keys.0
            .push((Key::Character("n".into()), Modifiers::empty(), Action::Next));
        let mut harness = Harness::new(picker, keys);
        harness.event(key_event(Named::Tab));
        harness.event(key_event(Named::Enter));
        harness.event(Event::Window(iced::window::Event::Unfocused));
        harness.event(Event::Window(iced::window::Event::Focused));
        let (_, _, scan) = harness.overlay_event(character("j"));
        assert_eq!(
            scan.focused(),
            Some(25),
            "removed Vim binding must not move overlay"
        );
        assert!(scan.entries[25].scrolls[0].translation.y > 0.);
        assert_eq!(scan.scrolling(25)[0].1.y, 0.);
        assert_eq!(harness.overlay_event(character("n")).2.focused(), Some(26));
        assert_eq!(
            harness.overlay_event(key_event(Named::Enter)).0,
            [Message::Selected(26)]
        );
        harness.event(key_event(Named::Enter));
        let (messages, captured, _) = harness.overlay_event(key_event(Named::Escape));
        assert_eq!(messages, [Message::Closed]);
        assert!(captured, "nested cancel must not close parent surface");
        assert!(!harness.event(key_event(Named::Escape)).1);
    }
}
