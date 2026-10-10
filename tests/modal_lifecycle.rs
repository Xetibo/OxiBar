use iced_layershell::reexport::IcedId;
use oxibar::{
    control_center::Action as ControlAction,
    launcher::Action as LauncherAction,
    messages::Message,
    monitor::{Output, OutputSnapshot},
    settings::Action,
    test_support::{FakePluginOptions, OPEN_MODAL, TestHost, emit_host_request, record},
};
use toml::Value;

fn options() -> FakePluginOptions {
    FakePluginOptions {
        modal: true,
        modal_closed: true,
        popup: true,
        ..FakePluginOptions::default()
    }
}

fn closures(host: &TestHost, plugin: &str) -> usize {
    host.seen_by_plugin(plugin)
        .iter()
        .filter(|entry| entry.as_str() == "modal closed")
        .count()
}

#[test]
fn outside_press_is_scoped_to_actual_overlay_bounds_and_cancels_once() {
    let mut host = TestHost::with_plugins([("Bluetooth", options())]);
    host.drive(Message::OpenPluginModal("Bluetooth".to_owned()));
    let id = host.modal_window().unwrap();
    host.drive(Message::LayerSurfaceResized(id, 100, 100));
    host.drive(Message::OverlayPointer(
        id,
        Some(iced::Point::new(90.0, 90.0)),
    ));
    host.drive(Message::OverlayPress(id));
    assert_eq!(host.modal_window(), Some(id));
    host.drive(Message::OverlayPointer(
        IcedId::unique(),
        Some(iced::Point::new(-1.0, -1.0)),
    ));
    host.drive(Message::OverlayPress(IcedId::unique()));
    assert_eq!(host.modal_window(), Some(id));
    host.drive(Message::OverlayPointer(
        id,
        Some(iced::Point::new(150.0, 10.0)),
    ));
    host.drive(Message::OverlayPress(id));
    assert!(host.modal_window().is_none());
    assert_eq!(closures(&host, "Bluetooth"), 1);
    host.drive(Message::OverlayPress(id));
    host.drive(Message::SurfaceClosed(id));
    assert_eq!(closures(&host, "Bluetooth"), 1);

    host.drive(Message::Settings(Action::Open));
    let settings = host.settings_window().unwrap();
    host.drive(Message::OverlayPointer(
        settings,
        Some(iced::Point::new(f32::NAN, -1.0)),
    ));
    host.drive(Message::OverlayPress(settings));
    assert_eq!(host.settings_window(), Some(settings));
    host.drive(Message::OverlayPointer(
        settings,
        Some(iced::Point::new(-1.0, 0.0)),
    ));
    host.drive(Message::OverlayPress(settings));
    assert!(host.settings_window().is_none());
}

#[test]
fn scoped_escape_and_late_window_close_notify_once_and_cleanup_still_updates_model() {
    let mut host = TestHost::with_plugins([("Bluetooth", options())]);
    host.drive(Message::OpenPluginModal("Bluetooth".to_owned()));
    let id = host.modal_window().unwrap();
    host.drive(Message::OpenPluginModal("Bluetooth".to_owned()));
    assert_eq!(host.modal_window(), Some(id));
    host.drive(Message::ModalEscape(IcedId::unique()));
    assert_eq!(closures(&host, "Bluetooth"), 0);
    host.drive(Message::ModalEscape(id));
    assert!(host.modal_window().is_none());
    assert_eq!(
        host.seen_by_plugin("Bluetooth"),
        ["modal closed", "modal cleanup"]
    );
    host.drive(Message::SurfaceClosed(id));
    host.drive(Message::ClosePluginModal("Bluetooth".to_owned()));
    assert_eq!(closures(&host, "Bluetooth"), 1);
    host.drive(Message::FromSurface(
        id,
        Box::new(Message::PluginSubMsg(
            "Bluetooth".to_owned(),
            record("stale UI"),
        )),
    ));
    host.drive(Message::PluginTask(
        Some(id),
        Box::new(Message::PluginSubMsg(
            "Bluetooth".to_owned(),
            record("late result"),
        )),
    ));
    assert_eq!(
        host.seen_by_plugin("Bluetooth"),
        ["modal closed", "modal cleanup", "late result"]
    );
}

