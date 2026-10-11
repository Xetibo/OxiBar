use std::sync::Arc;

use iced::Task;
use oxibar_plugin_api::{ApplyMode, SettingDescriptor, SettingKind};
use oxiced::theme::snapshot::{self, ThemeSnapshot};
use toml::{Table, Value};

use crate::{app::OxiBar, messages::Message};

pub(crate) struct ThemeRuntime {
    pub snapshot: Arc<ThemeSnapshot>,
    pub requested_font: String,
    pub load_fonts: bool,
    pub install_global: bool,
    pub font_loading: bool,
    pub failures: Vec<String>,
}

impl ThemeRuntime {
    pub fn new(config: &Table, revision: u64, load_fonts: bool) -> Self {
        let mut snapshot = ThemeSnapshot::from_config(config, revision).expect("validated theme");
        let requested_font = snapshot.font_family.clone();
        if load_fonts {
            snapshot.font_family = crate::font::configured_bar_font(config).to_owned();
        }
        Self {
            requested_font,
            snapshot: Arc::new(snapshot),
            load_fonts,
            install_global: load_fonts,
            font_loading: false,
            failures: Vec::new(),
        }
    }
}

pub fn schema() -> Vec<SettingDescriptor> {
    let mut fields = vec![
        SettingDescriptor::new(
            "theme.preset",
            "Spacing preset",
            SettingKind::Choice(vec!["current".into(), "compact".into(), "roomy".into()]),
            Some(Value::String("current".into())),
            ApplyMode::Live,
        ),
        SettingDescriptor::new(
            "theme.font",
            "Live font family",
            SettingKind::Text,
            Some(Value::String(String::new())),
            ApplyMode::Live,
        )
        .description(
            "Empty inherits the startup bar font. Upgraded surfaces update without restarting.",
        ),
        SettingDescriptor::new(
            "theme.density",
            "Spacing scale",
            SettingKind::Number {
                min: 0.75,
                max: 1.5,
            },
            Some(Value::Float(1.0)),
            ApplyMode::Live,
        ),
        SettingDescriptor::new(
            "theme.font_scale",
            "Text scale",
            SettingKind::Number {
                min: 0.75,
                max: 1.5,
            },
            Some(Value::Float(1.0)),
            ApplyMode::Live,
        ),
        SettingDescriptor::new(
            "theme.border_radius",
            "Corner radius",
            SettingKind::Integer { min: 0, max: 128 },
            None,
            ApplyMode::Live,
        ),
        SettingDescriptor::new(
            "theme.reduced_motion",
            "Reduced motion",
            SettingKind::Boolean,
            Some(Value::Boolean(false)),
            ApplyMode::Live,
        ),
        SettingDescriptor::new(
            "theme.animation_ms",
            "Animation duration (ms)",
            SettingKind::Integer { min: 0, max: 1000 },
            Some(Value::Integer(150)),
            ApplyMode::Live,
        ),
    ];
    fields.extend(snapshot::legacy_colors().into_iter().map(|(role, value)| {
        SettingDescriptor::new(
            &format!("theme.palette.{role}"),
            &role.replace('_', " "),
            SettingKind::Color,
            Some(Value::String(value)),
            ApplyMode::Live,
        )
    }));
    fields
}

impl OxiBar {
    pub(crate) fn apply_theme(&mut self, force: bool) -> Task<Message> {
        let mut config = self.config.snapshot.effective.clone();
        if config
            .get("theme")
            .and_then(|value| value.get("font"))
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        {
            let startup_font = self
                .config
                .boot
                .effective
                .get("bar")
                .and_then(|value| value.get("font"))
                .cloned()
                .unwrap_or(Value::String("Adwaita Sans".into()));
            config
                .entry("bar")
                .or_insert_with(|| Value::Table(Table::new()))
                .as_table_mut()
                .expect("bar table")
                .insert("font".into(), startup_font);
        }
        let mut next = match ThemeSnapshot::from_config(&config, self.config.snapshot.revision) {
            Ok(next) => next,
            Err(error) => {
                self.settings.error = Some(error);
                return Task::none();
            }
        };
        let requested = next.font_family.clone();
        let font_changed = force || requested != self.appearance.requested_font;
        if !font_changed || self.appearance.load_fonts {
            next.font_family = self.appearance.snapshot.font_family.clone();
        }
        self.appearance.requested_font = requested.clone();
        self.install_theme_snapshot(Arc::new(next));
        if !font_changed || !self.appearance.load_fonts {
            return Task::none();
        }
        self.appearance.font_loading = true;
        let expected = requested.clone();
        Task::perform(
            crate::background::run(move || crate::font::theme_font_data(&requested)),
            move |result| {
                Message::ThemeFontLoaded(expected.clone(), result.and_then(|(_, result)| result))
            },
        )
    }

    pub(crate) fn install_theme_snapshot(&mut self, next: Arc<ThemeSnapshot>) {
        if self.appearance.install_global
            && let Err(error) = snapshot::install(next.clone())
        {
            self.settings.error = Some(error);
            return;
        }
        self.appearance.snapshot = next.clone();
        self.theme = oxiced::theme::theme_impl::derived_theme(&next.tokens);
        self.appearance.failures.clear();
        for (name, (model, funcs)) in &self.plugins {
            if let Some(changed) = funcs.theme_changed
                && let Err(error) = unsafe { changed(model.clone(), next.clone()) }
            {
                tracing::warn!(plugin = name, %error, "theme snapshot rejected");
                self.settings.error = Some(format!("{name}: {error}"));
                self.appearance.failures.push(name.clone());
            }
        }
    }

    pub(crate) fn theme_font_loaded(
        &mut self,
        requested: String,
        result: Result<crate::font::FontData, String>,
    ) -> Task<Message> {
        if requested != self.appearance.requested_font {
            return Task::none();
        }
        match result {
            Ok(data) => {
                if !self.appearance.load_fonts {
                    return self.theme_font_ready(requested, Ok(data.family));
                }
                iced::font::load(data.bytes).map(move |result| {
                    Message::ThemeFontReady(
                        requested.clone(),
                        result
                            .map(|_| data.family.clone())
                            .map_err(|error| format!("{error:?}")),
                    )
                })
            }
            Err(error) => {
                self.appearance.font_loading = false;
                self.settings.error = Some(error);
                Task::none()
            }
        }
    }

    pub(crate) fn theme_font_ready(
        &mut self,
        requested: String,
        result: Result<String, String>,
    ) -> Task<Message> {
        if requested != self.appearance.requested_font {
            return Task::none();
        }
        self.appearance.font_loading = false;
        match result {
            Ok(family) => {
                let mut next = (*self.appearance.snapshot).clone();
                next.font_family = family;
                self.install_theme_snapshot(Arc::new(next));
            }
            Err(error) => self.settings.error = Some(error),
        }
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persisted_live_font_waits_for_uploaded_bytes_at_boot() {
        let config: Table = "[bar]\nfont='Boot Sans'\n[theme]\nfont='Live Serif'"
            .parse()
            .unwrap();
        let runtime = ThemeRuntime::new(&config, 1, true);
        assert_eq!(runtime.requested_font, "Live Serif");
        assert_eq!(runtime.snapshot.font_family, "Boot Sans");
        assert_eq!(
            ThemeRuntime::new(&config, 1, false).snapshot.font_family,
            "Live Serif"
        );
    }
}
