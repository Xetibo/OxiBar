use std::{
    ops::Deref,
    sync::{Arc, LazyLock, RwLock},
};

use iced::{Color, Font};
use toml::Table;

use super::base24::Base24;
use super::theme_impl::{ComputedOxiTheme, LEGACY_SEED, LEGACY_THEME};
use crate::utils::color::{darken_color, lighten_color};

pub const MAX_THEME_VALUES: usize = 128;
pub const COLOR_ROLES: &[&str] = &[
    "base",
    "mantle",
    "primary_bg",
    "secondary_bg",
    "tertiary_bg",
    "text",
    "text_muted",
    "tint",
    "shade",
    "primary",
    "secondary",
    "primary_contrast",
    "secondary_contrast",
    "good",
    "good_contrast",
    "bad",
    "bad_contrast",
    "info",
    "info_contrast",
    "warning",
    "warning_contrast",
    "rose",
    "lavender",
    "blue",
    "mauve",
    "flamingo",
    "border_color_weak",
    "border_color_strong",
];

#[derive(Debug, Clone, PartialEq)]
pub struct ThemeSnapshot {
    pub revision: u64,
    pub tokens: ComputedOxiTheme,
    pub font_family: String,
    pub preset: String,
    pub density: f32,
    pub font_scale: f32,
    pub reduced_motion: bool,
    pub animation_ms: u32,
    pub base24: Base24,
    pub accent: Option<Color>,
}

impl ThemeSnapshot {
    pub fn legacy() -> Self {
        Self {
            revision: 0,
            tokens: LEGACY_THEME.clone(),
            font_family: "Adwaita Sans".into(),
            preset: "current".into(),
            density: 1.0,
            font_scale: 1.0,
            reduced_motion: false,
            animation_ms: 150,
            base24: LEGACY_SEED.base24.clone(),
            accent: LEGACY_SEED.accent,
        }
    }

