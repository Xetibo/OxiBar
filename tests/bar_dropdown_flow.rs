use iced_layershell::reexport::{IcedId, KeyboardInteractivity};
use oxibar::{
    control_center,
    messages::Message,
    session_menu, settings,
    test_support::{FakePluginOptions, TestHost},
};
use toml::Value;

fn set(host: &mut TestHost, path: &str, value: Value) {
    host.drive(Message::Settings(settings::Action::Set(path.into(), value)));
}
fn array(names: &[&str]) -> Value {
    Value::Array(
        names
            .iter()
            .map(|name| Value::String((*name).into()))
            .collect(),
    )
}

#[test]
fn ordinary_dropdowns_and_legacy_panels_close_with_scoped_escape_even_when_captured() {
    let mut host = TestHost::with_plugins([
        ("Audio", FakePluginOptions::popup()),
        ("Notifications", FakePluginOptions::all_surfaces()),
    ]);
    let bar = host.main_window();
    let opened = host.drive(Message::TogglePluginPopup("Audio".into()));
    assert!(opened.iter().any(|message| matches!(message,
        Message::SetToastKeyboardInteractivity(id, KeyboardInteractivity::Exclusive) if *id == bar)));
    let escape = |id| {
        Message::ShortcutKey(
            id,
            iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
            iced::keyboard::Modifiers::empty(),
            iced::event::Status::Captured,
        )
    };
    host.drive(escape(IcedId::unique()));
    assert!(host.popup_open());
    host.drive(escape(bar));
    assert!(!host.popup_open());
    set(
        &mut host,
        "menus.notifications_dropdown",
        Value::Boolean(false),
    );
    let opened = host.drive(Message::TogglePluginPanel("Notifications".into()));
    let panel = opened
        .iter()
        .find_map(|message| match message {
            Message::OnOutput(_, inner) => match inner.as_ref() {
                Message::OpenPanelLayer(id) => Some(*id),
                _ => None,
            },
            _ => None,
        })
        .unwrap();
    host.drive(escape(bar));
    assert!(host.panel_open());
    host.drive(escape(panel));
    assert!(!host.panel_open());
    assert_eq!(host.bar_windows(), [bar]);
}

#[test]
fn session_ipc_open_is_idempotent_and_toggle_close_and_disable_reply_with_state() {
    use oxibar::ipc::{Command, WindowCommand};
    let mut host = TestHost::with_plugins([(
        "System",
        FakePluginOptions {
            session_menu: true,
            ..Default::default()
        },
    )]);
    let command = |host: &mut TestHost, action| host.request(None, Command::Session(action)).1;
    assert_eq!(
        command(&mut host, WindowCommand::Open)["data"]["session_open"],
        true
    );
    let session = host.session_window().unwrap();
    assert_eq!(command(&mut host, WindowCommand::Open)["ok"], true);
    assert_eq!(host.session_window(), Some(session));
    assert_eq!(
        command(&mut host, WindowCommand::Toggle)["data"]["session_open"],
        false
    );
    command(&mut host, WindowCommand::Toggle);
    assert!(host.session_window().is_some());
    assert_eq!(
        command(&mut host, WindowCommand::Close)["data"]["session_open"],
        false
    );
    set(&mut host, "menus.session_enabled", Value::Boolean(false));
    assert_eq!(command(&mut host, WindowCommand::Open)["ok"], false);
    assert_eq!(command(&mut host, WindowCommand::Close)["ok"], true);
    assert!(host.session_window().is_none());
}

