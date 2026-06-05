//! Hyprland workspaces plugin.
//!
//! Subscribes to the hyprland event socket on a dedicated `std::thread` (no
//! tokio runtime in the dylib — would clash with the host's iced runtime),
//! and pushes events through a sync `try_send` bridge into iced's stream.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use hyprland::{
    data::Workspace,
    event_listener::EventListener,
    shared::{HyprData, HyprDataActive},
};
use iced::{
    Alignment, Border, Element, Font, Shadow, Task,
    border::Radius,
    futures::Stream,
    stream,
    widget::{Row, text},
};
use oxibar_plugin_api::{
    ABI_VERSION, PluginModel, PluginMsg, PluginStream, drain_model_errors, plugin_model,
    toml::Table, with_model_read, with_model_write,
};
use oxiced::{theme::theme_impl::OXITHEME, widgets::oxi_button};

#[derive(Debug, Default)]
pub struct Model {
    workspaces: HashMap<String, hyprland::data::Workspace>,
    active_workspace_name: String,
    font_family: Option<&'static str>,
    /// Transient errors surfaced via the `errors()` ABI entry. Drained by the
    /// host on each update.
    errors: Vec<String>,
}

impl Model {
    pub fn new(global_config: Table) -> Model {
        let mut errors = Vec::new();
        let active_workspace_name = match Workspace::get_active() {
            Ok(w) => w.name,
            Err(e) => {
                errors.push(format!("could not query active workspace: {e}"));
                String::new()
            }
        };
        let font_family = global_config
            .get("bar")
            .and_then(|v| v.as_table())
            .and_then(|t| t.get("font"))
            .and_then(|v| v.as_str())
            .map(resolve_font_family)
            .map(|s| &*Box::leak(s.to_owned().into_boxed_str()));
        Model {
            workspaces: HashMap::new(),
            active_workspace_name,
            font_family,
            errors,
        }
    }
}

fn resolve_font_family(configured: &str) -> String {
    let Ok(output) = std::process::Command::new("fc-match")
        .args(["-f", "%{family}", configured])
        .output()
    else {
        return configured.to_owned();
    };
    if !output.status.success() {
        return configured.to_owned();
    }
    let family = String::from_utf8_lossy(&output.stdout);
    family
        .split(',')
        .next()
        .map(str::trim)
        .filter(|family| !family.is_empty())
        .unwrap_or(configured)
        .to_owned()
}

/// Wrap a typed `Message` in the dyn-`OxiAny` envelope expected by the host.
fn msg(m: Message) -> PluginMsg {
    Arc::new(m)
}

#[derive(Clone, Debug)]
pub enum Message {
    ActiveWorkspaceChanged(String),
    ActivateWorkspace(hyprland::shared::WorkspaceId),
    WorkspacesInit(HashMap<String, hyprland::data::Workspace>),
    WorkspaceAdded(hyprland::data::Workspace),
    WorkspaceRemoved(String),
}

// ---------------- Plugin ABI ----------------

#[unsafe(no_mangle)]
pub extern "Rust" fn abi_version() -> u32 {
    ABI_VERSION
}

#[unsafe(no_mangle)]
pub extern "Rust" fn name() -> &'static str {
    "Workspaces"
}

#[unsafe(no_mangle)]
pub extern "Rust" fn model(global_config: Table) -> (PluginModel, Option<Task<PluginMsg>>) {
    (plugin_model(Model::new(global_config)), None)
}

#[unsafe(no_mangle)]
pub extern "Rust" fn update(model: PluginModel, msg_in: PluginMsg) -> Option<Task<PluginMsg>> {
    let m = msg_in.downcast_ref::<Message>()?.clone();
    with_model_write::<Model, _>(&model, |model| match m {
        Message::WorkspacesInit(workspaces) => {
            model.workspaces = workspaces;
            None
        }
        Message::ActivateWorkspace(id) => {
            tracing::info!(workspace = id, "activating workspace");
            if let Err(e) = dispatch_workspace(id) {
                model
                    .errors
                    .push(format!("workspace dispatch to {id} failed: {e}"));
            }
            None
        }
        Message::WorkspaceAdded(workspace) => {
            model.workspaces.insert(workspace.name.clone(), workspace);
            None
        }
        Message::WorkspaceRemoved(name) => {
            model.workspaces.remove(&name);
            None
        }
        Message::ActiveWorkspaceChanged(name) => {
            model.active_workspace_name = name;
            None
        }
    })
    .flatten()
}

