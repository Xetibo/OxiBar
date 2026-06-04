//! Plugin loading and lifecycle.
//!
//! Plugins are dynamic libraries placed in `$XDG_CONFIG_HOME/oxibar/plugins/`
//! that expose the symbol set defined by `oxibar-plugin-api`. Each plugin
//! contributes a [`PluginModel`], `update`/`view` functions, and a
//! [`PluginStream`](oxibar_plugin_api::PluginStream) subscription.
//!
//! ## Lifetime model
//!
//! Both the loaded `Library` and the plugin's model are owned by [`OxiBar`]
//! (the host) — no `Box::leak`. We keep an `Arc<Library>` around for as long
//! as any of its symbols are reachable, so unloading happens automatically on
//! shutdown when the last `Arc` drops. Symbols are stored as raw fn pointers
//! (`Symbol::into_raw` + cast) so we don't have to fight the `'static`
//! borrow that `Symbol` would otherwise impose.
//!
//! ## TODO (architectural follow-ups)
//! - Plugins should receive their own sub-table from config (`[plugins.<name>]`)
//!   rather than the whole document.
//! - Plugins should declare a [`Slot`] preference (left/center/right).
//! - Replace the raw-pointer `Stream` handoff with an `extern "C"` callback
//!   channel so the ABI is fully `#[repr(C)]`.

use std::collections::HashMap;
use std::sync::Arc;

use libloading::Library;
use oxibar_plugin_api::{
    ABI_VERSION, AbiVersionFn, ErrorsFn, HOST_REQUEST_TOGGLE_POPUP, LaunchFn, ModelFn, NameFn,
    PluginModel, PluginMsg, PopupViewFn, SubscriptionFn, UpdateFn, ViewFn,
};
use toml::Table;
use tracing::{error, info, warn};

use crate::config::{get_allowed_plugins, get_oxirun_dir};

/// Resolved entry-point pointers for a single plugin. The `Library` is kept
/// alive via the `Arc` so that the raw fn pointers remain valid for the
/// lifetime of this struct.
pub struct PluginFuncs {
    _lib: Arc<Library>,
    pub model: ModelFn,
    pub update: UpdateFn,
    pub launch: LaunchFn,
    pub view: ViewFn,
    pub errors: ErrorsFn,
    pub name: NameFn,
    pub subscription: SubscriptionFn,
    pub popup_view: Option<PopupViewFn>,
}

impl std::fmt::Debug for PluginFuncs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginFuncs")
            .field("name", &unsafe { (self.name)() })
            .finish_non_exhaustive()
    }
}

/// Plugins keyed by their declared name. Switched from `usize` (read_dir
/// index) so renaming a `.so` doesn't shuffle ids and break persisted state
/// once we add any.
pub type PluginMap = HashMap<String, (PluginModel, Arc<PluginFuncs>)>;

/// Errors that can occur while attempting to load a single plugin. Bubbled up
/// to a `tracing::warn!` rather than aborting the whole bar.
#[derive(Debug)]
enum LoadError {
    Library(libloading::Error),
    MissingSymbol(&'static str, libloading::Error),
    AbiMismatch { plugin: u32, host: u32 },
    DuplicateName(String),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Library(e) => write!(f, "failed to dlopen: {e}"),
            LoadError::MissingSymbol(s, e) => write!(f, "missing symbol `{s}`: {e}"),
            LoadError::AbiMismatch { plugin, host } => {
                write!(f, "ABI mismatch (plugin={plugin}, host={host})")
            }
            LoadError::DuplicateName(n) => write!(f, "duplicate plugin name `{n}`"),
        }
    }
}

/// Resolve a single symbol and convert it into a bare fn pointer.
unsafe fn resolve<T: Copy>(lib: &Library, symbol: &'static str) -> Result<T, LoadError> {
    debug_assert_eq!(
        std::mem::size_of::<T>(),
        std::mem::size_of::<*const ()>(),
        "resolve<T>() must be instantiated with a fn pointer type"
    );
    unsafe {
        let sym: libloading::Symbol<'_, T> = lib
            .get(symbol.as_bytes())
            .map_err(|e| LoadError::MissingSymbol(symbol, e))?;
        // `Symbol::into_raw` returns a raw os symbol that outlives the
        // `Symbol` borrow; we keep the `Library` itself alive via Arc.
        let raw = sym.into_raw();
        Ok(*(&raw as *const _ as *const T))
    }
}

unsafe fn resolve_optional<T: Copy>(lib: &Library, symbol: &'static str) -> Option<T> {
    debug_assert_eq!(
        std::mem::size_of::<T>(),
        std::mem::size_of::<*const ()>(),
        "resolve_optional<T>() must be instantiated with a fn pointer type"
    );
    unsafe {
        let sym: libloading::Symbol<'_, T> = lib.get(symbol.as_bytes()).ok()?;
        let raw = sym.into_raw();
        Some(*(&raw as *const _ as *const T))
    }
}

