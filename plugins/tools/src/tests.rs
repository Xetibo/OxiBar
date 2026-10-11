use super::*;
use iced::futures::{StreamExt, executor::block_on};
use oxibar_plugin_api::worker::CancelToken;

fn configuration() -> config::Config {
    let table: TomlTable = "[tools]\nfile_roots = []\ncommand_terminal = false"
        .parse()
        .unwrap();
    config::read(&table).unwrap()
}

fn fixture() -> PluginModel {
    let mut model = Model::new(
        configuration(),
        Arc::new(UsageStore::at(
            std::env::temp_dir().join("oxibar-unused-tools-unit-state"),
        )),
    );
    model.index_worker = Err("fixture indexing disabled".into());
    plugin_model(model)
}

fn collect<T: Send + 'static>(task: Task<T>) -> Vec<T> {
    let Some(mut stream) = iced_runtime::task::into_stream(task.collect()) else {
        return Vec::new();
    };
    block_on(async move {
        match stream.next().await {
            Some(iced_runtime::Action::Output(values)) => values,
            None => Vec::new(),
            _ => panic!("expected task output"),
        }
    })
}

#[test]
fn explicit_prefixes_select_typed_arguments_calculator_bookmarks_actions_files_and_history() {
    let mut config = configuration();
    config.clipboard = true;
    config.bookmarks = vec![config::Bookmark {
        id: "docs".into(),
        name: "Documentation".into(),
        url: "https://example.invalid/docs".into(),
    }];
    config.actions = vec![config::NamedAction {
        id: "literal".into(),
        name: "Literal arguments".into(),
        argv: vec!["/bin/printf".into(), "%s".into(), "a b; not shell".into()],
        terminal: false,
        directory: None,
    }];
    let index = index::Index {
        commands: BTreeMap::from([("printf".into(), "/bin/printf".into())]),
        files: vec!["/tmp/Fixture Document.txt".into()],
    };
    let clipboard = clipboard::Snapshot {
        available: true,
        enabled: true,
        entries: Arc::new(vec![clipboard::Entry {
            id: 7,
            text: "private fixture history".into(),
        }]),
        ..Default::default()
    };
    let search = |query| {
        query::search(
            &config,
            &index,
            &clipboard,
            query,
            &BTreeMap::new(),
            &CancelToken::default(),
        )
        .unwrap()
    };
    let command = search("> printf '%s' 'hello world; untouched'");
    assert!(
        matches!(&command[0].intent, query::Intent::Execute { argv, terminal: false, .. } if argv == &["/bin/printf", "%s", "hello world; untouched"])
    );
    assert!(
        matches!(&search("= 6 * 7")[0].intent, query::Intent::Copy { text, identity } if text == "42" && identity == "calculator:42")
    );
    assert!(
        matches!(&search("@ doc")[0].intent, query::Intent::Open(url) if url == "https://example.invalid/docs")
    );
    assert!(
        matches!(&search("! literal")[0].intent, query::Intent::Execute { argv, .. } if argv[2] == "a b; not shell")
    );
    assert!(
        matches!(&search("/ fixture")[0].intent, query::Intent::Open(path) if path == "/tmp/Fixture Document.txt")
    );
    let history = search(": private");
    assert!(history[0].key.is_none());
    assert!(
        matches!(&history[0].intent, query::Intent::Copy { identity, .. } if identity == "clipboard:7")
    );
    assert!(
        query::search(
            &config,
            &index,
            &clipboard::Snapshot::default(),
            ":",
            &BTreeMap::new(),
            &CancelToken::default()
        )
        .is_err()
    );
    let canceled = CancelToken::default();
    canceled.cancel();
    assert!(
        query::search(
            &config,
            &index,
            &clipboard,
            "/",
            &BTreeMap::new(),
            &canceled
        )
        .unwrap()
        .is_empty()
    );
}

#[test]
fn typed_config_validation_rejects_unknown_url_duplicate_identity_and_bad_arguments() {
    for text in [
        "[tools]\nbookmarks = [{id='one', name='One', url='javascript:invalid'}]",
        "[tools]\nbookmarks = [{id='one', name='One', url='https://one.invalid'}, {id='one', name='Two', url='https://two.invalid'}]",
        "[tools]\nactions = [{id='one', name='One', command=\"unclosed '\"}]",
    ] {
        assert!(config_validate(text.parse().unwrap()).is_err());
    }
    let valid = "[tools]\nactions = [{id='one', name='One', command=\"printf '%s' 'a b'\"}]"
        .parse()
        .unwrap();
    assert_eq!(
        config::read(&valid).unwrap().actions[0].argv,
        ["printf", "%s", "a b"]
    );
}

