use iced_layershell::reexport::Anchor;
use oxibar::{
    messages::Message,
    monitor::{Output, OutputSnapshot},
    settings::Action,
    test_support::{FakePluginOptions, TestHost},
};
use oxibar_plugin_api::{HostOsdRequest, toml::Value};

#[test]
fn music_and_notifications_share_live_placement_and_output_local_stacking() {
    let mut host = TestHost::with_plugins([
        ("Notifications", FakePluginOptions::toast()),
        (
            "Audio",
            FakePluginOptions {
                osd: true,
                ..Default::default()
            },
        ),
    ]);
    host.drive(Message::Settings(Action::Set(
        "bar.outputs".into(),
        Value::Array(vec![Value::String("*".into())]),
    )));
    host.drive(Message::OutputsChanged(Ok(OutputSnapshot {
        backend: "fixture".into(),
        outputs: ["DP-1", "DP-2"]
            .into_iter()
            .map(|name| Output {
                name: name.into(),
                description: String::new(),
                make: String::new(),
                model: String::new(),
                serial: String::new(),
                enabled: true,
                focused: name == "DP-1",
                logical_size: (1920, 1080),
                physical_size: (1920, 1080),
                position: (0, 0),
                scale: 1.0,
                transform: 0,
                refresh_hz: 60.0,
                vrr: None,
                modes: vec![],
            })
            .collect(),
    })));
    let bars = host.bar_windows();
    let toast = host.drive(Message::FromSurface(
        bars[0],
        Box::new(Message::ShowPluginToast(
            "Notifications".into(),
            oxibar_plugin_api::HostToastRequest::show("one", 380, 140),
        )),
    ));
    let toast_id = toast
        .iter()
        .find_map(|message| match message {
            Message::OnOutput(_, inner) => match inner.as_ref() {
                Message::OpenToastLayer(id, _, _, placement) => {
                    assert_eq!(placement.anchor, Anchor::Top | Anchor::Right);
                    Some(*id)
                }
                _ => None,
            },
            _ => None,
        })
        .unwrap();
    let music = host.drive(Message::FromSurface(
        bars[0],
        Box::new(Message::ShowOsd(
            "Audio".into(),
            HostOsdRequest::new("media"),
        )),
    ));
    assert!(music.iter().any(|message| matches!(message, Message::OnOutput(_, inner) if matches!(inner.as_ref(), Message::OpenOsdLayer(_, config) if config.placement.is_some_and(|placement| placement.margin == (168,16,0,0))))));
    let music_id = host.osds()[0].id;
    let other = host.drive(Message::FromSurface(
        bars[1],
        Box::new(Message::ShowOsd(
            "Audio".into(),
            HostOsdRequest::new("media"),
        )),
    ));
    assert!(other.iter().any(|message| matches!(message, Message::OnOutput(_, inner) if matches!(inner.as_ref(), Message::OpenOsdLayer(_, config) if config.placement.is_some_and(|placement| placement.margin == (16,16,0,0))))));
    let moved = host.drive(Message::Settings(Action::Set(
        "toasts.position".into(),
        Value::String("bottom-left".into()),
    )));
    assert!(moved.iter().any(|message| matches!(message, Message::MoveToastLayer(id, placement) if *id == toast_id && placement.anchor == Anchor::Bottom | Anchor::Left && placement.margin == (0,0,16,16))));
    assert!(moved.iter().any(|message| matches!(message, Message::ConfigureOsdLayer(id, config) if *id == music_id && config.placement.is_some_and(|placement| placement.anchor == Anchor::Bottom | Anchor::Left && placement.margin == (0,0,168,16)))));
    let resized = host.drive_plugin_toast_show("Notifications", "one", 380, 200);
    assert!(resized.iter().any(|message| matches!(message, Message::ConfigureOsdLayer(id, config) if *id == music_id && config.placement.is_some_and(|placement| placement.margin.2 == 228))));
    let removed = host.drive_plugin_toast_close("Notifications", "one");
    assert!(removed.iter().any(|message| matches!(message, Message::ConfigureOsdLayer(id, config) if *id == music_id && config.placement.is_some_and(|placement| placement.margin.2 == 16))));
    let device = host.drive(Message::FromSurface(
        bars[0],
        Box::new(Message::ShowOsd(
            "Audio".into(),
            HostOsdRequest::new("volume"),
        )),
    ));
    assert!(device.iter().any(|message| matches!(message, Message::ConfigureOsdLayer(id, config) if *id == music_id && config.placement.is_none() && config.bottom)));
    assert_eq!(host.osds().len(), 2);
    host.drive(Message::Settings(Action::Set(
        "toasts.position".into(),
        Value::String("invalid".into()),
    )));
    assert!(host.settings_error().is_some());
    assert_eq!(host.config_snapshot().host.toasts.position, "bottom-left");
}
