use iced_layershell::reexport::IcedId;
use oxibar::{
    messages::Message,
    monitor::{Output, OutputSnapshot},
    settings::Action,
    test_support::{
        FakePluginOptions, OPEN_MODAL, TOGGLE_POPUP, TestHost, emit_host_request, record,
    },
};
use toml::Value;

fn output(name: &str, size: (u32, u32), focused: bool) -> Output {
    Output {
        name: name.to_owned(),
        description: name.to_owned(),
        make: String::new(),
        model: String::new(),
        serial: String::new(),
        enabled: true,
        focused,
        physical_size: size,
        logical_size: size,
        position: (0, 0),
        scale: 1.0,
        transform: 0,
        refresh_hz: 59.94,
        vrr: None,
        modes: Vec::new(),
    }
}

fn snapshot(outputs: Vec<Output>) -> OutputSnapshot {
    OutputSnapshot {
        backend: "fixture".to_owned(),
        outputs,
    }
}

fn all_outputs(host: &mut TestHost) {
    host.drive(Message::Settings(Action::Set(
        "bar.outputs".to_owned(),
        Value::Array(vec![Value::String("*".to_owned())]),
    )));
    host.drive(Message::OutputsChanged(Ok(snapshot(vec![
        output("DP-1", (2560, 1440), true),
        output("DP-2", (1080, 1920), false),
    ]))));
}

#[test]
fn per_output_bars_reconcile_without_recreating_plugin_models() {
    let mut host = TestHost::with_plugins([("Clock", FakePluginOptions::all_surfaces())]);
    all_outputs(&mut host);
    let windows = host.bar_windows();
    assert_eq!(windows.len(), 2);
    assert_eq!(host.bar_output(windows[0]).as_deref(), Some("DP-1"));
    assert_eq!(host.bar_dimensions(windows[1]).unwrap().width, 1080);
    host.drive(Message::LayerSurfaceResized(windows[1], 1080, 451));
    host.drive(Message::LayerSurfaceResized(windows[0], 2560, 451));
    assert_eq!(host.bar_dimensions(windows[0]).unwrap().width, 2560);
    assert_eq!(host.bar_dimensions(windows[1]).unwrap().width, 1080);
    host.drive_plugin_record("Clock", "before change");
    host.drive(Message::OutputsChanged(Ok(snapshot(vec![
        output("DP-2", (1080, 1920), true),
        output("DP-1", (2560, 1440), false),
    ]))));
    assert_eq!(host.bar_windows(), windows);
    host.drive_plugin_record("Clock", "after change");
    assert_eq!(
        host.seen_by_plugin("Clock"),
        ["before change", "after change"]
    );
}

#[test]
fn popup_input_and_modal_requests_follow_the_originating_bar() {
    let mut host = TestHost::with_plugins([("Clock", FakePluginOptions::all_surfaces())]);
    all_outputs(&mut host);
    let windows = host.bar_windows();
    let messages = host.drive(Message::FromSurface(
        windows[1],
        Box::new(Message::PluginSubMsg(
            "Clock".to_owned(),
            emit_host_request(TOGGLE_POPUP),
        )),
    ));
    assert_eq!(host.popup_window(), Some(windows[1]));
    assert!(messages.iter().any(|message| matches!(message, Message::SetBarInputRegion { id, open: true, bar_size, .. } if *id == windows[1] && bar_size.width == 1080)));
    assert!(!messages.iter().any(|message| matches!(message, Message::SetBarInputRegion { id, open: true, .. } if *id == windows[0])));
    let messages = host.drive(Message::FromSurface(
        windows[1],
        Box::new(Message::PluginSubMsg(
            "Clock".to_owned(),
            emit_host_request(OPEN_MODAL),
        )),
    ));
    assert!(
        messages.iter().any(
            |message| matches!(message, Message::OnOutput(Some(output), _) if output == "DP-2")
        )
    );
}

