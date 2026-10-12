use crate::{
    Phase,
    catalog::{Catalog, Session},
    ipc::{self, MAX_MESSAGE},
    model::Model,
};
use oxi_auth_ui::{Phase as AuthPhase, PromptKind};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    thread,
    time::{Duration, Instant},
};

fn request(stream: &mut UnixStream) -> Value {
    let mut header = [0; 4];
    stream.read_exact(&mut header).unwrap();
    let length = u32::from_ne_bytes(header) as usize;
    assert!(length <= MAX_MESSAGE);
    let mut body = vec![0; length];
    stream.read_exact(&mut body).unwrap();
    serde_json::from_slice(&body).unwrap()
}
fn response(stream: &mut UnixStream, value: Value) {
    let body = serde_json::to_vec(&value).unwrap();
    let mut frame = (body.len() as u32).to_ne_bytes().to_vec();
    frame.extend(body);
    for chunk in frame.chunks(3) {
        stream.write_all(chunk).unwrap();
        thread::sleep(Duration::from_micros(100));
    }
}
fn prompt(stream: &mut UnixStream, kind: &str) {
    response(
        stream,
        json!({"type":"auth_message", "auth_message_type":kind, "auth_message":format!("Private {kind} fixture")}),
    );
}
fn model(stream: UnixStream) -> Model {
    let session = Session {
        id: "fixture".into(),
        name: "Private fixture session".into(),
        cmd: vec!["/fixture/compositor".into(), "--private".into()],
        env: vec!["XDG_SESSION_TYPE=wayland".into()],
    };
    let mut model = Model::new(
        ipc::fixture(stream),
        Catalog {
            sessions: vec![session],
            warnings: vec![],
        },
    );
    model.set_username("fixture-user".into());
    model
}
fn pump(model: &mut Model, condition: impl Fn(&Model) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        let accepted = model.poll();
        assert_eq!(accepted, model.handoff.phase() == Phase::Handoff);
        if condition(model) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "private greetd fixture timed out"
        );
        thread::sleep(Duration::from_millis(1));
    }
}
fn pair() -> (UnixStream, UnixStream) {
    let (client, server) = UnixStream::pair().unwrap();
    server
        .set_read_timeout(Some(Duration::from_secs(4)))
        .unwrap();
    server
        .set_write_timeout(Some(Duration::from_secs(4)))
        .unwrap();
    (client, server)
}

#[test]
fn partial_frames_multiple_prompts_and_start_rejection_do_not_handoff_early() {
    let (client, mut server) = pair();
    let worker = thread::spawn(move || {
        assert_eq!(
            request(&mut server),
            json!({"type":"create_session", "username":"fixture-user"})
        );
        for kind in ["info", "error"] {
            prompt(&mut server, kind);
            assert_eq!(
                request(&mut server),
                json!({"type":"post_auth_message_response", "response":null})
            );
        }
        for (kind, answer) in [
            ("visible", "fixture"),
            ("secret", "fixture-password"),
            ("secret", "123456"),
        ] {
            prompt(&mut server, kind);
            assert_eq!(
                request(&mut server),
                json!({"type":"post_auth_message_response", "response":answer})
            );
        }
        response(&mut server, json!({"type":"success"}));
        let first = request(&mut server);
        assert_eq!(
            first,
            json!({"type":"start_session", "cmd":["/fixture/compositor","--private"], "env":["XDG_SESSION_TYPE=wayland"]})
        );
        response(
            &mut server,
            json!({"type":"error", "error_type":"error", "description":"Private rejected start"}),
        );
        assert_eq!(request(&mut server), first);
        response(&mut server, json!({"type":"success"}));
    });
    let mut model = model(client);
    assert!(!model.poll());
    model.submit();
    for (kind, answer) in [
        (PromptKind::Visible, "fixture"),
        (PromptKind::Secret, "fixture-password"),
        (PromptKind::Secret, "123456"),
    ] {
        pump(&mut model, |model| {
            model.interaction.phase == AuthPhase::Prompt
        });
        assert_eq!(model.interaction.prompt.as_ref().unwrap().kind, kind);
        model.set_username("other-user".into());
        assert_eq!(model.username, "fixture-user");
        model.interaction.input.push(answer);
        model.submit();
        assert!(model.interaction.input.is_empty());
        assert!(model.busy());
        assert_ne!(model.handoff.phase(), Phase::Handoff);
    }
    pump(&mut model, |model| {
        model.handoff.phase() == Phase::Authenticated
    });
    assert_eq!(model.interaction.notice, "Private rejected start");
    assert!(model.can_choose_session());
    model.submit();
    assert_eq!(model.handoff.phase(), Phase::Starting);
    model.cancel();
    assert_eq!(model.handoff.phase(), Phase::Starting);
    pump(&mut model, |model| model.handoff.phase() == Phase::Handoff);
    worker.join().unwrap();
}

