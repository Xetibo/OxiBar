use std::{
    fs,
    io::{self, Read, Write},
    net::Shutdown,
    os::unix::{fs::PermissionsExt, net::UnixStream},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use iced::{futures::SinkExt, stream};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixListener,
};
use toml::Table;

use crate::{config, messages::Message};

const MAX_COMMAND_BYTES: u64 = 1024 * 1024;
const MAX_RESPONSE_BYTES: u64 = 8 * MAX_COMMAND_BYTES;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(3);
const ACTION_TIMEOUT: Duration = Duration::from_secs(8);
pub const PROTOCOL_VERSION: u64 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowCommand {
    Toggle,
    Open,
    Close,
}

#[derive(Debug, Clone)]
pub enum ConfigCommand {
    Get(Option<String>),
    Change(crate::config::Mutation, Option<u64>),
    Reload,
    Schema,
    Export(bool),
}

#[derive(Debug, Clone)]
pub enum Command {
    Overview(WindowCommand),
    Desktop(crate::desktop::Action),
    Windows,
    Plugins,
    Action {
        plugin: String,
        action: String,
        arguments: Vec<String>,
    },
    Osd {
        plugin: String,
        topic: String,
    },
    CloseOsds,
    Outputs,
    Launcher(WindowCommand),
    Settings(WindowCommand),
    ControlCenter(WindowCommand),
    Session(WindowCommand),
    Config(ConfigCommand),
    Status,
}

#[derive(Clone)]
pub struct Reply(Arc<Mutex<Option<iced::futures::channel::oneshot::Sender<serde_json::Value>>>>);

impl std::fmt::Debug for Reply {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("IPC reply")
    }
}

impl Reply {
    pub(crate) fn channel() -> (
        Self,
        iced::futures::channel::oneshot::Receiver<serde_json::Value>,
    ) {
        let (sender, receiver) = iced::futures::channel::oneshot::channel();
        (Self(Arc::new(Mutex::new(Some(sender)))), receiver)
    }
    pub fn send(&self, response: serde_json::Value) {
        if let Ok(mut sender) = self.0.lock()
            && let Some(sender) = sender.take()
        {
            let _ = sender.send(response);
        }
    }
}

#[derive(Debug, Clone)]
pub struct ControlRequest {
    pub command: Command,
    pub reply: Reply,
}

pub fn parse_value(text: &str) -> Result<toml::Value, String> {
    let mut table: Table = toml::from_str(&format!("value = {text}"))
        .map_err(|error| format!("Invalid TOML value: {error}"))?;
    if table.len() != 1 {
        return Err("Expected one TOML value".to_owned());
    }
    table
        .remove("value")
        .ok_or_else(|| "Missing TOML value".to_owned())
}

