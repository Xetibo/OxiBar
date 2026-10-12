use iced_layershell::reexport::{IcedId, KeyboardInteractivity};
use oxibar::{
    messages::Message,
    monitor::{Output, OutputSnapshot},
    overview::{self, capture::Window},
    settings,
    test_support::{FakePluginOptions, TestHost},
};
use toml::Value;
fn outputs(names: &[&str]) -> OutputSnapshot {
    OutputSnapshot {
        backend: "Fixture".into(),
        outputs: names
            .iter()
            .enumerate()
            .map(|(i, name)| Output {
                name: (*name).into(),
                description: String::new(),
                make: String::new(),
                model: String::new(),
                serial: String::new(),
                enabled: true,
                focused: i == 0,
                physical_size: (1200, 800),
                logical_size: (1200, 800),
                position: (i as i32 * 1200, 0),
                scale: 1.,
                transform: 0,
                refresh_hz: 60.,
                vrr: None,
                modes: vec![],
            })
            .collect(),
    }
}
fn window(id: u64, output: &str) -> Window {
    Window {
        id,
        title: "Identical titles".into(),
        app_id: "same-app".into(),
        outputs: vec![output.into()],
        focused: id == 1,
        minimized: false,
        activatable: true,
        image: None,
        error: None,
        frames: 0,
    }
}
fn host() -> TestHost {
    let mut host = TestHost::with_plugins([("Clock", FakePluginOptions::popup())]);
    host.drive(Message::OutputsChanged(Ok(outputs(&["DP-1", "DP-2"]))));
    host
}
#[test]
fn per_monitor_overview_has_one_keyboard_owner_and_identity_safe_navigation() {
    let mut host = host();
    host.drive(Message::Overview(overview::Action::Open));
    let surfaces = host.overview_surfaces();
    assert_eq!(surfaces.len(), 2);
    let active = host.overview_state().1.unwrap().to_owned();
    let id = surfaces.iter().find(|(n, _)| n == &active).unwrap().1;
    host.set_overview_windows(vec![
        window(1, "DP-1"),
        window(2, "DP-1"),
        window(3, "DP-2"),
    ]);
    assert_eq!(host.overview_state().2, Some(1));
    host.drive(Message::OverviewKey(
        IcedId::unique(),
        overview::Action::Last,
    ));
    assert_eq!(host.overview_state().2, Some(1));
    host.drive(Message::OverviewKey(id, overview::Action::Move(1, 0)));
    assert_eq!(host.overview_state().2, Some(2));
    let keys = host.drive(Message::OverviewKey(id, overview::Action::Monitor(1)));
    assert_eq!(host.overview_state().1, Some("DP-2"));
    assert_eq!(host.overview_state().2, Some(3));
    assert_eq!(
        keys.iter()
            .filter(|m| matches!(
                m,
                Message::SetToastKeyboardInteractivity(_, KeyboardInteractivity::Exclusive)
            ))
            .count(),
        1
    );
    host.set_overview_windows(vec![window(4, "DP-2")]);
    assert_eq!(host.overview_state().2, Some(4));
    host.drive(Message::Overview(overview::Action::Activate(3)));
    assert!(host.overview_state().3);
    host.drive(Message::Overview(overview::Action::Close));
    assert!(!host.overview_state().3);
    host.drive(Message::FromSurface(
        id,
        Box::new(Message::Overview(overview::Action::Open)),
    ));
    assert!(!host.overview_state().3);
}

