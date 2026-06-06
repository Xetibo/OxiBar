use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

#[derive(Clone, Debug)]
pub(crate) struct BluetoothDevice {
    pub(crate) mac: String,
    pub(crate) name: String,
    pub(crate) icon: String,
    pub(crate) paired: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct Snapshot {
    pub(crate) connected: Vec<BluetoothDevice>,
    pub(crate) available: Vec<BluetoothDevice>,
}

#[derive(Clone, Debug)]
pub(crate) enum PairOutcome {
    Done,
    NeedsCode,
}

#[derive(Clone, Debug)]
pub(crate) enum BluetoothAction {
    Pair { mac: String, code: Option<String> },
    Disconnect { mac: String },
}

pub(crate) fn scan_devices(active_scan: bool) -> Result<Snapshot, String> {
    if active_scan {
        let _ = run_bluetoothctl_script("scan on\n", Some(Duration::from_secs(4)));
    }
    let all = parse_devices(&run_bluetoothctl(&["devices"])?);
    let connected_macs = parse_devices(&run_bluetoothctl(&["devices", "Connected"])?);
    let connected_macs = connected_macs
        .into_iter()
        .map(|device| device.mac)
        .collect::<Vec<_>>();
    let mut connected = Vec::new();
    let mut available = Vec::new();
    for basic in all {
        let info = device_info(&basic.mac).unwrap_or_default();
        let is_connected = connected_macs.contains(&basic.mac) || info.connected;
        let device = BluetoothDevice {
            mac: basic.mac,
            name: basic.name,
            icon: info.icon,
            paired: info.paired,
        };
        if is_connected {
            connected.push(device);
        } else if !is_noise(&device) {
            available.push(device);
        }
    }
    connected.sort_by(|a, b| a.name.cmp(&b.name));
    available.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Snapshot {
        connected,
        available,
    })
}

pub(crate) fn has_bluetooth_controller() -> bool {
    run_bluetoothctl(&["show"])
        .map(|output| output_has_controller(&output))
        .unwrap_or(false)
}

fn output_has_controller(output: &str) -> bool {
    output
        .lines()
        .any(|line| line.trim_start().starts_with("Controller "))
}

#[derive(Default)]
struct DeviceInfo {
    icon: String,
    paired: bool,
    connected: bool,
}

fn device_info(mac: &str) -> Result<DeviceInfo, String> {
    let output = run_bluetoothctl(&["info", mac])?;
    let mut info = DeviceInfo::default();
    for line in output.lines().map(str::trim) {
        if let Some(icon) = line.strip_prefix("Icon:") {
            info.icon = icon.trim().to_owned();
        } else if let Some(paired) = line.strip_prefix("Paired:") {
            info.paired = paired.trim() == "yes";
        } else if let Some(connected) = line.strip_prefix("Connected:") {
            info.connected = connected.trim() == "yes";
        }
    }
    Ok(info)
}

fn parse_devices(output: &str) -> Vec<BluetoothDevice> {
    output
        .lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("Device ")?;
            let (mac, name) = rest.split_once(' ')?;
            Some(BluetoothDevice {
                mac: mac.to_owned(),
                name: name.trim().to_owned(),
                icon: String::new(),
                paired: false,
            })
        })
        .collect()
}

fn is_noise(device: &BluetoothDevice) -> bool {
    device.name.is_empty()
        || device.name.eq_ignore_ascii_case(&device.mac)
        || looks_like_mac(&device.name)
        || device.icon.is_empty()
}

fn looks_like_mac(value: &str) -> bool {
    let parts = value.split(':').collect::<Vec<_>>();
    parts.len() == 6
        && parts
            .iter()
            .all(|part| part.len() == 2 && part.chars().all(|ch| ch.is_ascii_hexdigit()))
}

pub(crate) fn run_bluetooth_action(action: BluetoothAction) -> Result<PairOutcome, String> {
    match action {
        BluetoothAction::Pair { mac, code } => pair_device(&mac, code),
        BluetoothAction::Disconnect { mac } => {
            run_bluetoothctl(&["disconnect", &mac])?;
            Ok(PairOutcome::Done)
        }
    }
}