fn dispatch_workspace(id: hyprland::shared::WorkspaceId) -> Result<(), String> {
    // Equivalent to: hyprctl dispatch 'hl.dsp.focus({ workspace = <id> })'
    let command = format!("hl.dsp.focus({{ workspace = {id} }})");
    let output = std::process::Command::new("hyprctl")
        .args(["dispatch", &command])
        .output()
        .map_err(|e| format!("could not run hyprctl: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if output.status.success() && (stdout.is_empty() || stdout == "ok") {
        Ok(())
    } else if stderr.is_empty() {
        Err(stdout)
    } else {
        Err(stderr)
    }
}

#[unsafe(no_mangle)]
pub extern "Rust" fn launch(focused_index: usize, model: PluginModel) -> Option<Task<PluginMsg>> {
    // Treat `focused_index` as the position in the id-sorted workspace list:
    // a key binding "activate workspace #N" can call this without knowing
    // the workspace's stringly-typed name.
    with_model_read::<Model, _>(&model, |model| {
        let mut sorted: Vec<&hyprland::data::Workspace> = model.workspaces.values().collect();
        sorted.sort_by_key(|w| w.id);
        let target = sorted.get(focused_index)?;
        Some(Task::done(msg(Message::ActivateWorkspace(target.id))))
    })
    .ok()
    .flatten()
}

#[unsafe(no_mangle)]
pub extern "Rust" fn errors(model: PluginModel) -> Vec<String> {
    drain_model_errors::<Model>(&model, |model| &mut model.errors)
}

#[unsafe(no_mangle)]
pub extern "Rust" fn view(
    model: PluginModel,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    with_model_read::<Model, _>(&model, |model| {
        let palette = &OXITHEME;
        let mut sorted: Vec<hyprland::data::Workspace> =
            model.workspaces.values().cloned().collect();
        sorted.sort_by_key(|w| w.id);

        let workspace_entries: Vec<Element<PluginMsg>> = sorted
            .into_iter()
            .map(|workspace| {
                let is_active = model.active_workspace_name == workspace.name;
                let bg_color = if is_active {
                    iced::Background::Color(palette.primary)
                } else {
                    iced::Background::Color(palette.primary_bg)
                };
                let fg_color = if is_active {
                    palette.primary_contrast
                } else {
                    palette.primary
                };
                let base_font = match model.font_family {
                    Some(name) => Font::with_name(name),
                    None => Font::DEFAULT,
                };
                let font = Font {
                    weight: iced::font::Weight::Semibold,
                    ..base_font
                };

                let base = iced::widget::button::Style {
                    background: Some(bg_color),
                    text_color: fg_color,
                    border: Border {
                        color: iced::Color::TRANSPARENT,
                        width: 0.0,
                        radius: Radius::new(360 / 4),
                    },
                    shadow: Shadow {
                        blur_radius: 2.0,
                        ..Shadow::default()
                    },
                    snap: false,
                };
                let style =
                    move |base: iced::widget::button::Style,
                          status: iced::widget::button::Status| {
                        match status {
                            iced::widget::button::Status::Active => base,
                            iced::widget::button::Status::Pressed => iced::widget::button::Style {
                                background: Some(iced::Background::Color(palette.primary_active)),
                                ..base
                            },
                            iced::widget::button::Status::Hovered => iced::widget::button::Style {
                                background: Some(iced::Background::Color(palette.primary_hover)),
                                ..base
                            },
                            iced::widget::button::Status::Disabled => base,
                        }
                    };
                oxi_button::button(
                    text(format!("{}", workspace.id))
                        .size(13)
                        .font(font)
                        .align_y(Alignment::Center)
                        .align_x(Alignment::Center),
                    oxi_button::ButtonVariant::PrimaryBg,
                )
                .on_press(msg(Message::ActivateWorkspace(workspace.id)))
                .style(move |&_, status| (style)(base, status))
                .padding(0)
                .height(22.5)
                .width(22.5)
                .into()
            })
            .collect();

        vec![
            Row::from_vec(workspace_entries)
                .align_y(Alignment::Center)
                .spacing(5)
                .into(),
        ]
    })
}

/// Returns a raw pointer to a boxed stream of plugin messages.
///
/// We intentionally don't spin up tokio in the plugin (would create a second
/// runtime alongside the host's iced/tokio one). Instead a plain
/// `std::thread` runs hyprland's sync `EventListener` and pushes into the
/// stream via `try_send`.
///
/// The async block itself only awaits `pending::<()>()` so iced never drops
/// the stream while the listener thread is still running.
#[unsafe(no_mangle)]
pub extern "Rust" fn subscription() -> *mut PluginStream {
    let s = stream::channel(
        100,
        move |output: iced::futures::channel::mpsc::Sender<PluginMsg>| async move {
            // `Mutex` so the synchronous handler closures (which are only
            // `Fn`, not `FnMut`) can grab a `&mut Sender` to call `try_send`.
            let output = Arc::new(Mutex::new(output));

            std::thread::spawn(move || {
                let send = |m: Message| {
                    let _ = output.lock().unwrap().try_send(msg(m));
                };

                // Initial workspace fetch.
                if let Ok(workspaces) = hyprland::data::Workspaces::get() {
                    let map = workspaces
                        .into_iter()
                        .map(|w| (w.name.clone(), w))
                        .collect();
                    send(Message::WorkspacesInit(map));
                }

                let mut listener = EventListener::new();

                {
                    let output = output.clone();
                    listener.add_workspace_added_handler(move |_| {
                        if let Ok(workspaces) = hyprland::data::Workspaces::get() {
                            let map = workspaces
                                .into_iter()
                                .map(|w| (w.name.clone(), w))
                                .collect();
                            let _ = output
                                .lock()
                                .unwrap()
                                .try_send(msg(Message::WorkspacesInit(map)));
                        }
                    });
                }

                {
                    let output = output.clone();
                    listener.add_workspace_deleted_handler(move |workspace| {
                        let _ = output
                            .lock()
                            .unwrap()
                            .try_send(msg(Message::WorkspaceRemoved(workspace.name.to_string())));
                    });
                }

                {
                    let output = output.clone();
                    listener.add_workspace_changed_handler(move |workspace| {
                        let _ =
                            output
                                .lock()
                                .unwrap()
                                .try_send(msg(Message::ActiveWorkspaceChanged(
                                    workspace.name.to_string(),
                                )));
                    });
                }

                {
                    let output = output.clone();
                    listener.add_active_monitor_changed_handler(move |monitor| {
                        let name = monitor
                            .workspace_name
                            .map(|n| n.to_string())
                            .unwrap_or_default();
                        let _ = output
                            .lock()
                            .unwrap()
                            .try_send(msg(Message::ActiveWorkspaceChanged(name)));
                    });
                }

                if let Err(e) = listener.start_listener() {
                    tracing::error!("hyprland event listener stopped: {e}");
                }
            });

            // Keep the async task alive forever so iced doesn't drop the stream.
            std::future::pending::<()>().await;
        },
    );

    Box::into_raw(Box::new(s)) as *mut PluginStream
}

// Compile-time sanity check: confirm the channel-stream type really is a
// `PluginStream`-shaped trait object so the cast above is sound.
const _: fn() = || {
    fn assert_stream<S: Stream<Item = PluginMsg> + Send + 'static>(_: &S) {}
    let _ = |s: &iced::futures::stream::BoxStream<'static, PluginMsg>| assert_stream(s);
};

#[cfg(test)]
mod tests {
    use super::*;

    fn test_model() -> PluginModel {
        plugin_model(Model {
            workspaces: HashMap::new(),
            active_workspace_name: String::new(),
            font_family: None,
            errors: Vec::new(),
        })
    }

    #[test]
    fn update_tracks_active_workspace_without_hyprland() {
        let plugin_model = test_model();
        assert_eq!(name(), "Workspaces");
        assert_eq!(abi_version(), ABI_VERSION);

        let task = update(
            plugin_model.clone(),
            msg(Message::ActiveWorkspaceChanged("dev".to_owned())),
        );
        assert!(task.is_none());

        let guard = plugin_model.read().unwrap();
        let model = guard.downcast_ref::<Model>().unwrap();
        assert_eq!(model.active_workspace_name, "dev");
    }

    #[test]
    fn empty_view_launch_and_error_drain_are_deterministic() {
        let plugin_model = test_model();
        assert!(launch(0, plugin_model.clone()).is_none());
        assert_eq!(view(plugin_model.clone()).unwrap().len(), 1);

        {
            let mut guard = plugin_model.write().unwrap();
            let model = guard.downcast_mut::<Model>().unwrap();
            model.errors.push("hyprland failed".to_owned());
        }

        assert_eq!(errors(plugin_model.clone()), vec!["hyprland failed"]);
        assert!(errors(plugin_model).is_empty());
    }
}
