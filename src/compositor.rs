use crate::{app::OxiBar, messages::Message};
use iced::{
    Task,
    futures::{SinkExt, Stream},
    stream,
};
pub use oxibar_plugin_api::{CompositorSnapshot, CompositorWindow, CompositorWorkspace};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    time::Duration,
};

fn directory() -> Result<PathBuf, String> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").ok_or("No compositor runtime directory")?;
    let signature = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE")
        .ok_or("Window inventory requires a supported compositor adapter")?;
    Ok(PathBuf::from(runtime).join("hypr").join(signature))
}

fn rpc(command: &str) -> Result<String, String> {
    let mut socket = UnixStream::connect(directory()?.join(".socket.sock"))
        .map_err(|error| error.to_string())?;
    let timeout = Some(Duration::from_millis(500));
    socket
        .set_read_timeout(timeout)
        .map_err(|error| error.to_string())?;
    socket
        .set_write_timeout(timeout)
        .map_err(|error| error.to_string())?;
    socket
        .write_all(command.as_bytes())
        .map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    socket
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err("Compositor response exceeds limit".into());
    }
    String::from_utf8(bytes).map_err(|error| error.to_string())
}

fn json(command: &str) -> Result<serde_json::Value, String> {
    serde_json::from_str(&rpc(command)?).map_err(|error| error.to_string())
}

pub(crate) fn read_snapshot() -> Result<CompositorSnapshot, String> {
    parse(
        &json("j/clients")?,
        &json("j/monitors")?,
        &json("j/workspaces")?,
        &json("j/activewindow")?,
        &json("j/version")?,
    )
}

fn parse(
    clients: &serde_json::Value,
    monitors: &serde_json::Value,
    workspaces: &serde_json::Value,
    active: &serde_json::Value,
    version: &serde_json::Value,
) -> Result<CompositorSnapshot, String> {
    let names: BTreeMap<_, _> = monitors
        .as_array()
        .ok_or("Invalid monitor inventory")?
        .iter()
        .filter_map(|item| Some((item["id"].as_i64()?, item["name"].as_str()?.to_owned())))
        .collect();
    let mut snapshot = CompositorSnapshot {
        backend: "Hyprland".into(),
        ..Default::default()
    };
    let tag = version["tag"]
        .as_str()
        .or_else(|| version["version"].as_str())
        .unwrap_or_default()
        .trim_start_matches('v');
    let mut parts = tag.split('.');
    snapshot.lua_dispatch = parts
        .next()
        .and_then(|part| part.parse::<u32>().ok())
        .unwrap_or(0)
        > 0
        || parts
            .next()
            .and_then(|part| part.parse::<u32>().ok())
            .unwrap_or(0)
            >= 56;
    for monitor in monitors.as_array().into_iter().flatten() {
        if let (Some(name), Some(id)) = (
            monitor["name"].as_str(),
            monitor["activeWorkspace"]["id"].as_i64(),
        ) {
            snapshot.active_workspaces.insert(name.to_owned(), id);
        }
    }
    for item in clients.as_array().ok_or("Invalid window inventory")? {
        let Some(id) = item["address"].as_str().filter(|id| valid_window_id(id)) else {
            continue;
        };
        if item["mapped"].as_bool() == Some(false) || item["hidden"].as_bool() == Some(true) {
            continue;
        }
        snapshot.windows.push(CompositorWindow {
            id: id.into(),
            pid: item["pid"]
                .as_u64()
                .unwrap_or_default()
                .min(u32::MAX as u64) as u32,
            class: item["class"]
                .as_str()
                .or_else(|| item["initialClass"].as_str())
                .unwrap_or_default()
                .into(),
            title: item["title"].as_str().unwrap_or_default().into(),
            output: item["monitor"]
                .as_i64()
                .and_then(|id| names.get(&id).cloned()),
            workspace: item["workspace"]["id"].as_i64().unwrap_or_default(),
            focused: active["address"].as_str() == Some(id),
        });
    }
    snapshot.windows.sort_by(|a, b| a.id.cmp(&b.id));
    for item in workspaces.as_array().ok_or("Invalid workspace inventory")? {
        if let (Some(id), Some(output)) = (item["id"].as_i64(), item["monitor"].as_str()) {
            snapshot.workspaces.push(CompositorWorkspace {
                id,
                name: item["name"].as_str().unwrap_or_default().into(),
                output: output.into(),
                windows: item["windows"].as_u64().unwrap_or(0).min(u32::MAX as u64) as u32,
            });
        }
    }
    snapshot.workspaces.sort_by_key(|item| item.id);
    Ok(snapshot)
}