unsafe fn load_one(path: &std::path::Path) -> Result<(String, PluginFuncs), LoadError> {
    let lib = unsafe { Library::new(path) }.map_err(LoadError::Library)?;

    // ABI gate: refuse to load plugins compiled against an older/newer api.
    let abi_version: AbiVersionFn = unsafe { resolve(&lib, "abi_version")? };
    let plugin_abi = unsafe { abi_version() };
    if plugin_abi != ABI_VERSION {
        return Err(LoadError::AbiMismatch {
            plugin: plugin_abi,
            host: ABI_VERSION,
        });
    }

    let model: ModelFn = unsafe { resolve(&lib, "model")? };
    let update: UpdateFn = unsafe { resolve(&lib, "update")? };
    let launch: LaunchFn = unsafe { resolve(&lib, "launch")? };
    let view: ViewFn = unsafe { resolve(&lib, "view")? };
    let errors: ErrorsFn = unsafe { resolve(&lib, "errors")? };
    let name: NameFn = unsafe { resolve(&lib, "name")? };
    let subscription: SubscriptionFn = unsafe { resolve(&lib, "subscription")? };
    let popup_view: Option<PopupViewFn> = unsafe { resolve_optional(&lib, "popup_view") };

    let plugin_name = unsafe { name() }.to_owned();

    Ok((
        plugin_name,
        PluginFuncs {
            _lib: Arc::new(lib),
            model,
            update,
            launch,
            view,
            errors,
            name,
            subscription,
            popup_view,
        },
    ))
}

/// Discover, load and instantiate every allowed plugin. Failures are logged
/// and skipped — never fatal.
pub fn load_plugins(config: &Table) -> (PluginMap, Vec<iced::Task<crate::Message>>) {
    let mut plugins = PluginMap::new();
    let mut tasks = Vec::new();

    let plugin_dir = get_oxirun_dir().join("plugins");
    if !plugin_dir.is_dir()
        && let Err(e) = std::fs::create_dir(&plugin_dir)
    {
        error!("could not create plugin dir {}: {e}", plugin_dir.display());
        return (plugins, tasks);
    }

    let allowed = get_allowed_plugins(config);
    let entries = match plugin_dir.read_dir() {
        Ok(e) => e,
        Err(e) => {
            error!("could not read plugin dir: {e}");
            return (plugins, tasks);
        }
    };

    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(name_str) = file_name.to_str() else {
            continue;
        };
        if !allowed.contains(&name_str) {
            continue;
        }

        let path = entry.path();
        match unsafe { load_one(&path) } {
            Ok((plugin_name, funcs)) => {
                if plugins.contains_key(&plugin_name) {
                    warn!("{}", LoadError::DuplicateName(plugin_name));
                    continue;
                }
                let (model, init_task) = unsafe { (funcs.model)(config.clone()) };
                let funcs = Arc::new(funcs);
                let key = plugin_name.clone();
                if let Some(task) = init_task {
                    let key_for_task = key.clone();
                    tasks.push(task.map(move |msg| {
                        if msg
                            .downcast_ref::<String>()
                            .is_some_and(|request| request == HOST_REQUEST_TOGGLE_POPUP)
                        {
                            crate::Message::TogglePluginPopup(key_for_task.clone())
                        } else {
                            crate::Message::PluginSubMsg(key_for_task.clone(), msg)
                        }
                    }));
                }
                info!("loaded plugin `{plugin_name}` from {}", path.display());
                plugins.insert(key, (model, funcs));
            }
            Err(e) => warn!("skipping {}: {e}", path.display()),
        }
    }

    (plugins, tasks)
}

/// Helper used by the `view` path to render a single plugin without leaking
/// `unsafe` into `main.rs`.
pub fn render_plugin(
    funcs: &PluginFuncs,
    model: &PluginModel,
) -> Vec<iced::Element<'static, PluginMsg>> {
    match unsafe { (funcs.view)(model.clone()) } {
        Ok(elements) => elements,
        Err(e) => {
            warn!("plugin view error: {e}");
            Vec::new()
        }
    }
}

/// Render a plugin-provided popup body, if the plugin exposes one.
pub fn render_plugin_popup(
    funcs: &PluginFuncs,
    model: &PluginModel,
) -> Vec<iced::Element<'static, PluginMsg>> {
    let Some(popup_view) = funcs.popup_view else {
        return Vec::new();
    };
    match unsafe { popup_view(model.clone()) } {
        Ok(elements) => elements,
        Err(e) => {
            warn!("plugin popup view error: {e}");
            Vec::new()
        }
    }
}

/// Helper used by the `update` path. Returns `Task::none()` if the plugin
/// doesn't want to do anything.
pub fn dispatch_update(
    funcs: &PluginFuncs,
    model: PluginModel,
    msg: PluginMsg,
) -> Option<iced::Task<PluginMsg>> {
    unsafe { (funcs.update)(model, msg) }
}

/// Forward a launch request (e.g. keyboard binding "activate item N").
pub fn dispatch_launch(
    funcs: &PluginFuncs,
    focused_index: usize,
    model: PluginModel,
) -> Option<iced::Task<PluginMsg>> {
    unsafe { (funcs.launch)(focused_index, model) }
}

/// Drain a plugin's queued errors and emit them as warnings. Called whenever
/// the host has a chance (currently after each plugin update).
pub fn drain_errors(funcs: &PluginFuncs, model: &PluginModel) {
    let errs = unsafe { (funcs.errors)(model.clone()) };
    for e in errs {
        warn!(plugin = unsafe { (funcs.name)() }, "{e}");
    }
}
