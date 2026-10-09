use iced_layershell::reexport::IcedId;
use oxibar::{
    config::store::get_path,
    messages::Message,
    settings::Action,
    test_support::{FakePluginOptions, TestHost},
};
use toml::Value;

#[test]
fn live_height_changes_surface_and_input_region_without_replacing_plugins() {
    let mut host = TestHost::with_plugins([("Clock", FakePluginOptions::popup())]);
    let bar = IcedId::unique();
    host.drive(Message::LayerSurfaceResized(bar, 1920, 451));
    let messages = host.drive(Message::Settings(Action::Set(
        "bar.height".to_owned(),
        Value::Integer(38),
    )));
    assert_eq!(host.bar_size().height, 38);
    assert!(
        messages
            .iter()
            .any(|message| matches!(message, Message::ResizeLayer(id, 0, 458) if *id == bar))
    );
    assert!(
        messages
            .iter()
            .any(|message| matches!(message, Message::SetExclusiveZone(id, 38) if *id == bar))
    );
    assert!(messages.iter().any(|message| matches!(message, Message::SetPopupInputRegion { bar_size, .. } if bar_size.height == 38)));
    assert!(host.restart_fields().is_empty());
    host.drive_plugin_record("Clock", "still alive");
    assert_eq!(host.seen_by_plugin("Clock"), ["still alive"]);
    host.drive(Message::Settings(Action::Reset("bar.height".to_owned())));
    assert_eq!(host.bar_size().height, 31);
}

#[test]
fn settings_and_launcher_release_each_others_keyboard_surfaces() {
    let mut host = TestHost::with_plugins([("Apps", FakePluginOptions::runner(10))]);
    host.drive(Message::Launcher(oxibar::launcher::Action::Open));
    host.drive(Message::Settings(Action::Open));
    assert!(host.launcher_window().is_none());
    let settings = host.settings_window().unwrap();
    host.drive(Message::SettingsEscape(IcedId::unique()));
    assert_eq!(host.settings_window(), Some(settings));
    host.drive(Message::Launcher(oxibar::launcher::Action::Open));
    assert!(host.settings_window().is_none());
    assert!(host.launcher_window().is_some());
    host.drive(Message::Settings(Action::Open));
    host.drive(Message::SettingsEscape(host.settings_window().unwrap()));
    assert!(host.settings_window().is_none());
}

#[test]
fn plugin_hooks_use_schema_paths_independently_of_the_display_name() {
    let mut host = TestHost::with_plugins([("DifferentName", FakePluginOptions::live_settings())]);
    host.drive(Message::Settings(Action::Set(
        "custom.value".to_owned(),
        Value::Integer(12),
    )));
    assert_eq!(host.seen_by_plugin("DifferentName"), ["config:12"]);
    assert!(host.restart_fields().is_empty());
    host.drive(Message::Settings(Action::Set(
        "plugins".to_owned(),
        Value::Array(vec![Value::String("libfake.so".to_owned())]),
    )));
    assert_eq!(host.restart_fields(), ["plugins"]);
    host.drive_plugin_record("DifferentName", "still loaded");
    assert!(
        host.seen_by_plugin("DifferentName")
            .contains(&"still loaded".to_owned())
    );
}

#[test]
fn stale_ui_drafts_do_not_overwrite_newer_configuration() {
    let mut host = TestHost::with_plugins(std::iter::empty::<(&str, FakePluginOptions)>());
    host.drive(Message::Settings(Action::Edit(
        "bar.height".to_owned(),
        "40".to_owned(),
    )));
    host.drive(Message::Settings(Action::Set(
        "bar.transparent".to_owned(),
        Value::Boolean(true),
    )));
    host.drive(Message::Settings(Action::Apply("bar.height".to_owned())));
    assert_eq!(host.config_snapshot().host.bar_height, 31);
    assert!(host.config_snapshot().host.transparent);
    assert!(
        host.settings_error()
            .is_some_and(|error| error.contains("changed"))
    );
}

#[test]
fn editing_fallback_layout_preserves_other_sections_and_explicit_empty_lists() {
    let mut host = TestHost::with_plugins([("Clock", FakePluginOptions::popup())]);
    host.drive(Message::Settings(Action::SelectWidget(
        "center".to_owned(),
        "settings".to_owned(),
    )));
    host.drive(Message::Settings(Action::AddWidget("center".to_owned())));
    assert_eq!(host.widgets().0, ["Clock"]);
    assert_eq!(host.widgets().1, ["settings"]);
    assert_eq!(
        get_path(&host.config_snapshot().overrides, "bar.start"),
        Some(&Value::Array(vec![Value::String("Clock".to_owned())]))
    );
    host.drive(Message::Settings(Action::RemoveWidget(
        "start".to_owned(),
        0,
    )));
    host.drive(Message::Settings(Action::RemoveWidget(
        "center".to_owned(),
        0,
    )));
    assert!(
        host.widgets().0.is_empty() && host.widgets().1.is_empty() && host.widgets().2.is_empty()
    );
    host.drive(Message::Settings(Action::ResetAll));
    assert!(host.widgets().0.contains(&"Clock".to_owned()));
}
