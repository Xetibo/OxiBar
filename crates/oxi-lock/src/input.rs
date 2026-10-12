use iced::{Event, Point, keyboard, mouse};
use waycrate_xkbkeycode::{
    keyboard::{Key, KeyLocation, NamedKey},
    xkb_keyboard::{Context, ElementState, KeyEvent},
};
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle, WEnum,
    protocol::{wl_keyboard, wl_pointer, wl_seat},
};

use crate::backend::State;

pub(crate) struct Seat {
    proxy: wl_seat::WlSeat,
    keyboard: Option<Keyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    pointer_output: Option<u32>,
}

struct Keyboard {
    proxy: wl_keyboard::WlKeyboard,
    context: Context,
    output: Option<u32>,
    modifiers: keyboard::Modifiers,
}

impl Seat {
    pub fn new(proxy: wl_seat::WlSeat) -> Self {
        Self {
            proxy,
            keyboard: None,
            pointer: None,
            pointer_output: None,
        }
    }
    pub fn remove_output(&mut self, output: u32) {
        if self.pointer_output == Some(output) {
            self.pointer_output = None;
        }
        if let Some(keyboard) = &mut self.keyboard
            && keyboard.output == Some(output)
        {
            keyboard.output = None;
        }
    }
}

impl Drop for Keyboard {
    fn drop(&mut self) {
        if self.proxy.version() >= 3 {
            self.proxy.release();
        }
    }
}
impl Drop for Seat {
    fn drop(&mut self) {
        if let Some(pointer) = &self.pointer
            && pointer.version() >= 3
        {
            pointer.release();
        }
        if self.proxy.version() >= 5 {
            self.proxy.release();
        }
    }
}

