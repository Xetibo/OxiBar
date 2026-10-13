//! Argument-based app handoff, independent of the shell's systemd control group.
use iced::futures::{
    FutureExt, StreamExt,
    executor::block_on,
    future::{Either, select},
};
use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use zbus::{
    Connection, Proxy,
    zvariant::{OwnedObjectPath, Value},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Handoff {
    UserService(String),
    Direct(u32),
}
static NEXT_UNIT: AtomicU64 = AtomicU64::new(0);

fn application_environment(
    environment: impl IntoIterator<Item = (OsString, OsString)>,
) -> BTreeMap<OsString, OsString> {
    let mut environment: BTreeMap<_, _> = environment.into_iter().collect();
    if environment
        .get(std::ffi::OsStr::new("OXIBAR_APP_ENV_SAVED"))
        .is_some_and(|value| value == "1")
    {
        for key in ["LD_LIBRARY_PATH", "FONTCONFIG_FILE", "VK_ICD_FILENAMES"] {
            let original = environment.remove(std::ffi::OsStr::new(&format!("OXIBAR_APP_{key}")));
            environment.remove(std::ffi::OsStr::new(key));
            if let Some(value) = original.filter(|value| !value.is_empty()) {
                environment.insert(key.into(), value);
            }
        }
    }
    environment.retain(|key, _| {
        !matches!(
            key.to_str(),
            Some(
                "INVOCATION_ID"
                    | "JOURNAL_STREAM"
                    | "WAYLAND_SOCKET"
                    | "NOTIFY_SOCKET"
                    | "LISTEN_PID"
                    | "LISTEN_FDS"
                    | "LISTEN_FDNAMES"
                    | "SYSTEMD_EXEC_PID"
                    | "OXIBAR_APP_ENV_SAVED"
                    | "OXIBAR_APP_LD_LIBRARY_PATH"
                    | "OXIBAR_APP_FONTCONFIG_FILE"
                    | "OXIBAR_APP_VK_ICD_FILENAMES"
            )
        )
    });
    for key in [
        "GBM_BACKEND",
        "LIBVA_DRIVER_NAME",
        "__GLX_VENDOR_LIBRARY_NAME",
    ] {
        if environment
            .get(std::ffi::OsStr::new(key))
            .is_some_and(|value| value.is_empty())
        {
            environment.remove(std::ffi::OsStr::new(key));
        }
    }
    repair_wayland_display(&mut environment);
    environment
}

fn repair_wayland_display(environment: &mut BTreeMap<std::ffi::OsString, std::ffi::OsString>) {
    let Some(runtime) = environment.get(std::ffi::OsStr::new("XDG_RUNTIME_DIR")) else {
        return;
    };
    if let Some(display) = wayland_display(
        Path::new(runtime),
        environment
            .get(std::ffi::OsStr::new("WAYLAND_DISPLAY"))
            .map(OsString::as_os_str),
    ) {
        environment.insert("WAYLAND_DISPLAY".into(), display);
    }
}

/// Resolve a live inherited display or a bounded, sorted runtime-socket fallback.
pub fn wayland_display(runtime: &Path, display: Option<&OsStr>) -> Option<OsString> {
    use std::os::unix::{fs::FileTypeExt, net::UnixStream};
    if let Some(display) = display.filter(|value| !value.is_empty()) {
        let endpoint = runtime.join(display);
        if UnixStream::connect(endpoint).is_ok() {
            return Some(display.to_owned());
        }
    }
    let entries = std::fs::read_dir(runtime).ok()?;
    let mut sockets: Vec<_> = entries
        .take(4096)
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            (name
                .to_str()
                .is_some_and(|name| name.starts_with("wayland-") && !name.ends_with(".lock"))
                && entry.file_type().is_ok_and(|kind| kind.is_socket()))
            .then_some((name, entry.path()))
        })
        .collect();
    sockets.sort_by(|a, b| a.0.cmp(&b.0));
    sockets
        .into_iter()
        .take(64)
        .find(|(_, path)| UnixStream::connect(path).is_ok())
        .map(|(name, _)| name)
}

