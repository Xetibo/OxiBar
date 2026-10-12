use oxibar_plugin_api::{
    ABI_VERSION, AbiVersionFn, ErrorsFn, ModelFn, SubscriptionFn, SubscriptionPolicy,
    SubscriptionPolicyFn, UpdateFn, stream_from_raw, toml::Table,
};

#[test]
#[ignore = "requires a built OXIBAR_OXIPASTE_PLUGIN, owned private Wayland and session bus"]
fn oxipaste_dylib_backend_needs_no_host_tokio_reactor() {
    assert_eq!(std::env::var("OXIBAR_TEST_PRIVATE_BUS").as_deref(), Ok("1"));
    assert_eq!(
        std::env::var("OXIBAR_TEST_PRIVATE_WAYLAND").as_deref(),
        Ok("1")
    );
    let path = std::env::var_os("OXIBAR_OXIPASTE_PLUGIN")
        .expect("set OXIBAR_OXIPASTE_PLUGIN to liboxipaste.so");
    let library = unsafe { libloading::Library::new(path) }.unwrap();
    let abi: AbiVersionFn = unsafe { *library.get(b"abi_version").unwrap() };
    let model_fn: ModelFn = unsafe { *library.get(b"model").unwrap() };
    let update: UpdateFn = unsafe { *library.get(b"update").unwrap() };
    let errors: ErrorsFn = unsafe { *library.get(b"errors").unwrap() };
    let subscribe: SubscriptionFn = unsafe { *library.get(b"subscription").unwrap() };
    let policy: SubscriptionPolicyFn = unsafe { *library.get(b"subscription_policy").unwrap() };
    assert_eq!(unsafe { abi() }, ABI_VERSION);
    let config: Table = toml::from_str("[oxipaste]\npoll_seconds=1").unwrap();
    let (model, initial) = unsafe { model_fn(config) };
    assert!(initial.is_none());
    assert_eq!(unsafe { policy() }, SubscriptionPolicy::Always);
    let mut stream = unsafe { stream_from_raw(subscribe()) };
    iced::futures::executor::block_on(async {
        use iced::futures::{
            StreamExt,
            future::{Either, select},
        };
        let message = stream
            .next()
            .await
            .expect("bundled backend startup snapshot");
        assert!(unsafe { update(model.clone(), message) }.is_none());
        let mut replacement = unsafe { stream_from_raw(subscribe()) };
        let pending = match select(
            Box::pin(replacement.next()),
            Box::pin(async_io::Timer::after(std::time::Duration::from_millis(
                100,
            ))),
        )
        .await
        {
            Either::Right((_, pending)) => pending,
            Either::Left(_) => panic!("A second subscription must not start a duplicate backend"),
        };
        drop(stream);
        let restarted = match select(
            pending,
            Box::pin(async_io::Timer::after(std::time::Duration::from_secs(10))),
        )
        .await
        {
            Either::Left((Some(message), _)) => message,
            _ => panic!("Dropping the owner must let the replacement backend start"),
        };
        assert!(unsafe { update(model.clone(), restarted) }.is_none());
        drop(replacement);
    });
    assert!(unsafe { errors(model.clone()) }.is_empty());
    drop(model);
}
