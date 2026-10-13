use super::*;
use oxibar_plugin_api::worker::CancelToken;

fn config() -> config::Config {
    config::read(&TomlTable::new()).unwrap()
}

#[test]
fn commands_are_prefix_scoped_and_arguments_are_literal() {
    let index = BTreeMap::from([("printf".into(), "/bin/printf".into())]);
    let search = |text| {
        query::search(
            &config(),
            &index,
            text,
            &BTreeMap::new(),
            &CancelToken::default(),
        )
        .unwrap()
    };
    assert!(search("printf").is_empty());
    assert_eq!(
        search("> printf '%s' 'hello world; untouched'")[0].argv,
        ["/bin/printf", "%s", "hello world; untouched"]
    );
    assert_eq!(search("> printf").len(), 1);
    assert_eq!(search("> prtf")[0].argv, ["/bin/printf"]);
    assert!(
        query::search(
            &config(),
            &index,
            "> missing arg",
            &BTreeMap::new(),
            &CancelToken::default()
        )
        .is_err()
    );
    let mut config = config();
    config.search_unprefixed = true;
    assert_eq!(
        query::search(
            &config,
            &index,
            "printf",
            &BTreeMap::new(),
            &CancelToken::default()
        )
        .unwrap()
        .len(),
        1
    );
}

#[test]
fn cancelled_search_and_old_result_ids_cannot_launch() {
    let cancel = CancelToken::default();
    cancel.cancel();
    assert!(
        query::search(&config(), &BTreeMap::new(), ">", &BTreeMap::new(), &cancel)
            .unwrap()
            .is_empty()
    );
    let root = std::env::temp_dir().join(format!("oxibar-commands-{}", std::process::id()));
    let mut state = Model::new(config(), Arc::new(UsageStore::at(root)));
    let mut found = query::Found {
        title: "Run command".into(),
        key: "command:fixture".into(),
        argv: vec!["/private/fixture".into(), "first".into()],
        score: 1,
    };
    state.matches(std::slice::from_ref(&found));
    let old = state.entries[0].id;
    assert_eq!(
        state.entries[0].argv,
        ["kitty", "/private/fixture", "first"]
    );
    found.argv[1] = "second".into();
    state.matches(&[found]);
    assert_ne!(state.entries[0].id, old);
    let model = plugin_model(state);
    assert!(launch(old, model.clone()).is_none());
    runner_cancel(model.clone());
    assert!(runner_entries(model).is_empty());
}

#[test]
fn path_index_keeps_first_executable_and_follows_nix_profile_symlinks() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let root = std::env::temp_dir().join(format!("oxibar-command-index-{}", std::process::id()));
    let first = root.join("first");
    let second = root.join("second");
    std::fs::create_dir_all(&first).unwrap();
    std::fs::create_dir_all(&second).unwrap();
    let executable = second.join("fixture");
    std::fs::write(&executable, "fixture").unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    symlink(&executable, first.join("fixture")).unwrap();
    std::fs::write(first.join("not-executable"), "fixture").unwrap();
    let index = index::read(&[first.clone(), second], &CancelToken::default());
    assert_eq!(index.len(), 1);
    assert_eq!(index["fixture"], first.join("fixture"));
    let found = query::search(
        &config(),
        &index,
        &format!("> {} 'literal; argument'", executable.display()),
        &BTreeMap::new(),
        &CancelToken::default(),
    )
    .unwrap();
    assert_eq!(
        found[0].argv,
        [executable.to_str().unwrap(), "literal; argument"]
    );
    std::fs::remove_dir_all(root).unwrap();
}
