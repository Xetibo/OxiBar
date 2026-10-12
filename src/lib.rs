pub mod app;
mod background;
mod commands;
pub mod compositor;
pub mod config;
pub mod control_center;
pub mod desktop;
pub mod dock;
pub mod font;
pub mod ipc;
pub mod launcher;
pub mod layout;
pub mod messages;
pub mod monitor;
pub mod osd;
pub mod overview;
pub mod plugins;
pub mod session_menu;
pub mod settings;
pub mod shell;
pub mod shortcuts;
pub mod single_instance;
pub mod surfaces;
#[doc(hidden)]
pub mod test_support;
pub mod theme;

pub(crate) use messages::{Message, map_plugin_message};
