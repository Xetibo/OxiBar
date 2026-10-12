use iced::Color;
use toml::Table;

use super::{contrast, snapshot::parse_color, theme_impl::ComputedOxiTheme};

pub const SLOTS: [&str; 24] = [
    "base00", "base01", "base02", "base03", "base04", "base05", "base06", "base07", "base08",
    "base09", "base0a", "base0b", "base0c", "base0d", "base0e", "base0f", "base10", "base11",
    "base12", "base13", "base14", "base15", "base16", "base17",
];

#[derive(Debug, Clone, PartialEq)]
pub struct Base24 {
    pub colors: [Color; 24],
}

impl Base24 {
    pub fn inherited(theme: &ComputedOxiTheme) -> Self {
        Self {
            colors: [
                theme.mantle,
                theme.base,
                theme.secondary_bg,
                theme.tertiary_bg,
                theme.text_muted,
                theme.text,
                theme.rose,
                theme.lavender,
                theme.bad,
                theme.flamingo,
                theme.warning,
                theme.good,
                theme.info,
                theme.primary,
                theme.mauve,
                theme.flamingo,
                theme.primary_bg,
                theme.shade,
                theme.bad_hover,
                theme.warning_hover,
                theme.good_hover,
                theme.info_hover,
                theme.primary_hover,
                theme.secondary_hover,
            ],
        }
    }

    pub fn configure(&mut self, table: &Table) -> Result<(), String> {
        let mut seen = [false; 24];
        for (name, value) in table {
            let slot = SLOTS
                .iter()
                .position(|slot| slot.eq_ignore_ascii_case(name))
                .ok_or_else(|| format!("theme.base24.{name}: expected base00 through base17"))?;
            if seen[slot] {
                return Err(format!("theme.base24.{name}: duplicate color slot"));
            }
            seen[slot] = true;
            self.colors[slot] = parse_color(
                value
                    .as_str()
                    .ok_or("theme.base24: expected color strings")?,
            )?;
        }
        if seen[..16].iter().any(|seen| *seen) {
            for (slot, source) in [
                (0x10, 0x01),
                (0x11, 0x00),
                (0x12, 0x08),
                (0x13, 0x0a),
                (0x14, 0x0b),
                (0x15, 0x0c),
                (0x16, 0x0d),
                (0x17, 0x0e),
            ] {
                if !seen[slot] {
                    self.colors[slot] = self.colors[source];
                }
            }
        }
        Ok(())
    }

    pub fn apply(&self, theme: &mut ComputedOxiTheme) {
        let c = &self.colors;
        theme.mantle = c[0x00];
        theme.base = c[0x01];
        theme.secondary_bg = c[0x02];
        theme.tertiary_bg = c[0x03];
        theme.text_muted = c[0x04];
        theme.text = c[0x05];
        theme.rose = c[0x06];
        theme.lavender = c[0x07];
        theme.bad = c[0x08];
        theme.warning = c[0x0a];
        theme.good = c[0x0b];
        theme.info = c[0x0c];
        theme.primary = c[0x0d];
        theme.secondary = c[0x0e];
        theme.flamingo = c[0x0f];
        theme.primary_bg = c[0x10];
        theme.shade = c[0x11];
        theme.tint = c[0x07];
        theme.blue = c[0x16];
        theme.mauve = c[0x17];
        theme.border_color_weak = c[0x03];
        theme.border_color_strong = c[0x0d];
        for (color, contrast) in [
            (theme.primary, &mut theme.primary_contrast),
            (theme.secondary, &mut theme.secondary_contrast),
            (theme.good, &mut theme.good_contrast),
            (theme.bad, &mut theme.bad_contrast),
            (theme.info, &mut theme.info_contrast),
            (theme.warning, &mut theme.warning_contrast),
        ] {
            *contrast = self.contrast(color);
        }
    }

    pub fn apply_bright_states(
        &self,
        theme: &mut ComputedOxiTheme,
        overrides: Option<&Table>,
        accent: bool,
    ) {
        let has = |role| overrides.is_some_and(|table| table.contains_key(role));
        for (role, slot, hover, active) in [
            ("bad", 0x12, &mut theme.bad_hover, &mut theme.bad_active),
            (
                "warning",
                0x13,
                &mut theme.warning_hover,
                &mut theme.warning_active,
            ),
            ("good", 0x14, &mut theme.good_hover, &mut theme.good_active),
            ("info", 0x15, &mut theme.info_hover, &mut theme.info_active),
            (
                "primary",
                0x16,
                &mut theme.primary_hover,
                &mut theme.primary_active,
            ),
            (
                "secondary",
                0x17,
                &mut theme.secondary_hover,
                &mut theme.secondary_active,
            ),
        ] {
            if !(has(role) || role == "primary" && accent) {
                *hover = self.colors[slot];
                *active = self.colors[slot];
            }
        }
    }

    pub fn contrast(&self, color: Color) -> Color {
        let preferred = contrast::best(&[self.colors[0x11], self.colors[0x07]], &[color]);
        contrast::readable(preferred, &[color], preferred)
    }

    pub(crate) fn apply_contrast(
        &self,
        theme: &mut ComputedOxiTheme,
        overrides: Option<&Table>,
        accent: bool,
    ) {
        let has = |role| overrides.is_some_and(|table| table.contains_key(role));
        let backgrounds = [
            theme.mantle,
            theme.mantle_hover,
            theme.mantle_active,
            theme.primary_bg,
            theme.primary_bg_hover,
            theme.primary_bg_active,
            theme.secondary_bg,
            theme.secondary_bg_hover,
            theme.secondary_bg_active,
            theme.tertiary_bg,
            theme.tertiary_bg_hover,
            theme.tertiary_bg_active,
        ];
        if !has("text") {
            theme.text = contrast::readable(theme.text, &backgrounds, self.colors[0x07]);
        }
        if !has("text_muted") {
            theme.text_muted = contrast::readable(theme.text_muted, &backgrounds, theme.text);
        }
        for (role, color, hover, active, text) in [
            (
                "primary_contrast",
                theme.primary,
                theme.primary_hover,
                theme.primary_active,
                &mut theme.primary_contrast,
            ),
            (
                "secondary_contrast",
                theme.secondary,
                theme.secondary_hover,
                theme.secondary_active,
                &mut theme.secondary_contrast,
            ),
            (
                "good_contrast",
                theme.good,
                theme.good_hover,
                theme.good_active,
                &mut theme.good_contrast,
            ),
            (
                "bad_contrast",
                theme.bad,
                theme.bad_hover,
                theme.bad_active,
                &mut theme.bad_contrast,
            ),
            (
                "info_contrast",
                theme.info,
                theme.info_hover,
                theme.info_active,
                &mut theme.info_contrast,
            ),
            (
                "warning_contrast",
                theme.warning,
                theme.warning_hover,
                theme.warning_active,
                &mut theme.warning_contrast,
            ),
        ] {
            if !has(role) || role == "primary_contrast" && accent {
                let backgrounds = [color, hover, active];
                let preferred =
                    contrast::best(&[self.colors[0x11], self.colors[0x07]], &backgrounds);
                *text = contrast::readable(preferred, &backgrounds, preferred);
            }
        }
    }
}
