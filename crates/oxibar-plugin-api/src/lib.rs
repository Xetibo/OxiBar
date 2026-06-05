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
pub const ABI_VERSION: u32 = 5;

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

// -------- Function-pointer ABI --------
//
// Plugins expose the required `#[unsafe(no_mangle)] pub extern "Rust"` symbols.
// Optional view symbols can be absent. The signatures live here as type aliases
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

pub type ErrorsFn = unsafe extern "Rust" fn(model: PluginModel) -> Vec<String>;

pub type NameFn = unsafe extern "Rust" fn() -> &'static str;

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
