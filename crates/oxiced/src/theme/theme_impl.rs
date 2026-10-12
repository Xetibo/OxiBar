use iced::{Color, Theme, theme::Palette};
use once_cell::sync::Lazy;
use optional_struct::{Applicable, optional_struct};
use serde::Deserialize;

use crate::utils::{
    color::{mk_color, mk_dark_color, mk_light_color},
    file::get_theme_toml,
};

pub(crate) struct ThemeSeed {
    pub tokens: ComputedOxiTheme,
    pub base24: super::base24::Base24,
    pub accent: Option<Color>,
    pub configured: bool,
    pub roles: toml::Table,
}

pub(crate) static LEGACY_SEED: Lazy<ThemeSeed> = Lazy::new(|| {
    get_theme_toml()
        .ok()
        .and_then(|text| theme_from_toml(&text).ok())
        .unwrap_or_else(|| theme_from_toml("").expect("built-in theme"))
});
pub(crate) static LEGACY_THEME: Lazy<ComputedOxiTheme> = Lazy::new(|| LEGACY_SEED.tokens.clone());

pub(crate) fn theme_from_toml(text: &str) -> Result<ThemeSeed, String> {
    use super::{
        base24::{Base24, SLOTS},
        snapshot::{COLOR_ROLES, color_mut, derive_states, parse_color},
    };
    let values: toml::Table = toml::from_str(text).map_err(|error| error.to_string())?;
    let mut layout = values.clone();
    for role in COLOR_ROLES {
        layout.remove(*role);
    }
    let optional: OptionalOxiTheme = toml::Value::Table(layout)
        .try_into()
        .map_err(|error: toml::de::Error| error.to_string())?;
    let mut tokens = ComputedOxiTheme::from(optional.build(default_theme()));
    let mut scheme = Base24::inherited(&tokens);
    let mut slots = values
        .get("base24")
        .map(|value| value.as_table().cloned().ok_or("base24: expected a table"))
        .transpose()?
        .unwrap_or_default();
    for (name, value) in &values {
        if SLOTS.iter().any(|slot| slot.eq_ignore_ascii_case(name)) {
            if slots.keys().any(|slot| slot.eq_ignore_ascii_case(name)) {
                return Err(format!("duplicate Base24 slot: {name}"));
            }
            slots.insert(name.clone(), value.clone());
        }
    }
    let configured = !slots.is_empty();
    scheme.configure(&slots)?;
    if configured {
        scheme.apply(&mut tokens);
    }
    let mut roles = values
        .get("palette")
        .map(|value| value.as_table().cloned().ok_or("palette: expected a table"))
        .transpose()?
        .unwrap_or_default();
    for role in COLOR_ROLES {
        if let Some(value) = values.get(*role) {
            roles.insert((*role).into(), value.clone());
        }
    }
    for role in COLOR_ROLES {
        if let Some(value) = roles.get(*role) {
            *color_mut(&mut tokens, role).expect("known role") =
                parse_color(value.as_str().ok_or("palette: expected color strings")?)?;
        }
    }
    let accent = values
        .get("accent")
        .map(|value| parse_color(value.as_str().ok_or("accent: expected a color")?))
        .transpose()?;
    if let Some(color) = accent {
        scheme.colors[0x0d] = color;
        tokens.primary = color;
        tokens.blue = color;
        tokens.border_color_strong = color;
        tokens.primary_contrast = scheme.contrast(color);
    }
    derive_states(&mut tokens);
    if configured {
        scheme.apply_bright_states(&mut tokens, Some(&roles), accent.is_some());
    }
    scheme.apply_contrast(&mut tokens, Some(&roles), accent.is_some());
    if !configured {
        scheme = Base24::inherited(&tokens);
    }
    Ok(ThemeSeed {
        tokens,
        base24: scheme,
        accent,
        configured,
        roles,
    })
}

pub static OXITHEME: super::snapshot::ThemeHandle = super::snapshot::ThemeHandle;

pub fn get_derived_iced_theme() -> Theme {
    derived_theme(&OXITHEME)
}

pub fn derived_theme(theme: &ComputedOxiTheme) -> Theme {
    let palette = Palette {
        background: theme.mantle,
        text: theme.text,
        primary: theme.primary,
        success: theme.good,
        danger: theme.bad,
        warning: theme.warning,
    };
    Theme::custom(String::from("OxiTheme"), palette)
}

