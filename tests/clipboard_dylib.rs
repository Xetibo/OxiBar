use oxibar::test_support::collect_task;
use oxibar_plugin_api::{
    ABI_VERSION, AbiVersionFn, ErrorsFn, ModelFn, SubscriptionFn, UpdateFn, stream_from_raw,
    toml::Table,
};

#[test]
#[ignore = "requires a built OXIBAR_OXIPASTE_PLUGIN and an owned private session bus"]
fn oxipaste_dylib_tasks_and_timer_need_no_host_tokio_reactor() {
    assert_eq!(std::env::var("OXIBAR_TEST_PRIVATE_BUS").as_deref(), Ok("1"));
    let path = std::env::var_os("OXIBAR_OXIPASTE_PLUGIN")
        .expect("set OXIBAR_OXIPASTE_PLUGIN to liboxipaste.so");
    let library = unsafe { libloading::Library::new(path) }.unwrap();
    let abi: AbiVersionFn = unsafe { *library.get(b"abi_version").unwrap() };
    let model_fn: ModelFn = unsafe { *library.get(b"model").unwrap() };
    let update: UpdateFn = unsafe { *library.get(b"update").unwrap() };
    let errors: ErrorsFn = unsafe { *library.get(b"errors").unwrap() };
    let subscribe: SubscriptionFn = unsafe { *library.get(b"subscription").unwrap() };
    assert_eq!(unsafe { abi() }, ABI_VERSION);
    let config: Table = toml::from_str("[oxipaste]\npoll_seconds=1").unwrap();
    let (model, initial) = unsafe { model_fn(config) };
    let mut pending = collect_task(initial.unwrap());
    while let Some(message) = pending.pop() {
        if let Some(task) = unsafe { update(model.clone(), message) } {
            pending.extend(collect_task(task));
        }
    }
    // The private bus has no history daemon; failure must be visible, not a panic.
    assert!(!unsafe { errors(model.clone()) }.is_empty());
    let mut stream = unsafe { stream_from_raw(subscribe()) };
    iced::futures::executor::block_on(async {
        use iced::futures::StreamExt;
        assert!(stream.next().await.is_some());
        assert!(stream.next().await.is_some());
    });
    drop(stream);
    drop(model);
}
