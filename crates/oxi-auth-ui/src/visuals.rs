use std::{
    fs,
    io::{Cursor, Read},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::Arc,
};

use chrono::{
    Local,
    format::{Item, StrftimeItems},
};
use iced::{
    Element, Length, keyboard,
    widget::{Column, Row, Space, container, image, stack},
};
use oxiced::{
    theme::{
        snapshot::{self, ThemeSnapshot},
        theme_impl::OXITHEME,
    },
    widgets::oxi_plugin,
};
use serde::Deserialize;

use crate::view::Purpose;

const MAX_CONFIG: u64 = 65536;
const MAX_IMAGE: u64 = 16 * 1024 * 1024;

#[derive(Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Appearance {
    pub clock: bool,
    pub clock_format: String,
    pub date_format: String,
    pub wallpaper: Option<PathBuf>,
    pub avatar: Option<PathBuf>,
    pub overlay: f32,
}
impl Default for Appearance {
    fn default() -> Self {
        Self {
            clock: true,
            clock_format: "%H:%M".into(),
            date_format: "%A, %e %B".into(),
            wallpaper: None,
            avatar: None,
            overlay: 0.65,
        }
    }
}

pub struct Visuals {
    appearance: Appearance,
    wallpaper: Option<image::Handle>,
    avatar: Option<image::Handle>,
    clock: String,
    date: String,
    pub notice: String,
}
impl Default for Visuals {
    fn default() -> Self {
        let mut value = Self {
            appearance: Appearance::default(),
            wallpaper: None,
            avatar: None,
            clock: String::new(),
            date: String::new(),
            notice: String::new(),
        };
        value.tick();
        value
    }
}
impl Visuals {
    pub fn load(purpose: Purpose) -> Self {
        let path = match purpose {
            Purpose::Login => PathBuf::from("/etc/oxibar/auth.toml"),
            Purpose::Lock => std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .or_else(|| {
                    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config"))
                })
                .unwrap_or_default()
                .join("oxibar/auth.toml"),
        };
        let mut visuals = Self::default();
        if !path.exists() {
            return visuals;
        }
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            if matches!(purpose, Purpose::Login) {
                administrator_owned(&path)?;
            }
            let file = fs::File::open(&path)?;
            if !file.metadata()?.is_file() {
                return Err("authentication configuration is not a regular file".into());
            }
            let mut bytes = Vec::new();
            file.take(MAX_CONFIG + 1).read_to_end(&mut bytes)?;
            if bytes.len() as u64 > MAX_CONFIG {
                return Err("authentication configuration is too large".into());
            }
            let table: toml::Table = toml::from_str(std::str::from_utf8(&bytes)?)?;
            let appearance = parse_appearance(&table)?;
            let theme = ThemeSnapshot::from_config(&table, 1)?;
            snapshot::install(Arc::new(theme))?;
            visuals.appearance = appearance;
            Ok(())
        })();
        if result.is_err() {
            visuals.notice = "Appearance configuration unavailable; using defaults.".into();
        }
        for (path, destination) in [
            (&visuals.appearance.wallpaper, &mut visuals.wallpaper),
            (&visuals.appearance.avatar, &mut visuals.avatar),
        ] {
            if let Some(path) = path {
                *destination = load_image(path, matches!(purpose, Purpose::Login)).ok();
                if destination.is_none() {
                    visuals.notice = "Image unavailable; using opaque theme background.".into();
                }
            }
        }
        visuals.tick();
        visuals
    }
    pub fn tick(&mut self) -> bool {
        let now = Local::now();
        let clock = now.format(&self.appearance.clock_format).to_string();
        let date = now.format(&self.appearance.date_format).to_string();
        let changed = clock != self.clock || date != self.date;
        self.clock = clock;
        self.date = date;
        changed
    }
    pub fn heading<'a, M: 'a>(&self) -> Element<'a, M> {
        let mut body = Column::new()
            .spacing(OXITHEME.padding_sm)
            .align_x(iced::Alignment::Center);
        if self.appearance.clock {
            body = body
                .push(oxi_plugin::text(self.clock.clone()).size(OXITHEME.font_xxl * 2.0))
                .push(oxi_plugin::text(self.date.clone()).size(OXITHEME.font_md));
        }
        if let Some(avatar) = &self.avatar {
            return Row::new()
                .spacing(OXITHEME.padding_lg)
                .align_y(iced::Alignment::Center)
                .push(
                    image(avatar.clone())
                        .width(72)
                        .height(72)
                        .content_fit(iced::ContentFit::Contain),
                )
                .push(body)
                .into();
        }
        body.into()
    }
    pub fn scene<'a, M: 'a>(&self, content: Element<'a, M>) -> Element<'a, M> {
        let opaque = container(Space::new())
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| {
                container::Style::default().background(iced::Color {
                    a: 1.0,
                    ..OXITHEME.base
                })
            });
        let mut layers = vec![opaque.into()];
        if let Some(wallpaper) = &self.wallpaper {
            layers.push(
                image(wallpaper.clone())
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .content_fit(iced::ContentFit::Cover)
                    .into(),
            );
            let overlay = self.appearance.overlay;
            layers.push(
                container(Space::new())
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .style(move |_| {
                        container::Style::default().background(iced::Color {
                            a: overlay,
                            ..OXITHEME.base
                        })
                    })
                    .into(),
            );
        }
        layers.push(content);
        stack(layers).into()
    }
}