fn pair_device(mac: &str, code: Option<String>) -> Result<PairOutcome, String> {
    let has_code = code.is_some();
    let script = if let Some(code) = code {
        format!(
            "agent KeyboardDisplay\ndefault-agent\npair {mac}\n{code}\nyes\ntrust {mac}\nconnect {mac}\nquit\n"
        )
    } else {
        format!(
            "agent KeyboardDisplay\ndefault-agent\npair {mac}\ntrust {mac}\nconnect {mac}\nquit\n"
        )
    };
    match run_bluetoothctl_script(&script, Some(Duration::from_secs(20))) {
        Ok(_) => Ok(PairOutcome::Done),
        Err(error) if needs_code(&error) && !has_code => Ok(PairOutcome::NeedsCode),
        Err(error) => Err(error),
    }
}

fn needs_code(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    lower.contains("pin")
        || lower.contains("passkey")
        || lower.contains("code")
        || lower.contains("agent")
}

fn run_bluetoothctl(args: &[&str]) -> Result<String, String> {
    let output = Command::new("bluetoothctl")
        .args(args)
        .output()
        .map_err(|e| format!("bluetooth: failed to run bluetoothctl: {e}"))?;
    command_output(output)
}

fn run_bluetoothctl_script(script: &str, timeout: Option<Duration>) -> Result<String, String> {
    let mut child = Command::new("bluetoothctl")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("bluetooth: failed to run bluetoothctl: {e}"))?;
    child
        .stdin
        .as_mut()
        .ok_or_else(|| "bluetooth: could not open bluetoothctl stdin".to_owned())?
        .write_all(script.as_bytes())
        .map_err(|e| format!("bluetooth: failed to write bluetoothctl script: {e}"))?;
    if let Some(timeout) = timeout {
        let start = std::time::Instant::now();
        loop {
            if let Some(_status) = child
                .try_wait()
                .map_err(|e| format!("bluetooth: bluetoothctl wait failed: {e}"))?
            {
                break;
            }
            if start.elapsed() > timeout {
                let _ = child.kill();
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    let output = child
        .wait_with_output()
        .map_err(|e| format!("bluetooth: bluetoothctl output failed: {e}"))?;
    command_output(output)
}

fn command_output(output: std::process::Output) -> Result<String, String> {
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if output.status.success()
        && !stdout.to_ascii_lowercase().contains("failed")
        && !stderr.to_ascii_lowercase().contains("failed")
    {
        Ok(stdout)
    } else if stderr.is_empty() {
        Err(format!("bluetooth: bluetoothctl failed: {stdout}"))
    } else {
        Err(format!(
            "bluetooth: bluetoothctl failed: {stderr}\n{stdout}"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bluetoothctl_devices() {
        let devices = parse_devices(
            "Device AA:BB:CC:DD:EE:FF Headphones\nignored\nDevice 11:22:33:44:55:66 Keyboard",
        );

        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].mac, "AA:BB:CC:DD:EE:FF");
        assert_eq!(devices[0].name, "Headphones");
        assert_eq!(devices[1].name, "Keyboard");
    }

    #[test]
    fn detects_controller_from_show_output() {
        let present = "Controller 00:11:22:33:44:55 (public)\n\tName: laptop";
        let absent = "No default controller available";

        assert!(output_has_controller(present));
        assert!(!output_has_controller(absent));
    }

    #[test]
    fn detects_noise_and_mac_like_names() {
        assert!(looks_like_mac("AA:BB:CC:DD:EE:FF"));
        assert!(!looks_like_mac("Headphones"));

        let noisy = BluetoothDevice {
            mac: "AA:BB:CC:DD:EE:FF".to_owned(),
            name: "AA:BB:CC:DD:EE:FF".to_owned(),
            icon: String::new(),
            paired: false,
        };
        assert!(is_noise(&noisy));

        let usable = BluetoothDevice {
            mac: "AA:BB:CC:DD:EE:FF".to_owned(),
            name: "Headphones".to_owned(),
            icon: "audio-card".to_owned(),
            paired: false,
        };
        assert!(!is_noise(&usable));
    }

    #[test]
    fn detects_pairing_code_prompts() {
        assert!(needs_code("Request PIN code"));
        assert!(needs_code("Passkey required"));
        assert!(!needs_code("Connection refused"));
    }
}