fn default_theme() -> OxiTheme {
    OxiTheme {
        base: String::from("313244"), // TODO beforepr needed?
        mantle: String::from("1e1e2e"),
        primary_bg: String::from("181825"),
        secondary_bg: String::from("313244"),
        // String::from("45475a"),// TODO beforepr needed?
        tertiary_bg: String::from("45475a"),
        text: String::from("cdd6f4"),
        text_muted: String::from("bac2de"),
        tint: String::from("ffffff"),
        shade: String::from("000000"),
        primary: String::from("89b4fa"),
        secondary: String::from("b4befe"),
        primary_contrast: String::from("1e1e2e"),
        secondary_contrast: String::from("1e1e2e"),
        good: String::from("a6e3a1"),
        good_contrast: String::from("1e1e2e"),
        bad: String::from("f38ba8"),
        bad_contrast: String::from("1e1e2e"),
        info: String::from("94e2d5"),
        info_contrast: String::from("1e1e2e"),
        warning: String::from("f9e2af"),
        warning_contrast: String::from("1e1e2e"),
        rose: String::from("f5e0dc"),
        lavender: String::from("b4befe"),
        blue: String::from("89b4fa"),
        mauve: String::from("cba6f7"),
        flamingo: String::from("f2cdcd"),
        tint_amount: 0.04,
        shade_amount: 0.08,
        border_radius: 10,
        border_color_weak: String::from("45475a"),
        border_color_strong: String::from("89b4fa"),
        padding_xs: 4.0,
        padding_sm: 8.0,
        padding_md: 12.0,
        padding_lg: 16.0,
        padding_xl: 24.0,
        padding_xxl: 32.0,
        font_sm: 10.0,
        font_md: 14.0,
        font_lg: 18.0,
        font_xl: 24.0,
        font_xxl: 32.0,
    }
}

// TODO shadow color
#[optional_struct]
#[derive(Deserialize)]
#[allow(dead_code)]
pub struct OxiTheme {
    /// TODO beforepr
    /// Base16: base00 -> base
    pub base: String,
    /// Background of an application
    /// Base16: base01 -> mantle
    pub mantle: String,
    /// Background for sidebars and cards
    /// Base16: base02 -> surface0
    pub primary_bg: String,
    /// Background for selectors/pickers
    /// Base16: base02 -> surface1
    pub secondary_bg: String,
    /// Background for card in cards
    /// Base16: base03 -> surface2
    pub tertiary_bg: String,
    /// General text
    /// Base16: base04 -> text
    pub text: String,
    /// Muted text
    /// Base16: base03 -> surface2
    pub text_muted: String,

    /// Tint color
    /// highlights something
    pub tint: String,
    /// Shade color
    /// darkens something
    pub shade: String,

    // Base 16 ?? where do we take primary??
    /// Primary theme color
    /// Buttons etc
    pub primary: String,
    /// Secondary theme color
    /// Buttons etc
    pub secondary: String,
    /// Contrast text to primary
    pub primary_contrast: String,
    /// Contrast text to secondary
    pub secondary_contrast: String,

    /// Success/green
    /// Base16: base0B -> green
    pub good: String,
    pub good_contrast: String,
    /// Error/red
    /// Base16: base08 -> red
    pub bad: String,
    pub bad_contrast: String,
    /// Information/teal
    /// Base16: base0C -> teal
    pub info: String,
    pub info_contrast: String,
    /// Warning/yellow
    /// Base16: base0A -> yellow
    pub warning: String,
    pub warning_contrast: String,

    pub rose: String,
    pub lavender: String,
    pub blue: String,
    pub mauve: String,
    pub flamingo: String,

    pub shade_amount: f32,
    pub tint_amount: f32,

    pub border_radius: u32,
    /// Borders for Selectors and similar
    pub border_color_weak: String,
    /// Borders for hard cuts like sidebars
    pub border_color_strong: String,

    pub padding_xs: f32,
    pub padding_sm: f32,
    pub padding_md: f32,
    pub padding_lg: f32,
    pub padding_xl: f32,
    pub padding_xxl: f32,

    pub font_sm: f32,
    pub font_md: f32,
    pub font_lg: f32,
    pub font_xl: f32,
    pub font_xxl: f32,
}

