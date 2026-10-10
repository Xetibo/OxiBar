use iced::Point;
use iced_layershell::{
    actions::{LayershellCustomAction, LayershellCustomActionWithId},
    reexport::{Anchor, KeyboardInteractivity, Layer},
};
use oxibar::{
    desktop::Action as DesktopAction,
    dock::Action as DockAction,
    messages::Message,
    monitor::{Output, OutputSnapshot},
    settings::Action,
    test_support::{FakePluginOptions, TestHost, record},
};
use toml::Value;

fn output(name: &str, width: u32, height: u32) -> Output {
    Output {
        name: name.into(),
        description: name.into(),
        make: String::new(),
        model: String::new(),
        serial: String::new(),
        enabled: true,
        focused: name == "DP-1",
        physical_size: (width, height),
        logical_size: (width, height),
        position: (0, 0),
        scale: 1.0,
        transform: 0,
        refresh_hz: 60.0,
        vrr: None,
        modes: Vec::new(),
    }
}
fn outputs(host: &mut TestHost, second: bool) {
    let mut items = vec![output("DP-1", 1920, 1080)];
    if second {
        items.push(output("DP-2", 800, 600));
    }
    host.drive(Message::OutputsChanged(Ok(OutputSnapshot {
        backend: "fixture".into(),
        outputs: items,
    })));
}
fn set(host: &mut TestHost, path: &str, value: Value) -> Vec<Message> {
    host.drive(Message::Settings(Action::Set(path.into(), value)))
}
fn specs() -> Value {
    toml::from_str::<toml::Table>(r#"widgets=[{id="one",plugin="Clock",output="DP-1",x=32,y=128,width=320,height=240,future="retained"},{id="two",plugin="Clock",output="DP-2",x=640,y=480,width=320,height=240}]"#).unwrap().remove("widgets").unwrap()
}
fn host() -> TestHost {
    TestHost::with_plugins([(
        "Clock",
        FakePluginOptions {
            desktop: true,
            ..Default::default()
        },
    )])
}

#[test]
fn hidden_visual_widgets_pause_one_subscription_and_keep_models() {
    let mut host = TestHost::with_plugins([
        (
            "Clock",
            FakePluginOptions {
                desktop: true,
                osd: true,
                visible_subscription: true,
                ..Default::default()
            },
        ),
        (
            "Stats",
            FakePluginOptions {
                desktop: true,
                visible_subscription: true,
                ..Default::default()
            },
        ),
        ("Service", FakePluginOptions::default()),
    ]);
    set(&mut host, "bar.enabled", Value::Boolean(false));
    assert_eq!(host.subscribed_plugins(), ["Service"]);
    set(&mut host, "desktop.widgets", specs());
    assert_eq!(host.subscribed_plugins(), ["Clock", "Service"]);
    host.drive(Message::PluginSubMsg(
        "Clock".to_owned(),
        record("retained"),
    ));
    host.drive(Message::Desktop(DesktopAction::Edit));
    host.drive(Message::Desktop(DesktopAction::Toggle));
    assert_eq!(host.subscribed_plugins(), ["Clock", "Service"]);
    host.drive(Message::Desktop(DesktopAction::Select("two".to_owned())));
    host.drive(Message::Desktop(DesktopAction::Toggle));
    assert_eq!(host.subscribed_plugins(), ["Service"]);
    host.drive(Message::Desktop(DesktopAction::Provider(
        "Stats".to_owned(),
    )));
    host.drive(Message::Desktop(DesktopAction::Add));
    assert_eq!(host.subscribed_plugins(), ["Service", "Stats"]);
    host.drive(Message::Desktop(DesktopAction::Cancel));
    assert_eq!(host.subscribed_plugins(), ["Clock", "Service"]);
    assert_eq!(host.seen_by_plugin("Clock"), ["retained"]);
    set(&mut host, "desktop.enabled", Value::Boolean(false));
    assert_eq!(host.subscribed_plugins(), ["Service"]);
    host.drive(Message::ShowOsd(
        "Clock".to_owned(),
        oxibar_plugin_api::HostOsdRequest::new("visible"),
    ));
    assert_eq!(host.subscribed_plugins(), ["Clock", "Service"]);
    let osd = host.osds()[0].clone();
    host.drive(Message::ExpireOsd(osd.id, osd.generation));
    assert_eq!(host.subscribed_plugins(), ["Service"]);
    set(&mut host, "bar.enabled", Value::Boolean(true));
    assert_eq!(host.subscribed_plugins(), ["Clock", "Service", "Stats"]);
}

#[test]
fn desktop_coordinates_ignore_reserved_bar_areas_without_reserving_space() {
    let host = host();
    for (edit, interactive) in [(false, false), (false, true), (true, false)] {
        let action: LayershellCustomActionWithId = Message::OpenDesktopLayer(
            host.main_window(),
            oxibar::desktop::Rect {
                x: 32,
                y: 128,
                width: 320,
                height: 240,
            },
            edit,
            interactive,
        )
        .try_into()
        .unwrap();
        let LayershellCustomActionWithId(_, LayershellCustomAction::NewLayerShell { settings, .. }) =
            action
        else {
            panic!("desktop layer")
        };
        assert_eq!(settings.exclusive_zone, Some(-1));
        assert_eq!(settings.margin, Some((128, 0, 0, 32)));
        assert_eq!(settings.anchor, Anchor::Top | Anchor::Left);
        assert_eq!(settings.size, Some((320, 240)));
        assert_eq!(settings.events_transparent, !edit && !interactive);
        assert_eq!(
            settings.layer,
            if edit {
                Layer::Overlay
            } else {
                Layer::Background
            }
        );
        assert_eq!(
            settings.keyboard_interactivity,
            if edit {
                KeyboardInteractivity::Exclusive
            } else {
                KeyboardInteractivity::None
            }
        );
    }
}

#[test]
fn passive_widgets_pass_pointer_input_through_and_allow_persistent_instance_overrides() {
    let mut host = TestHost::with_plugins([(
        "Stats",
        FakePluginOptions {
            desktop: true,
            passive_desktop: true,
            ..Default::default()
        },
    )]);
    outputs(&mut host, false);
    let mut widget = oxibar_plugin_api::toml::Table::new();
    widget.insert("id".into(), Value::String("read-only".into()));
    widget.insert("plugin".into(), Value::String("Stats".into()));
    widget.insert("output".into(), Value::String("DP-1".into()));
    let actions = set(
        &mut host,
        "desktop.widgets",
        Value::Array(vec![Value::Table(widget.clone())]),
    );
    let automatic = host.desktop_widgets()[0].clone();
    assert!(!automatic.interactive);
    let creation = actions
        .into_iter()
        .find_map(|message| {
            let action: Result<LayershellCustomActionWithId, _> = message.try_into();
            match action.ok() {
                Some(LayershellCustomActionWithId(
                    _,
                    LayershellCustomAction::NewLayerShell { settings, id },
                )) if id == automatic.window => Some(settings),
                _ => None,
            }
        })
        .unwrap();
    assert!(creation.events_transparent);
    host.drive(Message::Desktop(DesktopAction::Edit));
    host.drive(Message::Desktop(DesktopAction::CycleInput));
    host.drive(Message::Desktop(DesktopAction::Save));
    let interactive = host.desktop_widgets()[0].clone();
    assert!(interactive.interactive);
    assert_ne!(interactive.window, automatic.window);
    assert_eq!(
        host.config_snapshot().effective["desktop"]["widgets"][0]["interactive"].as_bool(),
        Some(true)
    );
    host.drive(Message::Desktop(DesktopAction::Edit));
    host.drive(Message::Desktop(DesktopAction::CycleInput));
    host.drive(Message::Desktop(DesktopAction::CycleInput));
    host.drive(Message::Desktop(DesktopAction::Save));
    assert!(!host.desktop_widgets()[0].interactive);
    assert!(
        host.config_snapshot().effective["desktop"]["widgets"][0]
            .get("interactive")
            .is_none()
    );
    widget.insert("interactive".into(), Value::String("invalid".into()));
    set(
        &mut host,
        "desktop.widgets",
        Value::Array(vec![Value::Table(widget)]),
    );
    assert!(!host.desktop_widgets()[0].interactive);
    assert!(!host.settings_error().unwrap_or_default().is_empty());
}

#[test]
fn instances_share_models_and_keep_requested_placement_through_hotplug_and_hiding() {
    let mut host = host();
    outputs(&mut host, true);
    set(&mut host, "desktop.widgets", specs());
    let widgets = host.desktop_widgets();
    assert_eq!(widgets.len(), 2);
    assert_ne!(widgets[0].window, widgets[1].window);
    assert_eq!(widgets[1].rect.x, 480);
    assert_eq!(widgets[1].rect.y, 360);
    host.drive(Message::PluginSubMsg(
        "Clock".into(),
        record("shared state"),
    ));
    outputs(&mut host, false);
    assert_eq!(host.desktop_widgets()[1].output.as_deref(), Some("DP-1"));
    assert_eq!(
        host.config_snapshot().host.desktop.widgets[1].output,
        "DP-2"
    );
    assert_eq!(host.config_snapshot().host.desktop.widgets[1].x, 640);
    outputs(&mut host, true);
    assert_eq!(host.desktop_widgets()[1].output.as_deref(), Some("DP-2"));
    set(&mut host, "desktop.enabled", Value::Boolean(false));
    assert!(host.desktop_widgets().is_empty());
    set(&mut host, "desktop.enabled", Value::Boolean(true));
    assert_eq!(host.desktop_widgets().len(), 2);
    assert_eq!(host.seen_by_plugin("Clock"), ["shared state"]);
    let old = host.config_snapshot().revision;
    set(
        &mut host,
        "desktop.widgets",
        Value::Array(vec![
            Value::Table(toml::toml! {id="duplicate" plugin="Clock"}),
            Value::Table(toml::toml! {id="duplicate" plugin="Clock"}),
        ]),
    );
    assert_eq!(host.config_snapshot().revision, old);
    assert_eq!(host.desktop_widgets().len(), 2);
}

#[test]
fn explicit_editor_snaps_drag_resize_and_atomically_saves_unknown_fields() {
    let mut host = host();
    outputs(&mut host, true);
    set(&mut host, "desktop.widgets", specs());
    let retired = host.desktop_widgets()[0].window;
    host.drive(Message::Desktop(DesktopAction::Edit));
    assert!(host.desktop_widgets().is_empty());
    let id = host
        .desktop_editors()
        .iter()
        .find(|(output, _)| output.as_deref() == Some("DP-1"))
        .unwrap()
        .1;
    host.drive(Message::Desktop(DesktopAction::Pointer(
        id,
        Point::new(900.0, 700.0),
    )));
    host.drive(Message::FromSurface(
        id,
        Box::new(Message::Desktop(DesktopAction::StartAt(
            "one".into(),
            false,
            Point::new(45.0, 145.0),
        ))),
    ));
    host.drive(Message::Desktop(DesktopAction::Pointer(
        id,
        Point::new(76.0, 190.0),
    )));
    host.drive(Message::Desktop(DesktopAction::Release(id)));
    let draft = host.desktop_draft().unwrap();
    assert_eq!((draft[0].x, draft[0].y), (64, 176));
    host.drive(Message::FromSurface(
        id,
        Box::new(Message::Desktop(DesktopAction::Start("one".into(), true))),
    ));
    host.drive(Message::Desktop(DesktopAction::Pointer(
        id,
        Point::new(127.0, 223.0),
    )));
    host.drive(Message::Desktop(DesktopAction::Release(id)));
    assert_eq!(
        (
            host.desktop_draft().unwrap()[0].width,
            host.desktop_draft().unwrap()[0].height
        ),
        (368, 272)
    );
    host.drive(Message::FromSurface(
        retired,
        Box::new(Message::PluginSubMsg("Clock".into(), record("retired UI"))),
    ));
    host.drive(Message::PluginTask(
        Some(retired),
        Box::new(Message::PluginSubMsg("Clock".into(), record("late result"))),
    ));
    host.drive(Message::Desktop(DesktopAction::Save));
    assert!(host.desktop_draft().is_none());
    assert!(host.desktop_editors().is_empty());
    assert_eq!(host.desktop_widgets()[0].rect.x, 64);
    assert_eq!(
        host.config_snapshot().effective["desktop"]["widgets"][0]["future"].as_str(),
        Some("retained")
    );
    assert_eq!(host.seen_by_plugin("Clock"), ["late result"]);
}

#[test]
fn stale_editor_save_retains_draft_and_cancel_restores_latest_configuration() {
    let mut host = host();
    outputs(&mut host, true);
    set(&mut host, "desktop.widgets", specs());
    host.drive(Message::Desktop(DesktopAction::Edit));
    let id = host.desktop_editors()[0].1;
    host.drive(Message::Desktop(DesktopAction::Nudge(id, 1, 0, false)));
    set(&mut host, "bar.height", Value::Integer(40));
    host.drive(Message::Desktop(DesktopAction::Save));
    assert!(host.desktop_error().is_some());
    assert_eq!(host.desktop_draft().unwrap()[0].x, 48);
    host.drive(Message::Desktop(DesktopAction::Cancel));
    assert!(host.desktop_draft().is_none());
    assert_eq!(host.desktop_widgets()[0].rect.x, 32);
    host.drive(Message::SurfaceClosed(id));
    assert_eq!(host.desktop_widgets().len(), 2);
}

#[test]
fn editor_can_add_assign_hide_remove_and_cancel_without_writing() {
    let mut host = host();
    outputs(&mut host, true);
    let revision = host.config_snapshot().revision;
    host.drive(Message::Desktop(DesktopAction::Edit));
    host.drive(Message::Desktop(DesktopAction::Add));
    host.drive(Message::Desktop(DesktopAction::Output("DP-2".into())));
    assert_eq!(host.desktop_draft().unwrap()[0].output, "DP-2");
    host.drive(Message::Desktop(DesktopAction::Toggle));
    assert!(!host.desktop_draft().unwrap()[0].enabled);
    host.drive(Message::Desktop(DesktopAction::Toggle));
    host.drive(Message::Desktop(DesktopAction::Remove));
    assert!(host.desktop_draft().unwrap().is_empty());
    host.drive(Message::Desktop(DesktopAction::Cancel));
    assert_eq!(host.config_snapshot().revision, revision);
}

#[test]
fn dock_uses_resident_launch_ids_and_groups_running_windows_without_losing_models() {
    let mut host = TestHost::with_plugins([(
        "Applications",
        FakePluginOptions {
            dock: true,
            compositor: true,
            ..Default::default()
        },
    )]);
    outputs(&mut host, true);
    set(
        &mut host,
        "dock.outputs",
        Value::Array(vec![Value::String("*".into())]),
    );
    set(
        &mut host,
        "dock.pinned",
        Value::Array(vec![Value::String("fixture.desktop".into())]),
    );
    let docks = host.docks();
    assert_eq!(docks.len(), 2);
    host.drive(Message::Settings(oxibar::settings::Action::Open));
    assert!(host.settings_window().is_some());
    let action = docks[1].actions[0].clone().unwrap();
    host.drive(Message::FromSurface(
        docks[1].window,
        Box::new(Message::Dock(action)),
    ));
    assert_eq!(host.seen_by_plugin("Applications"), ["launch:7"]);
    assert!(host.settings_window().is_none());
    host.drive(Message::Dock(DockAction::Launch {
        plugin: "Applications".into(),
        index: 7,
        desktop_id: "other.desktop".into(),
    }));
    assert_eq!(host.seen_by_plugin("Applications"), ["launch:7"]);
    let snapshot = oxibar_plugin_api::CompositorSnapshot {
        backend: "fixture".into(),
        windows: vec![oxibar_plugin_api::CompositorWindow {
            id: "0xab".into(),
            pid: 123,
            class: "fixture".into(),
            title: "Fixture window".into(),
            output: Some("DP-2".into()),
            workspace: 2,
            focused: true,
        }],
        ..Default::default()
    };
    host.drive(Message::CompositorChanged(Ok(snapshot)));
    assert!(matches!(
        host.docks()[1].actions[0],
        Some(DockAction::Focus { pid: 123, .. })
    ));
    set(&mut host, "dock.enabled", Value::Boolean(false));
    assert!(host.docks().is_empty());
    assert_eq!(
        host.seen_by_plugin("Applications"),
        ["launch:7", "compositor:1"]
    );
}