    pub fn from_config(config: &Table, revision: u64) -> Result<Self, String> {
        let theme = config.get("theme").and_then(|value| value.as_table());
        let string = |key: &str, default: &str| {
            theme
                .and_then(|theme| theme.get(key))
                .and_then(|value| value.as_str())
                .unwrap_or(default)
                .to_owned()
        };
        let number = |key: &str| {
            theme
                .and_then(|theme| theme.get(key))
                .and_then(|value| {
                    value
                        .as_float()
                        .or_else(|| value.as_integer().map(|value| value as f64))
                })
                .unwrap_or(1.0) as f32
        };
        let mut snapshot = Self::legacy();
        snapshot.revision = revision;
        snapshot.preset = string("preset", "current");
        snapshot.density = number("density");
        snapshot.font_scale = number("font_scale");
        if !["current", "compact", "roomy"].contains(&snapshot.preset.as_str())
            || !snapshot.density.is_finite()
            || !(0.75..=1.5).contains(&snapshot.density)
            || !snapshot.font_scale.is_finite()
            || !(0.75..=1.5).contains(&snapshot.font_scale)
        {
            return Err("theme: invalid preset, density or font scale".into());
        }
        snapshot.font_family = string("font", "");
        if snapshot.font_family.is_empty() {
            snapshot.font_family = config
                .get("bar")
                .and_then(|value| value.get("font"))
                .and_then(|value| value.as_str())
                .unwrap_or("Adwaita Sans")
                .into();
        }
        if snapshot.font_family.is_empty()
            || snapshot.font_family.len() > 256
            || snapshot.font_family.contains('\0')
        {
            return Err("theme.font: expected a nonempty family of at most 256 bytes".into());
        }
        snapshot.reduced_motion = theme
            .and_then(|theme| theme.get("reduced_motion"))
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let duration = theme
            .and_then(|theme| theme.get("animation_ms"))
            .and_then(|value| value.as_integer())
            .unwrap_or(150);
        if !(0..=1000).contains(&duration) {
            return Err("theme.animation_ms: expected 0..1000".into());
        }
        snapshot.animation_ms = duration as u32;
        let base24 = theme.and_then(|theme| theme.get("base24"));
        let configured_scheme = if let Some(table) = base24 {
            let table = table.as_table().ok_or("theme.base24: expected a table")?;
            snapshot.base24.configure(table)?;
            !table.is_empty()
        } else {
            false
        };
        if configured_scheme {
            snapshot.base24.apply(&mut snapshot.tokens);
        }
        let mut roles = if configured_scheme {
            Table::new()
        } else {
            LEGACY_SEED.roles.clone()
        };
        if let Some(palette) = theme.and_then(|theme| theme.get("palette")) {
            let palette = palette
                .as_table()
                .ok_or("theme.palette: expected a table")?;
            roles.extend(palette.clone());
            for role in COLOR_ROLES {
                if let Some(value) = palette.get(*role) {
                    let color = parse_color(
                        value
                            .as_str()
                            .ok_or("theme.palette: expected color strings")?,
                    )?;
                    *color_mut(&mut snapshot.tokens, role).expect("known color role") = color;
                }
            }
        }
        if let Some(accent) = theme.and_then(|theme| theme.get("accent")) {
            let accent = parse_color(accent.as_str().ok_or("theme.accent: expected a color")?)?;
            snapshot.accent = Some(accent);
            snapshot.base24.colors[0x0d] = accent;
            snapshot.tokens.primary = accent;
            snapshot.tokens.blue = accent;
            snapshot.tokens.border_color_strong = accent;
            snapshot.tokens.primary_contrast = snapshot.base24.contrast(accent);
        }
        derive_states(&mut snapshot.tokens);
        if configured_scheme || LEGACY_SEED.configured {
            snapshot.base24.apply_bright_states(
                &mut snapshot.tokens,
                Some(&roles),
                snapshot.accent.is_some(),
            );
        }
        snapshot.base24.apply_contrast(
            &mut snapshot.tokens,
            Some(&roles),
            snapshot.accent.is_some(),
        );
        let density = snapshot.density
            * match snapshot.preset.as_str() {
                "compact" => 0.85,
                "roomy" => 1.15,
                _ => 1.0,
            };
        for padding in [
            &mut snapshot.tokens.padding_xs,
            &mut snapshot.tokens.padding_sm,
            &mut snapshot.tokens.padding_md,
            &mut snapshot.tokens.padding_lg,
            &mut snapshot.tokens.padding_xl,
            &mut snapshot.tokens.padding_xxl,
        ] {
            *padding *= density;
        }
        for size in [
            &mut snapshot.tokens.font_sm,
            &mut snapshot.tokens.font_md,
            &mut snapshot.tokens.font_lg,
            &mut snapshot.tokens.font_xl,
            &mut snapshot.tokens.font_xxl,
        ] {
            *size *= snapshot.font_scale;
        }
        if let Some(radius) = theme.and_then(|theme| theme.get("border_radius")) {
            let radius = radius
                .as_integer()
                .filter(|radius| (0..=128).contains(radius))
                .ok_or("theme.border_radius: expected 0..128")?;
            snapshot.tokens.border_radius = radius as u32;
        }
        Ok(snapshot)
    }

    pub fn duration(&self) -> std::time::Duration {
        std::time::Duration::from_millis(if self.reduced_motion {
            0
        } else {
            u64::from(self.animation_ms)
        })
    }

    pub fn palette_color(&self, role: &str) -> Option<Color> {
        color_mut(&mut self.tokens.clone(), role).copied()
    }
}

pub fn parse_color(value: &str) -> Result<Color, String> {
    let value = value.strip_prefix('#').unwrap_or(value);
    if ![6, 8].contains(&value.len()) || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("color: expected RRGGBB or RRGGBBAA, optionally prefixed with #".into());
    }
    let channel = |offset| {
        u8::from_str_radix(&value[offset..offset + 2], 16).expect("validated hex") as f32 / 255.0
    };
    Ok(Color {
        r: channel(0),
        g: channel(2),
        b: channel(4),
        a: if value.len() == 8 { channel(6) } else { 1.0 },
    })
}

pub fn color_hex(color: Color) -> String {
    let byte = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    let rgb = format!(
        "{:02x}{:02x}{:02x}",
        byte(color.r),
        byte(color.g),
        byte(color.b)
    );
    if color.a == 1.0 {
        rgb
    } else {
        format!("{rgb}{:02x}", byte(color.a))
    }
}

