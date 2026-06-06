//! Shared ABI surface for oxibar plugins.
//!
//! Both the host and every plugin dylib link against this crate so that the
//! type aliases and the trait-object vtables they hand to each other are
//! guaranteed to match. Plugin authors should depend on this crate instead of
//! pulling these definitions out of `oxiced`.

use std::any::TypeId;
use std::fmt::Debug;
use std::pin::Pin;
use std::sync::{Arc, RwLock};

pub use iced;
use iced::{Element, Task, futures::Stream};
pub use toml;

/// ABI version. Bump on any breaking change to the function signatures or
/// trait-object layouts crossing the dylib boundary. The host refuses to load
/// a plugin whose `abi_version()` does not match.
pub const ABI_VERSION: u32 = 6;

/// Shared, downcastable, thread-safe trait object. Moved here from
/// `oxiced::any_send` so plugins don't need oxiced just to participate in the
/// ABI. Implementation cribbed from `core::any::Any` with `Send + Sync`.
pub trait OxiAny: 'static + Send + Sync + Debug {
    fn type_id(&self) -> TypeId;
}

impl<T: 'static + ?Sized + Debug + Send + Sync> OxiAny for T {
    fn type_id(&self) -> TypeId {
        TypeId::of::<T>()
    }
}

impl dyn OxiAny {
    #[inline]
    pub fn is<T: OxiAny>(&self) -> bool {
        TypeId::of::<T>() == OxiAny::type_id(self)
    }

    #[inline]
    pub fn downcast_ref<T: OxiAny>(&self) -> Option<&T> {
        if self.is::<T>() {
            // SAFETY: type id matches, layout is `T`.
            unsafe { Some(&*(self as *const dyn OxiAny as *const T)) }
        } else {
            None
        }
    }

    #[inline]
    pub fn downcast_mut<T: OxiAny>(&mut self) -> Option<&mut T> {
        if self.is::<T>() {
            // SAFETY: type id matches, layout is `T`.
            unsafe { Some(&mut *(self as *mut dyn OxiAny as *mut T)) }
        } else {
            None
        }
    }
}

/// Shared, mutable plugin model. Owned via `Box`, so dropping the last `Arc`
/// drops the model — no `Box::leak` required to satisfy iced's `'static`
/// bound. (`'static` constrains the *type*, not the lifetime of the value.)
pub type PluginModel = Arc<RwLock<Box<dyn OxiAny>>>;

/// Plugin → host (and host → plugin) message envelope.
pub type PluginMsg = Arc<dyn OxiAny>;

/// Wrap a typed plugin model in the shared ABI container.
pub fn plugin_model<M: OxiAny>(model: M) -> PluginModel {
    Arc::new(RwLock::new(Box::new(model)))
}

/// Borrow a typed plugin model for read-only view rendering.
pub fn with_model_read<M: OxiAny, R>(
    model: &PluginModel,
    f: impl FnOnce(&M) -> R,
) -> Result<R, std::io::Error> {
    let guard = model.try_read().map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::WouldBlock, "model is write-locked")
    })?;
    let model = guard.downcast_ref::<M>().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "model has wrong type")
    })?;
    Ok(f(model))
}

/// Borrow a typed plugin model for mutation from ABI update/error hooks.
pub fn with_model_write<M: OxiAny, R>(
    model: &PluginModel,
    f: impl FnOnce(&mut M) -> R,
) -> Option<R> {
    let mut guard = model.try_write().ok()?;
    let model = guard.downcast_mut::<M>()?;
    Some(f(model))
}

/// Drain a plugin model's transient error buffer.
pub fn drain_model_errors<M: OxiAny>(
    model: &PluginModel,
    errors: impl FnOnce(&mut M) -> &mut Vec<String>,
) -> Vec<String> {
    with_model_write(model, |model| std::mem::take(errors(model))).unwrap_or_default()
}

/// String payload a plugin can send through [`PluginMsg`] to request the host
/// toggles that plugin's popup.
pub const HOST_REQUEST_TOGGLE_POPUP: &str = "oxibar.host.toggle-popup";

/// String payload a plugin can send through [`PluginMsg`] to request that the
/// host opens that plugin's center-screen modal surface.
pub const HOST_REQUEST_OPEN_MODAL: &str = "oxibar.host.open-modal";

/// String payload a plugin can send through [`PluginMsg`] to request that the
/// host closes that plugin's center-screen modal surface.
pub const HOST_REQUEST_CLOSE_MODAL: &str = "oxibar.host.close-modal";

/// String payload a plugin can send through [`PluginMsg`] to request that the
/// host toggles that plugin's right-side panel surface.
pub const HOST_REQUEST_TOGGLE_PANEL: &str = "oxibar.host.toggle-panel";

/// Backward-compatible name for the clock plugin's original popup request.
pub const HOST_REQUEST_TOGGLE_CALENDAR_POPUP: &str = HOST_REQUEST_TOGGLE_POPUP;

pub const HOST_REQUEST_SHOW_TOAST_PREFIX: &str = "oxibar.host.toast.show:";
pub const HOST_REQUEST_CLOSE_TOAST_PREFIX: &str = "oxibar.host.toast.close:";

/// Plugin request for the host to show or close a transient toast layer.
///
/// The host owns the layer-shell windows. Plugins own the content through their
/// optional `toast_view(model, toast_id)` export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostToastRequest {
    pub action: HostToastAction,
    pub toast_id: String,
    pub width: u32,
    pub height: u32,
}

