use super::*;
use crate::flow_tests::{model, pair, prompt, request, response};
use iced::{
    Font, Pixels, Point, Size, advanced::clipboard, event::Status, keyboard::key::Named, mouse,
};
use iced_runtime::{UserInterface, user_interface};
use serde_json::json;
use std::{thread, time::Instant};

fn greeter(stream: std::os::unix::net::UnixStream) -> Greeter {
    Greeter::new(
        model(stream),
        Some("fixture-user".into()),
        Arc::new(AtomicBool::new(false)),
        Visuals::default(),
        KeyboardStatus::default(),
    )
}

fn pump(greeter: &mut Greeter, condition: impl Fn(&Greeter) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        let _ = greeter.update(Message::Tick);
        if condition(greeter) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "private greeter fixture timed out"
        );
        thread::sleep(Duration::from_millis(1));
    }
}

fn key(
    greeter: &mut Greeter,
    key: keyboard::Key,
    modified_key: keyboard::Key,
    modifiers: keyboard::Modifiers,
    text: Option<&str>,
    status: Status,
) {
    let message = Message::Key(
        keyboard::Event::KeyPressed {
            key,
            modified_key,
            physical_key: keyboard::key::Physical::Unidentified(
                keyboard::key::NativeCode::Unidentified,
            ),
            location: keyboard::Location::Standard,
            modifiers,
            text: text.map(Into::into),
            repeat: false,
        },
        status,
        greeter.editing_user && greeter.power.is_none(),
    );
    assert_eq!(format!("{message:?}"), "Keyboard input [REDACTED]");
    let _ = greeter.update(message);
}

fn named(greeter: &mut Greeter, key: Named, modifiers: keyboard::Modifiers) {
    let status = if greeter.editing_user {
        Status::Ignored
    } else {
        Status::Captured
    };
    key_event(greeter, keyboard::Key::Named(key), modifiers, status);
}

fn key_event(
    greeter: &mut Greeter,
    key_value: keyboard::Key,
    modifiers: keyboard::Modifiers,
    status: Status,
) {
    key(
        greeter,
        key_value.clone(),
        key_value,
        modifiers,
        None,
        status,
    );
}

fn type_text(greeter: &mut Greeter, text: &str, status: Status) {
    for character in text.chars() {
        key_event(
            greeter,
            keyboard::Key::Character(character.to_string().into()),
            keyboard::Modifiers::empty(),
            status,
        );
    }
}

#[test]
fn captured_keys_and_missing_text_edit_new_prompts_and_only_acknowledged_start_exits() {
    let (client, mut server) = pair();
    let (accept, wait) = std::sync::mpsc::channel();
    let worker = thread::spawn(move || {
        assert_eq!(request(&mut server)["username"], "fixture-user");
        prompt(&mut server, "visible");
        assert_eq!(request(&mut server)["response"], "Aéb");
        prompt(&mut server, "secret");
        assert_eq!(request(&mut server)["response"], "private-fixture");
        response(&mut server, json!({"type":"success"}));
        assert_eq!(request(&mut server)["type"], "start_session");
        wait.recv_timeout(Duration::from_secs(4)).unwrap();
        response(&mut server, json!({"type":"success"}));
    });
    let mut greeter = greeter(client);
    let _ = greeter.update(Message::Auth(view::Message::Submit));
    pump(&mut greeter, |greeter| {
        greeter.model.interaction.phase == AuthPhase::Prompt
    });
    assert_eq!(greeter.focus, Control::Auth(Focus::Input));
    key(
        &mut greeter,
        keyboard::Key::Character("a".into()),
        keyboard::Key::Character("A".into()),
        keyboard::Modifiers::SHIFT,
        None,
        Status::Captured,
    );
    type_text(&mut greeter, "éb", Status::Captured);
    named(&mut greeter, Named::Tab, keyboard::Modifiers::empty());
    assert_eq!(greeter.focus, Control::Auth(Focus::Submit));
    type_text(&mut greeter, "ignored", Status::Captured);
    assert_eq!(greeter.model.interaction.input.expose(), "Aéb");
    named(&mut greeter, Named::Tab, keyboard::Modifiers::SHIFT);
    assert_eq!(greeter.focus, Control::Auth(Focus::Input));
    named(&mut greeter, Named::Enter, keyboard::Modifiers::empty());
    assert!(greeter.model.interaction.input.is_empty());
    pump(&mut greeter, |greeter| {
        greeter.model.interaction.phase == AuthPhase::Prompt
    });
    assert_eq!(greeter.focus, Control::Auth(Focus::Input));
    type_text(&mut greeter, "private-fixture", Status::Captured);
    assert_eq!(greeter.model.interaction.input.masked(), "•".repeat(15));
    assert_eq!(greeter.model.username, "fixture-user");
    named(&mut greeter, Named::Enter, keyboard::Modifiers::empty());
    pump(&mut greeter, |greeter| {
        greeter.model.handoff.phase() == Phase::Starting
    });
    assert!(greeter.model.interaction.input.is_empty());
    assert!(!greeter.accepted.load(Ordering::Acquire));
    accept.send(()).unwrap();
    pump(&mut greeter, |greeter| {
        greeter.accepted.load(Ordering::Acquire)
    });
    assert_eq!(greeter.model.handoff.phase(), Phase::Handoff);
    worker.join().unwrap();
}

