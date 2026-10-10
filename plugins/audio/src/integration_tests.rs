use std::{
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use iced::futures::{
    StreamExt,
    executor::block_on,
    future::{Either, select},
    stream::BoxStream,
};

use crate::{
    native::{self, Client},
    system::{DeviceAction, DeviceKind, DeviceSnapshot},
};

struct PrivateServer {
    directory: PathBuf,
    address: String,
    child: Option<Child>,
}

impl PrivateServer {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "oxibar-pulse-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        let address = format!("unix:{}", directory.join("pulse.sock").display());
        let mut server = Self {
            directory,
            address,
            child: None,
        };
        server.start();
        server
    }

    fn start(&mut self) {
        let _ = std::fs::remove_file(self.directory.join("pulse.sock"));
        self.child = Some(
            Command::new("pulseaudio")
                .args([
                    "-n",
                    "--daemonize=no",
                    "--exit-idle-time=-1",
                    "--use-pid-file=no",
                    "--log-target=stderr",
                ])
                .args([
                    "-L",
                    &format!(
                        "module-native-protocol-unix socket={} auth-anonymous=1",
                        self.directory.join("pulse.sock").display()
                    ),
                ])
                .args([
                    "-L",
                    "module-null-sink sink_name=oxibar_first channels=2",
                    "-L",
                    "module-null-sink sink_name=oxibar_second channels=2",
                    "-L",
                    "module-null-source source_name=oxibar_microphone channels=2",
                ])
                .env("HOME", &self.directory)
                .env("XDG_RUNTIME_DIR", &self.directory)
                .env("PULSE_RUNTIME_PATH", &self.directory)
                .env("PULSE_STATE_PATH", &self.directory)
                .env("PULSE_COOKIE", self.directory.join("cookie"))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("pulseaudio must be on PATH"),
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if self.try_pactl(&["info"]).is_ok() {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "private PulseAudio failed to start"
            );
            std::thread::sleep(Duration::from_millis(30));
        }
        self.pactl(&["set-default-sink", "oxibar_first"]);
        self.pactl(&["set-default-source", "oxibar_microphone"]);
    }

    fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    fn try_pactl(&self, args: &[&str]) -> Result<String, String> {
        let output = Command::new("pactl")
            .args(args)
            .env("PULSE_SERVER", &self.address)
            .env("LC_ALL", "C")
            .output()
            .expect("pactl must be on PATH");
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).into_owned())
        }
    }

    fn pactl(&self, args: &[&str]) -> String {
        self.try_pactl(args).unwrap()
    }
}

impl Drop for PrivateServer {
    fn drop(&mut self) {
        self.stop();
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

type Update = Result<DeviceSnapshot, String>;

fn wait_for(
    stream: &mut BoxStream<'static, Update>,
    predicate: impl Fn(&Update) -> bool,
) -> Update {
    block_on(async {
        let deadline = async_io::Timer::after(Duration::from_secs(6));
        iced::futures::pin_mut!(deadline);
        loop {
            match select(stream.next(), &mut deadline).await {
                Either::Left((Some(update), _)) if predicate(&update) => return update,
                Either::Left((Some(_), _)) => {}
                _ => panic!("native audio update did not arrive"),
            }
        }
    })
}

fn apply(client: &Client, action: DeviceAction) {
    block_on(async {
        match select(
            client.submit(action),
            Box::pin(async_io::Timer::after(Duration::from_secs(4))),
        )
        .await
        {
            Either::Left((result, _)) => result.unwrap().unwrap(),
            _ => panic!("native audio action did not complete"),
        }
    });
}

#[test]
#[ignore = "requires pulseaudio and pactl on PATH; starts only a private null-device server"]
fn private_server_events_actions_hotplug_reconnect_and_cancellation() {
    let mut server = PrivateServer::new();
    let client = Client::for_server(Some(server.address.clone())).unwrap();
    let mut stream = native::events(client.clone());
    let initial = wait_for(&mut stream, Result::is_ok).unwrap();
    assert_eq!(initial.outputs.len(), 2);
    assert_eq!(initial.inputs.len(), 1);
    assert_eq!(initial.inputs[0].name, "oxibar_microphone");
    assert_eq!(initial.default_output.as_deref(), Some("oxibar_first"));

    server.pactl(&["set-sink-volume", "oxibar_first", "30%", "60%"]);
    let _ = wait_for(&mut stream, |r| {
        r.as_ref().is_ok_and(|s| {
            s.outputs
                .iter()
                .any(|d| d.name == "oxibar_first" && d.volume == 60)
        })
    });
    apply(
        &client,
        DeviceAction::Volume(DeviceKind::Output, "oxibar_first".to_owned(), 120),
    );
    let _ = wait_for(&mut stream, |r| {
        r.as_ref().is_ok_and(|s| {
            s.outputs
                .iter()
                .any(|d| d.name == "oxibar_first" && d.volume == 120)
        })
    });
    let channels: Vec<u8> = server
        .pactl(&["get-sink-volume", "oxibar_first"])
        .split_whitespace()
        .filter_map(|word| word.strip_suffix('%')?.parse().ok())
        .collect();
    assert_eq!(channels, [60, 120]);
    apply(
        &client,
        DeviceAction::Mute(DeviceKind::Output, "oxibar_first".to_owned(), true),
    );
    let _ = wait_for(&mut stream, |r| {
        r.as_ref().is_ok_and(|s| {
            s.outputs
                .iter()
                .any(|d| d.name == "oxibar_first" && d.muted)
        })
    });
    assert!(
        server
            .pactl(&["get-sink-mute", "oxibar_first"])
            .ends_with("yes")
    );
    apply(
        &client,
        DeviceAction::Default(DeviceKind::Output, "oxibar_second".to_owned()),
    );
    let _ = wait_for(&mut stream, |r| {
        r.as_ref()
            .is_ok_and(|s| s.default_output.as_deref() == Some("oxibar_second"))
    });
    apply(
        &client,
        DeviceAction::Default(DeviceKind::Input, "oxibar_microphone".to_owned()),
    );
    apply(
        &client,
        DeviceAction::Volume(DeviceKind::Input, "oxibar_microphone".to_owned(), 42),
    );
    apply(
        &client,
        DeviceAction::Mute(DeviceKind::Input, "oxibar_microphone".to_owned(), true),
    );
    let _ = wait_for(&mut stream, |r| {
        r.as_ref()
            .is_ok_and(|s| s.inputs[0].volume == 42 && s.inputs[0].muted)
    });

    let module = server.pactl(&["load-module", "module-null-sink", "sink_name=oxibar_extra"]);
    let _ = wait_for(&mut stream, |r| {
        r.as_ref().is_ok_and(|s| s.outputs.len() == 3)
    });
    server.pactl(&["unload-module", &module]);
    let _ = wait_for(&mut stream, |r| {
        r.as_ref().is_ok_and(|s| s.outputs.len() == 2)
    });
    server.stop();
    wait_for(&mut stream, Result::is_err).unwrap_err();
    assert!(
        block_on(client.submit(DeviceAction::Volume(
            DeviceKind::Output,
            "oxibar_first".to_owned(),
            25
        )))
        .unwrap()
        .is_err()
    );
    server.start();
    let recovered = wait_for(&mut stream, Result::is_ok).unwrap();
    assert_eq!(
        recovered
            .outputs
            .iter()
            .find(|d| d.name == "oxibar_first")
            .unwrap()
            .volume,
        100
    );
    let started = Instant::now();
    drop(stream);
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "subscription drop must wake an idle native loop"
    );
}
