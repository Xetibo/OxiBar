use std::{
    collections::HashMap,
    fmt::Debug,
    sync::{Arc, Mutex, RwLock},
    thread,
};

use hyprland::{
    data::Workspace,
    dispatch::Dispatch,
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
use iced_anim::AnimationBuilder;
use oxiced::{any_send::OxiAny, theme::theme_impl::OXITHEME, widgets::oxi_button};
use toml::Table;

#[derive(Default)]
pub struct Model {
    workspaces: HashMap<String, hyprland::data::Workspace>,
    active_workspace_name: String,
}

/// Convenience: box a Message as a shared Arc<dyn OxiAny>.
/// Arc::new(val) coerces Message -> dyn OxiAny directly — no extra indirection layer.
pub fn to_plugin_msg(msg: Message) -> Arc<dyn OxiAny> {
    Arc::new(msg)
}

impl Model {
    pub fn new(_global_config: Table) -> Model {
        Model {
            workspaces: HashMap::new(),
            active_workspace_name: Workspace::get_active()
                .map(|workspace| workspace.name)
                .unwrap_or_default(),
        }
    }
}

impl Debug for Model {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!(
            "workspaces: {:?} \n active_workspace: {:?}",
            &self.workspaces, &self.active_workspace_name,
        ))
    }
}

unsafe impl Send for Model {}
unsafe impl Sync for Model {}

#[derive(Clone, Debug)]
pub enum Message {
    ActiveWorkspaceChanged(String),
    ActivateWorkspace(String),
    WorkspacesInit(HashMap<String, hyprland::data::Workspace>),
    WorkspaceAdded(hyprland::data::Workspace),
    WorkspaceRemoved(String),
}

#[unsafe(no_mangle)]
#[allow(improper_ctypes_definitions)]
pub extern "Rust" fn model(
    global_config: Table,
) -> (
    Arc<RwLock<&'static mut dyn OxiAny>>,
    Option<Task<Arc<dyn OxiAny>>>,
) {
    let m = Box::leak(Box::new(Model::new(global_config)));
    (Arc::new(RwLock::new(m as &'static mut dyn OxiAny)), None)
}

#[unsafe(no_mangle)]
#[allow(improper_ctypes_definitions)]
pub extern "Rust" fn update(
    _filter_text: String,
    model: Arc<RwLock<&'static mut dyn OxiAny>>,
    msg: Arc<dyn OxiAny>,
) -> Option<Task<Arc<dyn OxiAny>>> {
    let mut model_borrow = model.try_write().ok()?;
    let model = model_borrow.downcast_mut::<Model>()?;
    let msg = msg.downcast_ref::<Message>()?.to_owned();
    match msg {
        Message::WorkspacesInit(workspaces) => {
            model.workspaces = workspaces;
            None
        }
        Message::ActivateWorkspace(name) => {
            thread::spawn(move || {
                let _ = Dispatch::call(hyprland::dispatch::DispatchType::Workspace(
                    hyprland::dispatch::WorkspaceIdentifierWithSpecial::Name(&name),
                ));
            });
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
    }
}

#[unsafe(no_mangle)]
#[allow(improper_ctypes_definitions)]
pub extern "Rust" fn launch(
    _focused_index: usize,
    _model: Arc<RwLock<&'static mut dyn OxiAny>>,
) -> Option<Task<Arc<dyn OxiAny>>> {
    None
}

#[unsafe(no_mangle)]
#[allow(improper_ctypes_definitions)]
pub extern "Rust" fn view(
    model: Arc<RwLock<&'static mut dyn OxiAny>>,
) -> Result<Vec<Element<'static, Arc<dyn OxiAny>>>, std::io::Error> {
    let lock = model.try_read().map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Could not get model in view",
        )
    })?;
    let model = lock.downcast_ref::<Model>().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Could not get model in view",
        )
    })?;

    let palette = &OXITHEME;
    let mut sorted_entries: Vec<hyprland::data::Workspace> =
        model.workspaces.clone().into_values().collect();
    sorted_entries.sort_by(|a, b| a.id.cmp(&b.id));

    let workspace_entries: Vec<Element<Arc<dyn OxiAny>>> = sorted_entries
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
            let font = Font {
                family: iced::font::Family::SansSerif,
                weight: iced::font::Weight::Semibold,
                stretch: iced::font::Stretch::Normal,
                style: iced::font::Style::Normal,
            };

            AnimationBuilder::new((bg_color, fg_color), move |(bg_color, fg_color)| {
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
                let style = |base: iced::widget::button::Style,
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
                .on_press(to_plugin_msg(Message::ActivateWorkspace(
                    workspace.name.clone(),
                )))
                .style(move |&_, status| (style)(base, status))
                .padding(0)
                .height(22.5)
                .width(22.5)
                .into()
            })
            .into()
        })
        .collect();

    let workspace_row = Row::from_vec(workspace_entries)
        .align_y(Alignment::Center)
        .spacing(5);

    Ok(vec![workspace_row.into()])
}