fn parse_request(text: &str) -> Result<Command, String> {
    let request: serde_json::Value =
        serde_json::from_str(text).map_err(|error| error.to_string())?;
    if request.get("version").and_then(serde_json::Value::as_u64) != Some(PROTOCOL_VERSION) {
        return Err("Unsupported protocol version".to_owned());
    }
    let string = |key| {
        request
            .get(key)
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| format!("Missing {key}"))
    };
    let revision = match request.get("revision") {
        None | Some(serde_json::Value::Null) => None,
        Some(value) => Some(value.as_u64().ok_or("Invalid revision")?),
    };
    Ok(match string("command")? {
        "action" => {
            let arguments = request
                .get("args")
                .and_then(serde_json::Value::as_array)
                .ok_or("Missing args")?;
            if arguments.len() > 16 {
                return Err("Too many action arguments".to_owned());
            }
            let arguments: Vec<String> = arguments
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .filter(|value| value.len() <= 1024)
                        .map(str::to_owned)
                        .ok_or("Invalid action argument".to_owned())
                })
                .collect::<Result<_, _>>()?;
            let plugin = string("provider")?;
            let action = string("action")?;
            if plugin.is_empty() || plugin.len() > 128 || action.is_empty() || action.len() > 64 {
                return Err("Invalid action name".to_owned());
            }
            Command::Action {
                plugin: plugin.to_owned(),
                action: action.to_owned(),
                arguments,
            }
        }
        "osd" => match string("action")? {
            "close" => Command::CloseOsds,
            "show" => Command::Osd {
                plugin: string("provider")?.to_owned(),
                topic: string("topic")?.to_owned(),
            },
            _ => return Err("Unknown OSD action".to_owned()),
        },
        "launcher" | "settings" | "control-center" | "overview" | "session" => {
            let action = match string("action")? {
                "toggle" => WindowCommand::Toggle,
                "open" => WindowCommand::Open,
                "close" => WindowCommand::Close,
                _ => return Err("Unknown window action".to_owned()),
            };
            if string("command")? == "launcher" {
                Command::Launcher(action)
            } else if string("command")? == "settings" {
                Command::Settings(action)
            } else if string("command")? == "overview" {
                Command::Overview(action)
            } else if string("command")? == "session" {
                Command::Session(action)
            } else {
                Command::ControlCenter(action)
            }
        }
        "status" => Command::Status,
        "windows" => Command::Windows,
        "plugins" => Command::Plugins,
        "desktop" => Command::Desktop(match string("action")? {
            "edit" => crate::desktop::Action::Edit,
            "save" => crate::desktop::Action::Save,
            "cancel" => crate::desktop::Action::Cancel,
            _ => return Err("Unknown desktop action".into()),
        }),
        "outputs" => Command::Outputs,
        "config" => Command::Config(match string("action")? {
            "get" => ConfigCommand::Get(
                request
                    .get("path")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned),
            ),
            "set" => ConfigCommand::Change(
                crate::config::Mutation::Set(
                    string("path")?.to_owned(),
                    parse_value(string("value")?)?,
                ),
                revision,
            ),
            "reset" => ConfigCommand::Change(
                crate::config::Mutation::Reset(string("path")?.to_owned()),
                revision,
            ),
            "reset-all" => ConfigCommand::Change(crate::config::Mutation::ResetAll, revision),
            "replace" => ConfigCommand::Change(
                crate::config::Mutation::Replace(
                    toml::from_str(string("toml")?).map_err(|error| error.to_string())?,
                ),
                revision,
            ),
            "reload" => ConfigCommand::Reload,
            "schema" => ConfigCommand::Schema,
            "export" => ConfigCommand::Export(match string("layer")? {
                "overrides" => true,
                "effective" => false,
                _ => return Err("Unknown export layer".to_owned()),
            }),
            _ => return Err("Unknown configuration action".to_owned()),
        }),
        _ => return Err("Unknown command".to_owned()),
    })
}

pub fn response(revision: u64, data: serde_json::Value, error: Option<&str>) -> serde_json::Value {
    serde_json::json!({"version": PROTOCOL_VERSION, "revision": revision, "ok": error.is_none(), "data": data, "error": error})
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct Endpoint {
    pub path: PathBuf,
    pub record: PathBuf,
}

fn client_socket_path() -> PathBuf {
    use std::os::unix::ffi::OsStringExt;
    if let Ok(bytes) = fs::read(config::get_config_dir().join("control.socket"))
        && !bytes.is_empty()
    {
        return PathBuf::from(std::ffi::OsString::from_vec(bytes));
    }
    socket_path(&config::get_config())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LauncherCommand {
    Toggle,
    Open,
    Close,
}

impl LauncherCommand {
    fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "launcher" | "launcher toggle" => Some(Self::Toggle),
            "launcher open" => Some(Self::Open),
            "launcher close" => Some(Self::Close),
            _ => None,
        }
    }

    #[cfg(test)]
    fn wire(self) -> &'static str {
        match self {
            Self::Toggle => "launcher toggle",
            Self::Open => "launcher open",
            Self::Close => "launcher close",
        }
    }
}

pub fn socket_path(config: &Table) -> PathBuf {
    config
        .get("ipc")
        .and_then(|value| value.get("socket_path"))
        .and_then(|value| value.as_str())
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("XDG_RUNTIME_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(config::get_oxirun_dir)
                .join("oxibar.sock")
        })
}