fn click_response(greeter: &mut Greeter) {
    let mut renderer =
        iced::Renderer::Secondary(iced_tiny_skia::Renderer::new(Font::DEFAULT, Pixels(16.0)));
    let mut ui = UserInterface::build(
        greeter.view(),
        Size::new(960.0, 960.0),
        user_interface::Cache::default(),
        &mut renderer,
    );
    let mut messages = Vec::new();
    let point = (16..944)
        .step_by(8)
        .map(|y| Point::new(480.0, y as f32))
        .find(|point| {
            let (state, _) = ui.update(
                &[iced::Event::Mouse(mouse::Event::CursorMoved {
                    position: *point,
                })],
                mouse::Cursor::Available(*point),
                &mut renderer,
                &mut clipboard::Null,
                &mut messages,
            );
            matches!(
                state,
                user_interface::State::Updated {
                    mouse_interaction: mouse::Interaction::Text,
                    ..
                }
            )
        })
        .expect("masked response field exposes text-cursor interaction");
    ui.update(
        &[
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        ],
        mouse::Cursor::Available(point),
        &mut renderer,
        &mut clipboard::Null,
        &mut messages,
    );
    drop(ui);
    assert!(
        messages
            .iter()
            .any(|message| matches!(message, Message::Auth(view::Message::Focus(Focus::Input))))
    );
    for message in messages {
        let _ = greeter.update(message);
    }
}

#[test]
fn actual_masked_field_click_and_keyboard_clear_keep_prompt_ready_and_user_change_cancels() {
    let (client, mut server) = pair();
    let worker = thread::spawn(move || {
        assert_eq!(request(&mut server)["type"], "create_session");
        prompt(&mut server, "secret");
        assert_eq!(request(&mut server)["type"], "cancel_session");
        response(&mut server, json!({"type":"success"}));
    });
    let mut greeter = greeter(client);
    let _ = greeter.update(Message::Auth(view::Message::Submit));
    pump(&mut greeter, |greeter| {
        greeter.model.interaction.phase == AuthPhase::Prompt
    });
    named(&mut greeter, Named::Tab, keyboard::Modifiers::empty());
    assert_eq!(greeter.focus, Control::Auth(Focus::Submit));
    click_response(&mut greeter);
    assert_eq!(greeter.focus, Control::Auth(Focus::Input));
    type_text(&mut greeter, "private", Status::Captured);
    assert_eq!(greeter.model.interaction.input.masked(), "•••••••");
    named(&mut greeter, Named::Tab, keyboard::Modifiers::empty());
    named(&mut greeter, Named::Tab, keyboard::Modifiers::empty());
    assert_eq!(greeter.focus, Control::Auth(Focus::Cancel));
    named(&mut greeter, Named::Space, keyboard::Modifiers::empty());
    assert!(greeter.model.interaction.input.is_empty());
    assert_eq!(greeter.model.interaction.phase, AuthPhase::Prompt);
    assert_eq!(greeter.focus, Control::Auth(Focus::Input));
    let _ = greeter.update(Message::ChangeUser);
    pump(&mut greeter, |greeter| greeter.model.can_choose_user());
    assert_eq!(greeter.focus, Control::Username);
    assert_eq!(greeter.model.username, "fixture-user");
    assert_eq!(greeter.model.handoff.phase(), Phase::Choosing);
    assert!(!greeter.accepted.load(Ordering::Acquire));
    worker.join().unwrap();
}

