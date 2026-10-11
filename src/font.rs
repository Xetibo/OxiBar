use std::{
    io::Read,
    os::unix::fs::OpenOptionsExt,
    process::Stdio,
    time::{Duration, Instant},
};
use toml::Table;
use tracing::error;

const DEFAULT_FONT: &str = "Adwaita Sans";

#[derive(Debug, Clone)]
pub struct FontData {
    pub family: String,
    pub bytes: Vec<u8>,
}

pub(crate) fn theme_font_data(requested: &str) -> (String, Result<FontData, String>) {
    let result = (|| {
        let family = fontconfig_value(requested, "%{family}")
            .and_then(|family| {
                family
                    .split(',')
                    .next()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
            })
            .ok_or("theme: font family lookup failed")?;
        let path =
            fontconfig_value(requested, "%{file}").ok_or("theme: font file lookup failed")?;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32)
            .open(&path)
            .map_err(|error| error.to_string())?;
        let metadata = file.metadata().map_err(|error| error.to_string())?;
        if !metadata.is_file() || metadata.len() > 16 * 1024 * 1024 {
            return Err("theme: expected a font file of at most 16 MiB".into());
        }
        let mut bytes = Vec::new();
        file.take(16 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        if bytes.len() > 16 * 1024 * 1024 {
            return Err("theme: font exceeds 16 MiB".into());
        }
        Ok(FontData { family, bytes })
    })();
    (requested.to_owned(), result)
}

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
    let mut child = std::process::Command::new("fc-match")
        .args(["-f", format, "--", family])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    if rustix::fs::fcntl_setfl(&stdout, rustix::fs::OFlags::NONBLOCK).is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut bytes = Vec::new();
    let mut finished = None;
    let mut eof = false;
    loop {
        let mut buffer = [0; 256];
        match stdout.read(&mut buffer) {
            Ok(0) => eof = true,
            Ok(count) => bytes.extend_from_slice(&buffer[..count]),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
        if finished.is_none() {
            finished = child.try_wait().ok().flatten();
        }
        if let Some(status) = finished
            && eof
        {
            if !status.success() {
                return None;
            }
            break;
        }
        if bytes.len() > 4096 || Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let value = String::from_utf8_lossy(&bytes).trim().to_owned();
    if value.is_empty() { None } else { Some(value) }
}
