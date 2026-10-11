use std::sync::Arc;

use oxibar::{
    config::{ConfigStore, store::Mutation},
    launcher::Action,
    messages::Message,
    test_support::{FakePluginOptions, TestHost},
};
use oxibar_plugin_api::toml::{Table, Value};

#[test]
fn explicit_prefixes_cancel_other_providers_and_unprefixed_queries_resume_them() {
    let mut host = TestHost::with_plugins([
        (
            "Apps",
            FakePluginOptions {
                runner_prefix: Some("a"),
                ..FakePluginOptions::runner(10)
            },
        ),
        (
            "Tools",
            FakePluginOptions {
                runner_prefix: Some("ab"),
                ..FakePluginOptions::runner(20)
            },
        ),
        ("Legacy", FakePluginOptions::runner(30)),
    ]);
    host.drive(Message::Launcher(Action::Open));
    assert_eq!(host.launcher_entries().len(), 3);
    host.drive(Message::Launcher(Action::Query("ab term".into())));
    assert_eq!(
        host.launcher_entries()
            .iter()
            .map(|entry| entry.0.as_str())
            .collect::<Vec<_>>(),
        ["Tools"]
    );
    assert!(
        host.seen_by_plugin("Apps")
            .contains(&"runner canceled".into())
    );
    assert!(
        !host
            .seen_by_plugin("Legacy")
            .contains(&"query:ab term".into())
    );
    host.drive(Message::Launcher(Action::Query("term".into())));
    assert_eq!(host.launcher_entries().len(), 3);
    assert!(host.seen_by_plugin("Apps").contains(&"query:term".into()));
    host.drive(Message::Launcher(Action::Close));
    assert!(host.launcher_window().is_none());
    assert!(
        host.seen_by_plugin("Tools")
            .contains(&"runner canceled".into())
    );
}

#[test]
fn immutable_action_identity_and_sparse_live_favorites_survive_result_changes() {
    let mut host = TestHost::with_plugins([(
        "Apps",
        FakePluginOptions {
            runner_favorites: true,
            ..FakePluginOptions::runner(10)
        },
    )]);
    host.drive(Message::Launcher(Action::Open));
    host.drive(Message::Launcher(Action::Favorite("Apps".into(), 42)));
    assert_eq!(host.launcher_entries(), [("Apps".into(), 42, Some(true))]);
    assert_eq!(
        oxibar::config::store::get_path(&host.config_snapshot().overrides, "custom.favorites")
            .and_then(Value::as_array)
            .unwrap(),
        &[Value::String("result:42".into())]
    );
    host.set_runner_index("Apps", 99);
    host.drive_plugin_record("Apps", "new result");
    host.drive(Message::Launcher(Action::Activate("Apps".into(), 42)));
    assert!(host.launcher_window().is_some());
    assert!(!host.seen_by_plugin("Apps").contains(&"launch:42".into()));
    host.drive(Message::Launcher(Action::Activate("Apps".into(), 99)));
    assert!(host.seen_by_plugin("Apps").contains(&"launch:99".into()));
    assert!(host.launcher_window().is_none());
}

unsafe extern "Rust" fn validate_records(config: Table) -> Result<(), String> {
    if oxibar::config::store::get_path(&config, "custom.records")
        .and_then(Value::as_array)
        .is_some_and(|values| {
            values.iter().any(|value| {
                value
                    .as_table()
                    .and_then(|table| table.get("url"))
                    .and_then(Value::as_str)
                    .is_none_or(|url| !url.starts_with("https://"))
            })
        })
    {
        return Err("each record needs an https URL".into());
    }
    Ok(())
}

#[test]
fn provider_validation_pins_library_and_rejects_candidate_before_adoption() {
    let mut store = ConfigStore::memory(Table::new(), Table::new());
    let library: Arc<libloading::Library> = Arc::new(libloading::os::unix::Library::this().into());
    let weak = Arc::downgrade(&library);
    store
        .register_validator("Records".into(), validate_records, library)
        .unwrap();
    assert!(weak.upgrade().is_some());
    let revision = store.snapshot().revision;
    let records = "url = 'ftp://invalid'".parse::<Table>().unwrap();
    assert!(
        store
            .transact(
                Mutation::Set(
                    "custom.records".into(),
                    Value::Array(vec![Value::Table(records)])
                ),
                Some(revision)
            )
            .unwrap_err()
            .contains("Records")
    );
    assert_eq!(store.snapshot().revision, revision);
    assert!(
        oxibar::config::store::get_path(&store.snapshot().overrides, "custom.records").is_none()
    );
    let records = "url = 'https://example.invalid'".parse::<Table>().unwrap();
    assert!(
        store
            .transact(
                Mutation::Set(
                    "custom.records".into(),
                    Value::Array(vec![Value::Table(records)])
                ),
                Some(revision)
            )
            .is_ok()
    );
    drop(store);
    assert!(weak.upgrade().is_none());
}