fn unset_environment(
    environment: &BTreeMap<std::ffi::OsString, std::ffi::OsString>,
) -> Vec<String> {
    [
        "INVOCATION_ID",
        "JOURNAL_STREAM",
        "WAYLAND_SOCKET",
        "NOTIFY_SOCKET",
        "LISTEN_PID",
        "LISTEN_FDS",
        "LISTEN_FDNAMES",
        "SYSTEMD_EXEC_PID",
        "OXIBAR_APP_ENV_SAVED",
        "OXIBAR_APP_LD_LIBRARY_PATH",
        "OXIBAR_APP_FONTCONFIG_FILE",
        "OXIBAR_APP_VK_ICD_FILENAMES",
        "LD_LIBRARY_PATH",
        "FONTCONFIG_FILE",
        "VK_ICD_FILENAMES",
        "GBM_BACKEND",
        "LIBVA_DRIVER_NAME",
        "__GLX_VENDOR_LIBRARY_NAME",
    ]
    .into_iter()
    .filter(|key| !environment.contains_key(std::ffi::OsStr::new(key)))
    .map(str::to_owned)
    .collect()
}

/// Call from a worker, never from rendering. User-systemd is preferred; a host
/// without that manager gets an independently grouped, reaped direct child.
pub fn launch(arguments: Vec<String>, directory: Option<PathBuf>) -> Result<Handoff, String> {
    validate(&arguments, directory.as_deref())?;
    let executable = resolve(&arguments[0], directory.as_deref())?;
    block_on(async {
        let connection = bounded(
            async {
                zbus::connection::Builder::session()
                    .map_err(|error| error.to_string())?
                    .method_timeout(Duration::from_secs(3))
                    .build()
                    .await
                    .map_err(|error| error.to_string())
            },
            Duration::from_secs(1),
        )
        .await;
        match connection {
            Ok(connection) => {
                match handoff(&connection, &executable, &arguments, directory.as_deref()).await {
                    Ok(Some(unit)) => Ok(Handoff::UserService(unit)),
                    Ok(None) => direct(arguments, directory),
                    Err(error) => Err(error),
                }
            }
            Err(_) => direct(arguments, directory),
        }
    })
}

fn validate(arguments: &[String], directory: Option<&Path>) -> Result<(), String> {
    if arguments.is_empty()
        || arguments.len() > 256
        || arguments[0].is_empty()
        || arguments
            .iter()
            .any(|value| value.len() > 16_384 || value.contains('\0'))
        || arguments.iter().map(String::len).sum::<usize>() > 524_288
    {
        return Err("invalid application argument list".into());
    }
    if directory.is_some_and(|path| !path.is_dir()) {
        return Err("application working directory is unavailable".into());
    }
    Ok(())
}

fn resolve(program: &str, directory: Option<&Path>) -> Result<PathBuf, String> {
    let valid = |path: &Path| {
        path.metadata()
            .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
    };
    let path = Path::new(program);
    let relative_to_workdir = |path: &Path| {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            directory.unwrap_or_else(|| Path::new(".")).join(path)
        }
    };
    let candidate = if path.is_absolute() || program.contains('/') {
        let path = relative_to_workdir(path);
        valid(&path).then_some(path)
    } else {
        std::env::var_os("PATH").and_then(|paths| {
            std::env::split_paths(&paths)
                .map(|directory| relative_to_workdir(&directory.join(path)))
                .find(|path| valid(path))
        })
    }
    .ok_or_else(|| format!("application executable is unavailable: {program}"))?;
    candidate.canonicalize().map_err(|error| error.to_string())
}