#[test]
fn selected_user_gets_immediate_password_and_failed_worker_cleanup_keeps_service_connected() {
    let (client, mut server) = pair();
    let worker = thread::spawn(move || {
        assert_eq!(request(&mut server)["username"], "fixture-user");
        prompt(&mut server, "secret");
        assert_eq!(request(&mut server)["response"], "wrong-private-fixture");
        response(
            &mut server,
            json!({"type":"error", "error_type":"auth_error", "description":"Private failed authentication"}),
        );
        assert_eq!(request(&mut server)["type"], "cancel_session");
        response(
            &mut server,
            json!({"type":"error", "error_type":"error", "description":"unable to send message: Connection refused"}),
        );
        assert_eq!(request(&mut server)["username"], "fixture-user");
        prompt(&mut server, "secret");
        assert_eq!(request(&mut server)["response"], "right-private-fixture");
        response(&mut server, json!({"type":"success"}));
        assert_eq!(request(&mut server)["type"], "start_session");
        response(&mut server, json!({"type":"success"}));
    });
    let mut greeter = greeter(client);
    // Initialization, not a Continue click, starts the selected user's conversation.
    pump(&mut greeter, |greeter| {
        greeter.model.interaction.phase == AuthPhase::Prompt
    });
    assert_eq!(greeter.focus, Control::Auth(Focus::Input));
    type_text(&mut greeter, "wrong-private-fixture", Status::Captured);
    named(&mut greeter, Named::Enter, keyboard::Modifiers::empty());
    pump(&mut greeter, |greeter| {
        greeter.model.interaction.phase == AuthPhase::Prompt
    });
    assert!(greeter.model.client.is_some());
    assert!(!greeter.model.cancelling);
    assert!(greeter.model.interaction.input.is_empty());
    assert_eq!(greeter.focus, Control::Auth(Focus::Input));
    assert!(
        greeter
            .model
            .interaction
            .notice
            .contains("Private failed authentication")
    );
    type_text(&mut greeter, "right-private-fixture", Status::Captured);
    named(&mut greeter, Named::Enter, keyboard::Modifiers::empty());
    pump(&mut greeter, |greeter| {
        greeter.accepted.load(Ordering::Acquire)
    });
    worker.join().unwrap();
}

