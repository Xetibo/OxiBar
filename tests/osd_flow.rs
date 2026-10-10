use iced_layershell::{
    actions::LayershellCustomActionWithId,
    reexport::{Anchor, IcedId, KeyboardInteractivity, Layer},
};
use oxibar::{
    messages::Message,
    monitor::{Output, OutputSnapshot},
    settings::Action,
    test_support::{FakePluginOptions, TestHost, emit_osd},
};
use oxibar_plugin_api::{HostOsdRequest, toml::Value};

fn host() -> TestHost {
    let mut host = TestHost::with_plugins([
        (
            "Audio",
            FakePluginOptions {
                osd: true,
                command: true,
                ..Default::default()
            },
        ),
        (
            "System",
            FakePluginOptions {
                osd: true,
                ..Default::default()
            },
        ),
        ("Legacy", FakePluginOptions::default()),
    ]);
    host.drive(Message::Settings(Action::Set(
        "bar.outputs".to_owned(),
        Value::Array(vec![Value::String("*".to_owned())]),
    )));
    host.drive(Message::OutputsChanged(Ok(inventory(true))));
    host
}
fn inventory(second: bool) -> OutputSnapshot {
    OutputSnapshot {
        backend: "test".to_owned(),
        outputs: ["DP-1", "DP-2"]
            .into_iter()
            .take(if second { 2 } else { 1 })
            .map(|name| Output {
                name: name.to_owned(),
                enabled: true,
                logical_size: (1920, 1080),
                physical_size: (1920, 1080),
                scale: 1.0,
                description: String::new(),
                make: String::new(),
                model: String::new(),
                serial: String::new(),
                focused: name == "DP-1",
                position: (0, 0),
                transform: 0,
                refresh_hz: 60.0,
                vrr: None,
                modes: Vec::new(),
            })
            .collect(),
    }
}
fn bar(host: &TestHost, output: &str) -> IcedId {
    host.bar_windows()
        .into_iter()
        .find(|id| host.bar_output(*id).as_deref() == Some(output))
        .unwrap()
}
fn show(host: &mut TestHost, output: &str, plugin: &str, topic: &str) -> Vec<Message> {
    host.drive(Message::FromSurface(
        bar(host, output),
        Box::new(Message::PluginSubMsg(
            plugin.to_owned(),
            emit_osd(HostOsdRequest::new(topic)),
        )),
    ))
}

#[test]
fn source_scoped_osds_coalesce_per_output_and_reject_stale_expiry() {
    let mut host = host();
    let messages = show(&mut host, "DP-2", "Audio", "volume");
    let first = host.osds()[0].clone();
    assert_eq!(first.output.as_deref(), Some("DP-2"));
    let layer = messages.into_iter().find(|message| matches!(message, Message::OnOutput(_, inner) if matches!(**inner, Message::OpenOsdLayer(..)))).unwrap();
    let action: LayershellCustomActionWithId = layer.try_into().unwrap();
    let LayershellCustomActionWithId(
        _,
        iced_layershell::actions::LayershellCustomAction::NewLayerShell { settings, .. },
    ) = action
    else {
        panic!("new OSD layer");
    };
    assert_eq!(settings.layer, Layer::Overlay);
    assert_eq!(settings.keyboard_interactivity, KeyboardInteractivity::None);
    assert!(settings.events_transparent);
    assert_eq!(settings.exclusive_zone, Some(0));
    assert_eq!(settings.anchor, Anchor::Bottom);
    show(&mut host, "DP-2", "System", "brightness");
    let updated = host.osds()[0].clone();
    assert_eq!(updated.id, first.id);
    assert!(updated.generation > first.generation);
    assert_eq!(updated.plugin, "System");
    host.drive(Message::ExpireOsd(first.id, first.generation));
    assert_eq!(host.osds().len(), 1);
    show(&mut host, "DP-1", "Audio", "volume");
    assert_eq!(host.osds().len(), 2);
    host.drive(Message::ExpireOsd(updated.id, updated.generation));
    assert_eq!(host.osds().len(), 1);
    assert_eq!(host.osds()[0].output.as_deref(), Some("DP-1"));
}