pub(crate) fn valid_window_id(id: &str) -> bool {
    id.strip_prefix("0x").is_some_and(|value| {
        !value.is_empty() && value.len() <= 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

fn dispatch(
    snapshot: &CompositorSnapshot,
    window: Option<&str>,
    workspace: Option<i64>,
) -> Result<(), String> {
    let command = if let Some(window) = window {
        let expected = snapshot
            .windows
            .iter()
            .find(|entry| entry.id == window)
            .ok_or("Window is no longer available")?;
        let address = window_address(window).ok_or("Invalid window identity")?;
        let current = json("j/clients")?;
        if !current.as_array().into_iter().flatten().any(|entry| {
            entry["address"].as_str() == Some(address)
                && entry["pid"].as_u64() == Some(u64::from(expected.pid))
        }) {
            return Err("Window identity has changed".into());
        }
        if snapshot.lua_dispatch {
            format!("dispatch hl.dsp.focus({{window=\"address:{address}\"}})")
        } else {
            format!("dispatch focuswindow address:{address}")
        }
    } else if let Some(workspace) = workspace {
        if !(1..=i64::from(u32::MAX)).contains(&workspace) {
            return Err("Invalid workspace".into());
        }
        if snapshot.lua_dispatch {
            format!("dispatch hl.dsp.focus({{workspace={workspace}}})")
        } else {
            format!("dispatch workspace {workspace}")
        }
    } else {
        return Err("Missing compositor action".into());
    };
    let response = rpc(&command)?;
    if response.trim() == "ok"
        || serde_json::from_str::<serde_json::Value>(&response)
            .is_ok_and(|value| value["ok"].as_bool() == Some(true))
    {
        Ok(())
    } else {
        Err(response)
    }
}

pub(crate) fn watch() -> impl Stream<Item = Message> {
    stream::channel(8, async move |mut output| {
        use tokio::io::BufReader;
        let mut previous = None;
        let mut identities = WindowIdentities::default();
        loop {
            let path = match directory() {
                Ok(path) => path,
                Err(error) => {
                    if previous.as_ref() != Some(&Err(error.clone())) {
                        if output
                            .send(Message::CompositorChanged(Err(error.clone())))
                            .await
                            .is_err()
                        {
                            return;
                        }
                        previous = Some(Err(error));
                    }
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
            };
            let socket = match tokio::net::UnixStream::connect(path.join(".socket2.sock")).await {
                Ok(socket) => socket,
                Err(error) => {
                    if output
                        .send(Message::CompositorChanged(Err(error.to_string())))
                        .await
                        .is_err()
                    {
                        return;
                    }
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
            };
            identities.ids.clear();
            let mut lines = BufReader::new(socket);
            let mut pending = Vec::new();
            loop {
                let mut snapshot = crate::background::run(read_snapshot)
                    .await
                    .and_then(|result| result);
                if let Ok(snapshot) = &mut snapshot {
                    identities.assign(snapshot);
                }
                if previous.as_ref() != Some(&snapshot) {
                    if output
                        .send(Message::CompositorChanged(snapshot.clone()))
                        .await
                        .is_err()
                    {
                        return;
                    }
                    previous = Some(snapshot);
                }
                let next = loop {
                    match next_event(&mut lines, &mut pending).await {
                        Ok(Some(line)) if relevant(&line) => {
                            identities.observe(&line);
                            break true;
                        }
                        Ok(Some(_)) => continue,
                        _ => break false,
                    }
                };
                if !next {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(30)).await;
                while let Ok(Ok(Some(line))) = tokio::time::timeout(
                    Duration::from_millis(1),
                    next_event(&mut lines, &mut pending),
                )
                .await
                {
                    identities.observe(&line);
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    })
}

fn relevant(line: &str) -> bool {
    [
        "openwindow",
        "closewindow",
        "activewindow",
        "windowtitle",
        "movewindow",
        "workspace",
        "createworkspace",
        "destroyworkspace",
        "focusedmon",
        "monitoradded",
        "monitorremoved",
    ]
    .iter()
    .any(|prefix| line.starts_with(prefix))
}

#[derive(Default)]
struct WindowIdentities {
    serial: u64,
    ids: BTreeMap<(String, u32), u64>,
}

impl WindowIdentities {
    fn assign(&mut self, snapshot: &mut CompositorSnapshot) {
        self.ids.retain(|(address, pid), _| {
            snapshot
                .windows
                .iter()
                .any(|window| window.id == *address && window.pid == *pid)
        });
        for window in &mut snapshot.windows {
            let address = window.id.clone();
            let serial = *self
                .ids
                .entry((address.clone(), window.pid))
                .or_insert_with(|| {
                    self.serial = self.serial.wrapping_add(1).max(1);
                    self.serial
                });
            window.id = format!("hyprland:{serial}:{address}");
        }
    }
    fn observe(&mut self, line: &str) {
        if let Some(address) = line.strip_prefix("closewindow>>") {
            let address = if address.starts_with("0x") {
                address.to_owned()
            } else {
                format!("0x{address}")
            };
            self.ids.retain(|(candidate, _), _| candidate != &address);
        }
    }
}

fn window_address(id: &str) -> Option<&str> {
    let address = if let Some(id) = id.strip_prefix("hyprland:") {
        let (serial, address) = id.split_once(':')?;
        if serial.parse::<u64>().ok()? == 0 {
            return None;
        }
        address
    } else {
        id
    };
    valid_window_id(address).then_some(address)
}

async fn next_event<R: tokio::io::AsyncBufRead + Unpin>(
    reader: &mut R,
    pending: &mut Vec<u8>,
) -> std::io::Result<Option<String>> {
    use tokio::io::AsyncBufReadExt;
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return if pending.is_empty() {
                Ok(None)
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "Incomplete compositor event",
                ))
            };
        }
        let end = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|index| index + 1);
        let count = end.unwrap_or(available.len());
        if pending.len() + count > 65536 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Compositor event exceeds limit",
            ));
        }
        pending.extend_from_slice(&available[..count]);
        reader.consume(count);
        if end.is_some() {
            let bytes = std::mem::take(pending);
            return String::from_utf8(bytes)
                .map(|line| Some(line.trim_end_matches(['\r', '\n']).to_owned()))
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error));
        }
    }
}

