use std::sync::Arc;

use iced::futures::{FutureExt, StreamExt};
use oxibar::test_support::collect_task;
use oxibar_plugin_api::{
    ABI_VERSION, AbiVersionFn, CommandFn, ConfigChangedFn, ConfigValidateFn, DesktopViewFn,
    ModalClosedFn, ModelFn, NameFn, PluginViewContext, SettingsSchemaFn, SubscriptionFn,
    SubscriptionPolicy, SubscriptionPolicyFn, ThemeChangedFn, UpdateFn, ViewFn, stream_from_raw,
    toml::Table,
};

#[test]
#[ignore = "requires OXIBAR_EXAMPLE_PLUGIN pointing to the Nix-built SDK example"]
fn example_contract_owns_shared_models_and_cancels_streams_across_the_real_abi() {
    let path = std::env::var_os("OXIBAR_EXAMPLE_PLUGIN").expect("set OXIBAR_EXAMPLE_PLUGIN");
    let library = unsafe { libloading::Library::new(path) }.unwrap();
    let abi: AbiVersionFn = unsafe { *library.get(b"abi_version").unwrap() };
    let name: NameFn = unsafe { *library.get(b"name").unwrap() };
    let model_fn: ModelFn = unsafe { *library.get(b"model").unwrap() };
    let update: UpdateFn = unsafe { *library.get(b"update").unwrap() };
    let view: ViewFn = unsafe { *library.get(b"view").unwrap() };
    let desktop: DesktopViewFn = unsafe { *library.get(b"desktop_view").unwrap() };
    let command: CommandFn = unsafe { *library.get(b"command").unwrap() };
    let schema: SettingsSchemaFn = unsafe { *library.get(b"settings_schema").unwrap() };
    let validate: ConfigValidateFn = unsafe { *library.get(b"config_validate").unwrap() };
    let changed: ConfigChangedFn = unsafe { *library.get(b"config_changed").unwrap() };
    let closed: ModalClosedFn = unsafe { *library.get(b"modal_closed").unwrap() };
    let subscribe: SubscriptionFn = unsafe { *library.get(b"subscription").unwrap() };
    let policy: SubscriptionPolicyFn = unsafe { *library.get(b"subscription_policy").unwrap() };
    let theme: ThemeChangedFn = unsafe { *library.get(b"theme_changed").unwrap() };
    assert_eq!(unsafe { abi() }, ABI_VERSION);
    assert_eq!(unsafe { name() }, "Example");
    assert_eq!(unsafe { policy() }, SubscriptionPolicy::WhenVisible);
    assert_eq!(unsafe { schema() }[0].path, "example.label");
    assert!(unsafe { validate("[example]\nlabel=''".parse().unwrap()) }.is_err());

    for index in 0..25 {
        let (model, initial) = unsafe { model_fn(Table::new()) };
        assert!(initial.is_none());
        unsafe {
            theme(
                model.clone(),
                Arc::new(oxiced::theme::snapshot::ThemeSnapshot::legacy()),
            )
        }
        .unwrap();
        drop(unsafe { changed(model.clone(), "[example]\nlabel='Live'".parse().unwrap()) });
        for outcome in collect_task(
            unsafe { command(model.clone(), "increment".to_owned(), Vec::new()) }.unwrap(),
        ) {
            assert!(outcome.result.is_ok());
            drop(unsafe { update(model.clone(), outcome.message) });
        }
        let reset = collect_task(
            unsafe { command(model.clone(), "reset".to_owned(), Vec::new()) }.unwrap(),
        );
        let requests =
            collect_task(unsafe { update(model.clone(), reset[0].message.clone()) }.unwrap());
        assert_eq!(
            requests[0].downcast_ref::<String>().map(String::as_str),
            Some(oxibar_plugin_api::HOST_REQUEST_OPEN_MODAL)
        );
        assert!(unsafe { closed(model.clone()) }.is_none());
        assert!(unsafe { closed(model.clone()) }.is_none());
        let elements = unsafe { view(model.clone()) }.unwrap();
        let widgets = unsafe {
            desktop(
                model.clone(),
                PluginViewContext {
                    instance_id: format!("desktop:{index}"),
                    output_name: Some("Private".to_owned()),
                    output_scale: 1.25,
                },
            )
        }
        .unwrap();
        assert!(!elements.is_empty() && !widgets.is_empty());
        let mut stream = unsafe { stream_from_raw(subscribe()) };
        assert!(stream.next().now_or_never().is_none());
        drop(stream);
        drop(widgets);
        drop(elements);
        drop(requests);
        drop(reset);
        drop(model);
    }
}