#[test]
fn replacement_and_compositor_closure_notify_the_correct_plugin() {
    let mut host = TestHost::with_plugins([("Bluetooth", options()), ("Network", options())]);
    host.drive(Message::OpenPluginModal("Bluetooth".to_owned()));
    let old = host.modal_window().unwrap();
    host.drive(Message::OpenPluginModal("Network".to_owned()));
    let current = host.modal_window().unwrap();
    assert_ne!(current, old);
    assert_eq!(closures(&host, "Bluetooth"), 1);
    assert_eq!(closures(&host, "Network"), 0);
    host.drive(Message::SurfaceClosed(old));
    assert_eq!(host.modal_window(), Some(current));
    host.drive(Message::SurfaceClosed(current));
    assert_eq!(closures(&host, "Network"), 1);
    assert!(!host.modal_open());
}

#[test]
fn each_exclusive_host_overlay_closes_modal_owned_work() {
    for overlay in [
        Message::Settings(Action::Open),
        Message::Launcher(LauncherAction::Open),
        Message::ControlCenter(ControlAction::Open),
    ] {
        let mut host = TestHost::with_plugins([("Bluetooth", options())]);
        host.drive(Message::OpenPluginModal("Bluetooth".to_owned()));
        let id = host.modal_window().unwrap();
        host.drive(overlay);
        assert!(!host.modal_open());
        assert_eq!(closures(&host, "Bluetooth"), 1);
        host.drive(Message::SurfaceClosed(id));
        assert_eq!(closures(&host, "Bluetooth"), 1);
    }
}

#[test]
fn hiding_bars_preserves_modal_but_output_removal_cancels_it() {
    let output = |name: &str| Output {
        name: name.to_owned(),
        description: String::new(),
        make: String::new(),
        model: String::new(),
        serial: String::new(),
        enabled: true,
        focused: name == "DP-1",
        physical_size: (1920, 1080),
        logical_size: (1920, 1080),
        position: (0, 0),
        scale: 1.0,
        transform: 0,
        refresh_hz: 60.0,
        vrr: None,
        modes: Vec::new(),
    };
    let inventory = |outputs| OutputSnapshot {
        backend: "fixture".to_owned(),
        outputs,
    };
    let mut host = TestHost::with_plugins([("Bluetooth", options())]);
    host.drive(Message::Settings(Action::Set(
        "bar.outputs".to_owned(),
        Value::Array(vec![Value::String("*".to_owned())]),
    )));
    host.drive(Message::OutputsChanged(Ok(inventory(vec![
        output("DP-1"),
        output("DP-2"),
    ]))));
    let source = host.bar_windows()[1];
    host.drive(Message::FromSurface(
        source,
        Box::new(Message::PluginSubMsg(
            "Bluetooth".to_owned(),
            emit_host_request(OPEN_MODAL),
        )),
    ));
    let modal = host.modal_window().unwrap();
    assert_eq!(host.surface_output(modal), Some("DP-2"));
    host.drive(Message::Settings(Action::Set(
        "bar.enabled".to_owned(),
        Value::Boolean(false),
    )));
    assert_eq!(host.modal_window(), Some(modal));
    assert_eq!(closures(&host, "Bluetooth"), 0);
    host.drive(Message::OutputsChanged(Ok(inventory(vec![output("DP-1")]))));
    assert!(!host.modal_open());
    assert_eq!(closures(&host, "Bluetooth"), 1);
    host.drive(Message::SurfaceClosed(modal));
    assert_eq!(closures(&host, "Bluetooth"), 1);
}