#[test]
fn modal_is_bounded_and_shortcuts_cycle_windows_then_follow_monitor_geometry() {
    use iced::{
        event::Status,
        keyboard::{Key, Modifiers, key::Named},
    };
    let mut host = host();
    let opened = host.drive(Message::Overview(overview::Action::Open));
    assert!(opened.iter().any(|message| matches!(message, Message::OnOutput(_, inner) if matches!(inner.as_ref(), Message::OpenOverviewLayer(_, (1080, 720), _)))));
    host.set_overview_windows(vec![
        window(1, "DP-1"),
        window(2, "DP-1"),
        window(3, "DP-2"),
    ]);
    let first = host
        .overview_surfaces()
        .iter()
        .find(|(name, _)| name == "DP-1")
        .unwrap()
        .1;
    host.drive(Message::ShortcutKey(
        first,
        Key::Named(Named::Tab),
        Modifiers::empty(),
        Status::Ignored,
    ));
    assert_eq!(host.overview_state().1, Some("DP-1"));
    assert_eq!(host.overview_state().2, Some(2));
    host.drive(Message::ShortcutKey(
        first,
        Key::Named(Named::Tab),
        Modifiers::empty(),
        Status::Ignored,
    ));
    assert_eq!(host.overview_state().2, Some(1));
    host.drive(Message::ShortcutKey(
        first,
        Key::Character("l".into()),
        Modifiers::empty(),
        Status::Ignored,
    ));
    assert_eq!(host.overview_state().1, Some("DP-2"));
    let second = host
        .overview_surfaces()
        .iter()
        .find(|(name, _)| name == "DP-2")
        .unwrap()
        .1;
    host.drive(Message::ShortcutKey(
        first,
        Key::Character("h".into()),
        Modifiers::empty(),
        Status::Ignored,
    ));
    assert_eq!(host.overview_state().1, Some("DP-2"));
    host.drive(Message::ShortcutKey(
        second,
        Key::Character("h".into()),
        Modifiers::empty(),
        Status::Ignored,
    ));
    assert_eq!(host.overview_state().1, Some("DP-1"));
    let mut vertical = outputs(&["DP-1", "DP-2"]);
    vertical.outputs[1].position = (0, 800);
    host.drive(Message::OutputsChanged(Ok(vertical)));
    host.drive(Message::ShortcutKey(
        first,
        Key::Character("j".into()),
        Modifiers::empty(),
        Status::Ignored,
    ));
    assert_eq!(host.overview_state().1, Some("DP-2"));
}
#[test]
fn output_retirement_config_disable_and_competing_overlays_release_capture_owner() {
    let mut host = host();
    host.drive(Message::Overview(overview::Action::Open));
    let old = host.overview_state().0;
    host.set_overview_windows(vec![window(1, "DP-1"), window(2, "DP-2")]);
    host.drive(Message::OutputsChanged(Ok(outputs(&["DP-2"]))));
    assert_eq!(host.overview_surfaces().len(), 1);
    assert_eq!(host.overview_state().1, Some("DP-2"));
    assert_eq!(host.overview_state().2, Some(2));
    host.drive(Message::Settings(settings::Action::Open));
    assert!(!host.overview_state().3);
    host.drive(Message::OverviewSnapshot(old, Default::default()));
    assert!(!host.overview_state().3);
    host.drive(Message::Overview(overview::Action::Open));
    let id = host.overview_surfaces()[0].1;
    host.drive(Message::SurfaceClosed(id));
    assert!(!host.overview_state().3);
    host.drive(Message::Overview(overview::Action::Open));
    host.drive(Message::Settings(settings::Action::Set(
        "overview.enabled".into(),
        Value::Boolean(false),
    )));
    assert!(!host.overview_state().3);
    host.drive(Message::Overview(overview::Action::Open));
    assert!(!host.overview_state().3);
    assert!(
        host.drive(Message::Settings(settings::Action::Set(
            "overview.refresh_fps".into(),
            Value::Integer(9)
        )))
        .iter()
        .any(|m| matches!(m, Message::ConfigUpdated(_)))
    );
    assert!(host.settings_error().is_some());
}

#[test]
fn activation_waits_for_every_native_surface_retirement_and_ignores_late_close() {
    let mut host = host();
    host.drive(Message::Overview(overview::Action::Open));
    host.set_overview_windows(vec![window(1, "DP-1"), window(2, "DP-2")]);
    let surfaces = host.overview_surfaces();
    let generation = host.overview_state().0;
    host.drive(Message::Overview(overview::Action::Activate(2)));
    assert!(host.overview_surfaces().is_empty());
    assert!(host.overview_state().3);
    host.drive(Message::SurfaceClosed(IcedId::unique()));
    assert!(host.overview_state().3);
    host.drive(Message::SurfaceClosed(surfaces[0].1));
    assert!(host.overview_state().3);
    host.drive(Message::SurfaceClosed(surfaces[1].1));
    assert!(!host.overview_state().3);
    host.drive(Message::Overview(overview::Action::Open));
    host.drive(Message::SurfaceClosed(surfaces[1].1));
    host.drive(Message::OverviewRetirementTimeout(generation));
    assert!(host.overview_state().3);
    host.drive(Message::Overview(overview::Action::Close));
}

#[test]
fn exclusive_pointer_redirect_uses_exact_card_bounds_on_the_other_monitor() {
    let mut host = host();
    host.drive(Message::Overview(overview::Action::Open));
    host.set_overview_windows(vec![window(1, "DP-1"), window(2, "DP-2")]);
    let surfaces = host.overview_surfaces();
    let first = surfaces.iter().find(|(name, _)| name == "DP-1").unwrap().1;
    let second = surfaces.iter().find(|(name, _)| name == "DP-2").unwrap().1;
    host.drive(Message::OverviewBounds(
        second,
        Some(2),
        iced::Rectangle {
            x: 50.0,
            y: 100.0,
            width: 200.0,
            height: 100.0,
        },
    ));
    host.drive(Message::OverviewPointer(
        first,
        Some(iced::Point::new(1275.0, 150.0)),
    ));
    host.drive(Message::OverviewPress(IcedId::unique()));
    assert_eq!(host.overview_surfaces().len(), 2);
    host.drive(Message::OverviewBounds(
        second,
        Some(2),
        iced::Rectangle::default(),
    ));
    host.drive(Message::OverviewPress(first));
    assert_eq!(host.overview_surfaces().len(), 2);
    host.drive(Message::OverviewBounds(
        second,
        Some(2),
        iced::Rectangle {
            x: 50.0,
            y: 100.0,
            width: 200.0,
            height: 100.0,
        },
    ));
    host.drive(Message::OverviewPress(first));
    assert!(host.overview_surfaces().is_empty());
    assert_eq!(host.overview_state().2, Some(2));
    host.drive(Message::SurfaceClosed(first));
    host.drive(Message::SurfaceClosed(second));
    assert!(!host.overview_state().3);
    host.drive(Message::OverviewPress(first));
    assert!(!host.overview_state().3);
}