pub fn legacy_colors() -> Vec<(String, String)> {
    let mut tokens = LEGACY_THEME.clone();
    COLOR_ROLES
        .iter()
        .map(|role| {
            (
                role.to_string(),
                color_hex(*color_mut(&mut tokens, role).expect("known color role")),
            )
        })
        .collect()
}

pub(crate) fn color_mut<'a>(tokens: &'a mut ComputedOxiTheme, role: &str) -> Option<&'a mut Color> {
    Some(match role {
        "base" => &mut tokens.base,
        "mantle" => &mut tokens.mantle,
        "primary_bg" => &mut tokens.primary_bg,
        "secondary_bg" => &mut tokens.secondary_bg,
        "tertiary_bg" => &mut tokens.tertiary_bg,
        "text" => &mut tokens.text,
        "text_muted" => &mut tokens.text_muted,
        "tint" => &mut tokens.tint,
        "shade" => &mut tokens.shade,
        "primary" => &mut tokens.primary,
        "secondary" => &mut tokens.secondary,
        "primary_contrast" => &mut tokens.primary_contrast,
        "secondary_contrast" => &mut tokens.secondary_contrast,
        "good" => &mut tokens.good,
        "good_contrast" => &mut tokens.good_contrast,
        "bad" => &mut tokens.bad,
        "bad_contrast" => &mut tokens.bad_contrast,
        "info" => &mut tokens.info,
        "info_contrast" => &mut tokens.info_contrast,
        "warning" => &mut tokens.warning,
        "warning_contrast" => &mut tokens.warning_contrast,
        "rose" => &mut tokens.rose,
        "lavender" => &mut tokens.lavender,
        "blue" => &mut tokens.blue,
        "mauve" => &mut tokens.mauve,
        "flamingo" => &mut tokens.flamingo,
        "border_color_weak" => &mut tokens.border_color_weak,
        "border_color_strong" => &mut tokens.border_color_strong,
        _ => return None,
    })
}

pub(crate) fn derive_states(tokens: &mut ComputedOxiTheme) {
    for (base, hover, active) in [
        (
            tokens.mantle,
            &mut tokens.mantle_hover,
            &mut tokens.mantle_active,
        ),
        (
            tokens.primary_bg,
            &mut tokens.primary_bg_hover,
            &mut tokens.primary_bg_active,
        ),
        (
            tokens.secondary_bg,
            &mut tokens.secondary_bg_hover,
            &mut tokens.secondary_bg_active,
        ),
        (
            tokens.tertiary_bg,
            &mut tokens.tertiary_bg_hover,
            &mut tokens.tertiary_bg_active,
        ),
    ] {
        let adjust = if super::contrast::luminance(base) > super::contrast::luminance(tokens.text) {
            darken_color
        } else {
            lighten_color
        };
        *hover = adjust(&base, tokens.tint_amount);
        *active = adjust(&base, tokens.shade_amount);
    }
    for (base, hover, active, active_amount) in [
        (
            tokens.primary,
            &mut tokens.primary_hover,
            &mut tokens.primary_active,
            tokens.shade_amount,
        ),
        (
            tokens.secondary,
            &mut tokens.secondary_hover,
            &mut tokens.secondary_active,
            tokens.shade_amount,
        ),
        (
            tokens.good,
            &mut tokens.good_hover,
            &mut tokens.good_active,
            tokens.tint_amount,
        ),
        (
            tokens.bad,
            &mut tokens.bad_hover,
            &mut tokens.bad_active,
            tokens.tint_amount,
        ),
        (
            tokens.info,
            &mut tokens.info_hover,
            &mut tokens.info_active,
            tokens.tint_amount,
        ),
        (
            tokens.warning,
            &mut tokens.warning_hover,
            &mut tokens.warning_active,
            tokens.tint_amount,
        ),
    ] {
        *hover = darken_color(&base, tokens.tint_amount);
        *active = darken_color(&base, active_amount);
    }
}

struct Interned {
    palette: &'static ComputedOxiTheme,
    font: &'static str,
}

struct Registry {
    current: Arc<ThemeSnapshot>,
    values: Vec<Interned>,
    active: usize,
}

