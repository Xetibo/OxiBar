use iced::{
    Point,
    event::Status,
    keyboard::{Key, Modifiers, key::Named},
};
use iced_layershell::reexport::{IcedId, KeyboardInteractivity, Layer};
use oxibar::{
    control_center,
    ipc::{Command, WindowCommand},
    launcher,
    messages::Message,
    monitor::{Output, OutputSnapshot},
    settings,
    test_support::{FakePluginOptions, TestHost, record},
};
use toml::Value;

fn set(host: &mut TestHost, path: &str, value: Value) -> Vec<Message> {
    host.drive(Message::Settings(settings::Action::Set(path.into(), value)))
}

fn host() -> TestHost {
    TestHost::with_plugins([(
        "OxiPaste",
        FakePluginOptions {
            visible_subscription: true,
            ..FakePluginOptions::popup().with_popup_size(540, 560)
        },
    )])
}

fn outputs(names: &[&str]) -> OutputSnapshot {
    OutputSnapshot {
        backend: "fixture".into(),
        outputs: names
            .iter()
            .enumerate()
            .map(|(index, name)| Output {
                name: (*name).into(),
                description: (*name).into(),
                make: String::new(),
                model: String::new(),
                serial: String::new(),
                enabled: true,
                focused: index == 0,
                physical_size: (1920, 1080),
                logical_size: (600, 400),
                position: (0, 0),
                scale: 1.0,
                transform: 0,
                refresh_hz: 60.0,
                vrr: None,
                modes: Vec::new(),
            })
            .collect(),
    }
}

#[test]
fn unavailable_provider_returns_error_without_dismissing_other_surfaces() {
    let mut host = TestHost::with_plugins([("Other", FakePluginOptions::popup())]);
    host.drive(Message::Launcher(launcher::Action::Open));
    let launcher = host.launcher_window().unwrap();
    let (_, reply) = host.request(None, Command::OxiPaste(WindowCommand::Open));
    assert_eq!(reply["ok"], false);
    assert!(reply["error"].as_str().unwrap().contains("not enabled"));
    assert_eq!(host.launcher_window(), Some(launcher));
    assert!(host.oxipaste_window().is_none());
    let (_, reply) = host.request(None, Command::OxiPaste(WindowCommand::Close));
    assert_eq!(reply["ok"], true);

    let mut host = TestHost::with_plugins([("OxiPaste", FakePluginOptions::default())]);
    let (_, reply) = host.request(None, Command::OxiPaste(WindowCommand::Toggle));
    assert_eq!(reply["ok"], false);
    assert_eq!(reply["error"], "OxiPaste provider has no clipboard view");
}

#[test]
fn picker_is_source_scoped_bounded_and_shares_one_visible_provider() {
    let mut host = host();
    set(&mut host, "bar.outputs", Value::Array(vec!["*".into()]));
    host.drive(Message::OutputsChanged(Ok(outputs(&["DP-1", "DP-2"]))));
    let bar = host.bar_windows()[1];
    host.drive_plugin_record("OxiPaste", "existing history");
    let (messages, reply) = host.request(Some(bar), Command::OxiPaste(WindowCommand::Open));
    let picker = host.oxipaste_window().unwrap();
    assert_eq!(reply["ok"], true);
    assert_eq!(reply["data"]["oxipaste_open"], true);
    assert_eq!(reply["data"]["oxipaste_output"], "DP-2");
    assert_eq!(host.surface_output(picker), Some("DP-2"));
    let open = messages
        .iter()
        .find_map(|message| match message {
            Message::OnOutput(Some(output), message) if output == "DP-2" => {
                matches!(message.as_ref(), Message::OpenOxiPasteLayer(id, (540, 360)) if *id == picker)
                    .then_some(message.as_ref().clone())
            }
            _ => None,
        })
        .unwrap();
    let native: iced_layershell::actions::LayershellCustomActionWithId = open.try_into().unwrap();
    let iced_layershell::actions::LayershellCustomAction::NewLayerShell { settings, id } = native.1
    else {
        panic!("clipboard layer action");
    };
    assert_eq!(id, picker);
    assert_eq!(settings.layer, Layer::Overlay);
    assert_eq!(
        settings.keyboard_interactivity,
        KeyboardInteractivity::Exclusive
    );
    assert!(settings.anchor.is_empty());
    assert_eq!(settings.exclusive_zone, Some(-1));
    set(&mut host, "bar.enabled", Value::Boolean(false));
    assert_eq!(host.oxipaste_window(), Some(picker));
    assert_eq!(host.subscribed_plugins(), ["OxiPaste"]);
    host.request(None, Command::OxiPaste(WindowCommand::Open));
    assert_eq!(host.oxipaste_window(), Some(picker));
    host.request(None, Command::OxiPaste(WindowCommand::Toggle));
    assert!(host.oxipaste_window().is_none());
    assert!(host.subscribed_plugins().is_empty());
    host.request(None, Command::OxiPaste(WindowCommand::Toggle));
    assert!(host.oxipaste_window().is_some());
    assert_eq!(host.seen_by_plugin("OxiPaste"), ["existing history"]);
}