impl Dispatch<wl_seat::WlSeat, u32> for State {
    fn event(
        state: &mut Self,
        _: &wl_seat::WlSeat,
        event: wl_seat::Event,
        id: &u32,
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(capabilities),
        } = event
        else {
            return;
        };
        let Some(seat) = state.seats.get_mut(id) else {
            return;
        };
        if capabilities.contains(wl_seat::Capability::Keyboard) {
            if seat.keyboard.is_none() {
                match Context::new() {
                    Ok(context) => {
                        seat.keyboard = Some(Keyboard {
                            proxy: seat.proxy.get_keyboard(qh, *id),
                            context,
                            output: None,
                            modifiers: keyboard::Modifiers::empty(),
                        })
                    }
                    Err(_) => state.input_error("Cannot initialize keyboard context"),
                }
            }
        } else {
            seat.keyboard = None;
        }
        let Some(seat) = state.seats.get_mut(id) else {
            return;
        };
        if capabilities.contains(wl_seat::Capability::Pointer) {
            if seat.pointer.is_none() {
                seat.pointer = Some(seat.proxy.get_pointer(qh, *id));
            }
        } else if let Some(pointer) = seat.pointer.take() {
            if pointer.version() >= 3 {
                pointer.release();
            }
            if let Some(output) = seat.pointer_output.take() {
                state.input(output, Event::Mouse(mouse::Event::CursorLeft));
            }
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, u32> for State {
    fn event(
        state: &mut Self,
        proxy: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        id: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let focus = match &event {
            wl_keyboard::Event::Enter { surface, .. } => state.surface_id(surface),
            _ => None,
        };
        let Some(keyboard) = state
            .seats
            .get_mut(id)
            .and_then(|seat| seat.keyboard.as_mut())
            .filter(|keyboard| keyboard.proxy == *proxy)
        else {
            return;
        };
        let output = keyboard.output;
        let mut input = None;
        match event {
            wl_keyboard::Event::Keymap {
                format: WEnum::Value(wl_keyboard::KeymapFormat::XkbV1),
                fd,
                size,
            } => {
                if size > 0 && size <= 1024 * 1024 {
                    keyboard.context.set_keymap_from_fd(fd, size as usize);
                } else {
                    state.input_error("Invalid keyboard keymap size");
                }
            }
            wl_keyboard::Event::Enter { .. } => {
                keyboard.output = focus;
            }
            wl_keyboard::Event::Leave { surface, .. } => {
                if state.surface_id(&surface) == output
                    && let Some(keyboard) = state
                        .seats
                        .get_mut(id)
                        .and_then(|seat| seat.keyboard.as_mut())
                {
                    keyboard.output = None;
                    keyboard.modifiers = keyboard::Modifiers::empty();
                }
            }
            wl_keyboard::Event::Key {
                key,
                state: key_state,
                ..
            } if output.is_some() => {
                let pressed = match key_state {
                    WEnum::Value(wl_keyboard::KeyState::Pressed) => ElementState::Pressed,
                    WEnum::Value(wl_keyboard::KeyState::Released) => ElementState::Released,
                    _ => return,
                };
                if let Some(mut context) = keyboard.context.key_context() {
                    let event = context.process_key_event(key + 8, pressed, false);
                    input = Some(convert(event, keyboard.modifiers));
                }
            }
            wl_keyboard::Event::Modifiers {
                mods_depressed,
                mods_latched,
                mods_locked,
                group,
                ..
            } => {
                if let Some(xkb) = keyboard.context.state_mut() {
                    xkb.update_modifiers(mods_depressed, mods_latched, mods_locked, 0, 0, group);
                    let mods: waycrate_xkbkeycode::keyboard::ModifiersState =
                        xkb.modifiers().into();
                    let mut modifiers = keyboard::Modifiers::empty();
                    if mods.shift_key() {
                        modifiers |= keyboard::Modifiers::SHIFT;
                    }
                    if mods.control_key() {
                        modifiers |= keyboard::Modifiers::CTRL;
                    }
                    if mods.alt_key() {
                        modifiers |= keyboard::Modifiers::ALT;
                    }
                    if mods.meta_key() {
                        modifiers |= keyboard::Modifiers::LOGO;
                    }
                    keyboard.modifiers = modifiers;
                    input = Some(keyboard::Event::ModifiersChanged(modifiers));
                }
            }
            _ => {}
        }
        if let (Some(output), Some(event)) = (output, input) {
            state.input(output, Event::Keyboard(event));
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, u32> for State {
    fn event(
        state: &mut Self,
        proxy: &wl_pointer::WlPointer,
        event: wl_pointer::Event,
        id: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let enter = match &event {
            wl_pointer::Event::Enter { surface, .. } => state.surface_id(surface),
            _ => None,
        };
        let Some(seat) = state
            .seats
            .get_mut(id)
            .filter(|seat| seat.pointer.as_ref() == Some(proxy))
        else {
            return;
        };
        let mut output = seat.pointer_output;
        let event = match event {
            wl_pointer::Event::Enter {
                surface_x,
                surface_y,
                ..
            } => {
                seat.pointer_output = enter;
                output = enter;
                Some(mouse::Event::CursorMoved {
                    position: Point::new(surface_x as f32, surface_y as f32),
                })
            }
            wl_pointer::Event::Leave { .. } => {
                seat.pointer_output = None;
                Some(mouse::Event::CursorLeft)
            }
            wl_pointer::Event::Motion {
                surface_x,
                surface_y,
                ..
            } => Some(mouse::Event::CursorMoved {
                position: Point::new(surface_x as f32, surface_y as f32),
            }),
            wl_pointer::Event::Button {
                button,
                state: WEnum::Value(pressed),
                ..
            } => {
                let button = match button {
                    272 => mouse::Button::Left,
                    273 => mouse::Button::Right,
                    274 => mouse::Button::Middle,
                    _ => return,
                };
                Some(if pressed == wl_pointer::ButtonState::Pressed {
                    mouse::Event::ButtonPressed(button)
                } else {
                    mouse::Event::ButtonReleased(button)
                })
            }
            wl_pointer::Event::Axis {
                axis: WEnum::Value(axis),
                value,
                ..
            } => Some(mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Pixels {
                    x: if axis == wl_pointer::Axis::HorizontalScroll {
                        -(value as f32)
                    } else {
                        0.0
                    },
                    y: if axis == wl_pointer::Axis::VerticalScroll {
                        -(value as f32)
                    } else {
                        0.0
                    },
                },
            }),
            _ => None,
        };
        if let (Some(output), Some(event)) = (output, event) {
            state.input(output, Event::Mouse(event));
        }
    }
}

fn key(key: Key) -> keyboard::Key {
    match key {
        Key::Character(value) => keyboard::Key::Character(value.as_str().into()),
        Key::Named(value) => {
            use keyboard::key::Named;
            let named = match value {
                NamedKey::Enter => Named::Enter,
                NamedKey::Escape => Named::Escape,
                NamedKey::Backspace => Named::Backspace,
                NamedKey::Delete => Named::Delete,
                NamedKey::Tab => Named::Tab,
                NamedKey::ArrowLeft => Named::ArrowLeft,
                NamedKey::ArrowRight => Named::ArrowRight,
                NamedKey::ArrowUp => Named::ArrowUp,
                NamedKey::ArrowDown => Named::ArrowDown,
                NamedKey::Home => Named::Home,
                NamedKey::End => Named::End,
                NamedKey::Shift => Named::Shift,
                NamedKey::Control => Named::Control,
                NamedKey::Alt => Named::Alt,
                NamedKey::Meta => Named::Meta,
                NamedKey::CapsLock => Named::CapsLock,
                _ => return keyboard::Key::Unidentified,
            };
            keyboard::Key::Named(named)
        }
        _ => keyboard::Key::Unidentified,
    }
}

fn convert(event: KeyEvent, modifiers: keyboard::Modifiers) -> keyboard::Event {
    let key = key(event.key_without_modifiers);
    let modified_key = self::key(event.logical_key);
    let location = match event.location {
        KeyLocation::Standard => keyboard::Location::Standard,
        KeyLocation::Left => keyboard::Location::Left,
        KeyLocation::Right => keyboard::Location::Right,
        KeyLocation::Numpad => keyboard::Location::Numpad,
    };
    let physical_key =
        keyboard::key::Physical::Unidentified(keyboard::key::NativeCode::Unidentified);
    match event.state {
        ElementState::Pressed => keyboard::Event::KeyPressed {
            key,
            modified_key,
            physical_key,
            location,
            modifiers,
            text: event
                .text_with_all_modifiers
                .map(|text| text.as_str().into()),
            repeat: false,
        },
        ElementState::Released => keyboard::Event::KeyReleased {
            key,
            modified_key,
            physical_key,
            location,
            modifiers,
        },
    }
}
