use std::{
    pin::Pin,
    sync::{Arc, RwLock},
};

use iced::{
    Element, Task,
    futures::Stream,
};
use libloading::Library;
use oxiced::any_send::OxiAny;
use toml::Table;

pub type PluginModel = Arc<RwLock<&'static mut dyn OxiAny>>;
pub type PluginMsg = Arc<dyn OxiAny>;

/// The plugin's subscription function returns a raw pointer to a boxed stream.
/// Only raw pointers cross the dylib boundary — no non-repr(C) Rust types.
pub type SubscriptionFn =
    unsafe extern "Rust" fn() -> *mut (dyn Stream<Item = PluginMsg> + Send);

#[allow(improper_ctypes_definitions)]
#[derive(Clone, Debug)]
pub struct PluginFuncs {
    pub model: libloading::Symbol<
        'static,
        unsafe extern "Rust" fn(Table) -> (PluginModel, Option<Task<PluginMsg>>),
    >,
    pub update: libloading::Symbol<
        'static,
        unsafe extern "Rust" fn(
            filter_text: String,
            model: PluginModel,
            msg: PluginMsg,
        ) -> Option<Task<PluginMsg>>,
    >,
    pub launch: libloading::Symbol<
        'static,
        unsafe extern "Rust" fn(focused_index: usize, model: PluginModel) -> Option<Task<PluginMsg>>,
    >,
    pub view: libloading::Symbol<
        'static,
        unsafe extern "Rust" fn(
            model: PluginModel,
        ) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error>,
    >,
    pub errors:
        libloading::Symbol<'static, unsafe extern "Rust" fn(model: PluginModel) -> Vec<String>>,
    pub name: libloading::Symbol<'static, unsafe extern "Rust" fn() -> &'static str>,
    pub subscription: libloading::Symbol<'static, SubscriptionFn>,
}

pub fn load_plugin(lib: &'static Library) -> Option<PluginFuncs> {
    unsafe {
        let model: Result<
            libloading::Symbol<
                unsafe extern "Rust" fn(Table) -> (PluginModel, Option<Task<PluginMsg>>),
            >,
            libloading::Error,
        > = lib.get(b"model");
        let update: Result<
            libloading::Symbol<
                unsafe extern "Rust" fn(
                    filter_text: String,
                    model: PluginModel,
                    msg: PluginMsg,
                ) -> Option<Task<PluginMsg>>,
            >,
            libloading::Error,
        > = lib.get(b"update");
        let launch: Result<
            libloading::Symbol<
                unsafe extern "Rust" fn(
                    focused_index: usize,
                    model: PluginModel,
                ) -> Option<Task<PluginMsg>>,
            >,
            libloading::Error,
        > = lib.get(b"launch");
        let view: Result<
            libloading::Symbol<
                unsafe extern "Rust" fn(
                    model: PluginModel,
                ) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error>,
            >,
            libloading::Error,
        > = lib.get(b"view");
        let errors: Result<
            libloading::Symbol<unsafe extern "Rust" fn(model: PluginModel) -> Vec<String>>,
            libloading::Error,
        > = lib.get(b"errors");
        let name: Result<
            libloading::Symbol<unsafe extern "Rust" fn() -> &'static str>,
            libloading::Error,
        > = lib.get(b"name");
        let subscription: Result<libloading::Symbol<SubscriptionFn>, libloading::Error> =
            lib.get(b"subscription");

        match (model, update, launch, view, errors, name, subscription) {
            (
                Ok(model),
                Ok(update),
                Ok(launch),
                Ok(view),
                Ok(errors),
                Ok(name),
                Ok(subscription),
            ) => Some(PluginFuncs {
                model,
                update,
                view,
                launch,
                errors,
                name,
                subscription,
            }),
            _ => None,
        }
    }
}