#[test]
fn hotplug_removes_owned_surfaces_but_keeps_async_plugin_results() {
    let mut host = TestHost::with_plugins([("Clock", FakePluginOptions::all_surfaces())]);
    all_outputs(&mut host);
    let removed = host.bar_windows()[1];
    host.drive(Message::FromSurface(
        removed,
        Box::new(Message::PluginSubMsg(
            "Clock".to_owned(),
            emit_host_request(OPEN_MODAL),
        )),
    ));
    assert!(host.modal_open());
    host.drive(Message::OutputsChanged(Ok(snapshot(vec![output(
        "DP-1",
        (2560, 1440),
        true,
    )]))));
    assert_eq!(host.bar_windows().len(), 1);
    assert!(!host.modal_open());
    host.drive(Message::PluginTask(
        Some(removed),
        Box::new(Message::PluginSubMsg(
            "Clock".to_owned(),
            record("late worker result"),
        )),
    ));
    assert!(
        host.seen_by_plugin("Clock")
            .contains(&"late worker result".to_owned())
    );
    host.drive(Message::OutputsChanged(Ok(snapshot(vec![
        output("DP-1", (2560, 1440), true),
        output("DP-2", (1080, 1920), false),
    ]))));
    assert_eq!(host.bar_windows().len(), 2);
    assert_ne!(host.bar_windows()[1], removed);
}

#[test]
fn selected_output_fallback_and_explicit_disable_remain_reversible() {
    let mut host = TestHost::with_plugins([("Clock", FakePluginOptions::popup())]);
    host.drive(Message::Settings(Action::Set(
        "bar.outputs".to_owned(),
        Value::Array(vec![Value::String("missing".to_owned())]),
    )));
    host.drive(Message::OutputsChanged(Ok(snapshot(vec![output(
        "DP-1",
        (2560, 1440),
        true,
    )]))));
    assert_eq!(host.bar_output(host.main_window()).as_deref(), Some("DP-1"));
    host.drive(Message::Settings(Action::Set(
        "bar.output_fallback".to_owned(),
        Value::Boolean(false),
    )));
    assert!(host.bar_windows().is_empty());
    host.drive(Message::Settings(Action::Set(
        "bar.output_fallback".to_owned(),
        Value::Boolean(true),
    )));
    assert_eq!(host.bar_windows().len(), 1);
    let windows = host.bar_windows();
    host.drive(Message::OutputsChanged(Err(
        "temporarily unavailable".to_owned()
    )));
    assert_eq!(host.bar_windows(), windows);
    host.drive(Message::Settings(Action::Set(
        "bar.enabled".to_owned(),
        Value::Boolean(false),
    )));
    assert!(host.bar_windows().is_empty());
    host.drive(Message::Settings(Action::Open));
    assert!(host.settings_window().is_some());
}

#[test]
fn unknown_or_retired_window_events_cannot_claim_a_bar() {
    let mut host = TestHost::with_plugins([("Clock", FakePluginOptions::popup())]);
    let original = host.main_window();
    host.drive(Message::LayerSurfaceResized(IcedId::unique(), 1, 1));
    assert_eq!(host.main_window(), original);
    assert_eq!(host.bar_size().height, 31);
}

#[test]
fn disabling_a_bar_keeps_settings_on_a_still_connected_output() {
    let mut host = TestHost::with_plugins([("Clock", FakePluginOptions::popup())]);
    all_outputs(&mut host);
    let bar = host.bar_windows()[0];
    host.drive(Message::FromSurface(
        bar,
        Box::new(Message::Settings(Action::Open)),
    ));
    let settings = host.settings_window();
    host.drive(Message::Settings(Action::Set(
        "bar.enabled".to_owned(),
        Value::Boolean(false),
    )));
    assert!(host.bar_windows().is_empty());
    assert_eq!(host.settings_window(), settings);
}

