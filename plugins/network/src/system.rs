use std::collections::BTreeMap;
use std::process::Command;

#[derive(Clone, Debug)]
pub(crate) struct Snapshot {
    pub(crate) connected: Vec<ActiveNetwork>,
    pub(crate) saved: Vec<SavedConnection>,
    pub(crate) wifi: Vec<WifiNetwork>,
}

#[derive(Clone, Debug)]
pub(crate) struct ActiveNetwork {
    pub(crate) name: String,
    pub(crate) uuid: String,
    pub(crate) kind: String,
    pub(crate) device: String,
}

#[derive(Clone, Debug)]
pub(crate) struct SavedConnection {
    pub(crate) name: String,
    pub(crate) uuid: String,
    pub(crate) kind: String,
}

#[derive(Clone, Debug)]
pub(crate) struct WifiNetwork {
    pub(crate) ssid: String,
    pub(crate) bssid: String,
    pub(crate) signal: u8,
    pub(crate) security: String,
    pub(crate) connected: bool,
    pub(crate) active_uuid: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) enum NetworkAction {
    ConnectWifi { ssid: String },
    ConnectConnection { uuid: String },
    Disconnect { uuid: String },
    SaveWifi { ssid: String, password: String },
    SaveConnection { uuid: String, password: String },
}

impl NetworkAction {
    pub(crate) fn label(&self) -> String {
        match self {
            NetworkAction::ConnectWifi { ssid } => format!("Connecting to {ssid}"),
            NetworkAction::ConnectConnection { .. } => "Connecting".to_owned(),
            NetworkAction::Disconnect { .. } => "Disconnecting".to_owned(),
            NetworkAction::SaveWifi { ssid, .. } => format!("Saving {ssid}"),
            NetworkAction::SaveConnection { .. } => "Saving connection".to_owned(),
        }
    }
}

pub(crate) fn scan_networks() -> Result<Snapshot, String> {
    let connected = scan_active_connections()?;
    let saved = scan_saved_connections(&connected)?;
    let wifi = scan_wifi_networks(&connected)?;
    Ok(Snapshot {
        connected,
        saved,
        wifi,
    })
}

fn scan_active_connections() -> Result<Vec<ActiveNetwork>, String> {
    let output = run_nmcli(&[
        "-t",
        "-f",
        "NAME,UUID,TYPE,DEVICE",
        "connection",
        "show",
        "--active",
    ])?;
    let mut networks = Vec::new();
    for line in output.lines().filter(|line| !line.trim().is_empty()) {
        let fields = split_nmcli_line(line);
        if fields.len() < 4 || fields[1].is_empty() {
            continue;
        }
        networks.push(ActiveNetwork {
            name: fields[0].clone(),
            uuid: fields[1].clone(),
            kind: fields[2].clone(),
            device: fields[3].clone(),
        });
    }
    Ok(networks)
}

fn scan_saved_connections(active: &[ActiveNetwork]) -> Result<Vec<SavedConnection>, String> {
    let output = run_nmcli(&["-t", "-f", "NAME,UUID,TYPE", "connection", "show"])?;
    let mut connections = Vec::new();
    for line in output.lines().filter(|line| !line.trim().is_empty()) {
        let fields = split_nmcli_line(line);
        if fields.len() < 3 || fields[1].is_empty() {
            continue;
        }
        if active.iter().any(|network| network.uuid == fields[1]) {
            continue;
        }
        let kind = fields[2].clone();
        if kind == "loopback" {
            continue;
        }
        connections.push(SavedConnection {
            name: fields[0].clone(),
            uuid: fields[1].clone(),
            kind,
        });
    }
    connections.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(connections)
}

