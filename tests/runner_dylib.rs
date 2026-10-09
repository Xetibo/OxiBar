use oxibar::test_support::collect_task;
use oxibar_plugin_api::{
    ABI_VERSION, AbiVersionFn, ConfigChangedFn, ModelFn, PluginMsg, RunnerEntriesFn, RunnerQueryFn,
    SettingsSchemaFn, SubscriptionFn, UpdateFn, stream_from_raw, toml::Table,
};

#[test]
#[ignore = "requires OXIBAR_APPLICATIONS_PLUGIN pointing to a built libapplications.so"]
fn application_dylib_tasks_and_subscription_need_no_host_tokio_reactor() {
    let path = std::env::var_os("OXIBAR_APPLICATIONS_PLUGIN")
        .expect("set OXIBAR_APPLICATIONS_PLUGIN to the packaged plugin library");
    // Keep the library alive until every model, task, stream and result drops.
    let library = unsafe { libloading::Library::new(path) }.unwrap();
    let abi: AbiVersionFn = unsafe { *library.get(b"abi_version").unwrap() };
    let model_fn: ModelFn = unsafe { *library.get(b"model").unwrap() };
    let update: UpdateFn = unsafe { *library.get(b"update").unwrap() };
    let query: RunnerQueryFn = unsafe { *library.get(b"runner_query").unwrap() };
    let entries: RunnerEntriesFn = unsafe { *library.get(b"runner_entries").unwrap() };
    let subscribe: SubscriptionFn = unsafe { *library.get(b"subscription").unwrap() };
    let schema: SettingsSchemaFn = unsafe { *library.get(b"settings_schema").unwrap() };
    let changed: ConfigChangedFn = unsafe { *library.get(b"config_changed").unwrap() };
    assert_eq!(unsafe { abi() }, ABI_VERSION);
    let config: Table =
        toml::from_str("[applications]\nmax_entries = 3\nrefresh_seconds = 1").unwrap();
    let (model, initial) = unsafe { model_fn(config) };
    let mut pending = collect_task(initial.unwrap());
    while let Some(message) = pending.pop() {
        if let Some(task) = unsafe { update(model.clone(), message) } {
            pending.extend(collect_task(task));
        }
    }
    assert!(unsafe { entries(model.clone()) }.len() <= 3);
    assert!(
        unsafe { schema() }
            .iter()
            .any(|field| field.path == "applications.max_entries"
                && field.apply == oxibar_plugin_api::ApplyMode::Live)
    );
    let config: Table = toml::from_str("[applications]\nmax_entries=1\nrefresh_seconds=1").unwrap();
    for message in collect_task(unsafe { changed(model.clone(), config) }.unwrap()) {
        let _ = unsafe { update(model.clone(), message) };
    }
    assert!(unsafe { entries(model.clone()) }.len() <= 1);
    let task =
        unsafe { query("unlikely-oxibar-no-match-zzzzzz".to_owned(), model.clone()) }.unwrap();
    for message in collect_task(task) {
        let _ = unsafe { update(model.clone(), message) };
    }
    assert!(unsafe { entries(model.clone()) }.is_empty());
    let mut stream = unsafe { stream_from_raw(subscribe()) };
    let message: Option<PluginMsg> = iced::futures::executor::block_on(async {
        use iced::futures::StreamExt;
        stream.next().await
    });
    assert!(message.is_some());
    drop(stream);
    drop(message);
    drop(model);
}