#[test]
fn full_form_tab_session_selection_and_user_change_drain_old_attempt_before_immediate_prompt() {
    let (client, mut server) = pair();
    let worker = thread::spawn(move || {
        assert_eq!(request(&mut server)["username"], "fixture-user");
        prompt(&mut server, "secret");
        assert_eq!(request(&mut server)["type"], "cancel_session");
        response(&mut server, json!({"type":"success"}));
        assert_eq!(request(&mut server)["username"], "fixture-user");
        prompt(&mut server, "secret");
        assert_eq!(request(&mut server)["type"], "cancel_session");
        response(&mut server, json!({"type":"success"}));
        assert_eq!(request(&mut server)["username"], "other-private-user");
        prompt(&mut server, "secret");
        assert_eq!(request(&mut server)["response"], "other-private-password");
        response(&mut server, json!({"type":"success"}));
        assert_eq!(
            request(&mut server)["cmd"],
            json!(["/fixture/second-compositor"])
        );
        response(&mut server, json!({"type":"success"}));
    });
    let mut greeter = greeter(client);
    let second = Session {
        id: "second".into(),
        name: "Second private session".into(),
        cmd: vec!["/fixture/second-compositor".into()],
        env: vec![],
    };
    greeter.model.sessions.push(second.clone());
    let _ = greeter.update(Message::PowerCapabilities((true, true)));
    pump(&mut greeter, |greeter| {
        greeter.model.interaction.phase == AuthPhase::Prompt
    });
    let _ = greeter.update(Message::Power(Power::PowerOff));
    pump(&mut greeter, |greeter| greeter.model.can_choose_user());
    assert_eq!(
        greeter.controls(),
        vec![Control::ConfirmPower, Control::CancelPower]
    );
    named(&mut greeter, Named::Tab, keyboard::Modifiers::empty());
    assert_eq!(greeter.focus, Control::ConfirmPower);
    named(&mut greeter, Named::Tab, keyboard::Modifiers::empty());
    assert_eq!(greeter.focus, Control::CancelPower);
    named(&mut greeter, Named::Enter, keyboard::Modifiers::empty());
    pump(&mut greeter, |greeter| {
        greeter.model.interaction.phase == AuthPhase::Prompt
    });
    for control in [
        Control::Auth(Focus::Submit),
        Control::Auth(Focus::Cancel),
        Control::PowerOff,
        Control::Reboot,
        Control::ChangeUser,
        Control::Session,
        Control::Auth(Focus::Input),
    ] {
        named(&mut greeter, Named::Tab, keyboard::Modifiers::empty());
        assert_eq!(greeter.focus, control);
        if control != Control::Auth(Focus::Input) {
            type_text(&mut greeter, "ignored", Status::Captured);
        }
        assert!(greeter.model.interaction.input.is_empty());
    }
    named(&mut greeter, Named::Tab, keyboard::Modifiers::SHIFT);
    assert_eq!(greeter.focus, Control::Session);
    named(&mut greeter, Named::ArrowDown, keyboard::Modifiers::empty());
    assert_eq!(greeter.model.selected, Some(second));
    named(&mut greeter, Named::Tab, keyboard::Modifiers::SHIFT);
    assert_eq!(greeter.focus, Control::ChangeUser);
    named(&mut greeter, Named::Enter, keyboard::Modifiers::empty());
    assert!(greeter.editing_user);
    assert_eq!(greeter.focus, Control::Username);
    assert!(greeter.model.interaction.input.is_empty());
    let _ = greeter.update(Message::Username("other-private-user".into()));
    assert_eq!(greeter.model.username, "fixture-user");
    named(&mut greeter, Named::Tab, keyboard::Modifiers::empty());
    assert_eq!(greeter.focus, Control::ChooseUser);
    named(&mut greeter, Named::Tab, keyboard::Modifiers::SHIFT);
    assert_eq!(greeter.focus, Control::Username);
    let _ = greeter.update(Message::ChooseUser);
    pump(&mut greeter, |greeter| {
        greeter.model.interaction.phase == AuthPhase::Prompt
    });
    // A native username on_submit's delayed captured Enter must not submit an empty password.
    let _ = greeter.update(Message::Key(
        keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(Named::Enter),
            modified_key: keyboard::Key::Named(Named::Enter),
            physical_key: keyboard::key::Physical::Unidentified(
                keyboard::key::NativeCode::Unidentified,
            ),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::empty(),
            text: None,
            repeat: false,
        },
        Status::Captured,
        true,
    ));
    assert_eq!(greeter.model.interaction.phase, AuthPhase::Prompt);
    assert_eq!(greeter.model.username, "other-private-user");
    type_text(&mut greeter, "other-private-password", Status::Captured);
    named(&mut greeter, Named::Enter, keyboard::Modifiers::empty());
    pump(&mut greeter, |greeter| {
        greeter.accepted.load(Ordering::Acquire)
    });
    worker.join().unwrap();
}

