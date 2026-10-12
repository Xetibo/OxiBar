use super::{configuration::DisplayConfig, output::logical_size};

const SNAP_DISTANCE: i64 = 150;

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Placement {
    pub position: (i32, i32),
    pub horizontal: bool,
    pub vertical: bool,
    pub overlapping: bool,
}

#[derive(Clone, Copy)]
struct Bounds {
    left: i64,
    top: i64,
    width: i64,
    height: i64,
}

impl Bounds {
    fn new(output: &DisplayConfig, position: (i32, i32)) -> Self {
        let size = logical_size(
            (output.mode.width, output.mode.height),
            output.scale,
            output.transform,
        );
        Self {
            left: position.0.into(),
            top: position.1.into(),
            width: size.0.into(),
            height: size.1.into(),
        }
    }

    fn right(self) -> i64 {
        self.left + self.width
    }
    fn bottom(self) -> i64 {
        self.top + self.height
    }
    fn overlaps(self, other: Self) -> bool {
        self.left < other.right()
            && other.left < self.right()
            && self.top < other.bottom()
            && other.top < self.bottom()
    }
}

pub(crate) fn snap(outputs: &[DisplayConfig], name: &str, proposed: (i32, i32)) -> Placement {
    let plain = Placement {
        position: proposed,
        ..Default::default()
    };
    let Some(output) = outputs
        .iter()
        .find(|output| output.name == name && output.enabled)
    else {
        return plain;
    };
    let moving = Bounds::new(output, proposed);
    let peers = outputs
        .iter()
        .filter(|output| output.enabled && output.name != name)
        .map(|output| Bounds::new(output, output.position))
        .collect::<Vec<_>>();
    if peers.iter().any(|peer| moving.overlaps(*peer)) {
        return Placement {
            overlapping: true,
            ..plain
        };
    }
    let mut horizontal = vec![(proposed.0, false)];
    let mut vertical = vec![(proposed.1, false)];
    let candidates = |values: &mut Vec<(i32, bool)>, base: i64, edges: [i64; 4]| {
        for edge in edges {
            if (edge - base).abs() < SNAP_DISTANCE && (-32768..=32768).contains(&edge) {
                values.push((edge as i32, true));
            }
        }
    };
    for peer in &peers {
        if moving.top <= peer.bottom() + SNAP_DISTANCE
            && peer.top <= moving.bottom() + SNAP_DISTANCE
        {
            candidates(
                &mut horizontal,
                moving.left,
                [
                    peer.left - moving.width,
                    peer.right(),
                    peer.left,
                    peer.right() - moving.width,
                ],
            );
        }
        if moving.left <= peer.right() + SNAP_DISTANCE
            && peer.left <= moving.right() + SNAP_DISTANCE
        {
            candidates(
                &mut vertical,
                moving.top,
                [
                    peer.top - moving.height,
                    peer.bottom(),
                    peer.top,
                    peer.bottom() - moving.height,
                ],
            );
        }
    }
    horizontal.sort_unstable();
    horizontal.dedup();
    vertical.sort_unstable();
    vertical.dedup();
    horizontal
        .into_iter()
        .flat_map(|(x, horizontal)| {
            vertical.iter().map(move |&(y, vertical)| Placement {
                position: (x, y),
                horizontal,
                vertical,
                overlapping: false,
            })
        })
        .filter(|placement| {
            let bounds = Bounds::new(output, placement.position);
            !peers.iter().any(|peer| bounds.overlaps(*peer))
        })
        .min_by_key(|placement| {
            let dx = (i64::from(placement.position.0) - moving.left).abs();
            let dy = (i64::from(placement.position.1) - moving.top).abs();
            (
                2 - u8::from(placement.horizontal) - u8::from(placement.vertical),
                dx + dy,
                dx,
                dy,
                placement.position,
            )
        })
        .unwrap_or(plain)
}

pub(crate) fn dropped(
    outputs: &[DisplayConfig],
    name: &str,
    proposed: (i32, i32),
    original: (i32, i32),
) -> (i32, i32) {
    let placement = snap(outputs, name, proposed);
    if placement.overlapping {
        original
    } else {
        placement.position
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitor::OutputMode;

    fn display(name: &str, position: (i32, i32)) -> DisplayConfig {
        DisplayConfig {
            name: name.into(),
            enabled: true,
            mode: OutputMode {
                width: 500,
                height: 500,
                refresh_hz: 60.0,
            },
            scale: 1.0,
            position,
            transform: 0,
            vrr: None,
        }
    }

    #[test]
    fn reset_style_adjacent_edges_alignment_and_overlapping_drop() {
        let original = (1000, 1000);
        let outputs = [display("fixed", (0, 0)), display("moving", original)];
        for (proposed, expected) in [
            ((600, 0), (500, 0)),
            ((-600, 0), (-500, 0)),
            ((200, 550), (200, 500)),
            ((200, -550), (200, -500)),
            ((500, -50), (500, 0)),
            ((50, -500), (0, -500)),
            ((400, 0), original),
            ((900, 900), (900, 900)),
        ] {
            assert_eq!(
                dropped(&outputs, "moving", proposed, original),
                expected,
                "{proposed:?}"
            );
        }
        assert!(snap(&outputs, "moving", (600, 0)).horizontal);
        assert!(snap(&outputs, "moving", (400, 0)).overlapping);
    }

    #[test]
    fn snapping_uses_rotated_fractional_logical_sizes_and_ignores_disabled_outputs() {
        let mut fixed = display("fixed", (-800, -200));
        fixed.mode.width = 1200;
        fixed.mode.height = 900;
        fixed.scale = 1.5;
        fixed.transform = 1;
        let moving = display("moving", (1200, 1200));
        let mut disabled = display("disabled", (-100, -200));
        disabled.enabled = false;
        let outputs = [fixed, moving, disabled];
        assert_eq!(
            snap(&outputs, "moving", (-100, -160)).position,
            (-200, -200)
        );
        assert_eq!(
            snap(&outputs, "moving", (32768, 32768)).position,
            (32768, 32768)
        );
    }

    #[test]
    fn candidates_are_deterministic_and_do_not_snap_into_another_display() {
        let outputs = [
            display("left", (0, 0)),
            display("right", (1200, 0)),
            display("moving", (2000, 1000)),
        ];
        let proposed = (570, 10);
        let expected = snap(&outputs, "moving", proposed).position;
        let mut reversed = outputs.clone();
        reversed.reverse();
        assert_eq!(snap(&reversed, "moving", proposed).position, expected);
        assert_eq!(expected, (500, 0));
        assert_eq!(snap(&outputs, "moving", (50, 5000)).position, (50, 5000));
    }
}