#[test]
fn exclusive_surfaces_dismiss_picker_and_nested_escape_is_scoped() {
    let mut host = host();
    set(&mut host, "control_center.presentation", "window".into());
    for open_other in [
        Message::Launcher(launcher::Action::Open),
        Message::Settings(settings::Action::Open),
        Message::ControlCenter(control_center::Action::Open),
        Message::Session(oxibar::session_menu::Action::Open),
    ] {
        host.drive(open_other.clone());
        host.request(None, Command::OxiPaste(WindowCommand::Open));
        assert!(host.launcher_window().is_none());
        assert!(host.settings_window().is_none());
        assert!(host.control_center_window().is_none());
        assert!(host.session_window().is_none());
        host.drive(open_other);
        assert!(host.oxipaste_window().is_none());
    }
    host.request(None, Command::OxiPaste(WindowCommand::Open));
    let picker = host.oxipaste_window().unwrap();
    host.drive(Message::OxiPasteEscape(IcedId::unique()));
    assert_eq!(host.oxipaste_window(), Some(picker));
    host.drive(Message::ShortcutKey(
        IcedId::unique(),
        Key::Named(Named::Escape),
        Modifiers::empty(),
        Status::Captured,
    ));
    assert_eq!(host.oxipaste_window(), Some(picker));
    host.drive(Message::ShortcutKey(
        picker,
        Key::Named(Named::Escape),
        Modifiers::empty(),
        Status::Captured,
    ));
    assert_eq!(host.oxipaste_window(), Some(picker));
    host.drive(Message::ShortcutKey(
        picker,
        Key::Named(Named::Escape),
        Modifiers::empty(),
        Status::Ignored,
    ));
    assert!(host.oxipaste_window().is_none());
}

#[test]
fn outside_press_uses_registered_bounds_and_native_close_retires_only_current_picker() {
    let mut host = host();
    host.request(None, Command::OxiPaste(WindowCommand::Open));
    let picker = host.oxipaste_window().unwrap();
    host.drive(Message::OverlayPointer(
        picker,
        Some(Point::new(500.0, 500.0)),
    ));
    host.drive(Message::OverlayPress(picker));
    assert_eq!(host.oxipaste_window(), Some(picker));
    host.drive(Message::OverlayPointer(
        picker,
        Some(Point::new(541.0, 500.0)),
    ));
    host.drive(Message::OverlayPress(picker));
    assert!(host.oxipaste_window().is_none());
    host.request(None, Command::OxiPaste(WindowCommand::Open));
    let current = host.oxipaste_window().unwrap();
    host.drive(Message::SurfaceClosed(picker));
    assert_eq!(host.oxipaste_window(), Some(current));
    host.drive(Message::SurfaceClosed(current));
    assert!(host.oxipaste_window().is_none());
}

#[test]
fn output_removal_and_provider_disable_close_picker_but_retain_late_data() {
    let mut host = host();
    set(
        &mut host,
        "plugins",
        Value::Array(vec!["libfake.so".into()]),
    );
    host.drive(Message::OutputsChanged(Ok(outputs(&["DP-1"]))));
    set(&mut host, "bar.enabled", Value::Boolean(false));
    host.request(None, Command::OxiPaste(WindowCommand::Open));
    let picker = host.oxipaste_window().unwrap();
    host.drive(Message::OutputsChanged(Ok(outputs(&[]))));
    assert!(host.oxipaste_window().is_none());
    host.drive(Message::FromSurface(
        picker,
        Box::new(Message::PluginSubMsg(
            "OxiPaste".into(),
            record("retired click"),
        )),
    ));
    host.drive(Message::PluginTask(
        Some(picker),
        Box::new(Message::PluginSubMsg(
            "OxiPaste".into(),
            record("late history"),
        )),
    ));
    assert_eq!(host.seen_by_plugin("OxiPaste"), ["late history"]);
    host.drive(Message::OutputsChanged(Ok(outputs(&["DP-1"]))));
    host.request(None, Command::OxiPaste(WindowCommand::Open));
    assert!(host.oxipaste_window().is_some());
    set(&mut host, "plugins", Value::Array(vec![]));
    assert!(host.oxipaste_window().is_none());
    let (_, reply) = host.request(None, Command::OxiPaste(WindowCommand::Open));
    assert_eq!(reply["ok"], false);
    set(
        &mut host,
        "plugins",
        Value::Array(vec!["libfake.so".into()]),
    );
    host.request(None, Command::OxiPaste(WindowCommand::Open));
    assert!(host.oxipaste_window().is_some());
    assert_eq!(host.seen_by_plugin("OxiPaste"), ["late history"]);
}
