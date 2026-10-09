use serde_json::{Value, json};

#[derive(Debug, Clone, PartialEq)]
pub struct OutputMode {
    pub width: u32,
    pub height: u32,
    pub refresh_hz: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Output {
    pub name: String,
    pub description: String,
    pub make: String,
    pub model: String,
    pub serial: String,
    pub enabled: bool,
    pub focused: bool,
    pub physical_size: (u32, u32),
    pub logical_size: (u32, u32),
    pub position: (i32, i32),
    pub scale: f64,
    pub transform: u32,
    pub refresh_hz: f64,
    pub vrr: Option<bool>,
    pub modes: Vec<OutputMode>,
}

impl Output {
    pub fn json(&self) -> Value {
        json!({"name": self.name, "description": self.description, "make": self.make, "model": self.model,
            "serial": self.serial, "enabled": self.enabled, "focused": self.focused,
            "pixel_size": [self.physical_size.0, self.physical_size.1], "logical_size": [self.logical_size.0, self.logical_size.1],
            "position": [self.position.0, self.position.1], "scale": self.scale, "transform": self.transform,
            "refresh_hz": self.refresh_hz, "vrr": self.vrr,
            "modes": self.modes.iter().map(|mode| json!({"width": mode.width, "height": mode.height, "refresh_hz": mode.refresh_hz})).collect::<Vec<_>>()})
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OutputSnapshot {
    pub backend: String,
    pub outputs: Vec<Output>,
}

impl OutputSnapshot {
    pub fn focused(&self) -> Option<&Output> {
        self.outputs
            .iter()
            .find(|output| output.enabled && output.focused)
            .or_else(|| self.outputs.iter().find(|output| output.enabled))
    }
}

/// Mode dimensions are unrotated pixels; layout dimensions are logical units.
pub fn logical_size(size: (u32, u32), scale: f64, transform: u32) -> (u32, u32) {
    let (width, height) = if matches!(transform, 1 | 3 | 5 | 7) {
        (size.1, size.0)
    } else {
        size
    };
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    (
        (width as f64 / scale).round().max(1.0) as u32,
        (height as f64 / scale).round().max(1.0) as u32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fractional_scale_and_all_rotation_variants_use_logical_geometry() {
        assert_eq!(logical_size((3840, 2160), 1.5, 0), (2560, 1440));
        for transform in [1, 3, 5, 7] {
            assert_eq!(logical_size((1920, 1080), 1.5, transform), (720, 1280));
        }
        assert_eq!(logical_size((1920, 1080), f64::NAN, 99), (1920, 1080));
    }
}