impl Registry {
    fn new() -> Self {
        let snapshot = Arc::new(ThemeSnapshot::legacy());
        Self {
            values: vec![Interned {
                palette: Box::leak(Box::new(snapshot.tokens.clone())),
                font: Box::leak(snapshot.font_family.clone().into_boxed_str()),
            }],
            current: snapshot,
            active: 0,
        }
    }

    fn index(&self, snapshot: &ThemeSnapshot) -> Option<usize> {
        self.values.iter().position(|value| {
            *value.palette == snapshot.tokens && value.font == snapshot.font_family
        })
    }

    fn check(&self, snapshot: &ThemeSnapshot) -> Result<(), String> {
        if self.index(snapshot).is_none() && self.values.len() >= MAX_THEME_VALUES {
            Err(format!(
                "theme: {MAX_THEME_VALUES} distinct palette/font values reached; restart before adding another"
            ))
        } else {
            Ok(())
        }
    }

    fn install(&mut self, snapshot: Arc<ThemeSnapshot>) -> Result<(), String> {
        self.check(&snapshot)?;
        self.active = match self.index(&snapshot) {
            Some(index) => index,
            None => {
                // Existing field-access callers can retain references. Interned values
                // never mutate or expire; the explicit budget bounds their lifetime cost.
                self.values.push(Interned {
                    palette: Box::leak(Box::new(snapshot.tokens.clone())),
                    font: Box::leak(snapshot.font_family.clone().into_boxed_str()),
                });
                self.values.len() - 1
            }
        };
        self.current = snapshot;
        Ok(())
    }
}

static REGISTRY: LazyLock<RwLock<Registry>> = LazyLock::new(|| RwLock::new(Registry::new()));

pub struct ThemeHandle;
impl Deref for ThemeHandle {
    type Target = ComputedOxiTheme;
    fn deref(&self) -> &Self::Target {
        let registry = REGISTRY.read().expect("theme registry");
        registry.values[registry.active].palette
    }
}

pub fn current_snapshot() -> Arc<ThemeSnapshot> {
    REGISTRY.read().expect("theme registry").current.clone()
}
pub fn current_font() -> Font {
    let registry = REGISTRY.read().expect("theme registry");
    Font::with_name(registry.values[registry.active].font)
}
pub fn current_font_name() -> &'static str {
    let registry = REGISTRY.read().expect("theme registry");
    registry.values[registry.active].font
}
pub fn check_capacity(snapshot: &ThemeSnapshot) -> Result<(), String> {
    REGISTRY.read().expect("theme registry").check(snapshot)
}
pub fn install(snapshot: Arc<ThemeSnapshot>) -> Result<(), String> {
    REGISTRY.write().expect("theme registry").install(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn immutable_snapshots_preserve_seed_and_derive_roles_density_and_motion() {
        let original = ThemeSnapshot::legacy();
        let config: Table = toml::from_str("[theme]\npreset='roomy'\ndensity=1.2\nfont_scale=1.1\nfont='Monospace'\nreduced_motion=true\n[theme.palette]\nprimary='#12345680'").unwrap();
        let next = ThemeSnapshot::from_config(&config, 9).unwrap();
        assert_eq!(original.tokens, *LEGACY_THEME);
        assert_eq!(next.tokens.primary, parse_color("12345680").unwrap());
        assert_eq!(next.tokens.primary_hover.a, next.tokens.primary.a);
        assert!((next.tokens.padding_lg - original.tokens.padding_lg * 1.38).abs() < 0.001);
        assert_eq!(next.duration(), std::time::Duration::ZERO);
        assert!(parse_color("#bad").is_err());
    }
    #[test]
    fn interned_references_stay_valid_and_repeated_install_is_bounded() {
        let mut registry = Registry::new();
        let original = registry.values[0].palette;
        let mut changed = (*registry.current).clone();
        changed.tokens.primary = Color::BLACK;
        for revision in 1..1000 {
            changed.revision = revision;
            registry.install(Arc::new(changed.clone())).unwrap();
        }
        assert_eq!(registry.values.len(), 2);
        assert_eq!(*original, *LEGACY_THEME);
        for value in 2..MAX_THEME_VALUES {
            changed.font_family = format!("Fixture {value}");
            registry.install(Arc::new(changed.clone())).unwrap();
        }
        changed.font_family = "one too many".into();
        assert!(registry.install(Arc::new(changed)).is_err());
    }
}