impl OxiBar {
    pub(crate) fn compositor_changed(
        &mut self,
        result: Result<CompositorSnapshot, String>,
    ) -> Task<Message> {
        let mut tasks = Vec::new();
        match result {
            Ok(snapshot) => {
                self.compositor_error = None;
                self.compositor = Some(snapshot);
            }
            Err(error) => self.compositor_error = Some(error),
        }
        let mut snapshot = self.compositor.clone().unwrap_or_default();
        snapshot.error = self.compositor_error.clone();
        for (id, (model, funcs)) in &self.plugins {
            if let Some(callback) = funcs.compositor_changed
                && let Some(task) = unsafe { callback(model.clone(), snapshot.clone()) }
            {
                let id = id.clone();
                tasks.push(
                    task.map(move |message| {
                        crate::messages::map_plugin_message(id.clone(), message)
                    }),
                );
            }
        }
        tasks.push(self.reconcile_docks());
        Task::batch(tasks)
    }
    pub(crate) fn compositor_action(
        &mut self,
        window: Option<String>,
        workspace: Option<i64>,
    ) -> Task<Message> {
        let Some(snapshot) = self
            .compositor
            .clone()
            .filter(|_| self.compositor_error.is_none())
        else {
            self.compositor_action_error = Some("Compositor backend is unavailable".into());
            return Task::none();
        };
        Task::perform(
            crate::background::run(move || dispatch(&snapshot, window.as_deref(), workspace)),
            |result| Message::CompositorActionDone(result.and_then(|result| result)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snapshot_normalizes_windows_workspaces_and_rejects_injected_addresses() {
        let snapshot = parse(&serde_json::json!([{ "address":"0xab", "pid":22, "class":"Terminal", "mapped":true, "monitor":1, "workspace":{"id":2} }, {"address":"0x1;bad"}]), &serde_json::json!([{"id":1,"name":"DP-1","activeWorkspace":{"id":2}}]), &serde_json::json!([{"id":2,"monitor":"DP-1","name":"two","windows":1}]), &serde_json::json!({"address":"0xab"}), &serde_json::json!({"tag":"v0.56.2"})).unwrap();
        assert!(snapshot.lua_dispatch);
        assert_eq!(snapshot.windows.len(), 1);
        assert!(snapshot.windows[0].focused);
        assert_eq!(snapshot.windows[0].output.as_deref(), Some("DP-1"));
        assert_eq!(snapshot.active_workspaces["DP-1"], 2);
        assert!(!valid_window_id("0x"));
    }

    #[test]
    fn recycled_addresses_and_reconnect_never_reuse_a_displayed_action() {
        let window = CompositorWindow {
            id: "0xab".into(),
            pid: 22,
            class: "app".into(),
            title: "window".into(),
            output: None,
            workspace: 1,
            focused: false,
        };
        let mut identities = WindowIdentities::default();
        let mut first = CompositorSnapshot {
            windows: vec![window.clone()],
            ..Default::default()
        };
        identities.assign(&mut first);
        let id = first.windows[0].id.clone();
        let mut same = CompositorSnapshot {
            windows: vec![window.clone()],
            ..Default::default()
        };
        identities.assign(&mut same);
        assert_eq!(same.windows[0].id, id);
        identities.observe("closewindow>>ab");
        let mut replacement = CompositorSnapshot {
            windows: vec![window.clone()],
            ..Default::default()
        };
        identities.assign(&mut replacement);
        assert_ne!(replacement.windows[0].id, id);
        identities.ids.clear();
        let mut reconnect = CompositorSnapshot {
            windows: vec![window],
            ..Default::default()
        };
        identities.assign(&mut reconnect);
        assert_ne!(reconnect.windows[0].id, replacement.windows[0].id);
        assert_eq!(window_address(&id), Some("0xab"));
        assert!(window_address("hyprland:1:0xab;exec").is_none());
    }

    #[test]
    fn compositor_event_reader_bounds_input_and_preserves_partial_lines_on_cancel() {
        use tokio::io::AsyncWriteExt;
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let (mut writer, reader) = tokio::io::duplex(70000);
                let mut reader = tokio::io::BufReader::new(reader);
                let mut pending = Vec::new();
                writer.write_all(b"closewindow>>").await.unwrap();
                assert!(
                    tokio::time::timeout(
                        Duration::from_millis(5),
                        next_event(&mut reader, &mut pending)
                    )
                    .await
                    .is_err()
                );
                writer.write_all(b"ab\n").await.unwrap();
                assert_eq!(
                    next_event(&mut reader, &mut pending)
                        .await
                        .unwrap()
                        .as_deref(),
                    Some("closewindow>>ab")
                );
                writer.write_all(&vec![b'x'; 65537]).await.unwrap();
                assert!(next_event(&mut reader, &mut pending).await.is_err());
            });
    }
}
