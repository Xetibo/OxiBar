use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use toml::{Table, Value};

use super::{
    ConfigSnapshot, ConfigStore, Mutation, Source, schema,
    store::{get_path, merge},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "oxibar-settings-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn base(&self, text: &str) {
        fs::write(self.0.join("config.toml"), text).unwrap();
    }
    fn open(&self) -> ConfigStore {
        ConfigStore::open(self.0.clone())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn merge_replaces_arrays_and_preserves_unknown_tables() {
    let mut base: Table =
        toml::from_str("[bar]\nstart=['clock', 'audio']\n[custom.future]\nflag=true").unwrap();
    let layer: Table = toml::from_str("[bar]\nstart=[]").unwrap();
    merge(&mut base, &layer);
    assert_eq!(
        get_path(&base, "bar.start"),
        Some(&Value::Array(Vec::new()))
    );
    assert_eq!(
        get_path(&base, "custom.future.flag"),
        Some(&Value::Boolean(true))
    );
}

#[test]
fn read_only_nix_symlink_and_gui_intent_survive_regeneration_reset_and_restart() {
    let fixture = Fixture::new();
    let first = fixture.0.join("nix-first.toml");
    fs::write(&first, "[bar]\nheight=31\n[launcher]\nmax_entries=7").unwrap();
    fs::set_permissions(&first, fs::Permissions::from_mode(0o444)).unwrap();
    symlink(&first, fixture.0.join("config.toml")).unwrap();
    let mut store = fixture.open();
    store
        .transact(
            Mutation::Set("bar.height".to_owned(), Value::Integer(31)),
            Some(store.snapshot().revision),
        )
        .unwrap();
    assert_eq!(store.snapshot().source("bar.height"), Source::Gui);
    assert!(fixture.0.join("config.toml").is_symlink());
    assert_eq!(
        fs::read_to_string(&first).unwrap(),
        "[bar]\nheight=31\n[launcher]\nmax_entries=7"
    );
    let second = fixture.0.join("nix-second.toml");
    fs::write(&second, "[bar]\nheight=34\n[launcher]\nmax_entries=10").unwrap();
    let replacement = fixture.0.join("base-replacement");
    symlink(&second, &replacement).unwrap();
    fs::rename(replacement, fixture.0.join("config.toml")).unwrap();
    store.reload().unwrap();
    assert_eq!(store.snapshot().host.bar_height, 31);
    assert_eq!(store.snapshot().host.launcher_limit, 10);
    let mut restarted = fixture.open();
    assert_eq!(restarted.snapshot().host.bar_height, 31);
    restarted
        .transact(Mutation::Reset("bar.height".to_owned()), None)
        .unwrap();
    assert_eq!(restarted.snapshot().host.bar_height, 34);
    assert_eq!(restarted.snapshot().source("bar.height"), Source::Base);
    assert!(restarted.snapshot().overrides.is_empty());
}

#[test]
fn defaults_are_not_frozen_and_reset_prunes_only_empty_parents() {
    let fixture = Fixture::new();
    let mut store = fixture.open();
    store
        .transact(
            Mutation::Set("custom.future.flag".to_owned(), Value::Boolean(true)),
            None,
        )
        .unwrap();
    store
        .transact(
            Mutation::Set("bar.height".to_owned(), Value::Integer(38)),
            None,
        )
        .unwrap();
    let text = fs::read_to_string(fixture.0.join("overrides.toml")).unwrap();
    assert!(!text.contains("launcher"));
    store
        .transact(Mutation::Reset("bar.height".to_owned()), None)
        .unwrap();
    assert!(!store.snapshot().overrides.contains_key("bar"));
    assert_eq!(
        get_path(&store.snapshot().overrides, "custom.future.flag"),
        Some(&Value::Boolean(true))
    );
}

#[test]
fn invalid_external_edits_and_failed_mutations_retain_last_valid_snapshot() {
    let fixture = Fixture::new();
    fixture.base("[bar]\nheight=35");
    let mut store = fixture.open();
    let revision = store.snapshot().revision;
    assert!(
        store
            .transact(
                Mutation::Set("bar.height".to_owned(), Value::Integer(-1)),
                None
            )
            .is_err()
    );
    assert_eq!(store.snapshot().revision, revision);
    assert!(!fixture.0.join("overrides.toml").exists());
    fixture.base("[bar\ninvalid");
    assert!(store.reload().is_err());
    assert_eq!(store.snapshot().host.bar_height, 35);
    assert_eq!(store.snapshot().revision, revision);
    assert!(store.transact(Mutation::ResetAll, None).is_err());
    fixture.base("[bar]\nheight=36");
    assert!(store.reload().unwrap());
    assert_eq!(store.snapshot().host.bar_height, 36);
    assert!(store.error.is_none());
}

#[test]
fn revisions_protect_stale_edits_and_self_writes_do_not_reload() {
    let fixture = Fixture::new();
    let mut store = fixture.open();
    let revision = store.snapshot().revision;
    store
        .transact(
            Mutation::Set("bar.height".to_owned(), Value::Integer(38)),
            Some(revision),
        )
        .unwrap();
    assert!(!store.reload().unwrap());
    assert!(
        store
            .transact(
                Mutation::Set("launcher.max_entries".to_owned(), Value::Integer(10)),
                Some(revision)
            )
            .is_err()
    );
    fixture.base("[launcher]\nmax_entries=12");
    let revision = store.snapshot().revision;
    assert!(store.transact(Mutation::ResetAll, Some(revision)).is_err());
    assert_eq!(store.snapshot().host.launcher_limit, 12);
    assert_eq!(store.snapshot().host.bar_height, 38);
}

#[test]
fn writes_are_private_and_cannot_replace_managed_override_symlinks() {
    let fixture = Fixture::new();
    let mut store = fixture.open();
    store
        .transact(
            Mutation::Set("bar.height".to_owned(), Value::Integer(38)),
            None,
        )
        .unwrap();
    let overrides = fixture.0.join("overrides.toml");
    assert_eq!(
        fs::metadata(&overrides).unwrap().permissions().mode() & 0o777,
        0o600
    );
    fs::remove_file(&overrides).unwrap();
    let managed = fixture.0.join("managed.toml");
    fs::write(&managed, "[bar]\nheight=40").unwrap();
    symlink(&managed, &overrides).unwrap();
    assert!(
        store
            .transact(
                Mutation::Set("bar.height".to_owned(), Value::Integer(41)),
                None
            )
            .is_err()
    );
    assert!(overrides.is_symlink());
    assert_eq!(fs::read_to_string(managed).unwrap(), "[bar]\nheight=40");
    assert_eq!(store.snapshot().host.bar_height, 40);
}

#[test]
fn parent_type_changes_are_validated_and_unknown_values_round_trip() {
    let base: Table = toml::from_str("[bar]\nheight=31\n[custom]\nvalues=[1,2,3]").unwrap();
    let overrides: Table = toml::from_str("bar='wrong'").unwrap();
    assert!(
        ConfigSnapshot::from_tables(base.clone(), overrides, schema::host_schema(), 1).is_err()
    );
    let snapshot =
        ConfigSnapshot::from_tables(base, Table::new(), schema::host_schema(), 1).unwrap();
    assert_eq!(snapshot.source("bar.height"), Source::Base);
    assert_eq!(snapshot.source("launcher.max_entries"), Source::Builtin);
    assert_eq!(
        get_path(&snapshot.effective, "custom.values")
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        3
    );
}

#[test]
fn invalid_plugin_schema_and_command_arguments_are_rejected_without_panics() {
    use oxibar_plugin_api::{ApplyMode, SettingDescriptor, SettingKind};
    let fixture = Fixture::new();
    let mut store = fixture.open();
    assert!(
        store
            .register_schema(vec![SettingDescriptor::new(
                "plugins.clock.format",
                "Conflicting namespace",
                SettingKind::Text,
                Some(Value::String("%H".to_owned())),
                ApplyMode::Live
            )])
            .is_err()
    );
    store
        .register_schema(vec![SettingDescriptor::new(
            "applications.terminal",
            "Terminal",
            SettingKind::Arguments,
            Some(Value::String("kitty".to_owned())),
            ApplyMode::Live,
        )])
        .unwrap();
    assert!(
        store
            .transact(
                Mutation::Set(
                    "applications.terminal".to_owned(),
                    Value::String("kitty 'unterminated".to_owned())
                ),
                None
            )
            .is_err()
    );
    assert!(
        store
            .transact(
                Mutation::Set("launcher.width".to_owned(), Value::Integer(i32::MAX as i64)),
                None
            )
            .is_err()
    );
}