#[test]
fn stable_intents_distinguish_aliases_and_cannot_retarget_removed_or_stale_results() {
    let model = fixture();
    let found = |key: &str, url: &str| query::Found {
        title: key.into(),
        description: "Bookmark".into(),
        key: Some(key.into()),
        intent: query::Intent::Open(url.into()),
        score: 0,
    };
    drop(update(
        model.clone(),
        msg(Message::Matches(
            0,
            Ok(vec![
                found("bookmark:a", "https://same.invalid"),
                found("bookmark:b", "https://same.invalid"),
            ]),
        )),
    ));
    let old = runner_entries(model.clone());
    assert_ne!(old[0].index, old[1].index);
    drop(update(
        model.clone(),
        msg(Message::Matches(
            0,
            Ok(vec![found("bookmark:a", "https://changed.invalid")]),
        )),
    ));
    let new = runner_entries(model.clone())[0].index;
    assert_ne!(new, old[0].index);
    assert!(launch(old[0].index, model.clone()).is_none());
    drop(update(
        model.clone(),
        msg(Message::Matches(
            99,
            Ok(vec![found("bookmark:a", "https://stale.invalid")]),
        )),
    ));
    assert_eq!(runner_entries(model.clone())[0].index, new);
    runner_cancel(model.clone());
    assert!(runner_entries(model).is_empty());
}

#[test]
fn clipboard_text_is_not_retained_in_identity_ledger_usage_or_opt_out_results() {
    let model = fixture();
    let secret = "private fixture clipboard text";
    let found = query::Found {
        title: "History".into(),
        description: String::new(),
        key: None,
        intent: query::Intent::Copy {
            text: secret.into(),
            identity: "clipboard:42".into(),
        },
        score: 0,
    };
    drop(update(
        model.clone(),
        msg(Message::Matches(0, Ok(vec![found]))),
    ));
    drop(update(model.clone(), msg(Message::Launched(None, Ok(())))));
    with_model_read::<Model, _>(&model, |state| {
        assert_eq!(
            state.identities.keys().collect::<Vec<_>>(),
            [&"clipboard:42".to_owned()]
        );
        assert!(state.usage.items.is_empty());
        assert_eq!(state.usage_generation, 0);
    })
    .unwrap();
    with_model_write::<Model, _>(&model, |state| {
        state.config.clipboard = true;
        state.clipboard_snapshot.enabled = true;
        state.clipboard_snapshot.entries = Arc::new(vec![clipboard::Entry {
            id: 42,
            text: secret.into(),
        }]);
    });
    drop(config_changed(model.clone(), TomlTable::new()));
    with_model_read::<Model, _>(&model, |state| {
        assert!(!state.clipboard_snapshot.enabled);
        assert!(state.clipboard_snapshot.entries.is_empty());
        assert!(state.entries.is_empty());
    })
    .unwrap();
}

#[test]
fn offline_copy_returns_actual_failure_without_usage_and_arguments_are_never_implicitly_shell() {
    let model = fixture();
    let results = collect(command(model.clone(), "copy".into(), vec!["fixture".into()]).unwrap());
    assert!(results[0].result.is_err());
    drop(update(model.clone(), results[0].message.clone()));
    assert!(with_model_read::<Model, _>(&model, |state| state.usage.items.is_empty()).unwrap());
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!(
        "oxibar-tools-argv-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    let executable = root.join("fixture");
    let marker = root.join("marker");
    std::fs::write(&executable, "#!/bin/sh\nprintf '%s' \"$1\" > \"$2\"\n").unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    execute(
        configuration(),
        query::Intent::Execute {
            argv: vec![
                executable.to_string_lossy().into_owned(),
                "literal; $not_expanded".into(),
                marker.to_string_lossy().into_owned(),
            ],
            terminal: false,
            directory: Some(root.clone()),
        },
    )
    .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::fs::read(&marker).unwrap_or_default() != b"literal; $not_expanded" {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    std::fs::remove_dir_all(root).unwrap();
}