#[test]
fn cancelling_an_inflight_reply_drains_it_before_retry_and_invalidates_old_attempt() {
    let (client, mut server) = pair();
    let (release, wait) = std::sync::mpsc::channel();
    let worker = thread::spawn(move || {
        assert_eq!(request(&mut server)["type"], "create_session");
        wait.recv().unwrap();
        response(&mut server, json!({"type":"success"}));
        assert_eq!(request(&mut server), json!({"type":"cancel_session"}));
        response(&mut server, json!({"type":"success"}));
        assert_eq!(request(&mut server)["username"], "second-user");
        prompt(&mut server, "secret");
        assert_eq!(request(&mut server)["response"], "fixture-password");
        response(
            &mut server,
            json!({"type":"error", "error_type":"auth_error", "description":"Private authentication failure"}),
        );
        assert_eq!(request(&mut server)["type"], "cancel_session");
        response(&mut server, json!({"type":"success"}));
    });
    let mut model = model(client);
    model.submit();
    let old = model.interaction.attempt.unwrap();
    model.poll();
    model.interaction.input.push("discarded-private-fixture");
    model.cancel();
    assert!(model.interaction.input.is_empty());
    assert!(model.cancelling);
    model.set_username("blocked-user".into());
    assert_eq!(model.username, "fixture-user");
    release.send(()).unwrap();
    pump(&mut model, |model| !model.cancelling);
    assert_eq!(model.handoff.phase(), Phase::Choosing);
    assert!(!model.interaction.complete(old, true));
    model.set_username("second-user".into());
    model.submit();
    assert_ne!(model.interaction.attempt, Some(old));
    pump(&mut model, |model| {
        model.interaction.phase == AuthPhase::Prompt
    });
    model.interaction.input.push("fixture-password");
    model.submit();
    pump(&mut model, |model| {
        model.interaction.attempt.is_none() && !model.cancelling
    });
    assert_eq!(model.handoff.phase(), Phase::Choosing);
    assert!(model.interaction.notice.contains("failure"));
    worker.join().unwrap();
}

#[test]
fn oversized_malformed_and_truncated_frames_cannot_authenticate_or_handoff() {
    for bytes in [
        (MAX_MESSAGE as u32 + 1).to_ne_bytes().to_vec(),
        vec![0; 4],
        [3u32.to_ne_bytes().as_slice(), b"bad"].concat(),
        [40u32.to_ne_bytes().as_slice(), b"short"].concat(),
    ] {
        let (client, mut server) = pair();
        let worker = thread::spawn(move || {
            request(&mut server);
            server.write_all(&bytes).unwrap();
        });
        let mut model = model(client);
        model.submit();
        pump(&mut model, |model| model.client.is_none());
        assert_eq!(model.handoff.phase(), Phase::Choosing);
        assert!(model.interaction.input.is_empty());
        assert!(model.interaction.attempt.is_none());
        assert!(!model.poll());
        worker.join().unwrap();
    }
}
