use toml::Table;
use tracing::error;

const DEFAULT_FONT: &str = "Adwaita Sans";

/// Read `[bar] font` from the global config, falling back to [`DEFAULT_FONT`]
/// when unset. Exposed as a leaked `&'static str` because both
/// `Font::with_name` and `iced::application::default_font` want a `'static`
/// reference and the value lives for the whole program.
pub(crate) fn configured_bar_font(config: &Table) -> &'static str {
    config
        .get("bar")
        .and_then(|v| v.as_table())
        .and_then(|t| t.get("font"))
        .and_then(|v| v.as_str())
        .map(|s| &*Box::leak(s.to_owned().into_boxed_str()))
        .unwrap_or(DEFAULT_FONT)
}

pub(crate) fn bar_font(config: &Table) -> &'static str {
    let configured = configured_bar_font(config);
    fontconfig_value(configured, "%{family}")
        .and_then(|family| {
            family
                .split(',')
                .next()
                .map(str::trim)
                .filter(|family| !family.is_empty())
                .map(ToOwned::to_owned)
        })
        .map(|family| &*Box::leak(family.into_boxed_str()))
        .unwrap_or(configured)
}

/// Read `[bar] font_file` and load the file's bytes into a leaked `'static`
/// slice so iced can register it.
pub(crate) fn bar_font_bytes(config: &Table) -> Option<&'static [u8]> {
    let configured_path = config
        .get("bar")
        .and_then(|v| v.as_table())
        .and_then(|t| t.get("font_file"))
        .and_then(|v| v.as_str());
    let path = configured_path
        .map(ToOwned::to_owned)
        .or_else(|| fontconfig_value(configured_bar_font(config), "%{file}"))?;
    read_font_bytes(&path)
}

fn read_font_bytes(path: &str) -> Option<&'static [u8]> {
    match std::fs::read(path) {
        Ok(bytes) => Some(Box::leak(bytes.into_boxed_slice())),
        Err(e) => {
            error!("could not read bar font file `{path}`: {e}");
            None
        }
    }
}

fn fontconfig_value(family: &str, format: &str) -> Option<String> {
    let output = std::process::Command::new("fc-match")
        .args(["-f", format, family])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if value.is_empty() { None } else { Some(value) }
}
