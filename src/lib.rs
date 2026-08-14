pub mod app;
pub mod config;
pub mod font;
pub mod layout;
pub mod messages;
mod monitor;
pub mod plugins;
pub mod single_instance;
pub mod surfaces;
#[doc(hidden)]
pub mod test_support;

pub(crate) use messages::{Message, map_plugin_message};