#[test]
fn live_osd_geometry_and_disable_preserve_plugins_and_validate_values() {
    let mut host = host();
    show(&mut host, "DP-1", "Audio", "volume");
    let before = host.osds()[0].clone();
    host.drive(Message::Settings(Action::Set(
        "osd.position".to_owned(),
        Value::String("top".to_owned()),
    )));
    let messages = host.drive(Message::Settings(Action::Set(
        "osd.width".to_owned(),
        Value::Integer(420),
    )));
    assert!(messages.iter().any(|message| matches!(message, Message::ConfigureOsdLayer(id, config) if *id == before.id && config.size.0 == 420 && !config.bottom)));
    assert_eq!(host.osds()[0].id, before.id);
    assert!(host.osds()[0].generation > before.generation);
    host.drive(Message::Settings(Action::Set(
        "osd.timeout_ms".to_owned(),
        Value::Integer(0),
    )));
    assert!(host.settings_error().is_some());
    assert_eq!(host.config_snapshot().host.osd.timeout_ms, 1800);
    host.drive(Message::Settings(Action::Set(
        "osd.enabled".to_owned(),
        Value::Boolean(false),
    )));
    assert!(host.osds().is_empty());
    show(&mut host, "DP-1", "Audio", "volume");
    assert!(host.osds().is_empty());
    host.drive_plugin_record("Audio", "still running");
    assert_eq!(host.seen_by_plugin("Audio"), ["still running"]);
    assert!(host.restart_fields().is_empty());
}

#[test]
fn hiding_bars_retains_osds_but_output_removal_and_late_requests_cannot_recreate_them() {
    let mut host = host();
    show(&mut host, "DP-2", "Audio", "volume");
    let osd = host.osds()[0].clone();
    host.drive(Message::Settings(Action::Set(
        "bar.enabled".to_owned(),
        Value::Boolean(false),
    )));
    assert_eq!(host.osds()[0].id, osd.id);
    host.drive(Message::OutputsChanged(Ok(inventory(false))));
    assert!(host.osds().is_empty());
    host.drive(Message::ExpireOsd(osd.id, osd.generation));
    assert!(host.osds().is_empty());
    host.drive(Message::OnCommandOutput(
        Some("DP-2".to_owned()),
        Box::new(Message::ShowOsd(
            "Audio".to_owned(),
            HostOsdRequest::new("volume"),
        )),
    ));
    assert!(host.osds().is_empty());
}

#[test]
fn resident_commands_report_real_result_and_keep_captured_output_for_async_osds() {
    let mut host = host();
    let source = bar(&host, "DP-2");
    let (_, response) = host.action(Some(source), "audio", "record", &["completed"]);
    assert_eq!(response["ok"], true);
    assert_eq!(host.seen_by_plugin("Audio"), ["completed"]);
    let (_, response) = host.action(Some(source), "Audio", "fail", &[]);
    assert_eq!(response["ok"], false);
    assert_eq!(response["error"], "Fixture action failed");
    assert_eq!(
        host.seen_by_plugin("Audio").last().unwrap(),
        "failed action updated"
    );
    for (plugin, action) in [
        ("Missing", "record"),
        ("Legacy", "record"),
        ("Audio", "unknown"),
    ] {
        assert_eq!(
            host.action(Some(source), plugin, action, &[]).1["ok"],
            false
        );
    }
    assert!(host.osds().is_empty());
    assert_eq!(
        host.action(Some(source), "Audio", "volume", &[]).1["ok"],
        true
    );
    assert_eq!(host.osds()[0].output.as_deref(), Some("DP-2"));
    let count = host.osds().len();
    show(&mut host, "DP-1", "Audio", "bad");
    assert_eq!(host.osds().len(), count);
}