impl From<OxiTheme> for ComputedOxiTheme {
    fn from(value: OxiTheme) -> Self {
        Self {
            base: mk_color(&value.base),
            mantle: mk_color(&value.mantle),
            mantle_hover: mk_light_color(&value.mantle, value.tint_amount),
            mantle_active: mk_light_color(&value.mantle, value.shade_amount),
            primary_bg: mk_color(&value.primary_bg),
            primary_bg_hover: mk_light_color(&value.primary_bg, value.tint_amount),
            primary_bg_active: mk_light_color(&value.primary_bg, value.shade_amount),
            secondary_bg: mk_color(&value.secondary_bg),
            secondary_bg_hover: mk_light_color(&value.secondary_bg, value.tint_amount),
            secondary_bg_active: mk_light_color(&value.secondary_bg, value.shade_amount),
            tertiary_bg: mk_color(&value.tertiary_bg),
            tertiary_bg_hover: mk_light_color(&value.tertiary_bg, value.tint_amount),
            tertiary_bg_active: mk_light_color(&value.tertiary_bg, value.shade_amount),
            text: mk_color(&value.text),
            text_muted: mk_color(&value.text_muted),
            tint: mk_color(&value.tint),
            shade: mk_color(&value.shade),
            primary: mk_color(&value.primary),
            primary_hover: mk_dark_color(&value.primary, value.tint_amount),
            primary_active: mk_dark_color(&value.primary, value.shade_amount),
            secondary: mk_color(&value.secondary),
            secondary_hover: mk_dark_color(&value.secondary, value.tint_amount),
            secondary_active: mk_dark_color(&value.secondary, value.shade_amount),
            primary_contrast: mk_color(&value.primary_contrast),
            secondary_contrast: mk_color(&value.secondary_contrast),
            good: mk_color(&value.good),
            good_hover: mk_dark_color(&value.good, value.tint_amount),
            good_active: mk_dark_color(&value.good, value.tint_amount),
            good_contrast: mk_color(&value.good_contrast),
            bad: mk_color(&value.bad),
            bad_hover: mk_dark_color(&value.bad, value.tint_amount),
            bad_active: mk_dark_color(&value.bad, value.tint_amount),
            bad_contrast: mk_color(&value.bad_contrast),
            info: mk_color(&value.info),
            info_hover: mk_dark_color(&value.info, value.tint_amount),
            info_active: mk_dark_color(&value.info, value.tint_amount),
            info_contrast: mk_color(&value.info_contrast),
            warning: mk_color(&value.warning),
            warning_hover: mk_dark_color(&value.warning, value.tint_amount),
            warning_active: mk_dark_color(&value.warning, value.tint_amount),
            warning_contrast: mk_color(&value.warning_contrast),
            rose: mk_color(&value.rose),
            lavender: mk_color(&value.lavender),
            blue: mk_color(&value.blue),
            mauve: mk_color(&value.mauve),
            flamingo: mk_color(&value.flamingo),
            shade_amount: value.shade_amount,
            tint_amount: value.tint_amount,
            border_radius: value.border_radius,
            border_color_weak: mk_color(&value.border_color_weak),
            border_color_strong: mk_color(&value.border_color_strong),
            padding_xs: value.padding_xs,
            padding_sm: value.padding_sm,
            padding_md: value.padding_md,
            padding_lg: value.padding_lg,
            padding_xl: value.padding_xl,
            padding_xxl: value.padding_xxl,
            font_sm: value.font_sm,
            font_md: value.font_md,
            font_lg: value.font_lg,
            font_xl: value.font_xl,
            font_xxl: value.font_xxl,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub struct ComputedOxiTheme {
    pub base: Color,

    pub mantle: Color,
    pub mantle_hover: Color,
    pub mantle_active: Color,

    pub primary_bg: Color,
    pub primary_bg_hover: Color,
    pub primary_bg_active: Color,

    pub secondary_bg: Color,
    pub secondary_bg_hover: Color,
    pub secondary_bg_active: Color,

    pub tertiary_bg: Color,
    pub tertiary_bg_hover: Color,
    pub tertiary_bg_active: Color,

    pub text: Color,
    pub text_muted: Color,

    pub tint: Color,
    pub shade: Color,

    pub primary: Color,
    pub primary_hover: Color,
    pub primary_active: Color,
    pub secondary: Color,
    pub secondary_hover: Color,
    pub secondary_active: Color,

    pub primary_contrast: Color,
    pub secondary_contrast: Color,

    pub good: Color,
    pub good_hover: Color,
    pub good_active: Color,
    pub good_contrast: Color,

    pub bad: Color,
    pub bad_hover: Color,
    pub bad_active: Color,
    pub bad_contrast: Color,

    pub info: Color,
    pub info_hover: Color,
    pub info_active: Color,
    pub info_contrast: Color,

    pub warning: Color,
    pub warning_hover: Color,
    pub warning_active: Color,
    pub warning_contrast: Color,

    pub rose: Color,
    pub lavender: Color,
    pub blue: Color,
    pub mauve: Color,
    pub flamingo: Color,

    pub shade_amount: f32,
    pub tint_amount: f32,

    pub border_radius: u32,
    pub border_color_weak: Color,
    pub border_color_strong: Color,

    pub padding_xs: f32,
    pub padding_sm: f32,
    pub padding_md: f32,
    pub padding_lg: f32,
    pub padding_xl: f32,
    pub padding_xxl: f32,

    pub font_sm: f32,
    pub font_md: f32,
    pub font_lg: f32,
    pub font_xl: f32,
    pub font_xxl: f32,
}