pub fn run_cli(args: &[String]) -> Result<bool, Box<dyn std::error::Error>> {
    if args.is_empty() {
        return Ok(false);
    }
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["--help"] | ["-h"] => {
            println!(
                "Usage: oxibar [launcher|settings|control-center|overview|session [toggle|open|close]]\n\
                 oxibar config get [path]\n\
                 oxibar config set <path> <TOML value>\n\
                 oxibar config reset <path> | reset-all | reload | schema\n\
                 oxibar config export [effective|overrides]\n\
                 oxibar status\n\n\
                  oxibar outputs | windows | plugins\n\
                  oxibar desktop [edit|save|cancel]\n\n\
                 oxibar action <provider> <verb> [args...]\n\
                 oxibar osd <provider> <topic> | osd close\n\n\
                  No arguments: start the resident widget shell.\n\
                   Window commands: control the running shell (bare command toggles)."
            );
        }
        ["--version"] | ["-V"] => println!("oxibar {}", env!("CARGO_PKG_VERSION")),
        _ => {
            if let Some(command) = LauncherCommand::parse(&args.join(" ")) {
                let action = match command {
                    LauncherCommand::Toggle => "toggle",
                    LauncherCommand::Open => "open",
                    LauncherCommand::Close => "close",
                };
                let response = send_request(
                    &client_socket_path(),
                    &serde_json::json!({"version": PROTOCOL_VERSION, "command": "launcher", "action": action}),
                )?;
                if response["ok"].as_bool() != Some(true) {
                    return Err(response["error"]
                        .as_str()
                        .unwrap_or("command failed")
                        .to_owned()
                        .into());
                }
            } else {
                let arguments: Vec<_> = args.iter().map(String::as_str).collect();
                let mut request = serde_json::json!({"version": PROTOCOL_VERSION});
                match arguments.as_slice() {
                    [command @ ("settings" | "control-center" | "overview" | "session")]
                    | [
                        command @ ("settings" | "control-center" | "overview" | "session"),
                        "toggle",
                    ] => {
                        request["command"] = (*command).into();
                        request["action"] = "toggle".into();
                    }
                    [
                        command @ ("settings" | "control-center" | "overview" | "session"),
                        action @ ("open" | "close"),
                    ] => {
                        request["command"] = (*command).into();
                        request["action"] = (*action).into();
                    }
                    ["status"] => request["command"] = "status".into(),
                    ["windows"] => request["command"] = "windows".into(),
                    ["plugins"] => request["command"] = "plugins".into(),
                    ["desktop"] | ["desktop", "edit"] => {
                        request["command"] = "desktop".into();
                        request["action"] = "edit".into();
                    }
                    ["desktop", action @ ("save" | "cancel")] => {
                        request["command"] = "desktop".into();
                        request["action"] = (*action).into();
                    }
                    ["action", provider, action, arguments @ ..] => {
                        request["command"] = "action".into();
                        request["provider"] = (*provider).into();
                        request["action"] = (*action).into();
                        request["args"] = serde_json::json!(arguments);
                    }
                    ["osd", "close"] => {
                        request["command"] = "osd".into();
                        request["action"] = "close".into();
                    }
                    ["osd", provider, topic] => {
                        request["command"] = "osd".into();
                        request["action"] = "show".into();
                        request["provider"] = (*provider).into();
                        request["topic"] = (*topic).into();
                    }
                    ["outputs"] => request["command"] = "outputs".into(),
                    ["config", "get"] => {
                        request["command"] = "config".into();
                        request["action"] = "get".into();
                    }
                    ["config", "get", path] => {
                        request["command"] = "config".into();
                        request["action"] = "get".into();
                        request["path"] = (*path).into();
                    }
                    ["config", "set", path, rest @ ..] if !rest.is_empty() => {
                        request["command"] = "config".into();
                        request["action"] = "set".into();
                        request["path"] = (*path).into();
                        request["value"] = rest.join(" ").into();
                    }
                    ["config", "reset", path] => {
                        request["command"] = "config".into();
                        request["action"] = "reset".into();
                        request["path"] = (*path).into();
                    }
                    ["config", action @ ("reset-all" | "reload" | "schema")] => {
                        request["command"] = "config".into();
                        request["action"] = (*action).into();
                    }
                    ["config", "export"] | ["config", "export", "effective"] => {
                        request["command"] = "config".into();
                        request["action"] = "export".into();
                        request["layer"] = "effective".into();
                    }
                    ["config", "export", "overrides"] => {
                        request["command"] = "config".into();
                        request["action"] = "export".into();
                        request["layer"] = "overrides".into();
                    }
                    _ => return Err("unknown command; use oxibar --help".into()),
                }
                let response = send_request(&client_socket_path(), &request)?;
                if response["ok"].as_bool() != Some(true) {
                    return Err(response["error"]
                        .as_str()
                        .unwrap_or("command failed")
                        .to_owned()
                        .into());
                }
                if request["action"] == "export" {
                    print!("{}", response["data"].as_str().unwrap_or_default());
                } else {
                    println!("{}", serde_json::to_string_pretty(&response)?);
                }
            }
        }
    }
    Ok(true)
}

