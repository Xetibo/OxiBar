use iced_layershell::reexport::IcedId;
use oxibar::{
    control_center::Action,
    launcher,
    messages::Message,
    monitor::{Output, OutputSnapshot},
    settings,
    test_support::{FakePluginOptions, OPEN_MODAL, TestHost, emit_host_request, record},
};
use toml::Value;

fn set(host: &mut TestHost, path: &str, value: Value) -> Vec<Message> {
    host.drive(Message::Settings(settings::Action::Set(
        path.to_owned(),
        value,
    )))
}

fn outputs(names: &[&str]) -> OutputSnapshot {
    OutputSnapshot {
        backend: "fixture".to_owned(),
        outputs: names
            .iter()
            .enumerate()
            .map(|(index, name)| Output {
                name: (*name).to_owned(),
                description: (*name).to_owned(),
                make: String::new(),
                model: String::new(),
                serial: String::new(),
                enabled: true,
                focused: index == 0,
                physical_size: (1920, 1080),
                logical_size: (1920, 1080),
                position: (0, 0),
                scale: 1.0,
                transform: 0,
                refresh_hz: 59.94,
                vrr: None,
                modes: Vec::new(),
            })
            .collect(),
    }
}

#[test]
fn configured_widgets_use_available_shared_plugin_capabilities_in_order() {
    let mut host = TestHost::with_plugins([
        ("Audio", FakePluginOptions::popup()),
        ("Notifications", FakePluginOptions::panel()),
        (
            "Custom",
            FakePluginOptions {
                control_center: true,
                ..Default::default()
            },
        ),
        ("Runner", FakePluginOptions::runner(1)),
    ]);
    set(
        &mut host,
        "control_center.widgets",
        Value::Array(
            [
                "custom",
                "missing",
                "audio",
                "AUDIO",
                "Notifications",
                "Runner",
            ]
            .into_iter()
            .map(|name| Value::String(name.to_owned()))
            .collect(),
        ),
    );
    assert_eq!(
        host.control_center_widgets(),
        ["Custom", "Audio", "Notifications"]
    );
    host.drive(Message::ControlCenter(Action::Open));
    assert_eq!(host.control_center_selected(), Some("Custom"));
    host.drive(Message::ControlCenter(Action::Select("missing".to_owned())));
    assert_eq!(host.control_center_selected(), Some("Custom"));
    host.drive(Message::ControlCenter(Action::SelectIndex(2)));
    assert_eq!(host.control_center_selected(), Some("Notifications"));
    host.drive_plugin_record("Notifications", "existing state");
    host.drive(Message::ControlCenter(Action::Close));
    host.drive(Message::ControlCenter(Action::Open));
    assert_eq!(host.control_center_selected(), Some("Notifications"));
    assert_eq!(host.seen_by_plugin("Notifications"), ["existing state"]);
}

#[test]
fn control_center_actions_and_async_modals_follow_the_source_output() {
    let mut host = TestHost::with_plugins([("Network", FakePluginOptions::all_surfaces())]);
    set(
        &mut host,
        "bar.outputs",
        Value::Array(vec![Value::String("*".to_owned())]),
    );
    host.drive(Message::OutputsChanged(Ok(outputs(&["DP-1", "DP-2"]))));
    let bar = host.bar_windows()[1];
    let messages = host.drive(Message::FromSurface(
        bar,
        Box::new(Message::ControlCenter(Action::Open)),
    ));
    let center = host.control_center_window().unwrap();
    assert_eq!(host.surface_output(center), Some("DP-2"));
    assert!(messages.iter().any(|message| matches!(message, Message::OnOutput(Some(output), inner)
        if output == "DP-2" && matches!(inner.as_ref(), Message::OpenControlCenterLayer(id, _, _) if *id == center))));
    host.drive(Message::FromSurface(
        center,
        Box::new(Message::PluginSubMsg(
            "Network".to_owned(),
            record("from controls"),
        )),
    ));
    let messages = host.drive(Message::FromSurface(
        center,
        Box::new(Message::PluginSubMsg(
            "Network".to_owned(),
            emit_host_request(OPEN_MODAL),
        )),
    ));
    assert!(host.modal_open());
    assert!(messages.iter().any(
        |message| matches!(message, Message::OnOutput(Some(output), inner)
        if output == "DP-2" && matches!(inner.as_ref(), Message::OpenModalLayer(_)))
    ));
    assert_eq!(host.control_center_window(), Some(center));
    assert_eq!(
        host.seen_by_plugin("Network"),
        ["from controls", OPEN_MODAL]
    );
}