#[test]
fn dropdowns_follow_the_pressed_button_instead_of_section_edges_and_update_input_bounds() {
    let mut host = TestHost::with_plugins([("Audio", FakePluginOptions::popup())]);
    set(&mut host, "bar.start", array(&["Audio", "Audio"]));
    let bar = host.main_window();
    host.drive(Message::LayerSurfaceResized(bar, 1000, 791));
    let anchor = |key: &str, x: f32, pressed| Message::BarAnchor {
        id: bar,
        key: key.into(),
        provider: "Audio".into(),
        bounds: iced::Rectangle {
            x,
            y: 4.0,
            width: 50.0,
            height: 22.5,
        },
        pressed,
    };
    host.drive(anchor("start:0:0", 520.0, true));
    let opened = host.drive(Message::TogglePluginPopup("Audio".into()));
    assert!(opened.iter().any(|message| matches!(
        message,
        Message::SetBarInputRegion {
            open: true,
            x: Some(369),
            ..
        }
    )));
    let moved = host.drive(anchor("start:0:0", 600.0, false));
    assert!(moved.iter().any(|message| matches!(
        message,
        Message::SetBarInputRegion {
            open: true,
            x: Some(449),
            ..
        }
    )));
    let edge = host.drive(anchor("start:0:0", 970.0, true));
    assert!(edge.iter().any(|message| matches!(
        message,
        Message::SetBarInputRegion {
            open: true,
            x: Some(648),
            ..
        }
    )));
    let other = host.drive(anchor("start:1:0", 0.0, true));
    assert!(other.iter().any(|message| matches!(
        message,
        Message::SetBarInputRegion {
            open: true,
            x: Some(0),
            ..
        }
    )));
    let removed = host.drive(Message::Settings(settings::Action::Set(
        "bar.start".into(),
        array(&["Audio"]),
    )));
    assert!(removed.iter().any(|message| matches!(
        message,
        Message::SetBarInputRegion {
            open: true,
            x: Some(648),
            ..
        }
    )));
    assert_eq!(host.drive(anchor("start:1:0", 0.0, true)).len(), 1);
    host.drive(Message::SurfaceClosed(bar));
    let retired = host.drive(anchor("start:0:0", 500.0, true));
    assert!(
        !retired
            .iter()
            .any(|message| matches!(message, Message::SetBarInputRegion { id, .. } if *id == bar))
    );
}

#[test]
fn clock_controls_and_notification_inbox_share_the_bar_and_close_without_destroying_it() {
    let mut host = TestHost::with_plugins([
        ("Clock", FakePluginOptions::popup()),
        ("Audio", FakePluginOptions::popup()),
        ("Notifications", FakePluginOptions::all_surfaces()),
    ]);
    let bar = host.main_window();
    host.drive(Message::FromSurface(
        bar,
        Box::new(Message::TogglePluginPopup("Clock".into())),
    ));
    assert_eq!(host.control_center_window(), Some(bar));
    assert_eq!(host.control_center_selected(), Some("Clock"));
    assert_eq!(host.popup_plugin(), Some("@control-center"));
    host.drive(Message::ControlCenterKey(
        IcedId::unique(),
        control_center::Action::Close,
    ));
    assert!(host.popup_open());
    host.drive(Message::ControlCenter(control_center::Action::Select(
        "Audio".into(),
    )));
    assert_eq!(host.control_center_selected(), Some("Audio"));
    host.drive(Message::FromSurface(
        bar,
        Box::new(Message::TogglePluginPopup("Clock".into())),
    ));
    assert!(host.control_center_window().is_none());
    assert!(!host.popup_open());
    assert_eq!(host.bar_windows(), vec![bar]);
    host.drive(Message::TogglePluginPopup("Clock".into()));
    assert_eq!(host.control_center_selected(), Some("Clock"));
    host.drive_plugin_toast_show("Notifications", "existing", 380, 140);
    host.drive(Message::TogglePluginPanel("Notifications".into()));
    assert!(host.control_center_window().is_none());
    assert_eq!(host.popup_plugin(), Some("Notifications"));
    assert!(!host.panel_open());
    assert_eq!(host.toast_count(), 0);
    host.drive_plugin_toast_show("Notifications", "hidden", 380, 140);
    assert_eq!(host.toast_count(), 0);
    let closed = host.drive(Message::SetPopupOpen(false));
    assert!(!host.popup_open());
    assert_eq!(host.bar_windows(), vec![bar]);
    assert!(closed.iter().any(|message| matches!(message, Message::SetToastKeyboardInteractivity(id, KeyboardInteractivity::None) if *id == bar)));
    host.drive_plugin_toast_show("Notifications", "after", 380, 140);
    assert_eq!(host.toast_count(), 1);
    set(&mut host, "control_center.widgets", array(&["Audio"]));
    host.drive(Message::TogglePluginPopup("Clock".into()));
    assert_eq!(host.control_center_selected(), Some("Audio"));
    host.drive(Message::Settings(settings::Action::Open));
    assert!(!host.popup_open());
    assert_eq!(host.bar_windows(), vec![bar]);
}