fn scan_wifi_networks(active: &[ActiveNetwork]) -> Result<Vec<WifiNetwork>, String> {
    let output = run_nmcli(&[
        "-t",
        "-f",
        "IN-USE,SSID,BSSID,SIGNAL,SECURITY",
        "device",
        "wifi",
        "list",
        "--rescan",
        "auto",
    ])?;
    let mut by_ssid = BTreeMap::<String, WifiNetwork>::new();
    for line in output.lines().filter(|line| !line.trim().is_empty()) {
        let fields = split_nmcli_line(line);
        if fields.len() < 5 || fields[1].trim().is_empty() {
            continue;
        }
        let ssid = fields[1].clone();
        let signal = fields[3].parse::<u8>().unwrap_or_default();
        let connected = fields[0].trim() == "*";
        let active_uuid = active
            .iter()
            .find(|network| network.name == ssid && network.kind.contains("wireless"))
            .map(|network| network.uuid.clone());
        let network = WifiNetwork {
            ssid: ssid.clone(),
            bssid: fields[2].clone(),
            signal,
            security: fields[4].clone(),
            connected,
            active_uuid,
        };
        match by_ssid.get(&ssid) {
            Some(existing) if existing.signal >= signal => {}
            _ => {
                by_ssid.insert(ssid, network);
            }
        }
    }
    let mut networks = by_ssid.into_values().collect::<Vec<_>>();
    networks.sort_by(|a, b| b.connected.cmp(&a.connected).then(b.signal.cmp(&a.signal)));
    Ok(networks)
}

pub(crate) fn run_network_action(action: NetworkAction) -> Result<String, String> {
    match action {
        NetworkAction::ConnectWifi { ssid } => connect_wifi(&ssid),
        NetworkAction::ConnectConnection { uuid } => run_nmcli_owned(vec![
            "connection".to_owned(),
            "up".to_owned(),
            "uuid".to_owned(),
            uuid,
        ]),
        NetworkAction::Disconnect { uuid } => run_nmcli_owned(vec![
            "connection".to_owned(),
            "down".to_owned(),
            "uuid".to_owned(),
            uuid,
        ]),
        NetworkAction::SaveWifi { ssid, password } => connect_wifi_with_password(&ssid, &password),
        NetworkAction::SaveConnection { uuid, password } => {
            if !password.is_empty() {
                run_nmcli_owned(vec![
                    "connection".to_owned(),
                    "modify".to_owned(),
                    "uuid".to_owned(),
                    uuid.clone(),
                    "802-11-wireless-security.psk".to_owned(),
                    password,
                ])?;
            }
            run_nmcli_owned(vec![
                "connection".to_owned(),
                "up".to_owned(),
                "uuid".to_owned(),
                uuid,
            ])
        }
    }
}

fn connect_wifi(ssid: &str) -> Result<String, String> {
    run_nmcli_owned(vec![
        "connection".to_owned(),
        "up".to_owned(),
        "id".to_owned(),
        ssid.to_owned(),
    ])
    .or_else(|_| {
        run_nmcli_owned(vec![
            "device".to_owned(),
            "wifi".to_owned(),
            "connect".to_owned(),
            ssid.to_owned(),
        ])
    })
}

fn connect_wifi_with_password(ssid: &str, password: &str) -> Result<String, String> {
    let mut args = vec![
        "device".to_owned(),
        "wifi".to_owned(),
        "connect".to_owned(),
        ssid.to_owned(),
    ];
    if !password.is_empty() {
        args.push("password".to_owned());
        args.push(password.to_owned());
    }
    run_nmcli_owned(args)
}

fn run_nmcli(args: &[&str]) -> Result<String, String> {
    let output = Command::new("nmcli")
        .args(args)
        .output()
        .map_err(|e| format!("network: failed to run nmcli: {e}"))?;
    command_output(output)
}

fn run_nmcli_owned(args: Vec<String>) -> Result<String, String> {
    let output = Command::new("nmcli")
        .args(&args)
        .output()
        .map_err(|e| format!("network: failed to run nmcli: {e}"))?;
    command_output(output)
}

fn command_output(output: std::process::Output) -> Result<String, String> {
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if output.status.success() {
        Ok(stdout)
    } else if stderr.is_empty() {
        Err(format!("network: nmcli failed: {stdout}"))
    } else {
        Err(format!("network: nmcli failed: {stderr}"))
    }
}

fn split_nmcli_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut escaped = false;
    for ch in line.chars() {
        if escaped {
            field.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == ':' {
            fields.push(field);
            field = String::new();
        } else {
            field.push(ch);
        }
    }
    fields.push(field);
    fields
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_nmcli_line_unescapes_colons() {
        assert_eq!(
            split_nmcli_line(r"home\:wifi:uuid:802-11-wireless:wlan0"),
            vec!["home:wifi", "uuid", "802-11-wireless", "wlan0"]
        );
    }
}