fn native_event(
    key: keyboard::Key,
    modifiers: keyboard::Modifiers,
    text: Option<&str>,
) -> iced::Event {
    iced::Event::Keyboard(keyboard::Event::KeyPressed {
        modified_key: key.clone(),
        key,
        physical_key: keyboard::key::Physical::Unidentified(
            keyboard::key::NativeCode::Unidentified,
        ),
        location: keyboard::Location::Standard,
        modifiers,
        text: text.map(Into::into),
        repeat: false,
    })
}

fn native_batch(greeter: &mut Greeter, events: Vec<iced::Event>) {
    let username_widget = greeter.editing_user && greeter.power.is_none();
    let mut renderer =
        iced::Renderer::Secondary(iced_tiny_skia::Renderer::new(Font::DEFAULT, Pixels(16.0)));
    let mut ui = UserInterface::build(
        greeter.view(),
        Size::new(960.0, 960.0),
        user_interface::Cache::default(),
        &mut renderer,
    );
    let mut messages = Vec::new();
    let (_, statuses) = ui.update(
        &events,
        mouse::Cursor::Unavailable,
        &mut renderer,
        &mut clipboard::Null,
        &mut messages,
    );
    drop(ui);
    for message in messages {
        let _ = greeter.update(message);
    }
    for (event, status) in events.into_iter().zip(statuses) {
        if let iced::Event::Keyboard(event) = event {
            let _ = greeter.update(Message::Key(event, status, username_widget));
        }
    }
}

#[test]
fn native_username_tab_and_submit_stop_editing_before_later_batched_keys() {
    let (client, _server) = pair();
    let mut greeter = greeter(client);
    let _ = greeter.update(Message::ChangeUser);
    let mut events = vec![
        iced::Event::Keyboard(keyboard::Event::ModifiersChanged(keyboard::Modifiers::CTRL)),
        native_event(
            keyboard::Key::Character("a".into()),
            keyboard::Modifiers::CTRL,
            None,
        ),
        iced::Event::Keyboard(keyboard::Event::ModifiersChanged(
            keyboard::Modifiers::empty(),
        )),
    ];
    events.extend("alice".chars().map(|value| {
        let value = value.to_string();
        native_event(
            keyboard::Key::Character(value.clone().into()),
            keyboard::Modifiers::empty(),
            Some(&value),
        )
    }));
    events.push(native_event(
        keyboard::Key::Named(Named::Tab),
        keyboard::Modifiers::empty(),
        None,
    ));
    events.extend("ignored".chars().map(|value| {
        let value = value.to_string();
        native_event(
            keyboard::Key::Character(value.clone().into()),
            keyboard::Modifiers::empty(),
            Some(&value),
        )
    }));
    native_batch(&mut greeter, events);
    assert_eq!(greeter.username_draft, "alice");
    assert_eq!(greeter.focus, Control::ChooseUser);
    assert_eq!(greeter.model.username, "fixture-user");
    named(&mut greeter, Named::Tab, keyboard::Modifiers::SHIFT);
    assert_eq!(greeter.focus, Control::Username);
    let mut events = vec![native_event(
        keyboard::Key::Named(Named::Enter),
        keyboard::Modifiers::empty(),
        None,
    )];
    events.extend("not-a-username".chars().map(|value| {
        let value = value.to_string();
        native_event(
            keyboard::Key::Character(value.clone().into()),
            keyboard::Modifiers::empty(),
            Some(&value),
        )
    }));
    native_batch(&mut greeter, events);
    assert!(!greeter.editing_user);
    assert_eq!(greeter.model.username, "alice");
    assert_eq!(greeter.username_draft, "alice");
    assert!(greeter.model.interaction.input.is_empty());
    assert!(!greeter.accepted.load(Ordering::Acquire));
}
