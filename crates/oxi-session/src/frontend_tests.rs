use super::*;
use crate::flow_tests::{model, pair, prompt, request, response};
use iced::{
    Font, Pixels, Point, Size, advanced::clipboard, event::Status, keyboard::key::Named, mouse,
};
use iced_runtime::{UserInterface, user_interface};
use serde_json::json;
use std::{thread, time::Instant};

fn greeter(stream: std::os::unix::net::UnixStream) -> Greeter {
    Greeter {
        model: model(stream),
        accepted: Arc::new(AtomicBool::new(false)),
        visuals: Visuals::default(),
        keyboard: KeyboardStatus::default(),
        focus: None,
        power: None,
        power_busy: false,
        power_off: false,
        reboot: false,
    }
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
    );
    assert_eq!(format!("{message:?}"), "Keyboard input [REDACTED]");
    let _ = greeter.update(message);
}

fn named(greeter: &mut Greeter, key: Named, modifiers: keyboard::Modifiers) {
    key_event(
        greeter,
        keyboard::Key::Named(key),
        modifiers,
        Status::Captured,
    );
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
    assert_eq!(greeter.focus, Some(Focus::Input));
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
    assert_eq!(greeter.focus, Some(Focus::Submit));
    type_text(&mut greeter, "ignored", Status::Captured);
    assert_eq!(greeter.model.interaction.input.expose(), "Aéb");
    named(&mut greeter, Named::Tab, keyboard::Modifiers::SHIFT);
    assert_eq!(greeter.focus, Some(Focus::Input));
    named(&mut greeter, Named::Enter, keyboard::Modifiers::empty());
    assert!(greeter.model.interaction.input.is_empty());
    pump(&mut greeter, |greeter| {
        greeter.model.interaction.phase == AuthPhase::Prompt
    });
    assert_eq!(greeter.focus, Some(Focus::Input));
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
fn actual_masked_field_click_focuses_input_and_tab_clear_cancels_without_handoff() {
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
    assert_eq!(greeter.focus, Some(Focus::Submit));
    click_response(&mut greeter);
    assert_eq!(greeter.focus, Some(Focus::Input));
    type_text(&mut greeter, "private", Status::Captured);
    assert_eq!(greeter.model.interaction.input.masked(), "•••••••");
    named(&mut greeter, Named::Tab, keyboard::Modifiers::empty());
    named(&mut greeter, Named::Tab, keyboard::Modifiers::empty());
    assert_eq!(greeter.focus, Some(Focus::Cancel));
    named(&mut greeter, Named::Space, keyboard::Modifiers::empty());
    assert!(greeter.model.interaction.input.is_empty());
    pump(&mut greeter, |greeter| greeter.model.can_choose_user());
    assert_eq!(greeter.focus, None);
    assert_eq!(greeter.model.username, "fixture-user");
    assert_eq!(greeter.model.handoff.phase(), Phase::Choosing);
    assert!(!greeter.accepted.load(Ordering::Acquire));
    worker.join().unwrap();
}