#[test]
fn session_tiles_use_capabilities_and_open_confirmation_on_the_same_output() {
    let mut host = TestHost::with_plugins([(
        "System",
        FakePluginOptions {
            session_menu: true,
            command: true,
            modal: true,
            ..Default::default()
        },
    )]);
    host.drive(Message::OutputsChanged(Ok(
        oxibar::monitor::OutputSnapshot {
            backend: "fixture".into(),
            outputs: vec![oxibar::monitor::Output {
                name: "sandbox".into(),
                description: String::new(),
                make: String::new(),
                model: String::new(),
                serial: String::new(),
                enabled: true,
                focused: true,
                physical_size: (1920, 1080),
                logical_size: (1920, 1080),
                position: (0, 0),
                scale: 1.0,
                transform: 0,
                refresh_hz: 60.0,
                vrr: None,
                modes: vec![],
            }],
        },
    )));
    let bar = host.main_window();
    let opened = host.drive(Message::FromSurface(
        bar,
        Box::new(Message::Session(session_menu::Action::Toggle)),
    ));
    let session = host.session_window().unwrap();
    assert!(!host.popup_open());
    assert!(opened.iter().any(|message| matches!(message, Message::OnOutput(output, inner) if output.as_deref() == Some("sandbox") && matches!(inner.as_ref(), Message::OpenSessionLayer(id, (800, 480)) if *id == session))));
    host.drive(Message::Session(session_menu::Action::Choose(
        "hibernate".into(),
    )));
    assert!(!host.modal_open());
    host.drive(Message::SessionKey(
        IcedId::unique(),
        session_menu::Action::Choose("logout".into()),
    ));
    assert!(!host.modal_open());
    host.drive(Message::SessionKey(
        session,
        session_menu::Action::Choose("logout".into()),
    ));
    assert!(host.modal_open());
    assert!(host.session_window().is_none());
    assert!(!host.popup_open());
    assert_eq!(host.bar_windows(), vec![bar]);
    assert_eq!(
        host.surface_output(host.modal_window().unwrap()),
        host.surface_output(bar)
    );
    host.drive(Message::FromSurface(
        bar,
        Box::new(Message::Session(session_menu::Action::Toggle)),
    ));
    let reopened = host.session_window().unwrap();
    assert_ne!(reopened, session);
    host.drive(Message::SessionKey(session, session_menu::Action::Close));
    assert_eq!(host.session_window(), Some(reopened));
    set(&mut host, "menus.session_enabled", Value::Boolean(false));
    assert!(host.session_window().is_none());
    assert_eq!(host.bar_windows(), vec![bar]);
}