async fn bounded<T>(
    future: impl std::future::Future<Output = Result<T, String>>,
    timeout: Duration,
) -> Result<T, String> {
    match select(
        future.boxed_local(),
        FutureExt::boxed_local(async_io::Timer::after(timeout)),
    )
    .await
    {
        Either::Left((result, _)) => result,
        Either::Right(_) => Err("application handoff timed out".into()),
    }
}

async fn handoff(
    connection: &Connection,
    executable: &Path,
    arguments: &[String],
    directory: Option<&Path>,
) -> Result<Option<String>, String> {
    let Some(executable) = executable.to_str() else {
        return Ok(None);
    };
    let directory = directory
        .map(Path::canonicalize)
        .transpose()
        .map_err(|error| error.to_string())?;
    if directory
        .as_ref()
        .is_some_and(|path| path.to_str().is_none())
    {
        return Ok(None);
    }
    let manager = Proxy::new(
        connection,
        "org.freedesktop.systemd1",
        "/org/freedesktop/systemd1",
        "org.freedesktop.systemd1.Manager",
    )
    .await
    .map_err(|error| error.to_string())?;
    let mut jobs = match manager.receive_signal("JobRemoved").await {
        Ok(stream) => stream,
        Err(_) => return Ok(None),
    };
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let unit = format!(
        "oxibar-app-{}-{nonce}-{}.service",
        std::process::id(),
        NEXT_UNIT.fetch_add(1, Ordering::Relaxed)
    );
    let environment = application_environment(std::env::vars_os());
    let unset = unset_environment(&environment);
    let environment: Vec<String> = environment
        .into_iter()
        .filter_map(|(key, value)| Some((key.into_string().ok()?, value.into_string().ok()?)))
        .map(|(key, value)| format!("{key}={value}"))
        .collect();
    let mut properties = vec![
        ("Description", Value::from("Application launched by Oxibar")),
        ("Type", Value::from("exec")),
        ("Slice", Value::from("app.slice")),
        ("CollectMode", Value::from("inactive-or-failed")),
        ("StandardInput", Value::from("null")),
        ("StandardOutput", Value::from("null")),
        ("StandardError", Value::from("journal")),
        ("Environment", Value::from(environment)),
        ("UnsetEnvironment", Value::from(unset)),
        (
            "ExecStart",
            Value::from(vec![(executable.to_owned(), arguments.to_vec(), false)]),
        ),
    ];
    if let Some(directory) = directory {
        properties.push((
            "WorkingDirectory",
            Value::from(directory.to_str().expect("checked UTF-8").to_owned()),
        ));
    }
    let auxiliaries: Vec<(String, Vec<(String, Value<'_>)>)> = Vec::new();
    let job: OwnedObjectPath = match manager
        .call(
            "StartTransientUnit",
            &(unit.as_str(), "fail", properties, auxiliaries),
        )
        .await
    {
        Ok(job) => job,
        Err(zbus::Error::MethodError(name, _, _))
            if matches!(
                name.as_str(),
                "org.freedesktop.DBus.Error.ServiceUnknown"
                    | "org.freedesktop.DBus.Error.NameHasNoOwner"
                    | "org.freedesktop.DBus.Error.UnknownMethod"
            ) =>
        {
            return Ok(None);
        }
        Err(error) => return Err(format!("application service submission failed: {error}")),
    };
    // Once accepted, never spawn a fallback: that could execute the same app twice.
    bounded(
        async {
            while let Some(message) = jobs.next().await {
                let Ok((_, path, name, result)) =
                    message
                        .body()
                        .deserialize::<(u32, OwnedObjectPath, String, String)>()
                else {
                    continue;
                };
                if path == job && name == unit {
                    return if result == "done" {
                        Ok(Some(unit))
                    } else {
                        Err(format!("application service failed: {result}"))
                    };
                }
            }
            Err("application service completion stream stopped".into())
        },
        Duration::from_secs(4),
    )
    .await
}

/// Direct fallback for hosts without a user manager and hermetic executable tests.
pub fn direct(arguments: Vec<String>, directory: Option<PathBuf>) -> Result<Handoff, String> {
    validate(&arguments, directory.as_deref())?;
    let mut command = Command::new(&arguments[0]);
    command
        .args(&arguments[1..])
        .env_clear()
        .envs(application_environment(std::env::vars_os()))
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    if let Some(directory) = directory {
        command.current_dir(directory);
    }
    let child = command.spawn().map_err(|error| error.to_string())?;
    let pid = child.id();
    let child = Arc::new(Mutex::new(Some(child)));
    let reaper = child.clone();
    if let Err(error) = std::thread::Builder::new()
        .name("oxibar-app-reaper".into())
        .spawn(move || {
            if let Some(mut child) = reaper
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take()
            {
                let _ = child.wait();
            }
        })
    {
        if let Some(mut child) = child
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
        {
            let _ = child.kill();
            let _ = child.wait();
        }
        return Err(error.to_string());
    }
    Ok(Handoff::Direct(pid))
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::{interface, object_server::SignalEmitter, zvariant::OwnedValue};

    struct Manager {
        children: Arc<Mutex<Vec<std::process::Child>>>,
        fail_reply: Arc<std::sync::atomic::AtomicBool>,
    }
    #[interface(name = "org.freedesktop.systemd1.Manager")]
    impl Manager {
        async fn start_transient_unit(
            &self,
            name: &str,
            mode: &str,
            properties: Vec<(String, OwnedValue)>,
            _auxiliaries: Vec<(String, Vec<(String, OwnedValue)>)>,
            #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        ) -> zbus::fdo::Result<OwnedObjectPath> {
            assert_eq!(mode, "fail");
            let mut properties: std::collections::HashMap<_, _> = properties.into_iter().collect();
            for (field, expected) in [
                ("Type", "exec"),
                ("Slice", "app.slice"),
                ("CollectMode", "inactive-or-failed"),
            ] {
                assert_eq!(
                    String::try_from(properties.remove(field).unwrap()).unwrap(),
                    expected
                );
            }
            let entries: Vec<(String, Vec<String>, bool)> =
                properties.remove("ExecStart").unwrap().try_into().unwrap();
            let (executable, arguments, ignore_failure) = &entries[0];
            assert!(!ignore_failure);
            let environment: Vec<String> = properties
                .remove("Environment")
                .unwrap()
                .try_into()
                .unwrap();
            assert!(
                environment
                    .iter()
                    .all(|entry| !entry.starts_with("INVOCATION_ID=")
                        && !entry.starts_with("NOTIFY_SOCKET="))
            );
            let mut command = Command::new(executable);
            command
                .env("WAYLAND_SOCKET", "99")
                .env("GBM_BACKEND", "stale-manager-backend");
            command
                .args(&arguments[1..])
                .process_group(0)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            if let Some(directory) = properties.remove("WorkingDirectory") {
                command.current_dir(String::try_from(directory).unwrap());
            }
            for entry in environment {
                if let Some((key, value)) = entry.split_once('=') {
                    command.env(key, value);
                }
            }
            let unset: Vec<String> = properties
                .remove("UnsetEnvironment")
                .unwrap()
                .try_into()
                .unwrap();
            assert!(unset.iter().any(|key| key == "WAYLAND_SOCKET"));
            for key in unset {
                command.env_remove(key);
            }
            self.children.lock().unwrap().push(
                command
                    .spawn()
                    .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))?,
            );
            if self.fail_reply.load(Ordering::Relaxed) {
                return Err(zbus::fdo::Error::Failed(
                    "uncertain accepted request".into(),
                ));
            }
            let path: OwnedObjectPath = "/org/freedesktop/systemd1/job/1".try_into().unwrap();
            let signal_path = path.clone();
            let emitter = emitter.to_owned();
            let name = name.to_owned();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(20));
                block_on(Self::job_removed(&emitter, 1, &signal_path, &name, "done")).unwrap();
            });
            Ok(path)
        }
        #[zbus(signal)]
        async fn job_removed(
            emitter: &SignalEmitter<'_>,
            id: u32,
            job: &OwnedObjectPath,
            unit: &str,
            result: &str,
        ) -> zbus::Result<()>;
    }

    #[test]
    #[ignore = "requires an explicitly private dbus-run-session"]
    fn private_user_manager_handoff_survives_caller_exit() {
        assert_eq!(std::env::var("OXIBAR_TEST_PRIVATE_BUS").as_deref(), Ok("1"));
        let children = Arc::new(Mutex::new(Vec::new()));
        let fail_reply = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let connection = block_on(
            zbus::connection::Builder::session()
                .unwrap()
                .name("org.freedesktop.systemd1")
                .unwrap()
                .serve_at(
                    "/org/freedesktop/systemd1",
                    Manager {
                        children: children.clone(),
                        fail_reply: fail_reply.clone(),
                    },
                )
                .unwrap()
                .build(),
        )
        .unwrap();
        let root = std::env::temp_dir().join(format!(
            "oxibar-handoff-{}-{}",
            std::process::id(),
            NEXT_UNIT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let marker = root.join("alive");
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "launch::tests::private_user_service_child",
                "--ignored",
                "--nocapture",
            ])
            .env("OXIBAR_TEST_LAUNCH_CHILD", "1")
            .env("OXIBAR_TEST_LAUNCH_MARKER", &marker)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !marker.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(marker.exists());
        {
            let mut children = children.lock().unwrap();
            assert_eq!(children.len(), 1);
            assert!(
                children[0].try_wait().unwrap().is_none(),
                "application died with the caller"
            );
            children[0].kill().unwrap();
            children[0].wait().unwrap();
        }
        fail_reply.store(true, Ordering::Relaxed);
        let uncertain = root.join("uncertain");
        assert!(
            launch(
                vec![
                    "sh".into(),
                    "-c".into(),
                    format!("printf x >> '{}'", uncertain.display())
                ],
                Some(root.clone())
            )
            .is_err()
        );
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(
            std::fs::read_to_string(uncertain).unwrap(),
            "x",
            "uncertain acceptance must not execute a fallback copy"
        );
        children.lock().unwrap()[1].wait().unwrap();
        block_on(connection.close()).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "child entry point for private user-manager test"]
    fn private_user_service_child() {
        if std::env::var("OXIBAR_TEST_LAUNCH_CHILD").as_deref() != Ok("1") {
            return;
        }
        let marker = std::env::var("OXIBAR_TEST_LAUNCH_MARKER").unwrap();
        let result = launch(
            vec![
                "sh".into(),
                "-c".into(),
                format!("test -z \"${{WAYLAND_SOCKET+x}}\" || exit 1; printf alive > '{marker}'; exec sleep 60"),
            ],
            None,
        )
        .unwrap();
        assert!(matches!(result, Handoff::UserService(_)));
    }
    #[test]
    fn invalid_arguments_are_rejected_without_spawning() {
        assert!(direct(vec![], None).is_err());
        assert!(launch(vec!["bad\0argument".into()], None).is_err());
        assert!(resolve("/missing/oxibar-program", None).is_err());
    }
    #[test]
    fn application_handoff_restores_session_graphics_and_drops_shell_service_metadata() {
        let environment = application_environment(
            [
                ("OXIBAR_APP_ENV_SAVED", "1"),
                ("OXIBAR_APP_LD_LIBRARY_PATH", ""),
                ("OXIBAR_APP_FONTCONFIG_FILE", "/session/fonts.conf"),
                ("OXIBAR_APP_VK_ICD_FILENAMES", "/session/driver.json"),
                ("LD_LIBRARY_PATH", "/oxibar/libs"),
                ("FONTCONFIG_FILE", "/oxibar/fonts.conf"),
                ("VK_ICD_FILENAMES", "/oxibar/driver.json"),
                ("WAYLAND_DISPLAY", "wayland-private"),
                ("PATH", "/session/bin"),
                ("INVOCATION_ID", "shell"),
                ("WAYLAND_SOCKET", "9"),
                ("NOTIFY_SOCKET", "/private/socket"),
                ("LISTEN_FDS", "2"),
            ]
            .map(|(key, value)| (key.into(), value.into())),
        );
        assert!(!environment.contains_key(std::ffi::OsStr::new("LD_LIBRARY_PATH")));
        assert_eq!(
            environment[std::ffi::OsStr::new("FONTCONFIG_FILE")],
            "/session/fonts.conf"
        );
        assert_eq!(
            environment[std::ffi::OsStr::new("VK_ICD_FILENAMES")],
            "/session/driver.json"
        );
        assert_eq!(
            environment[std::ffi::OsStr::new("WAYLAND_DISPLAY")],
            "wayland-private"
        );
        assert_eq!(environment.len(), 4);
        let unset = unset_environment(&environment);
        assert!(unset.iter().any(|key| key == "LD_LIBRARY_PATH"));
        assert!(!unset.iter().any(|key| key == "FONTCONFIG_FILE"));
        let unwrapped = application_environment([("LD_LIBRARY_PATH".into(), "/user/libs".into())]);
        assert_eq!(
            unwrapped[std::ffi::OsStr::new("LD_LIBRARY_PATH")],
            "/user/libs"
        );
    }
    #[test]
    fn empty_or_stale_display_uses_live_socket_but_preserves_valid_explicit_display() {
        use std::os::unix::net::UnixListener;
        let root = std::env::temp_dir().join(format!(
            "oxibar-wayland-handoff-{}-{}",
            std::process::id(),
            NEXT_UNIT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let stale = UnixListener::bind(root.join("wayland-0")).unwrap();
        drop(stale);
        let _live = UnixListener::bind(root.join("wayland-1")).unwrap();
        let _explicit = UnixListener::bind(root.join("wayland-2")).unwrap();
        std::fs::write(root.join("wayland-not-a-socket"), "private fixture").unwrap();
        let build = |display: &str| {
            application_environment([
                ("XDG_RUNTIME_DIR".into(), root.as_os_str().to_owned()),
                ("WAYLAND_DISPLAY".into(), display.into()),
                ("GBM_BACKEND".into(), "".into()),
                ("WAYLAND_SOCKET".into(), "99".into()),
            ])
        };
        for display in ["", "wayland-0", "wayland-missing"] {
            let environment = build(display);
            assert_eq!(
                environment[std::ffi::OsStr::new("WAYLAND_DISPLAY")],
                "wayland-1"
            );
            assert!(!environment.contains_key(std::ffi::OsStr::new("WAYLAND_SOCKET")));
            assert!(!environment.contains_key(std::ffi::OsStr::new("GBM_BACKEND")));
        }
        assert_eq!(
            build("wayland-2")[std::ffi::OsStr::new("WAYLAND_DISPLAY")],
            "wayland-2"
        );
        assert_eq!(
            build(root.join("wayland-2").to_str().unwrap())
                [std::ffi::OsStr::new("WAYLAND_DISPLAY")],
            root.join("wayland-2").as_os_str()
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn relative_executable_uses_the_requested_working_directory() {
        let root = std::env::temp_dir().join(format!(
            "oxibar-executable-{}-{}",
            std::process::id(),
            NEXT_UNIT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let program = root.join("private-program");
        std::fs::write(&program, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(
            resolve("./private-program", Some(&root)).unwrap(),
            program.canonicalize().unwrap()
        );
        assert!(resolve("./missing", Some(&root)).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