#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub struct KeyboardStatus {
    pub caps_lock: Option<bool>,
    pub layout: String,
}
impl KeyboardStatus {
    pub fn configured() -> Self {
        let layout = std::env::var("OXI_KEYBOARD_LAYOUT")
            .ok()
            .filter(|s| {
                s.len() <= 128
                    && s.chars()
                        .all(|c| c.is_ascii_alphanumeric() || "_,-+".contains(c))
            })
            .unwrap_or_else(|| "Compositor layout".into());
        Self {
            caps_lock: None,
            layout,
        }
    }
    pub fn observe(&mut self, event: &keyboard::Event) {
        if let keyboard::Event::KeyPressed {
            key: keyboard::Key::Character(key),
            modified_key: keyboard::Key::Character(modified),
            modifiers,
            ..
        } = event
        {
            if key.len() == 1
                && key.as_bytes()[0].is_ascii_alphabetic()
                && modified.len() == 1
                && modified.as_bytes()[0].is_ascii_alphabetic()
            {
                self.caps_lock =
                    Some(modified.as_bytes()[0].is_ascii_uppercase() ^ modifiers.shift());
            }
        } else if matches!(
            event,
            keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::CapsLock),
                repeat: false,
                ..
            }
        ) {
            self.caps_lock = self.caps_lock.map(|value| !value);
        }
    }
    pub fn view<'a, M: 'a>(&self) -> Element<'a, M> {
        Row::new()
            .spacing(OXITHEME.padding_lg)
            .push(
                oxi_plugin::text(format!(
                    "Keyboard: {}",
                    if self.layout.is_empty() {
                        "Compositor layout"
                    } else {
                        &self.layout
                    }
                ))
                .size(OXITHEME.font_sm),
            )
            .push(
                oxi_plugin::text(match self.caps_lock {
                    Some(true) => "Caps Lock on",
                    Some(false) => "Caps Lock off",
                    None => "Caps Lock: check letter case",
                })
                .size(OXITHEME.font_sm)
                .color(if self.caps_lock == Some(true) {
                    OXITHEME.warning
                } else {
                    OXITHEME.text_muted
                }),
            )
            .into()
    }
}