#[unsafe(no_mangle)]
#[allow(improper_ctypes_definitions)]
pub extern "Rust" fn name() -> &'static str {
    "Workspaces"
}

#[unsafe(no_mangle)]
#[allow(improper_ctypes_definitions)]
pub extern "Rust" fn errors(_model: Arc<RwLock<&'static mut dyn OxiAny>>) -> Vec<String> {
    vec![]
}

/// Returns a raw pointer to a boxed stream of plugin messages.
/// Uses a plain std::thread with the sync hyprland EventListener — no tokio I/O in the plugin.
/// This avoids the two-tokio-instance problem (separate dylib = separate reactor).
/// The stream bridge uses `try_send` which is sync and doesn't need a runtime.
#[unsafe(no_mangle)]
#[allow(improper_ctypes_definitions)]
pub extern "Rust" fn subscription() -> *mut (dyn Stream<Item = Arc<dyn OxiAny>> + Send) {
    let s =
        stream::channel(
            100,
            move |output: iced::futures::channel::mpsc::Sender<Arc<dyn OxiAny>>| async move {
                // Wrap sender in a Mutex so Fn closures (not FnMut) can call try_send via &mut.
                let output = Arc::new(Mutex::new(output));

                std::thread::spawn(move || {
                    // Initial workspace fetch (sync).
                    if let Ok(workspaces) = hyprland::data::Workspaces::get() {
                        let map = workspaces
                            .into_iter()
                            .map(|w| (w.name.clone(), w))
                            .collect();
                        let _ = output
                            .lock()
                            .unwrap()
                            .try_send(to_plugin_msg(Message::WorkspacesInit(map)));
                    }

                    let mut listener = EventListener::new();

                    {
                        let out = output.clone();
                        listener.add_workspace_added_handler(move |_| {
                            if let Ok(workspaces) = hyprland::data::Workspaces::get() {
                                let map = workspaces
                                    .into_iter()
                                    .map(|w| (w.name.clone(), w))
                                    .collect();
                                let _ = out
                                    .lock()
                                    .unwrap()
                                    .try_send(to_plugin_msg(Message::WorkspacesInit(map)));
                            }
                        });
                    }

                    {
                        let out = output.clone();
                        listener.add_workspace_deleted_handler(move |workspace| {
                            let _ = out.lock().unwrap().try_send(to_plugin_msg(
                                Message::WorkspaceRemoved(workspace.name.to_string()),
                            ));
                        });
                    }

                    {
                        let out = output.clone();
                        listener.add_workspace_changed_handler(move |workspace| {
                            let _ = out.lock().unwrap().try_send(to_plugin_msg(
                                Message::ActiveWorkspaceChanged(workspace.name.to_string()),
                            ));
                        });
                    }

                    {
                        let out = output.clone();
                        listener.add_active_monitor_changed_handler(move |monitor| {
                            let name = monitor
                                .workspace_name
                                .map(|n| n.to_string())
                                .unwrap_or_default();
                            let _ = out
                                .lock()
                                .unwrap()
                                .try_send(to_plugin_msg(Message::ActiveWorkspaceChanged(name)));
                        });
                    }

                    let _ = listener.start_listener();
                });

                // Keep the async block alive forever so iced doesn't drop the stream.
                std::future::pending::<()>().await;
            },
        );

    Box::into_raw(Box::new(s))
}