#[test]
fn layout_drag_is_revision_bound_moves_between_sections_and_preserves_other_settings() {
    let mut host = TestHost::with_plugins([
        ("Clock", FakePluginOptions::popup()),
        ("Audio", FakePluginOptions::popup()),
    ]);
    set(&mut host, "bar.start", array(&["settings", "Audio"]));
    set(&mut host, "bar.center", array(&["Clock"]));
    set(&mut host, "bar.end", array(&[]));
    let revision = host.config_snapshot().revision;
    host.drive(Message::Settings(settings::Action::BeginWidgetDrag {
        section: "start".into(),
        index: 1,
        name: "Audio".into(),
        revision,
    }));
    host.drive(Message::Settings(settings::Action::DropWidget(
        "center".into(),
        0,
    )));
    assert_eq!(host.config_snapshot().host.start, ["settings"]);
    assert_eq!(host.config_snapshot().host.center, ["Audio", "Clock"]);
    let revision = host.config_snapshot().revision;
    host.drive(Message::Settings(settings::Action::BeginWidgetDrag {
        section: "center".into(),
        index: 0,
        name: "Audio".into(),
        revision,
    }));
    set(&mut host, "bar.height", Value::Integer(40));
    host.drive(Message::Settings(settings::Action::DropWidget(
        "end".into(),
        0,
    )));
    assert_eq!(host.config_snapshot().host.center, ["Audio", "Clock"]);
    assert_eq!(host.config_snapshot().host.bar_height, 40);
    assert!(host.settings_error().is_some());
}

#[test]
fn default_launcher_providers_are_sparse_live_settings_and_explicit_disable_wins() {
    let mut host = TestHost::with_plugins(std::iter::empty::<(&str, FakePluginOptions)>());
    host.drive(Message::Settings(settings::Action::EnableDefaultProviders(
        true,
    )));
    let snapshot = host.config_snapshot();
    assert_eq!(
        oxibar::config::get_allowed_plugins(&snapshot.effective),
        ["libapplications.so", "libcommands.so", "libtools.so"]
    );
    assert!(
        !host
            .restart_fields()
            .contains(&"launcher.default_plugins".into())
    );
    host.drive(Message::Settings(settings::Action::EnablePlugin(
        "libtools.so".into(),
        false,
    )));
    assert_eq!(
        oxibar::config::get_allowed_plugins(&host.config_snapshot().effective),
        ["libapplications.so", "libcommands.so"]
    );
    host.drive(Message::Settings(settings::Action::EnableDefaultProviders(
        false,
    )));
    assert!(oxibar::config::get_allowed_plugins(&host.config_snapshot().effective).is_empty());
}

#[test]
fn live_provider_disable_hides_consumers_and_reenable_keeps_model_and_pending_results() {
    let mut host = TestHost::with_plugins([(
        "Applications",
        FakePluginOptions {
            popup: true,
            runner: true,
            ..FakePluginOptions::default()
        },
    )]);
    set(&mut host, "plugins", array(&["libfake.so"]));
    host.set_runner_index("Applications", 41);
    host.drive(Message::PluginSubMsg(
        "Applications".into(),
        oxibar::test_support::record("before-disable"),
    ));
    host.drive(Message::TogglePluginPopup("Applications".into()));
    assert!(host.popup_open());
    set(&mut host, "plugins", array(&[]));
    assert!(!host.popup_open());
    assert!(host.launcher_entries().is_empty());
    host.drive(Message::PluginEmission(
        "Applications".into(),
        oxibar::test_support::record("pending-index-result"),
    ));
    host.drive(Message::PluginEmission(
        "Applications".into(),
        std::sync::Arc::new(oxibar_plugin_api::HOST_REQUEST_TOGGLE_LAUNCHER.to_owned()),
    ));
    assert!(host.launcher_window().is_none());
    set(&mut host, "plugins", array(&["libfake.so"]));
    assert!(
        host.seen_by_plugin("Applications")
            .contains(&"before-disable".into())
    );
    assert!(
        host.seen_by_plugin("Applications")
            .contains(&"pending-index-result".into())
    );
    host.drive(Message::Launcher(oxibar::launcher::Action::Open));
    assert_eq!(host.launcher_entries()[0].1, 41);
    assert!(host.restart_fields().is_empty());
}