fn parse_appearance(table: &toml::Table) -> Result<Appearance, Box<dyn std::error::Error>> {
    let appearance: Appearance = table
        .get("appearance")
        .cloned()
        .map(toml::Value::try_into)
        .transpose()?
        .unwrap_or_default();
    if !appearance.overlay.is_finite() || !(0.4..=1.0).contains(&appearance.overlay) {
        return Err("appearance.overlay: expected 0.4..1.0".into());
    }
    for format in [&appearance.clock_format, &appearance.date_format] {
        if format.len() > 128
            || format.chars().any(char::is_control)
            || StrftimeItems::new(format).any(|item| matches!(item, Item::Error))
        {
            return Err("invalid clock format".into());
        }
    }
    for path in [&appearance.wallpaper, &appearance.avatar]
        .into_iter()
        .flatten()
    {
        if !path.is_absolute() {
            return Err("appearance image paths must be absolute".into());
        }
    }
    Ok(appearance)
}
fn administrator_owned(path: &Path) -> std::io::Result<()> {
    let resolved = path.canonicalize()?;
    for path in path.ancestors().chain(resolved.ancestors()) {
        let metadata = fs::symlink_metadata(path)?;
        if metadata.uid() != 0
            || (!metadata.file_type().is_symlink()
                && metadata.mode() & 0o022 != 0
                && !(metadata.is_dir() && metadata.mode() & 0o1000 != 0))
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "greeter appearance must be administrator-owned",
            ));
        }
    }
    Ok(())
}
fn load_image(path: &Path, system: bool) -> Result<image::Handle, Box<dyn std::error::Error>> {
    if system {
        administrator_owned(path)?;
    }
    let file = fs::File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err("image is not a regular file".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_IMAGE + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_IMAGE {
        return Err("image is too large".into());
    }
    let mut reader = ::image::ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = ::image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode()?.into_rgba8();
    let (width, height) = image.dimensions();
    Ok(image::Handle::from_rgba(width, height, image.into_raw()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_formats_and_unbounded_images_do_not_reach_rendering() {
        for text in [
            "[appearance]\nclock_format='%Q'",
            "[appearance]\nwallpaper='relative.png'",
            "[appearance]\noverlay=0.1",
        ] {
            assert!(parse_appearance(&toml::from_str(text).unwrap()).is_err());
        }
        assert!(
            parse_appearance(&toml::from_str("[appearance]\nclock_format='%H:%M:%S'").unwrap())
                .is_ok()
        );
    }
    #[test]
    fn image_decoding_accepts_pixels_but_rejects_corruption_and_large_dimensions() {
        let directory =
            std::env::temp_dir().join(format!("oxi-image-bounds-{}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        let path = directory.join("avatar.png");
        ::image::RgbaImage::from_pixel(4, 4, ::image::Rgba([32, 64, 96, 255]))
            .save_with_format(&path, ::image::ImageFormat::Png)
            .unwrap();
        assert!(load_image(&path, false).is_ok());
        fs::write(&path, b"invalid image").unwrap();
        assert!(load_image(&path, false).is_err());
        ::image::RgbaImage::new(8193, 1)
            .save_with_format(&path, ::image::ImageFormat::Png)
            .unwrap();
        assert!(load_image(&path, false).is_err());
        fs::remove_file(path).unwrap();
        fs::remove_dir(directory).unwrap();
    }
    #[test]
    fn caps_inference_accounts_for_shift_and_does_not_claim_unknown_initial_state() {
        let mut status = KeyboardStatus::default();
        assert_eq!(status.caps_lock, None);
        for (modified, shift, caps) in [
            ("A", false, true),
            ("A", true, false),
            ("a", true, true),
            ("a", false, false),
        ] {
            status.observe(&keyboard::Event::KeyPressed {
                key: keyboard::Key::Character("a".into()),
                modified_key: keyboard::Key::Character(modified.into()),
                physical_key: keyboard::key::Physical::Unidentified(
                    keyboard::key::NativeCode::Unidentified,
                ),
                location: keyboard::Location::Standard,
                modifiers: if shift {
                    keyboard::Modifiers::SHIFT
                } else {
                    keyboard::Modifiers::empty()
                },
                text: None,
                repeat: false,
            });
            assert_eq!(status.caps_lock, Some(caps));
        }
    }
}
