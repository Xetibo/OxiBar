use iced::Color;

pub fn luminance(color: Color) -> f32 {
    let channel = |value: f32| {
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(color.r) + 0.7152 * channel(color.g) + 0.0722 * channel(color.b)
}

pub fn ratio(foreground: Color, background: Color) -> f32 {
    let foreground = mix(background, foreground, foreground.a);
    let a = luminance(foreground);
    let b = luminance(background);
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

fn mix(from: Color, to: Color, amount: f32) -> Color {
    Color::from_rgba(
        from.r + (to.r - from.r) * amount,
        from.g + (to.g - from.g) * amount,
        from.b + (to.b - from.b) * amount,
        from.a + (to.a - from.a) * amount,
    )
}

fn minimum(color: Color, backgrounds: &[Color]) -> f32 {
    backgrounds
        .iter()
        .map(|background| ratio(color, *background))
        .fold(f32::INFINITY, f32::min)
}

pub(crate) fn best(candidates: &[Color], backgrounds: &[Color]) -> Color {
    candidates
        .iter()
        .copied()
        .max_by(|a, b| minimum(*a, backgrounds).total_cmp(&minimum(*b, backgrounds)))
        .expect("contrast candidates")
}

pub(crate) fn readable(color: Color, backgrounds: &[Color], target: Color) -> Color {
    if minimum(color, backgrounds) >= 4.5 {
        return color;
    }
    let target = if minimum(target, backgrounds) >= 4.5 {
        target
    } else {
        best(&[Color::BLACK, Color::WHITE], backgrounds)
    };
    if minimum(target, backgrounds) < 4.5 {
        return target;
    }
    let (mut low, mut high) = (0.0, 1.0);
    for _ in 0..16 {
        let amount = (low + high) / 2.0;
        if minimum(mix(color, target, amount), backgrounds) >= 4.5 {
            high = amount;
        } else {
            low = amount;
        }
    }
    mix(color, target, high)
}