pub fn send_request(path: &Path, request: &serde_json::Value) -> io::Result<serde_json::Value> {
    let mut socket = UnixStream::connect(path)?;
    socket.set_read_timeout(Some(if request["command"] == "action" {
        ACTION_TIMEOUT + COMMAND_TIMEOUT
    } else {
        COMMAND_TIMEOUT
    }))?;
    socket.set_write_timeout(Some(COMMAND_TIMEOUT))?;
    let bytes = serde_json::to_vec(request)?;
    if bytes.len() as u64 > MAX_COMMAND_BYTES {
        return Err(io::Error::other("Request too large"));
    }
    socket.write_all(&bytes)?;
    socket.shutdown(Shutdown::Write)?;
    let mut response = String::new();
    socket
        .take(MAX_RESPONSE_BYTES)
        .read_to_string(&mut response)?;
    serde_json::from_str(&response).map_err(io::Error::other)
}

#[cfg(test)]
fn send_command(path: &Path, command: LauncherCommand) -> io::Result<()> {
    let mut socket = UnixStream::connect(path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "cannot reach oxibar at {}: {error}; start oxibar first",
                path.display()
            ),
        )
    })?;
    socket.set_read_timeout(Some(COMMAND_TIMEOUT))?;
    socket.set_write_timeout(Some(COMMAND_TIMEOUT))?;
    socket.write_all(command.wire().as_bytes())?;
    socket.shutdown(Shutdown::Write)?;
    let mut response = String::new();
    socket
        .take(MAX_COMMAND_BYTES)
        .read_to_string(&mut response)?;
    if response.trim() == "ok" {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "oxibar rejected command: {}",
            response.trim()
        )))
    }
}

struct SocketGuard(PathBuf, Option<PathBuf>);

impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
        if let Some(record) = self.1.as_ref() {
            use std::os::unix::ffi::OsStrExt;
            if fs::read(record).is_ok_and(|bytes| bytes == self.0.as_os_str().as_bytes()) {
                let _ = fs::remove_file(record);
            }
        }
    }
}

fn bind(path: &Path) -> io::Result<(UnixListener, SocketGuard)> {
    use std::os::unix::fs::FileTypeExt;

    if let Ok(metadata) = fs::symlink_metadata(path) {
        if !metadata.file_type().is_socket() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "IPC path is not a socket",
            ));
        }
        match UnixStream::connect(path) {
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    "IPC socket is active",
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => {
                fs::remove_file(path)?
            }
            Err(error) => return Err(error),
        }
    }
    let listener = UnixListener::bind(path)?;
    let guard = SocketGuard(path.to_owned(), None);
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok((listener, guard))
}

#[cfg(test)]
pub(crate) fn listen(path: &Path) -> impl iced::futures::Stream<Item = Message> + use<> {
    serve(path.to_owned(), None)
}

pub(crate) fn listen_endpoint(
    endpoint: &Endpoint,
) -> impl iced::futures::Stream<Item = Message> + use<> {
    serve(endpoint.path.clone(), Some(endpoint.record.clone()))
}

fn serve(path: PathBuf, record: Option<PathBuf>) -> impl iced::futures::Stream<Item = Message> {
    stream::channel(8, async move |output| {
        let (listener, mut guard) = match bind(&path) {
            Ok(bound) => bound,
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "could not start IPC listener");
                return;
            }
        };
        if let Some(record) = record {
            use std::os::unix::ffi::OsStrExt;
            use std::os::unix::fs::OpenOptionsExt;
            let temp = record.with_extension(format!("{}.tmp", std::process::id()));
            let result = (|| -> io::Result<()> {
                let mut file = fs::OpenOptions::new()
                    .create(true)
                    .truncate(true)
                    .write(true)
                    .mode(0o600)
                    .open(&temp)?;
                file.write_all(path.as_os_str().as_bytes())?;
                fs::rename(&temp, &record)
            })();
            if result.is_ok() {
                guard.1 = Some(record);
            } else {
                tracing::warn!(error = ?result.err(), "could not record active control endpoint");
            }
        }
        let mut handlers = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                accepted = listener.accept(), if handlers.len() < 32 => {
                    let Ok((socket, _)) = accepted else { break; };
                    handlers.spawn(handle_connection(socket, output.clone()));
                }
                _ = handlers.join_next(), if !handlers.is_empty() => {}
            }
        }
    })
}