impl HostToastRequest {
    pub fn show(toast_id: impl Into<String>, width: u32, height: u32) -> Self {
        Self {
            action: HostToastAction::Show,
            toast_id: toast_id.into(),
            width,
            height,
        }
    }

    pub fn close(toast_id: impl Into<String>) -> Self {
        Self {
            action: HostToastAction::Close,
            toast_id: toast_id.into(),
            width: 0,
            height: 0,
        }
    }

    pub fn to_host_request_string(&self) -> String {
        match self.action {
            HostToastAction::Show => format!(
                "{HOST_REQUEST_SHOW_TOAST_PREFIX}{}:{}:{}",
                self.width, self.height, self.toast_id
            ),
            HostToastAction::Close => {
                format!("{HOST_REQUEST_CLOSE_TOAST_PREFIX}{}", self.toast_id)
            }
        }
    }

    pub fn parse_host_request(value: &str) -> Option<Self> {
        if let Some(rest) = value.strip_prefix(HOST_REQUEST_SHOW_TOAST_PREFIX) {
            let mut parts = rest.splitn(3, ':');
            let width = parts.next()?.parse().ok()?;
            let height = parts.next()?.parse().ok()?;
            let toast_id = parts.next()?.to_owned();
            return Some(Self::show(toast_id, width, height));
        }

        value
            .strip_prefix(HOST_REQUEST_CLOSE_TOAST_PREFIX)
            .map(Self::close)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostToastAction {
    Show,
    Close,
}

/// Boxed, pinned, `Send` stream of plugin messages.
///
/// Crossing the dylib boundary as a raw pointer is technically unsound under a
/// strict reading of the Rust reference (non-`#[repr(C)]` trait objects), but
/// works in practice when host and plugin are built with the same toolchain
/// and the same `oxibar-plugin-api` version. The `ABI_VERSION` check guards
/// against version skew.
pub type PluginStream = dyn Stream<Item = PluginMsg> + Send;

/// Layout slot a plugin requests. Currently unused by the host (everything is
/// rendered as a single row) but plugins should declare their preference now
/// so the layout work in a future pass is non-breaking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Left,
    Center,
    Right,
}

/// Optional plugin metadata read by the host when a plugin exports a
/// `metadata` symbol. Missing metadata keeps host defaults.
#[derive(Debug, Clone, Copy, Default)]
pub struct PluginMetadata {
    /// Visible popup body size, including host chrome.
    pub popup_size: Option<(u32, u32)>,
    /// Optional input/layout region for detached overlays such as context
    /// menus. Defaults to [`Self::popup_size`] when unset.
    pub popup_input_size: Option<(u32, u32)>,
}

/// Optional runtime popup metrics read from plugins whose popup size depends on
/// model state.
#[derive(Debug, Clone, Copy, Default)]
pub struct PluginPopupMetrics {
    /// Visible popup body size, including host chrome.
    pub popup_size: Option<(u32, u32)>,
    /// Optional input/layout region for detached overlays such as context
    /// menus. Defaults to the resolved visible popup size when unset.
    pub popup_input_size: Option<(u32, u32)>,
}

/// Optional plugin load gate used by the host before it instantiates a plugin
/// model or starts subscriptions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginAvailability {
    Available,
    Unavailable(&'static str),
}

// -------- Function-pointer ABI --------
//
// Plugins expose the required `#[unsafe(no_mangle)] pub extern "Rust"` symbols.
// Optional symbols can be absent. The signatures live here as type aliases
// so that host and plugin agree on the exact shape.

pub type ModelFn = unsafe extern "Rust" fn(toml::Table) -> (PluginModel, Option<Task<PluginMsg>>);

pub type UpdateFn =
    unsafe extern "Rust" fn(model: PluginModel, msg: PluginMsg) -> Option<Task<PluginMsg>>;

pub type LaunchFn =
    unsafe extern "Rust" fn(focused_index: usize, model: PluginModel) -> Option<Task<PluginMsg>>;

pub type ViewFn =
    unsafe extern "Rust" fn(
        model: PluginModel,
    ) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error>;

pub type PopupViewFn =
    unsafe extern "Rust" fn(
        model: PluginModel,
    ) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error>;

pub type ModalViewFn =
    unsafe extern "Rust" fn(
        model: PluginModel,
    ) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error>;

pub type PanelViewFn =
    unsafe extern "Rust" fn(
        model: PluginModel,
    ) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error>;

pub type ToastViewFn =
    unsafe extern "Rust" fn(
        model: PluginModel,
        toast_id: &str,
    ) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error>;

pub type ErrorsFn = unsafe extern "Rust" fn(model: PluginModel) -> Vec<String>;

pub type NameFn = unsafe extern "Rust" fn() -> &'static str;

pub type MetadataFn = unsafe extern "Rust" fn() -> PluginMetadata;

pub type AvailabilityFn = unsafe extern "Rust" fn(toml::Table) -> PluginAvailability;

pub type PopupMetricsFn = unsafe extern "Rust" fn(model: PluginModel) -> PluginPopupMetrics;

pub type SubscriptionFn = unsafe extern "Rust" fn() -> *mut PluginStream;

pub type AbiVersionFn = unsafe extern "Rust" fn() -> u32;

/// Helper plugins can use to reconstitute their boxed stream into a `Pin<Box<_>>`
/// without each plugin needing to spell out the `unsafe` block.
///
/// # Safety
/// `ptr` must come from `Box::into_raw(Box::new(stream))` produced by the same
/// build of this crate.
pub unsafe fn stream_from_raw(ptr: *mut PluginStream) -> Pin<Box<PluginStream>> {
    unsafe { Pin::new_unchecked(Box::from_raw(ptr)) }
}
