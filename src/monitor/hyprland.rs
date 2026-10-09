use ::hyprland::{data::Monitor, shared::HyprDataActive};

use super::{Output, OutputMode, OutputSnapshot, output::logical_size};
use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    time::Duration,
};

pub(super) fn active_output_name() -> Option<String> {
    Monitor::get_active().ok().map(|monitor| monitor.name)
}

pub(super) fn inventory() -> Result<OutputSnapshot, String> {
    let signature = std::env::var("HYPRLAND_INSTANCE_SIGNATURE")
        .map_err(|_| "Not a Hyprland session".to_owned())?;
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .ok_or("Missing runtime directory")?;
    let path = runtime.join("hypr").join(signature).join(".socket.sock");
    let mut socket = UnixStream::connect(path).map_err(|error| error.to_string())?;
    socket
        .set_read_timeout(Some(Duration::from_millis(500)))
        .map_err(|error| error.to_string())?;
    socket
        .set_write_timeout(Some(Duration::from_millis(500)))
        .map_err(|error| error.to_string())?;
    socket
        .write_all(b"j/monitors all")
        .map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    socket
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err("Monitor response too large".to_owned());
    }
    parse_inventory(&bytes)
}

pub(super) fn parse_inventory(bytes: &[u8]) -> Result<OutputSnapshot, String> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    let values = value
        .as_array()
        .ok_or("Monitor response must be an array")?;
    let mut outputs = Vec::new();
    for value in values {
        let string = |key| {
            value
                .get(key)
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let name = string("name");
        if name.is_empty() {
            continue;
        }
        let dimension = |key| {
            value
                .get(key)
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or(0)
        };
        let scale = value
            .get("scale")
            .and_then(serde_json::Value::as_f64)
            .filter(|value| value.is_finite() && *value > 0.0)
            .unwrap_or(1.0);
        let transform = dimension("transform");
        let size = (dimension("width"), dimension("height"));
        let coordinate = |key| {
            value
                .get(key)
                .and_then(serde_json::Value::as_i64)
                .and_then(|value| i32::try_from(value).ok())
                .unwrap_or(0)
        };
        let refresh = value
            .get("refreshRate")
            .and_then(serde_json::Value::as_f64)
            .filter(|value| value.is_finite() && *value >= 0.0)
            .unwrap_or(0.0);
        let mut modes: Vec<_> = value
            .get("availableModes")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
            .filter_map(parse_mode)
            .collect();
        modes.sort_by(|a, b| {
            (a.width, a.height)
                .cmp(&(b.width, b.height))
                .then_with(|| a.refresh_hz.total_cmp(&b.refresh_hz))
        });
        modes.dedup();
        outputs.push(Output {
            name,
            description: string("description"),
            make: string("make"),
            model: string("model"),
            serial: string("serial"),
            enabled: !value
                .get("disabled")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
                && size.0 > 0
                && size.1 > 0,
            focused: value
                .get("focused")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            physical_size: size,
            logical_size: logical_size(size, scale, transform),
            position: (coordinate("x"), coordinate("y")),
            scale,
            transform,
            refresh_hz: refresh,
            vrr: value.get("vrr").and_then(serde_json::Value::as_bool),
            modes,
        });
    }
    outputs.sort_by(|a, b| a.name.cmp(&b.name));
    outputs.dedup_by(|a, b| a.name == b.name);
    Ok(OutputSnapshot {
        backend: "Hyprland IPC".to_owned(),
        outputs,
    })
}

fn parse_mode(value: &str) -> Option<OutputMode> {
    let (size, refresh) = value.split_once('@')?;
    let (width, height) = size.split_once('x')?;
    let refresh_hz: f64 = refresh.trim_end_matches("Hz").parse().ok()?;
    let mode = OutputMode {
        width: width.parse().ok()?,
        height: height.parse().ok()?,
        refresh_hz,
    };
    (mode.width > 0 && mode.height > 0 && mode.refresh_hz.is_finite() && mode.refresh_hz > 0.0)
        .then_some(mode)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monitor_parser_keeps_precise_refresh_and_handles_partial_objects() {
        let snapshot = parse_inventory(br#"[{"name":"DP-1","width":1920,"height":1080,"scale":1.25,"transform":1,"x":-864,"refreshRate":143.98123,"availableModes":["1920x1080@143.98123Hz","bad"],"focused":true},{"name":"DP-2","disabled":true}]"#).unwrap();
        assert_eq!(snapshot.outputs[0].logical_size, (864, 1536));
        assert_eq!(snapshot.outputs[0].position, (-864, 0));
        assert_eq!(snapshot.outputs[0].refresh_hz, 143.98123);
        assert_eq!(snapshot.outputs[0].modes[0].refresh_hz, 143.98123);
        assert!(!snapshot.outputs[1].enabled);
        assert!(parse_inventory(br#"{}"#).is_err());
    }
}