async fn handle_connection(
    mut socket: tokio::net::UnixStream,
    mut output: iced::futures::channel::mpsc::Sender<Message>,
) {
    let mut request = String::new();
    let read = tokio::time::timeout(
        COMMAND_TIMEOUT,
        (&mut socket)
            .take(MAX_COMMAND_BYTES + 1)
            .read_to_string(&mut request),
    )
    .await;
    let response = if !matches!(read, Ok(Ok(bytes)) if bytes <= MAX_COMMAND_BYTES as usize) {
        response(
            0,
            serde_json::Value::Null,
            Some("Invalid or oversized request"),
        )
        .to_string()
    } else if let Some(command) = LauncherCommand::parse(&request) {
        if output
            .send(Message::ControlLauncher(command))
            .await
            .is_err()
        {
            return;
        }
        "ok\n".to_owned()
    } else {
        match parse_request(&request) {
            Err(error) => response(0, serde_json::Value::Null, Some(&error)).to_string(),
            Ok(command) => {
                let timeout = if matches!(command, Command::Action { .. }) {
                    ACTION_TIMEOUT
                } else {
                    COMMAND_TIMEOUT
                };
                let (reply, receiver) = Reply::channel();
                if output
                    .send(Message::ControlRequest(ControlRequest { command, reply }))
                    .await
                    .is_err()
                {
                    return;
                }
                match tokio::time::timeout(timeout, receiver).await {
                    Ok(Ok(response)) => response.to_string(),
                    _ => {
                        response(0, serde_json::Value::Null, Some("Command timed out")).to_string()
                    }
                }
            }
        }
    };
    let _ = tokio::time::timeout(COMMAND_TIMEOUT, socket.write_all(response.as_bytes())).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_center_commands_share_the_versioned_window_protocol() {
        assert!(matches!(
            parse_request(r#"{"version":1,"command":"control-center","action":"open"}"#).unwrap(),
            Command::ControlCenter(WindowCommand::Open)
        ));
        assert!(
            parse_request(r#"{"version":1,"command":"control-center","action":"select"}"#).is_err()
        );
        for (action, expected) in [
            ("open", WindowCommand::Open),
            ("close", WindowCommand::Close),
            ("toggle", WindowCommand::Toggle),
        ] {
            let request = serde_json::json!({"version":1,"command":"session","action":action});
            assert!(
                matches!(parse_request(&request.to_string()).unwrap(), Command::Session(command) if command == expected)
            );
        }
        assert!(parse_request(r#"{"version":1,"command":"session","action":"logout"}"#).is_err());
    }

    #[test]
    fn resident_actions_validate_arguments_and_do_not_block_fast_requests() {
        assert!(matches!(parse_request(r#"{"version":1,"command":"action","provider":"Audio","action":"volume","args":["+5"]}"#).unwrap(), Command::Action { .. }));
        assert!(parse_request(r#"{"version":1,"command":"action","provider":"Audio","action":"volume","args":[1]}"#).is_err());
        assert!(matches!(
            parse_request(r#"{"version":1,"command":"osd","action":"close"}"#).unwrap(),
            Command::CloseOsds
        ));
        let path =
            std::env::temp_dir().join(format!("oxibar-ipc-concurrent-{}.sock", std::process::id()));
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            use iced::futures::StreamExt;
            let mut commands = Box::pin(listen(&path));
            let client_path = path.clone();
            let action = tokio::task::spawn_blocking(move || {
                for _ in 0..100 { if UnixStream::connect(&client_path).is_ok() { break; } std::thread::sleep(Duration::from_millis(5)); }
                send_request(&client_path, &serde_json::json!({"version":1,"command":"action","provider":"Audio","action":"volume","args":["+5"]}))
            });
            let Message::ControlRequest(pending) = commands.next().await.unwrap() else { panic!("action request"); };
            assert!(matches!(pending.command, Command::Action { .. }));
            let client_path = path.clone();
            let status = tokio::task::spawn_blocking(move || send_request(&client_path, &serde_json::json!({"version":1,"command":"status"})).unwrap());
            let Message::ControlRequest(fast) = tokio::time::timeout(Duration::from_millis(500), commands.next()).await.unwrap().unwrap() else { panic!("fast status request"); };
            assert!(matches!(fast.command, Command::Status));
            fast.reply.send(response(3, serde_json::json!({"ready":true}), None));
            assert_eq!(status.await.unwrap()["ok"], true);
            drop(commands);
            assert!(!path.exists());
            assert!(tokio::time::timeout(Duration::from_secs(1), action).await.unwrap().unwrap().is_err());
        });
    }

    #[test]
    fn typed_commands_validate_version_revision_and_toml_literals() {
        assert!(
            matches!(parse_request(r#"{"version":1,"command":"config","action":"set","path":"bar.height","value":"38","revision":2}"#).unwrap(), Command::Config(ConfigCommand::Change(crate::config::Mutation::Set(path, toml::Value::Integer(38)), Some(2))) if path == "bar.height")
        );
        assert!(parse_request(r#"{"version":2,"command":"status"}"#).is_err());
        assert!(
            parse_request(r#"{"version":1,"command":"config","action":"export","layer":"wrong"}"#)
                .is_err()
        );
        assert!(parse_value("31\nunrelated = true").is_err());
        assert_eq!(
            parse_value("['clock', 'settings']")
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn structured_socket_reply_reports_actual_command_result() {
        let path =
            std::env::temp_dir().join(format!("oxibar-ipc-json-test-{}.sock", std::process::id()));
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            use iced::futures::StreamExt;
            let mut commands = Box::pin(listen(&path));
            let client_path = path.clone();
            let client = tokio::task::spawn_blocking(move || {
                for _ in 0..100 { if UnixStream::connect(&client_path).is_ok() { break; } std::thread::sleep(Duration::from_millis(5)); }
                let response = send_request(&client_path, &serde_json::json!({"version": 1, "command": "status"})).unwrap();
                assert_eq!(response["revision"], 7);
                assert_eq!(response["data"]["ready"], true);
            });
            let Message::ControlRequest(request) = commands.next().await.unwrap() else { panic!("expected structured request") };
            request.reply.send(response(7, serde_json::json!({"ready": true}), None));
            tokio::select! { result = client => result.unwrap(), _ = commands.next() => panic!("unexpected command") }
            drop(commands);
            assert!(!path.exists());
        });
    }

    #[test]
    fn accepts_only_launcher_commands() {
        assert_eq!(
            LauncherCommand::parse("launcher\n"),
            Some(LauncherCommand::Toggle)
        );
        assert_eq!(
            LauncherCommand::parse("launcher open"),
            Some(LauncherCommand::Open)
        );
        assert_eq!(
            LauncherCommand::parse("launcher close"),
            Some(LauncherCommand::Close)
        );
        assert_eq!(
            LauncherCommand::parse("launcher open\nlauncher close"),
            None
        );
    }

    #[test]
    fn socket_config_overrides_runtime_default() {
        let table = toml::from_str("[ipc]\nsocket_path = '/tmp/custom-oxibar.sock'").unwrap();
        assert_eq!(
            socket_path(&table),
            PathBuf::from("/tmp/custom-oxibar.sock")
        );
    }

    #[test]
    fn socket_roundtrip_and_stale_socket_recovery() {
        let path =
            std::env::temp_dir().join(format!("oxibar-ipc-test-{}.sock", std::process::id()));
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            use iced::futures::StreamExt;
            let stale = std::os::unix::net::UnixListener::bind(&path).unwrap();
            drop(stale);
            let mut commands = Box::pin(listen(&path));
            let client_path = path.clone();
            let client = tokio::task::spawn_blocking(move || {
                // Polling the stream below starts the listener.
                for _ in 0..100 {
                    if UnixStream::connect(&client_path).is_ok() {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                send_command(&client_path, LauncherCommand::Open).unwrap();
            });
            let message = commands.next().await.unwrap();
            assert!(matches!(
                message,
                Message::ControlLauncher(LauncherCommand::Open)
            ));
            // Keep polling so the stream can finish writing the response.
            tokio::select! {
                result = client => result.unwrap(),
                _ = commands.next() => panic!("unexpected second command"),
            }
            drop(commands);
            assert!(!path.exists());
        });
    }
}