#[test]
fn exclusive_host_surfaces_dismiss_each_other_and_keys_are_window_scoped() {
    let mut host = TestHost::with_plugins([("Clock", FakePluginOptions::popup())]);
    host.drive(Message::Settings(settings::Action::Open));
    host.drive(Message::ControlCenter(Action::Open));
    let center = host.control_center_window().unwrap();
    assert!(host.settings_window().is_none());
    host.drive(Message::ControlCenterKey(IcedId::unique(), Action::Close));
    assert_eq!(host.control_center_window(), Some(center));
    host.drive(Message::LayerSurfaceResized(center, 680, 760));
    assert_eq!(host.bar_size(), oxibar::layout::DEFAULT_BAR_SIZE);
    host.drive(Message::Launcher(launcher::Action::Open));
    assert!(host.control_center_window().is_none());
    assert!(host.launcher_window().is_some());
    host.drive(Message::ControlCenter(Action::Open));
    assert!(host.launcher_window().is_none());
    host.drive(Message::Settings(settings::Action::Open));
    assert!(host.control_center_window().is_none());
    host.drive(Message::ControlCenter(Action::Open));
    let center = host.control_center_window().unwrap();
    host.drive(Message::ControlCenterKey(center, Action::Close));
    assert!(host.control_center_window().is_none());
}

#[test]
fn live_preferences_resize_disable_and_reset_without_losing_plugin_state() {
    let mut host = TestHost::with_plugins([
        ("Audio", FakePluginOptions::popup()),
        ("Clock", FakePluginOptions::popup()),
    ]);
    host.drive(Message::ControlCenter(Action::Open));
    let center = host.control_center_window().unwrap();
    let messages = set(&mut host, "control_center.width", Value::Integer(720));
    assert!(
        messages
            .iter()
            .any(|message| matches!(message, Message::ResizeLayer(id, 720, 760) if *id == center))
    );
    set(&mut host, "control_center.width", Value::Integer(0));
    assert!(host.settings_error().is_some());
    assert_eq!(host.config_snapshot().host.control_center_size, (720, 760));
    host.drive_plugin_record("Audio", "still warm");
    set(
        &mut host,
        "control_center.widgets",
        Value::Array(vec![Value::String("clock".to_owned())]),
    );
    assert_eq!(host.control_center_selected(), Some("Clock"));
    assert!(host.restart_fields().is_empty());
    set(&mut host, "control_center.enabled", Value::Boolean(false));
    assert!(host.control_center_window().is_none());
    host.drive(Message::ControlCenter(Action::Open));
    assert!(host.control_center_window().is_none());
    host.drive(Message::Settings(settings::Action::Reset(
        "control_center.enabled".to_owned(),
    )));
    host.drive(Message::ControlCenter(Action::Open));
    assert!(host.control_center_window().is_some());
    assert_eq!(host.seen_by_plugin("Audio"), ["still warm"]);
}

#[test]
fn viewing_notifications_clears_toasts_and_suppresses_only_that_visible_provider() {
    let mut host = TestHost::with_plugins([
        ("Audio", FakePluginOptions::popup()),
        ("Notifications", FakePluginOptions::all_surfaces()),
    ]);
    host.drive_plugin_toast_show("Notifications", "before", 380, 144);
    host.drive(Message::ControlCenter(Action::Open));
    assert_eq!(host.toast_count(), 1);
    host.drive(Message::ControlCenter(Action::Select(
        "Notifications".to_owned(),
    )));
    assert_eq!(host.toast_count(), 0);
    host.drive_plugin_toast_show("Notifications", "during", 380, 144);
    assert_eq!(host.toast_count(), 0);
    host.drive(Message::ControlCenter(Action::Select("Audio".to_owned())));
    host.drive_plugin_toast_show("Notifications", "after", 380, 144);
    assert_eq!(host.toast_ids(), ["after"]);
    host.drive(Message::ControlCenter(Action::Close));
    host.drive_plugin_toast_show("Notifications", "closed", 380, 144);
    assert_eq!(host.toast_count(), 2);
}

#[test]
fn hiding_bars_keeps_controls_but_output_removal_retires_the_surface() {
    let mut host = TestHost::with_plugins([("Audio", FakePluginOptions::popup())]);
    host.drive(Message::OutputsChanged(Ok(outputs(&["DP-1"]))));
    let bar = host.main_window();
    host.drive(Message::FromSurface(
        bar,
        Box::new(Message::ControlCenter(Action::Open)),
    ));
    let center = host.control_center_window().unwrap();
    set(&mut host, "bar.enabled", Value::Boolean(false));
    assert!(host.bar_windows().is_empty());
    assert_eq!(host.control_center_window(), Some(center));
    host.drive(Message::OutputsChanged(Ok(outputs(&[]))));
    assert!(host.control_center_window().is_none());
    host.drive(Message::FromSurface(
        center,
        Box::new(Message::PluginSubMsg(
            "Audio".to_owned(),
            record("stale click"),
        )),
    ));
    host.drive(Message::PluginTask(
        Some(center),
        Box::new(Message::PluginSubMsg(
            "Audio".to_owned(),
            record("late result"),
        )),
    ));
    assert_eq!(host.seen_by_plugin("Audio"), ["late result"]);
}
