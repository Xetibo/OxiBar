use super::{configuration::DisplayConfig, output::logical_size, runtime::Action, snapping};
use crate::Message;
use iced::{Point, Rectangle, Renderer, Size, Theme, mouse, widget::canvas};
use oxiced::theme::{snapshot::current_font, theme_impl::OXITHEME};

pub(crate) struct Diagram<'a> {
    pub outputs: &'a [DisplayConfig],
    pub enabled: bool,
}

#[derive(Clone, Copy)]
struct Mapping {
    origin: Point,
    scale: f32,
    offset: Point,
}

fn dimensions(output: &DisplayConfig) -> Size {
    let (width, height) = logical_size(
        (output.mode.width, output.mode.height),
        output.scale,
        output.transform,
    );
    Size::new(width as f32, height as f32)
}

impl Mapping {
    fn fit(outputs: &[DisplayConfig], size: Size) -> Self {
        let mut left = 0.0_f32;
        let mut top = 0.0_f32;
        let mut right = 1.0_f32;
        let mut bottom = 1.0_f32;
        for (index, output) in outputs.iter().enumerate() {
            let x = output.position.0 as f32;
            let y = output.position.1 as f32;
            let logical = dimensions(output);
            if index == 0 {
                left = x;
                top = y;
                right = x + logical.width;
                bottom = y + logical.height;
            } else {
                left = left.min(x);
                top = top.min(y);
                right = right.max(x + logical.width);
                bottom = bottom.max(y + logical.height);
            }
        }
        let padding = OXITHEME.padding_lg;
        let scale = ((size.width - padding * 2.0).max(1.0) / (right - left).max(1.0))
            .min((size.height - padding * 2.0).max(1.0) / (bottom - top).max(1.0));
        Self {
            origin: Point::new(left, top),
            scale,
            offset: Point::new(
                (size.width - (right - left) * scale) / 2.0,
                (size.height - (bottom - top) * scale) / 2.0,
            ),
        }
    }

    fn rectangle(self, output: &DisplayConfig) -> Rectangle {
        let logical = dimensions(output);
        Rectangle {
            x: self.offset.x + (output.position.0 as f32 - self.origin.x) * self.scale,
            y: self.offset.y + (output.position.1 as f32 - self.origin.y) * self.scale,
            width: logical.width * self.scale,
            height: logical.height * self.scale,
        }
    }
}

struct Gesture {
    name: String,
    mapping: Mapping,
    cursor: Point,
    position: (i32, i32),
    proposed: (i32, i32),
    placement: snapping::Placement,
}

#[derive(Default)]
pub(crate) struct State {
    gesture: Option<Gesture>,
}

fn moved(gesture: &Gesture, cursor: Point) -> (i32, i32) {
    let coordinate = |base: i32, delta: f32| {
        (base as f32 + delta / gesture.mapping.scale)
            .round()
            .clamp(-32768.0, 32768.0) as i32
    };
    (
        coordinate(gesture.position.0, cursor.x - gesture.cursor.x),
        coordinate(gesture.position.1, cursor.y - gesture.cursor.y),
    )
}

pub(crate) fn rectangles(outputs: &[DisplayConfig], bounds: Rectangle) -> Vec<(String, Rectangle)> {
    let mapping = Mapping::fit(outputs, bounds.size());
    outputs
        .iter()
        .map(|output| {
            let mut rectangle = mapping.rectangle(output);
            rectangle.x += bounds.x;
            rectangle.y += bounds.y;
            (output.name.clone(), rectangle)
        })
        .collect()
}

