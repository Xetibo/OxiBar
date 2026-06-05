use oxibar::{
    messages::Message,
    test_support::{
        CLOSE_MODAL, FakePluginOptions, OPEN_MODAL, TOGGLE_PANEL, TOGGLE_POPUP, TestHost,
        is_host_request,
    },
};

fn contains_host_request(messages: &[Message], plugin_id: &str, request: &str) -> bool {
    messages
        .iter()
        .any(|message| is_host_request(message, plugin_id, request))
}

#[test]
fn plugin_popup_request_drives_host_popup_state_without_view() {
    let mut host = TestHost::with_plugins([(
        "Clock",
        FakePluginOptions::popup().with_popup_size(360, 280),
    )]);

    let applied = host.drive_plugin_request("Clock", TOGGLE_POPUP);

    assert!(contains_host_request(&applied, "Clock", TOGGLE_POPUP));
    assert_eq!(host.popup_plugin(), Some("Clock"));
    assert!(host.popup_open());
    assert_eq!(host.seen_by_plugin("Clock"), vec![TOGGLE_POPUP]);
}

#[test]
fn popup_request_uses_input_metadata_for_detached_overlays() {
    let mut host = TestHost::with_plugins([(
        "Tray",
        FakePluginOptions::popup()
            .with_popup_size(300, 180)
            .with_popup_input_size(300, 420),
    )]);

    let applied = host.drive_plugin_request("Tray", TOGGLE_POPUP);

    assert!(
        applied
            .iter()
            .any(|message| { matches!(message, Message::SetPopupInputRegion(true, _, 332, 420)) })
    );
}

#[test]
fn repeated_popup_request_closes_open_popup_without_clearing_plugin() {
    let mut host = TestHost::with_plugins([("Clock", FakePluginOptions::popup())]);

    host.drive_plugin_request("Clock", TOGGLE_POPUP);
    host.drive_plugin_request("Clock", TOGGLE_POPUP);

    assert_eq!(host.popup_plugin(), Some("Clock"));
    assert!(!host.popup_open());
}

#[test]
fn popup_request_is_ignored_when_plugin_has_no_popup_view() {
    let mut host = TestHost::with_plugins([("Clock", FakePluginOptions::default())]);

    host.drive_plugin_request("Clock", TOGGLE_POPUP);

    assert_eq!(host.popup_plugin(), None);
    assert!(!host.popup_open());
}

#[test]
fn modal_open_and_close_requests_drive_host_modal_state_without_view() {
    let mut host = TestHost::with_plugins([("Bluetooth", FakePluginOptions::modal())]);

    let open = host.drive_plugin_request("Bluetooth", OPEN_MODAL);

    assert!(contains_host_request(&open, "Bluetooth", OPEN_MODAL));
    assert_eq!(host.modal_plugin(), Some("Bluetooth"));
    assert!(host.modal_open());

    let close = host.drive_plugin_request("Bluetooth", CLOSE_MODAL);

    assert!(contains_host_request(&close, "Bluetooth", CLOSE_MODAL));
    assert_eq!(host.modal_plugin(), None);
    assert!(!host.modal_open());
}

#[test]
fn panel_toggle_request_opens_and_closes_panel_without_view() {
    let mut host = TestHost::with_plugins([("Notifications", FakePluginOptions::panel())]);

    let open = host.drive_plugin_request("Notifications", TOGGLE_PANEL);

    assert!(contains_host_request(&open, "Notifications", TOGGLE_PANEL));
    assert_eq!(host.panel_plugin(), Some("Notifications"));
    assert!(host.panel_open());

    host.drive_plugin_request("Notifications", TOGGLE_PANEL);

    assert_eq!(host.panel_plugin(), None);
    assert!(!host.panel_open());
}

#[test]
fn local_plugin_messages_update_plugin_model_but_do_not_change_host_state() {
    let mut host = TestHost::with_plugins([("Audio", FakePluginOptions::all_surfaces())]);

    let applied = host.drive_plugin_record("Audio", "local-only");

    assert!(matches!(applied.as_slice(), [Message::PluginSubMsg(_, _)]));
    assert_eq!(host.seen_by_plugin("Audio"), vec!["local-only"]);
    assert_eq!(host.popup_plugin(), None);
    assert_eq!(host.modal_plugin(), None);
    assert_eq!(host.panel_plugin(), None);
}

#[test]
fn unknown_plugin_message_is_dropped_without_state_change() {
    let mut host = TestHost::with_plugins([("Clock", FakePluginOptions::all_surfaces())]);

    host.drive_plugin_request("Missing", TOGGLE_POPUP);

    assert_eq!(host.popup_plugin(), None);
    assert!(!host.popup_open());
    assert_eq!(host.seen_by_plugin("Clock"), Vec::<String>::new());
}

#[test]
fn plugin_toast_request_opens_and_closes_host_toast_without_view_interaction() {
    let mut host = TestHost::with_plugins([("Notifications", FakePluginOptions::toast())]);

    host.drive_plugin_toast_show("Notifications", "n1", 380, 144);

    assert_eq!(host.toast_ids(), vec!["n1"]);
    assert_eq!(host.toast_count(), 1);

    host.drive_plugin_toast_close("Notifications", "n1");

    assert_eq!(host.toast_count(), 0);
}

#[test]
fn toast_request_is_ignored_while_panel_is_open() {
    let mut host = TestHost::with_plugins([("Notifications", FakePluginOptions::all_surfaces())]);

    host.drive_plugin_request("Notifications", TOGGLE_PANEL);
    host.drive_plugin_toast_show("Notifications", "n1", 380, 144);

    assert!(host.panel_open());
    assert_eq!(host.toast_count(), 0);
}

#[test]
fn opening_panel_closes_existing_toasts() {
    let mut host = TestHost::with_plugins([("Notifications", FakePluginOptions::all_surfaces())]);

    host.drive_plugin_toast_show("Notifications", "n1", 380, 144);
    host.drive_plugin_request("Notifications", TOGGLE_PANEL);

    assert!(host.panel_open());
    assert_eq!(host.toast_count(), 0);
}