#[test]
fn output_profiles_control_geometry_without_changing_global_preferences() {
    let mut host = TestHost::with_plugins([("Clock", FakePluginOptions::popup())]);
    all_outputs(&mut host);
    let profile = toml::from_str::<toml::Table>(
        "height=44\nposition='bottom'\ncenter=['clock']\nshow_labels=true",
    )
    .unwrap();
    host.drive(Message::Settings(Action::Set(
        "bar.profiles.DP-2".to_owned(),
        Value::Table(profile),
    )));
    let window = host.bar_windows()[1];
    assert_eq!(host.bar_dimensions(window).unwrap().height, 44);
    assert_eq!(host.config_snapshot().host.bar_height, 31);
    assert_eq!(host.bar_show_labels(host.bar_windows()[0]), Some(false));
    assert_eq!(host.bar_show_labels(window), Some(true));
    let windows = host.bar_windows();
    host.drive_plugin_record("Clock", "labels off");
    host.drive(Message::Settings(Action::Set(
        "bar.show_labels".into(),
        Value::Boolean(true),
    )));
    assert_eq!(host.bar_windows(), windows);
    assert_eq!(host.bar_show_labels(windows[0]), Some(true));
    host.drive(Message::Settings(Action::Set(
        "bar.show_labels".into(),
        Value::Boolean(false),
    )));
    host.drive_plugin_record("Clock", "labels on");
    assert_eq!(host.bar_show_labels(window), Some(true));
    assert_eq!(host.bar_show_labels(windows[0]), Some(false));
    assert_eq!(host.seen_by_plugin("Clock"), ["labels off", "labels on"]);
    let messages = host.drive(Message::LayerSurfaceResized(window, 1080, 464));
    assert!(messages.iter().any(|message| matches!(message, Message::SetBarInputRegion { id, bottom: true, .. } if *id == window)));
    host.drive(Message::Settings(Action::Set(
        "bar.profiles.DP-2.height".to_owned(),
        Value::Integer(-1),
    )));
    assert!(host.settings_error().is_some());
    assert_eq!(host.bar_dimensions(window).unwrap().height, 44);
}

#[test]
fn edge_reveal_is_per_output_pins_dropdowns_and_preserves_bar_ids_and_models() {
    let mut host = TestHost::with_plugins([("Audio", FakePluginOptions::popup())]);
    all_outputs(&mut host);
    let bars = host.bar_windows();
    let profile =
        toml::from_str::<toml::Table>("position='bottom'\nauto_hide=true\nreveal_zone=6").unwrap();
    let changed = host.drive(Message::Settings(Action::Set(
        "bar.profiles.DP-2".into(),
        Value::Table(profile),
    )));
    let bar = bars[1];
    assert!(host.bar_visible(bars[0]));
    assert!(!host.bar_visible(bar));
    assert!(
        changed
            .iter()
            .any(|message| matches!(message, Message::SetExclusiveZone(id, 0) if *id == bar))
    );
    let reserve = changed
        .iter()
        .find_map(|message| match message {
            Message::SetBarInputRegion {
                id,
                bottom: true,
                reserve,
                edge_trigger: Some(6),
                ..
            } if *id == bar => Some(*reserve),
            _ => None,
        })
        .unwrap();
    let revealed = host.drive(Message::OverlayPointer(
        bar,
        Some(iced::Point::new(200.0, reserve as f32 + 30.0)),
    ));
    assert!(host.bar_visible(bar));
    assert!(revealed.iter().any(|message| matches!(message, Message::SetBarInputRegion { id, edge_trigger: None, .. } if *id == bar)));
    host.drive(Message::FromSurface(
        bar,
        Box::new(Message::TogglePluginPopup("Audio".into())),
    ));
    host.drive(Message::OverlayLeave(bar));
    assert!(host.bar_visible(bar));
    host.drive(Message::DropdownEscape(bars[0]));
    assert!(host.popup_open());
    let hidden = host.drive(Message::DropdownEscape(bar));
    assert!(!host.bar_visible(bar));
    assert!(hidden.iter().any(|message| matches!(message, Message::SetBarInputRegion { id, open: false, edge_trigger: Some(6), .. } if *id == bar)));
    host.drive(Message::OverlayPointer(
        IcedId::unique(),
        Some(iced::Point::new(200.0, reserve as f32 + 30.0)),
    ));
    assert!(!host.bar_visible(bar));
    host.drive_plugin_record("Audio", "retained while hidden");
    host.drive(Message::Settings(Action::Set(
        "bar.profiles.DP-2.reveal_zone".into(),
        Value::Integer(0),
    )));
    assert!(host.settings_error().is_some());
    let restored = host.drive(Message::Settings(Action::Set(
        "bar.profiles.DP-2.auto_hide".into(),
        Value::Boolean(false),
    )));
    assert!(host.bar_visible(bar));
    assert!(
        restored
            .iter()
            .any(|message| matches!(message, Message::SetExclusiveZone(id, 31) if *id == bar))
    );
    assert_eq!(host.bar_windows(), bars);
    assert_eq!(host.seen_by_plugin("Audio"), ["retained while hidden"]);
}