impl canvas::Program<Message> for Diagram<'_> {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        if !self.enabled {
            state.gesture = None;
            return None;
        }
        match event {
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let cursor = cursor.position_in(bounds)?;
                let mapping = Mapping::fit(self.outputs, bounds.size());
                let output = self
                    .outputs
                    .iter()
                    .rev()
                    .find(|output| mapping.rectangle(output).contains(cursor))?;
                state.gesture = Some(Gesture {
                    name: output.name.clone(),
                    mapping,
                    cursor,
                    position: output.position,
                    proposed: output.position,
                    placement: snapping::Placement {
                        position: output.position,
                        ..Default::default()
                    },
                });
                Some(canvas::Action::capture())
            }
            iced::Event::Mouse(mouse::Event::CursorMoved { position }) => {
                let gesture = state.gesture.as_mut()?;
                let cursor = Point::new(position.x - bounds.x, position.y - bounds.y);
                gesture.proposed = moved(gesture, cursor);
                gesture.placement = snapping::snap(self.outputs, &gesture.name, gesture.proposed);
                Some(
                    canvas::Action::publish(Message::Monitor(Action::Position(
                        gesture.name.clone(),
                        gesture.placement.position,
                    )))
                    .and_capture(),
                )
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                let gesture = state.gesture.take()?;
                let position = snapping::dropped(
                    self.outputs,
                    &gesture.name,
                    gesture.proposed,
                    gesture.position,
                );
                Some(
                    canvas::Action::publish(Message::Monitor(Action::Position(
                        gesture.name,
                        position,
                    )))
                    .and_capture(),
                )
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        state: &State,
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mapping = state
            .gesture
            .as_ref()
            .map(|gesture| gesture.mapping)
            .unwrap_or_else(|| Mapping::fit(self.outputs, bounds.size()));
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        for output in self.outputs {
            let rectangle = mapping.rectangle(output);
            let selected = state
                .gesture
                .as_ref()
                .is_some_and(|gesture| gesture.name == output.name);
            let shape = canvas::Path::rectangle(rectangle.position(), rectangle.size());
            frame.fill(
                &shape,
                if output.enabled {
                    OXITHEME.primary_bg
                } else {
                    OXITHEME.secondary_bg
                },
            );
            frame.stroke(
                &shape,
                canvas::Stroke::default()
                    .with_color(if selected {
                        OXITHEME.primary
                    } else {
                        OXITHEME.border_color_weak
                    })
                    .with_width(if selected { 3.0 } else { 1.0 }),
            );
            frame.fill_text(canvas::Text {
                content: output.name.clone(),
                position: Point::new(
                    rectangle.x + OXITHEME.padding_sm,
                    rectangle.y + OXITHEME.padding_sm,
                ),
                color: if output.enabled {
                    OXITHEME.text
                } else {
                    OXITHEME.text_muted
                },
                font: current_font(),
                size: OXITHEME.font_md.into(),
                ..Default::default()
            });
        }
        if let Some(gesture) = &state.gesture {
            if let Some(output) = self
                .outputs
                .iter()
                .find(|output| output.name == gesture.name)
            {
                let rectangle = mapping.rectangle(output);
                let stroke = canvas::Stroke::default()
                    .with_color(OXITHEME.primary)
                    .with_width(1.5);
                if gesture.placement.horizontal {
                    for x in [rectangle.x, rectangle.x + rectangle.width] {
                        frame.stroke(
                            &canvas::Path::line(
                                Point::new(x, OXITHEME.padding_sm),
                                Point::new(x, bounds.height - OXITHEME.padding_sm),
                            ),
                            stroke,
                        );
                    }
                }
                if gesture.placement.vertical {
                    for y in [rectangle.y, rectangle.y + rectangle.height] {
                        frame.stroke(
                            &canvas::Path::line(
                                Point::new(OXITHEME.padding_sm, y),
                                Point::new(bounds.width - OXITHEME.padding_sm, y),
                            ),
                            stroke,
                        );
                    }
                }
            }
            if gesture.placement.overlapping {
                frame.fill_text(canvas::Text {
                    content: "Overlapping displays — release to restore".into(),
                    position: Point::new(
                        OXITHEME.padding_sm,
                        bounds.height - OXITHEME.font_md * 1.5,
                    ),
                    color: OXITHEME.primary,
                    font: current_font(),
                    size: OXITHEME.font_md.into(),
                    ..Default::default()
                });
            }
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if !self.enabled {
            return mouse::Interaction::default();
        }
        if state.gesture.is_some() {
            return mouse::Interaction::Grabbing;
        }
        if cursor.position_in(bounds).is_some_and(|position| {
            let mapping = Mapping::fit(self.outputs, bounds.size());
            self.outputs
                .iter()
                .any(|output| mapping.rectangle(output).contains(position))
        }) {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drag_uses_captured_mapping_and_preserves_negative_logical_coordinates() {
        let gesture = Gesture {
            name: "DP-1".into(),
            mapping: Mapping {
                origin: Point::ORIGIN,
                scale: 0.1,
                offset: Point::ORIGIN,
            },
            cursor: Point::new(20.0, 30.0),
            position: (-1920, 40),
            proposed: (-1920, 40),
            placement: snapping::Placement::default(),
        };
        assert_eq!(moved(&gesture, Point::new(40.0, 20.0)), (-1720, -60));
        assert_eq!(moved(&gesture, Point::new(1e9, -1e9)), (32768, -32768));
    }
}
