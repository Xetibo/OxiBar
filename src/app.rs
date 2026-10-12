use std::{
    os::unix::net::UnixStream,
    panic::{self, AssertUnwindSafe},
    path::{Path, PathBuf},
    pin::Pin,
    sync::{Arc, Mutex},
    time::Duration,
};

use iced::{Font, Subscription, Task, Theme, event, theme::Style, window};
use iced_layershell::reexport::{IcedId, KeyboardInteractivity, WithConnection};
use oxibar_plugin_api::{
    HostToastRequest, PluginMsg, PluginPopupMetrics, PluginStream, SubscriptionFn,
};
use oxiced::{theme::theme_impl::get_derived_iced_theme, widgets::oxi_layer::layer_theme};
#[cfg(test)]
use toml::Table;
use wayland_client::{
    Connection, Dispatch, QueueHandle,
    globals::{Global, GlobalListContents, registry_queue_init},
    protocol::wl_registry,
};

use crate::{
    config::{self, ConfigStore, get_config},
    control_center::ControlCenter,
    font,
    ipc::{self, LauncherCommand},
    launcher::{self, Launcher},
    layout::{self, BarDimensions, BarSection, DEFAULT_POPUP_SIZE, PopupMetrics, SCALE_FACTOR},
    messages::{Message, map_plugin_message},
    plugins::{
        PluginMap, dispatch_launch, dispatch_update, drain_errors, load_plugins,
        query_plugin_popup_metrics,
    },
    settings::{self, ConfigRuntime, SettingsUi},
    shell::{Shell, SurfaceKind},
    single_instance::{self, SingleInstanceError},
};

pub fn run() -> Result<(), iced_layershell::Error> {
    init_tracing();
    let startup_config = get_config();

    let _instance_guard = match single_instance::acquire(&config::instance_policy(&startup_config))
    {
        Ok(guard) => guard,
        Err(SingleInstanceError::AlreadyRunning) => {
            eprintln!(
                "oxibar: another instance is already running; \
                 set [instance] allow_multiple_instances = true to run concurrent bars"
            );
            std::process::exit(1);
        }
        Err(SingleInstanceError::Io(err)) => {
            tracing::warn!(error = ?err, "could not acquire single-instance lock; continuing without");
            None
        }
    };

    let policy = config::startup_retry_policy(&startup_config);
    let mut attempts: u32 = 0;
    let mut backoff: u64 = policy.initial_delay_ms;

    loop {
        let is_last = !policy.can_attempt_more(attempts);
        let connection = if policy.enabled {
            compositor_ready()
        } else {
            None
        };
        let ready = !policy.enabled || connection.is_some();

        if ready || is_last {
            let outcome = panic::catch_unwind(AssertUnwindSafe(|| run_bar(connection)));

            match outcome {
                Ok(Ok(())) => return Ok(()),
                Ok(Err(err)) => {
                    if is_last || !policy.enabled {
                        return Err(err);
                    }
                    tracing::warn!(
                        retry = attempts + 1,
                        error = ?err,
                        "oxibar daemon failed to start; backing off before retry"
                    );
                }
                Err(payload) => {
                    let retryable = policy.catch_panics && policy.enabled && !is_last;
                    if !retryable {
                        panic::resume_unwind(payload);
                    }
                    tracing::warn!(
                        retry = attempts + 1,
                        "oxibar startup panicked; backing off before retry"
                    );
                }
            }

            attempts += 1;
        } else {
            tracing::info!(
                retry = attempts + 1,
                delay_ms = backoff,
                "wayland compositor not ready yet; waiting before retrying"
            );
            attempts += 1;
        }

        std::thread::sleep(Duration::from_millis(backoff));
        backoff = backoff.saturating_mul(2).min(policy.max_delay_ms);
    }
}

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();
}

/// Probe the compositor and return a working connection when the layer-shell
/// globals the bar needs are advertised. Returns `None` while the compositor
/// is not yet accepting clients (or not reachable), in which case the caller
/// backs off and retries.
fn compositor_ready() -> Option<Connection> {
    let connection = connect_wayland()?;
    let ready = registry_queue_init::<CompositorReadiness>(&connection)
        .map(|(globals, _event_queue)| {
            globals
                .contents()
                .with_list(has_required_layer_shell_globals)
        })
        .unwrap_or(false);
    ready.then_some(connection)
}

/// Connect to the compositor, tolerating a missing, empty, or stale
/// `WAYLAND_DISPLAY` (e.g. launched from a compositor startup hook before the
/// environment was populated) by falling back to scanning
/// `$XDG_RUNTIME_DIR` for a connectable `wayland-*` socket. The returned
/// connection is handed explicitly to the layer-shell backend, so the bar no
/// longer depends on the inherited environment at surface-creation time.
pub(crate) fn connect_wayland() -> Option<Connection> {
    if let Ok(connection) = Connection::connect_to_env() {
        return Some(connection);
    }
    let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from)?;
    discover_wayland_connection(&runtime_dir)
}

fn discover_wayland_connection(runtime_dir: &Path) -> Option<Connection> {
    wayland_socket_candidates_in(runtime_dir)
        .into_iter()
        .find_map(|path| connection_from_socket_path(&path))
}

fn wayland_socket_candidates_in(runtime_dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(runtime_dir) else {
        return Vec::new();
    };
    let mut candidates: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("wayland-"))
        })
        .collect();
    candidates.sort();
    candidates
}

fn connection_from_socket_path(path: &Path) -> Option<Connection> {
    let stream = UnixStream::connect(path).ok()?;
    Connection::from_socket(stream).ok()
}

struct CompositorReadiness;

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for CompositorReadiness {
    fn event(
        _state: &mut Self,
        _proxy: &wl_registry::WlRegistry,
        _event: wl_registry::Event,
        _data: &GlobalListContents,
        _connection: &Connection,
        _queue_handle: &QueueHandle<Self>,
    ) {
    }
}

fn has_required_layer_shell_globals(globals: &[Global]) -> bool {
    // Minimal on purpose: the layer-shell backend hard-fails without these
    // two, while anything stricter (e.g. also requiring wl_output) risks a
    // false negative that waits forever even though the bar could start. A
    // false positive here only costs one startup attempt, which the retry
    // loop absorbs.
    ["wl_compositor", "zwlr_layer_shell_v1"]
        .into_iter()
        .all(|required| globals.iter().any(|global| global.interface == required))
}

fn run_bar(connection: Option<Connection>) -> Result<(), iced_layershell::Error> {
    let store = Arc::new(Mutex::new(ConfigStore::open(config::get_config_dir())));
    let initial = store.lock().expect("initial configuration").snapshot();
    let inventory = crate::monitor::read_inventory();
    let mut settings = layout::layer_shell_settings(&initial.effective);
    if let Some(connection) = connection {
        settings.with_connection = Some(WithConnection::Value(connection));
    }
    let mut app = iced_layershell::daemon(
        move || OxiBar::new(store.clone(), inventory.clone()),
        OxiBar::namespace,
        OxiBar::update,
        OxiBar::view,
    )
    .subscription(OxiBar::subscription)
    .settings(settings)
    .theme(OxiBar::theme)
    .default_font(Font::with_name(font::bar_font(&initial.effective)))
    .style(OxiBar::style)
    .scale_factor(OxiBar::scale_factor);
    if let Some(bytes) = font::bar_font_bytes(&initial.effective) {
        app = app.font(bytes);
    }
    app.run()
}

pub(crate) struct OxiBar {
    pub(crate) paste: crate::paste::PasteState,
    pub(crate) overview: crate::overview::Overview,
    pub(crate) appearance: crate::theme::ThemeRuntime,
    pub(crate) docks: crate::dock::Docks,
    pub(crate) desktop: crate::desktop::Desktop,
    pub(crate) compositor: Option<oxibar_plugin_api::CompositorSnapshot>,
    pub(crate) compositor_error: Option<String>,
    pub(crate) compositor_action_error: Option<String>,
    pub(crate) osds: crate::osd::Osds,
    pub(crate) shell: Shell,
    pub(crate) config: ConfigRuntime,
    pub(crate) settings: SettingsUi,
    pub(crate) control_center: ControlCenter,
    pub(crate) plugin_order: Vec<String>,
    pub(crate) plugin_failures: std::collections::BTreeMap<String, String>,
    pub(crate) plugin_runtime: crate::plugins::PluginRuntime,
    pub(crate) launcher: Launcher,
    pub(crate) theme: Theme,
    pub(crate) plugins: PluginMap,
    pub(crate) transparent: bool,
    pub(crate) bar_window_id: Option<IcedId>,
    pub(crate) bar_size: BarDimensions,
    pub(crate) start_widgets: Vec<String>,
    pub(crate) center_widgets: Vec<String>,
    pub(crate) end_widgets: Vec<String>,
    pub(crate) popup_plugin: Option<String>,
    pub(crate) popup_open: bool,
    pub(crate) modal_plugin: Option<String>,
    pub(crate) modal_window_id: Option<IcedId>,
    pub(crate) panel_plugin: Option<String>,
    pub(crate) panel_window_id: Option<IcedId>,
    pub(crate) toasts: Vec<ToastEntry>,
}

#[derive(Clone, Debug)]
pub(crate) struct ToastEntry {
    pub(crate) plugin_id: String,
    pub(crate) toast_id: String,
    pub(crate) window_id: IcedId,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

/// Decide whether a layer-surface size event must (re)apply the input region.
/// The boot-time region task races surface creation and iced_layershell
/// silently drops `SetInputRegion` while no surface is ready, so the first
/// event for the main window must always re-apply: otherwise the Wayland
/// default (the whole bar + popup-reserve surface) stays clickable and swallows
/// clicks meant for windows below the bar.
fn should_refresh_input_region(
    first_ready: bool,
    previous: BarDimensions,
    current: BarDimensions,
) -> bool {
    first_ready || previous != current
}

impl OxiBar {
    fn new(
        store: Arc<Mutex<ConfigStore>>,
        inventory: Result<crate::monitor::OutputSnapshot, String>,
    ) -> (Self, Task<Message>) {
        let initial = store.lock().expect("initial configuration").snapshot();
        let loaded = load_plugins(&initial.effective);
        let schema = loaded
            .plugins
            .values()
            .flat_map(|(_, funcs)| {
                funcs
                    .settings_schema
                    .map(|schema| unsafe { schema() })
                    .unwrap_or_default()
                    .into_iter()
                    .map(|mut field| {
                        if funcs.config_changed.is_none() {
                            field.apply = oxibar_plugin_api::ApplyMode::Restart;
                        }
                        field
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        let mut error = store.lock().expect("initial configuration").error.clone();
        if let Err(problem) = store
            .lock()
            .expect("initial configuration")
            .register_schema(schema)
        {
            error = Some(problem);
        }
        for (name, (_, funcs)) in &loaded.plugins {
            if let Some(validate) = funcs.config_validate
                && let Err(error) = store
                    .lock()
                    .expect("initial configuration")
                    .register_validator(name.clone(), validate, funcs._lib.clone())
            {
                tracing::warn!("Plugin configuration validation failed: {error}");
            }
        }
        error = store
            .lock()
            .expect("initial configuration")
            .error
            .clone()
            .or(error);
        let runtime = ConfigRuntime::shared(store);
        let mut settings_ui = SettingsUi::new(&runtime.snapshot);
        settings_ui.error = error;
        let config = &runtime.snapshot.effective;
        let bar_table = config.get("bar").and_then(|v| v.as_table());
        let transparent = bar_table
            .and_then(|t| t.get("transparent"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let read_section = |key: &str| -> Vec<String> {
            bar_table
                .and_then(|t| t.get(key))
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_owned()))
                        .collect()
                })
                .unwrap_or_default()
        };
        let start_widgets = read_section("start");
        let center_widgets = read_section("center");
        let end_widgets = read_section("end");
        let bar_size = layout::bar_size_from_config(config);
        let default_layout =
            config::schema::fallback_layout(&runtime.snapshot.host, &runtime.snapshot.overrides);
        let start_widgets = if default_layout {
            config::schema::fallback_widgets(&runtime.snapshot.host, &loaded.order)
        } else {
            start_widgets
        };
        let launcher = Launcher::new(config);
        let appearance = crate::theme::ThemeRuntime::new(config, runtime.snapshot.revision, true);
        let mut bar = Self {
            paste: crate::paste::PasteState::default(),
            overview: crate::overview::Overview::default(),
            appearance,
            docks: crate::dock::Docks::default(),
            desktop: crate::desktop::Desktop::default(),
            compositor: None,
            compositor_error: None,
            compositor_action_error: None,
            osds: crate::osd::Osds::default(),
            shell: Shell::new(inventory),
            config: runtime,
            settings: settings_ui,
            control_center: ControlCenter::default(),
            plugin_order: loaded.order,
            plugin_failures: loaded.failures,
            plugin_runtime: crate::plugins::PluginRuntime::default(),
            launcher,
            plugins: loaded.plugins,
            theme: get_derived_iced_theme(),
            transparent,
            bar_window_id: None,
            bar_size,
            start_widgets,
            center_widgets,
            end_widgets,
            popup_plugin: None,
            popup_open: false,
            modal_plugin: None,
            modal_window_id: None,
            panel_plugin: None,
            panel_window_id: None,
            toasts: Vec::new(),
        };
        bar.refresh_plugin_catalog();
        let mut tasks = loaded.tasks;
        tasks.push(bar.apply_theme(true));
        tasks.push(bar.reconcile_bars());
        tasks.push(bar.reconcile_docks());
        tasks.push(bar.reconcile_desktop());
        if crate::config::store::get_path(&bar.config.snapshot.effective, "monitor.apply_on_start")
            .and_then(toml::Value::as_bool)
            .unwrap_or(true)
            && crate::config::store::get_path(&bar.config.snapshot.effective, "monitor.profiles")
                .and_then(toml::Value::as_array)
                .is_some_and(|profiles| !profiles.is_empty())
        {
            tasks.push(bar.update_monitors(crate::monitor::runtime::Action::ApplySaved));
        }
        (bar, Task::batch(tasks))
    }

    fn namespace() -> String {
        String::from("OxiBar")
    }

    pub(crate) fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::PluginsLoaded(outcome) => self.plugins_loaded(&outcome),
            Message::PluginEmission(name, message) => {
                let message = crate::messages::translate_plugin_message(name.clone(), message);
                if self.plugins.contains_key(&name) {
                    self.update(message)
                } else if let Message::PluginSubMsg(_, message) = message {
                    self.resident_plugin_update(&name, message)
                } else {
                    Task::none()
                }
            }
            Message::ShortcutKey(id, key, modifiers, status) => self
                .shortcut(id, key, modifiers, status)
                .map(|message| self.update(message))
                .unwrap_or_else(Task::none),
            Message::Overview(action) => self.update_overview(action),
            Message::OverviewBounds(id, target, bounds) => self.overview_bounds(id, target, bounds),
            Message::OverviewPointer(id, position) => self.overview_pointer(id, position),
            Message::OverviewPress(id) => self.overview_press(id),
            Message::OverviewKey(id, action) => {
                if self.overview.surfaces.iter().any(|(output, surface)| {
                    surface.id == id && Some(output) == self.overview.active.as_ref()
                }) {
                    self.update_overview(action)
                } else {
                    Task::none()
                }
            }
            Message::OverviewSnapshot(generation, snapshot) => {
                self.overview_snapshot(generation, snapshot)
            }
            Message::OverviewRetirementTimeout(generation) => {
                self.overview_retirement_timeout(generation)
            }
            Message::OverviewActivated(generation, result) => {
                if generation != self.overview.generation {
                    return Task::none();
                }
                self.compositor_action_error = result.err();
                self.close_overview()
            }
            Message::DropdownEscape(id) => {
                if self.popup_open && self.shell.popup_bar == Some(id) {
                    self.close_bar_dropdown()
                } else if self.panel_window_id == Some(id) {
                    self.close_panel()
                } else {
                    Task::none()
                }
            }
            Message::BarAnchor {
                id,
                key,
                provider,
                bounds,
                pressed,
            } => self.bar_anchor(id, key, provider, bounds, pressed),
            Message::ThemeFontLoaded(requested, result) => {
                self.theme_font_loaded(requested, result)
            }
            Message::ThemeFontReady(requested, result) => self.theme_font_ready(requested, result),
            Message::OverlayPointer(id, position) => {
                let position = position.filter(|point| point.x.is_finite() && point.y.is_finite());
                if let Some(record) = self.shell.registry.get_mut(id) {
                    record.pointer = position;
                }
                if position.is_some() {
                    self.bar_pointer(id, position)
                } else {
                    Task::none()
                }
            }
            Message::OverlayLeave(id) => {
                if let Some(record) = self.shell.registry.get_mut(id) {
                    record.pointer = None;
                }
                self.bar_pointer(id, None)
            }
            Message::OverlayPress(id) => self.overlay_press(id),
            Message::CloseBarDropdown => self.close_bar_dropdown(),
            Message::SettingsDragRelease(id) => {
                if self.settings.window_id == Some(id) {
                    self.settings.widget_drag = None;
                }
                Task::none()
            }
            Message::Session(action) => self.update_session_menu(action),
            Message::SessionKey(id, action) => {
                if self.shell.session_window == Some(id) {
                    self.update_session_menu(action)
                } else {
                    Task::none()
                }
            }
            Message::LocalCommandFinished {
                provider,
                source,
                outcome,
            } => {
                self.shell.session_error = outcome.result.err();
                self.update(Message::PluginTask(
                    source,
                    Box::new(map_plugin_message(provider, outcome.message)),
                ))
            }
            Message::CompositorChanged(result) => self.compositor_changed(result),
            Message::CompositorActionDone(result) => {
                if let Err(error) = &result {
                    tracing::warn!(%error,"compositor action failed");
                }
                self.compositor_action_error = result.err();
                Task::none()
            }
            Message::Workspace(id) => self.compositor_action(None, Some(id)),
            Message::Dock(action) => self.dock_action(action),
            Message::Desktop(action) => self.update_desktop(action),
            Message::ConfigureDockLayer(id, size, bottom, margin, reserve) => Task::batch([
                Task::done(Message::ResizeLayer(id, size.0, size.1)),
                Task::done(Message::SetLayerAnchor(id, bottom)),
                Task::done(Message::SetLayerMargin(
                    id,
                    if bottom {
                        (0, 0, margin as i32, 0)
                    } else {
                        (margin as i32, 0, 0, 0)
                    },
                )),
                Task::done(Message::SetExclusiveZone(
                    id,
                    if reserve { (size.1 + margin) as i32 } else { 0 },
                )),
            ]),
            Message::ConfigureDesktopLayer(id, rect) => Task::batch([
                Task::done(Message::ResizeLayer(id, rect.width, rect.height)),
                Task::done(Message::SetLayerMargin(
                    id,
                    (rect.y as i32, 0, 0, rect.x as i32),
                )),
            ]),
            Message::ShowOsd(plugin, request) => {
                self.request_osd(plugin, request).unwrap_or_else(|error| {
                    tracing::debug!(%error, "OSD request rejected");
                    Task::none()
                })
            }
            Message::ExpireOsd(id, generation) => self.expire_osd(id, generation),
            Message::CommandFinished {
                plugin,
                outcome,
                reply,
                output,
            } => self.finish_command(plugin, outcome, reply, output),
            Message::OnCommandOutput(output, message) => {
                let previous = self.shell.command_output.replace(output.clone());
                let task = self.update(*message);
                self.shell.command_output = previous;
                task.map(move |message| Message::OnCommandOutput(output.clone(), Box::new(message)))
            }
            Message::Exit => std::process::exit(0),
            Message::OutputsChanged(result) => self.outputs_changed(result),
            Message::Monitor(action) => self.update_monitors(action),
            Message::MonitorsLoaded(result) => self.monitors_loaded(result),
            Message::MonitorsPreviewed(result) => self.monitors_previewed(result),
            Message::MonitorsReverted(result) => self.monitors_reverted(result),
            Message::MonitorsSaved(result) => self.monitors_saved(result),
            Message::MonitorsExpired(event) => self.monitors_expired(event),
            Message::SurfaceClosed(id) => {
                let paste = self.paste_focus(id, false);
                Task::batch([self.surface_closed(id), paste])
            }
            Message::SurfaceFocus(id, focused) => self.paste_focus(id, focused),
            Message::Paste(plugin, request) => self.request_paste(plugin, request),
            Message::PasteDismissed(generation) => self.paste_dismissed(generation),
            Message::PasteTimeout(generation) => self.paste_timeout(generation),
            Message::PasteFinished(generation, result) => self.finish_paste(generation, result),
            Message::ModalEscape(id) => {
                if self.modal_window_id == Some(id) {
                    self.close_modal()
                } else {
                    Task::none()
                }
            }
            Message::ModalFocus(id, forward) => {
                if self.modal_window_id != Some(id) {
                    return Task::none();
                }
                if forward {
                    iced::widget::operation::focus_next()
                } else {
                    iced::widget::operation::focus_previous()
                }
            }
            Message::FromSurface(id, message) => self.source_message(id, *message),
            Message::PluginTask(source, message) => {
                if let Some(id) = source
                    && self.shell.registry.get(id).is_some()
                {
                    self.source_message(id, *message)
                } else {
                    self.update(*message)
                }
            }
            Message::Settings(action) => self.update_settings(action),
            Message::ControlCenter(action) => self.update_control_center(action),
            Message::ControlCenterKey(id, action) => {
                if self.control_center.window_id == Some(id) {
                    self.update_control_center(action)
                } else {
                    Task::none()
                }
            }
            Message::ControlCenterFocus(id, forward) => {
                if self.control_center.window_id != Some(id) {
                    return Task::none();
                }
                if forward {
                    iced::widget::operation::focus_next()
                } else {
                    iced::widget::operation::focus_previous()
                }
            }
            Message::SettingsEscape(id) => {
                if self.settings.window_id == Some(id) {
                    self.close_settings()
                } else {
                    Task::none()
                }
            }
            Message::SettingsFocus(id, forward) => {
                if self.settings.window_id == Some(id) {
                    if forward {
                        iced::widget::operation::focus_next()
                    } else {
                        iced::widget::operation::focus_previous()
                    }
                } else {
                    Task::none()
                }
            }
            Message::OxiPasteEscape(id) => {
                if self.shell.oxipaste_window == Some(id) {
                    self.close_oxipaste()
                } else {
                    Task::none()
                }
            }
            Message::OxiPasteFocus(id, forward) => {
                if self.shell.oxipaste_window != Some(id) {
                    return Task::none();
                }
                if forward {
                    iced::widget::operation::focus_next()
                } else {
                    iced::widget::operation::focus_previous()
                }
            }
            Message::SettingsSearchFocus(id) => {
                if self.settings.window_id == Some(id) {
                    iced::widget::operation::focus(settings::SEARCH_ID)
                } else {
                    Task::none()
                }
            }
            Message::SettingsCategory(id, category) => {
                if self.settings.window_id == Some(id) {
                    self.update_settings(settings::Action::Category(category))
                } else {
                    Task::none()
                }
            }
            Message::ControlRequest(request) => self.control_request(request),
            Message::ReloadConfig => self.reload_config(None),
            Message::ConfigUpdated(outcome) => {
                self.desktop_config_outcome(&outcome);
                self.config_updated(&outcome)
            }
            Message::ControlLauncher(command) => self.update_launcher(match command {
                LauncherCommand::Toggle => launcher::Action::Toggle,
                LauncherCommand::Open => launcher::Action::Open,
                LauncherCommand::Close => launcher::Action::Close,
            }),
            Message::Launcher(action) => self.update_launcher(action),
            Message::LauncherKey(id, action) => {
                if self.launcher.window_id == Some(id) {
                    self.update_launcher(action)
                } else {
                    Task::none()
                }
            }
            Message::SetPopupInputRegion { .. }
            | Message::Ignore
            | Message::OpenOsdLayer(_, _)
            | Message::OpenOverviewLayer(_, _, _)
            | Message::OpenSessionLayer(_, _)
            | Message::OpenOxiPasteLayer(_, _)
            | Message::OpenDockLayer(_, _, _, _, _)
            | Message::OpenDesktopLayer(_, _, _, _)
            | Message::SetLayerAnchor(_, _)
            | Message::ConfigureOsdLayer(_, _)
            | Message::SetLayerMargin(_, _)
            | Message::OnOutput(_, _)
            | Message::OpenBarLayer(_, _, _)
            | Message::ConfigureBar(_, _)
            | Message::RemoveSurface(_)
            | Message::SetBarInputRegion { .. }
            | Message::OpenSettingsLayer(_, _, _)
            | Message::OpenControlCenterLayer(_, _, _)
            | Message::CloseSettingsLayer(_)
            | Message::ResizeLayer(_, _, _)
            | Message::SetExclusiveZone(_, _)
            | Message::OpenLauncherLayer(_, _, _)
            | Message::CloseLauncherLayer(_)
            | Message::OpenModalLayer(_)
            | Message::CloseModalLayer(_)
            | Message::OpenPanelLayer(_)
            | Message::ClosePanelLayer(_)
            | Message::OpenToastLayer(_, _, _, _)
            | Message::CloseToastLayer(_)
            | Message::MoveToastLayer(_, _)
            | Message::ResizeToastLayer(_, _, _)
            | Message::SetToastKeyboardInteractivity(_, _) => Task::none(),
            Message::LayerSurfaceResized(id, width, height) => {
                self.update_bar_size(id, width, height)
            }
            Message::SetPopupPlugin(plugin_id) => {
                self.popup_plugin = plugin_id;
                Task::none()
            }
            Message::SetPopupOpen(open) => {
                if !open {
                    return self.close_bar_dropdown();
                }
                self.popup_open = open;
                Task::none()
            }
            Message::RefreshPopupInputRegion(plugin_id) => {
                self.refresh_popup_input_region(&plugin_id)
            }
            Message::TogglePluginPopup(plugin_id) => self.toggle_plugin_popup(plugin_id),
            Message::TogglePluginPanel(plugin_id) => self.toggle_plugin_panel(plugin_id),
            Message::OpenPluginModal(plugin_id) => self.open_plugin_modal(plugin_id),
            Message::ClosePluginModal(plugin_id) => self.close_plugin_modal(plugin_id),
            Message::ShowPluginToast(plugin_id, request) => {
                self.show_plugin_toast(plugin_id, request)
            }
            Message::ClosePluginToast(plugin_id, toast_id) => {
                self.close_plugin_toast(&plugin_id, &toast_id)
            }
            Message::SetPluginToastKeyboard(plugin_id, toast_id, keyboard_interactivity) => {
                self.set_plugin_toast_keyboard(&plugin_id, &toast_id, keyboard_interactivity)
            }
            Message::PluginSubMsg(plugin_id, msg) => {
                let Some((model, funcs)) = self.plugins.get(&plugin_id) else {
                    return self.resident_plugin_update(&plugin_id, msg);
                };
                let task = dispatch_update(funcs, model.clone(), msg);
                let refresh_dock = funcs.dock_entries.is_some();
                let refresh_desktop = funcs.desktop_interactive.is_some();
                drain_errors(funcs, model);
                if funcs.runner_entries.is_some() {
                    self.launcher.refresh(&self.plugins);
                }
                let refresh = if self.popup_open
                    && self.popup_plugin.as_deref() == Some(plugin_id.as_str())
                {
                    Task::done(Message::RefreshPopupInputRegion(plugin_id.clone()))
                } else {
                    Task::none()
                };
                let refresh = if refresh_dock {
                    Task::batch([refresh, self.reconcile_docks()])
                } else {
                    refresh
                };
                let refresh = if refresh_desktop {
                    Task::batch([refresh, self.reconcile_desktop()])
                } else {
                    refresh
                };
                match task {
                    Some(task) => {
                        let id = plugin_id.clone();
                        let source = self
                            .shell
                            .request_source
                            .or_else(|| self.shell.plugin_origins.get(&plugin_id).copied());
                        refresh.chain(task.map(move |msg| {
                            let message = map_plugin_message(id.clone(), msg);
                            if source.is_some() {
                                Message::PluginTask(source, Box::new(message))
                            } else {
                                message
                            }
                        }))
                    }
                    None => refresh,
                }
            }
        }
    }

    pub(crate) fn update_launcher(&mut self, action: launcher::Action) -> Task<Message> {
        use launcher::Action;
        match action {
            Action::Toggle if self.launcher.window_id.is_some() => self.close_launcher(),
            Action::Toggle | Action::Open => {
                if !self.launcher.enabled || self.launcher.window_id.is_some() {
                    return Task::none();
                }
                let id = IcedId::unique();
                let output = self.overlay_output();
                self.register_overlay(
                    id,
                    SurfaceKind::Launcher,
                    self.shell.active_bar,
                    output.clone(),
                );
                self.launcher.window_id = Some(id);
                self.launcher.open_started_at = Some(std::time::Instant::now());
                self.launcher.query.clear();
                self.launcher.focused = 0;
                self.popup_open = false;
                let close_modal = self.close_modal();
                let close_panel = self.close_panel();
                let close_settings = self.close_settings();
                let close_control_center = self.close_control_center();
                let close_editor = if self.desktop.draft.is_some() {
                    self.cancel_desktop_edit()
                } else {
                    Task::none()
                };
                let input_region = self.current_input_region_task();
                let query = self.query_runner_plugins();
                Task::batch([
                    self.close_overview(),
                    self.close_oxipaste(),
                    close_modal,
                    close_panel,
                    close_settings,
                    close_control_center,
                    self.close_bar_dropdown(),
                    close_editor,
                    input_region,
                    query,
                    Task::done(Message::OnOutput(
                        output,
                        Box::new(Message::OpenLauncherLayer(
                            id,
                            self.launcher.size.0,
                            self.launcher.size.1,
                        )),
                    )),
                ])
            }
            Action::Close => self.close_launcher(),
            Action::Query(query) => {
                self.launcher.query_revision = self.launcher.query_revision.saturating_add(1);
                self.launcher.query_started_at = Some(std::time::Instant::now());
                self.launcher.query_elapsed_us = None;
                self.launcher.query = query;
                self.launcher.focused = 0;
                self.query_runner_plugins()
            }
            Action::MoveFocus(forward) => {
                self.launcher.move_focus(forward);
                Task::none()
            }
            Action::LaunchFocused => self.launch_runner_entry(self.launcher.focused),
            Action::Launch(index) => self.launch_runner_entry(index),
            Action::Activate(plugin, index) => self
                .launcher
                .entries
                .iter()
                .position(|entry| entry.plugin_id == plugin && entry.result.index == index)
                .map(|position| self.launch_runner_entry(position))
                .unwrap_or_else(Task::none),
            Action::Favorite(plugin, index) => {
                if self.config.jobs > 0 {
                    return Task::none();
                }
                let Some((model, funcs)) = self.plugins.get(&plugin) else {
                    return Task::none();
                };
                let Some(favorite) = funcs
                    .runner_favorite
                    .and_then(|favorite| unsafe { favorite(model.clone(), index) })
                else {
                    return Task::none();
                };
                if !self.config.snapshot.schema.iter().any(|field| {
                    field.path == favorite.path
                        && field.kind == oxibar_plugin_api::SettingKind::TextList
                }) {
                    return Task::none();
                }
                self.config_job(
                    Some(crate::config::Mutation::Set(
                        favorite.path,
                        toml::Value::Array(
                            favorite
                                .values
                                .into_iter()
                                .map(toml::Value::String)
                                .collect(),
                        ),
                    )),
                    Some(self.config.snapshot.revision),
                    None,
                    None,
                    false,
                )
            }
        }
    }

    pub(crate) fn query_runner_plugins(&mut self) -> Task<Message> {
        let tasks: Vec<_> = self
            .plugins
            .iter()
            .filter_map(|(plugin_id, (model, funcs))| {
                let query = funcs.runner_query?;
                funcs.runner_entries?;
                let task = if self.launcher.accepts(&self.plugins, funcs, model) {
                    unsafe { query(self.launcher.query.clone(), model.clone()) }
                } else {
                    if let Some(cancel) = funcs.runner_cancel {
                        unsafe { cancel(model.clone()) };
                    }
                    None
                };
                let id = plugin_id.clone();
                task.map(|task| task.map(move |message| map_plugin_message(id.clone(), message)))
            })
            .collect();
        self.launcher.refresh(&self.plugins);
        Task::batch(tasks)
    }

    fn launch_runner_entry(&mut self, index: usize) -> Task<Message> {
        if self.launcher.window_id.is_none() {
            return Task::none();
        }
        let Some(entry) = self.launcher.entries.get(index) else {
            return Task::none();
        };
        let Some((model, funcs)) = self.plugins.get(&entry.plugin_id) else {
            return Task::none();
        };
        let task = dispatch_launch(funcs, entry.result.index, model.clone());
        drain_errors(funcs, model);
        let plugin_id = entry.plugin_id.clone();
        let task = task
            .map(|task| task.map(move |message| map_plugin_message(plugin_id.clone(), message)))
            .unwrap_or_else(Task::none);
        // Only the launcher surface closes; providers, subscriptions and the bar stay warm.
        Task::batch([task, self.close_launcher()])
    }

    pub(crate) fn close_launcher(&mut self) -> Task<Message> {
        self.launcher.open_started_at = None;
        for (model, funcs) in self.plugins.values() {
            if let Some(cancel) = funcs.runner_cancel {
                unsafe { cancel(model.clone()) };
            }
        }
        let mut tasks = Vec::new();
        tasks.push(
            self.launcher
                .window_id
                .take()
                .map(|id| {
                    self.shell.registry.remove(id);
                    Task::done(Message::CloseLauncherLayer(id))
                })
                .unwrap_or_else(Task::none),
        );
        Task::batch(tasks)
    }

    pub(crate) fn plugin_section(&self, plugin_id: &str) -> BarSection {
        if self
            .start_widgets
            .iter()
            .any(|name| name.eq_ignore_ascii_case(plugin_id))
        {
            BarSection::Start
        } else if self
            .center_widgets
            .iter()
            .any(|name| name.eq_ignore_ascii_case(plugin_id))
        {
            BarSection::Center
        } else {
            BarSection::End
        }
    }

    fn refresh_popup_input_region(&self, plugin_id: &str) -> Task<Message> {
        if !self.popup_open || self.popup_plugin.as_deref() != Some(plugin_id) {
            return Task::none();
        }

        self.shell
            .popup_bar
            .map(|id| self.bar_input_region_task(id))
            .unwrap_or_else(Task::none)
    }

    fn update_bar_size(
        &mut self,
        id: IcedId,
        layer_width: u32,
        layer_height: u32,
    ) -> Task<Message> {
        if let Some(record) = self.shell.registry.get_mut(id) {
            record.size = Some((layer_width, layer_height));
        }
        if self.settings.window_id == Some(id) {
            if self.settings.awaiting_focus {
                self.settings.awaiting_focus = false;
                return iced::widget::operation::focus(settings::SEARCH_ID);
            }
            return Task::none();
        }
        if self.launcher.window_id == Some(id) {
            if let Some(started) = self.launcher.open_started_at.take() {
                tracing::debug!(
                    elapsed_ms = started.elapsed().as_secs_f64() * 1000.0,
                    "launcher surface configured"
                );
            }
            return iced::widget::operation::focus(launcher::SEARCH_ID);
        }
        if self.shell.oxipaste_window == Some(id) {
            return iced::widget::operation::focus(crate::oxipaste::SEARCH_ID);
        }
        let Some(bar) = self.shell.bar_mut(id) else {
            return Task::none();
        };
        let first_ready = !bar.ready;
        let previous = bar.size;
        bar.ready = true;
        bar.size = layout::bar_size_from_layer_surface(previous, layer_width, layer_height);
        bar.size.height = bar.config.height;
        let current = bar.size;
        if self.bar_window_id == Some(id) {
            self.bar_size = current;
        }
        if !should_refresh_input_region(first_ready, previous, current) {
            return Task::none();
        }
        self.bar_input_region_task(id)
    }

    pub(crate) fn current_input_region_task(&self) -> Task<Message> {
        Task::batch(
            self.shell
                .bars
                .values()
                .map(|bar| self.bar_input_region_task(bar.id)),
        )
    }

    fn plugin_popup_metrics(&self, plugin_id: &str) -> Option<PluginPopupMetrics> {
        let (model, funcs) = self.plugins.get(plugin_id)?;
        query_plugin_popup_metrics(funcs, model)
    }

    fn popup_size(&self, plugin_id: &str, dynamic: Option<PluginPopupMetrics>) -> (u32, u32) {
        layout::popup_size_from_config(&self.config.snapshot.effective, plugin_id)
            .or_else(|| dynamic.and_then(|metrics| metrics.popup_size))
            .or_else(|| {
                self.plugins
                    .get(plugin_id)
                    .and_then(|(_, funcs)| funcs.metadata.popup_size)
            })
            .unwrap_or(DEFAULT_POPUP_SIZE)
    }

    fn toggle_plugin_popup(&mut self, plugin_id: String) -> Task<Message> {
        if plugin_id.eq_ignore_ascii_case("Clock")
            && self.config.snapshot.host.control_center_enabled
            && self.config.snapshot.host.menus.controls_dropdown
            && self.config.snapshot.host.menus.attach_to_clock
            && let Some(bar) = self.dropdown_bar()
        {
            return if self.control_center.attached_to == Some(bar)
                && self
                    .control_center
                    .selected
                    .as_deref()
                    .is_some_and(|name| name.eq_ignore_ascii_case(&plugin_id))
            {
                self.close_bar_dropdown()
            } else {
                self.open_attached_controls(bar, Some(plugin_id))
            };
        }
        let Some((_model, funcs)) = self.plugins.get(&plugin_id) else {
            tracing::warn!(plugin = plugin_id, "unknown plugin requested popup");
            return Task::none();
        };
        if funcs.popup_view.is_none()
            && !(plugin_id.eq_ignore_ascii_case("Notifications") && funcs.panel_view.is_some())
        {
            tracing::warn!(
                plugin = plugin_id,
                "plugin requested popup but has no popup_view"
            );
            return Task::none();
        }

        let Some(bar) = self.dropdown_bar() else {
            return Task::none();
        };
        if self.popup_open
            && self.shell.popup_bar == Some(bar)
            && self.popup_plugin.as_deref() == Some(plugin_id.as_str())
        {
            return self.close_bar_dropdown();
        }
        let notifications = plugin_id.eq_ignore_ascii_case("Notifications");
        let dismiss = Task::batch([
            self.close_oxipaste(),
            self.close_control_center(),
            self.close_bar_dropdown(),
            self.close_panel(),
        ]);
        self.shell.popup_bar = Some(bar);
        self.popup_plugin = Some(plugin_id);
        self.popup_open = true;
        dismiss.chain(Task::batch([
            self.current_input_region_task(),
            if notifications {
                self.close_all_toasts()
            } else {
                Task::none()
            },
            Task::done(Message::SetToastKeyboardInteractivity(
                bar,
                KeyboardInteractivity::Exclusive,
            )),
        ]))
    }

    fn open_plugin_modal(&mut self, plugin_id: String) -> Task<Message> {
        let Some((_model, funcs)) = self.plugins.get(&plugin_id) else {
            tracing::warn!(plugin = plugin_id, "unknown plugin requested modal");
            return Task::none();
        };
        if funcs.modal_view.is_none() {
            tracing::warn!(
                plugin = plugin_id,
                "plugin requested modal but has no modal_view"
            );
            return Task::none();
        }
        if self.modal_plugin.as_deref() == Some(plugin_id.as_str()) {
            return Task::none();
        }

        let id = IcedId::unique();
        let owner = self
            .shell
            .plugin_origins
            .get(&plugin_id)
            .copied()
            .or(self.shell.active_bar);
        let output = if self.shell.request_source.is_some() {
            self.overlay_output()
        } else {
            self.output_for_bar(owner).or_else(|| self.overlay_output())
        };
        let close_existing = self.close_modal();
        self.register_overlay(
            id,
            SurfaceKind::Modal(plugin_id.clone()),
            owner,
            output.clone(),
        );
        self.modal_plugin = Some(plugin_id);
        self.modal_window_id = Some(id);
        Task::batch([
            close_existing,
            self.close_oxipaste(),
            self.close_bar_dropdown(),
            Task::done(Message::OnOutput(
                output,
                Box::new(Message::OpenModalLayer(id)),
            )),
        ])
    }

    fn toggle_plugin_panel(&mut self, plugin_id: String) -> Task<Message> {
        if plugin_id.eq_ignore_ascii_case("Notifications")
            && self.config.snapshot.host.menus.notifications_dropdown
            && self.dropdown_bar().is_some()
        {
            return self.toggle_plugin_popup(plugin_id);
        }
        let Some((_model, funcs)) = self.plugins.get(&plugin_id) else {
            tracing::warn!(plugin = plugin_id, "unknown plugin requested panel");
            return Task::none();
        };
        if funcs.panel_view.is_none() {
            tracing::warn!(
                plugin = plugin_id,
                "plugin requested panel but has no panel_view"
            );
            return Task::none();
        }
        if self.panel_plugin.as_deref() == Some(plugin_id.as_str()) {
            return self.close_panel();
        }

        let id = IcedId::unique();
        let close_existing = self.panel_window_id.take();
        let owner = self
            .shell
            .plugin_origins
            .get(&plugin_id)
            .copied()
            .or(self.shell.active_bar);
        let output = if self.shell.request_source.is_some() {
            self.overlay_output()
        } else {
            self.output_for_bar(owner).or_else(|| self.overlay_output())
        };
        self.register_overlay(
            id,
            SurfaceKind::Panel(plugin_id.clone()),
            owner,
            output.clone(),
        );
        if let Some(existing) = close_existing {
            self.shell.registry.remove(existing);
        }
        self.panel_plugin = Some(plugin_id);
        self.panel_window_id = Some(id);
        let close_toasts = Task::batch([self.close_all_toasts(), self.close_oxipaste()]);

        if let Some(existing) = close_existing {
            close_toasts
                .chain(Task::done(Message::ClosePanelLayer(existing)))
                .chain(Task::done(Message::OnOutput(
                    output,
                    Box::new(Message::OpenPanelLayer(id)),
                )))
        } else {
            close_toasts.chain(Task::done(Message::OnOutput(
                output,
                Box::new(Message::OpenPanelLayer(id)),
            )))
        }
    }

    pub(crate) fn close_panel(&mut self) -> Task<Message> {
        self.panel_plugin = None;
        let Some(id) = self.panel_window_id.take() else {
            return Task::none();
        };
        self.shell.registry.remove(id);
        Task::done(Message::ClosePanelLayer(id))
    }

    fn close_plugin_modal(&mut self, plugin_id: String) -> Task<Message> {
        if self.modal_plugin.as_deref() != Some(plugin_id.as_str()) {
            return Task::none();
        }
        self.close_modal()
    }

    pub(crate) fn close_modal(&mut self) -> Task<Message> {
        let plugin = self.modal_plugin.take();
        let Some(id) = self.modal_window_id.take() else {
            return Task::none();
        };
        self.shell.registry.remove(id);
        Task::batch([
            self.modal_close_callback(plugin.as_deref(), id),
            Task::done(Message::CloseModalLayer(id)),
        ])
    }

    pub(crate) fn modal_close_callback(
        &self,
        plugin: Option<&str>,
        source: IcedId,
    ) -> Task<Message> {
        let Some(plugin) = plugin else {
            return Task::none();
        };
        let Some((model, funcs)) = self.plugins.get(plugin) else {
            return Task::none();
        };
        let Some(callback) = funcs.modal_closed else {
            return Task::none();
        };
        let Some(task) = (unsafe { callback(model.clone()) }) else {
            return Task::none();
        };
        let plugin = plugin.to_owned();
        task.map(move |message| {
            Message::PluginTask(
                Some(source),
                Box::new(crate::messages::map_plugin_message(plugin.clone(), message)),
            )
        })
    }

    fn show_plugin_toast(&mut self, plugin_id: String, request: HostToastRequest) -> Task<Message> {
        if self.panel_plugin.is_some()
            || (self.popup_open && self.popup_plugin.as_deref() == Some(plugin_id.as_str()))
            || (self.control_center.window_id.is_some()
                && self.control_center.selected.as_deref() == Some(plugin_id.as_str()))
        {
            return Task::none();
        }
        let Some((_model, funcs)) = self.plugins.get(&plugin_id) else {
            tracing::warn!(plugin = plugin_id, "unknown plugin requested toast");
            return Task::none();
        };
        if funcs.toast_view.is_none() {
            tracing::warn!(
                plugin = plugin_id,
                "plugin requested toast but has no toast_view"
            );
            return Task::none();
        }
        if request.width == 0 || request.height == 0 {
            tracing::warn!(
                plugin = plugin_id,
                toast_id = request.toast_id,
                "plugin requested zero-sized toast"
            );
            return Task::none();
        }

        if let Some(index) = self
            .toasts
            .iter()
            .position(|toast| toast.plugin_id == plugin_id && toast.toast_id == request.toast_id)
        {
            let window_id = self.toasts[index].window_id;
            self.toasts[index].width = request.width;
            self.toasts[index].height = request.height;
            return Task::done(Message::ResizeToastLayer(
                window_id,
                request.width,
                request.height,
            ))
            .chain(Task::batch(self.toast_layout_tasks()));
        }

        let window_id = IcedId::unique();
        let output = self.overlay_output();
        self.register_overlay(
            window_id,
            SurfaceKind::Toast(plugin_id.clone(), request.toast_id.clone()),
            None,
            output.clone(),
        );
        self.toasts.push(ToastEntry {
            plugin_id,
            toast_id: request.toast_id,
            window_id,
            width: request.width,
            height: request.height,
        });
        Task::done(Message::OnOutput(
            output,
            Box::new(Message::OpenToastLayer(
                window_id,
                request.width,
                request.height,
                self.config
                    .snapshot
                    .host
                    .toasts
                    .placement(self.config.snapshot.host.toasts.margin as i32),
            )),
        ))
        .chain(Task::batch(self.toast_layout_tasks()))
    }

    pub(crate) fn close_plugin_toast(&mut self, plugin_id: &str, toast_id: &str) -> Task<Message> {
        let Some(index) = self
            .toasts
            .iter()
            .position(|toast| toast.plugin_id == plugin_id && toast.toast_id == toast_id)
        else {
            return Task::none();
        };
        let toast = self.toasts.remove(index);
        self.shell.registry.remove(toast.window_id);
        Task::done(Message::CloseToastLayer(toast.window_id))
            .chain(Task::batch(self.toast_layout_tasks()))
    }

    fn set_plugin_toast_keyboard(
        &self,
        plugin_id: &str,
        toast_id: &str,
        keyboard_interactivity: KeyboardInteractivity,
    ) -> Task<Message> {
        let Some(toast) = self
            .toasts
            .iter()
            .find(|toast| toast.plugin_id == plugin_id && toast.toast_id == toast_id)
        else {
            return Task::none();
        };
        Task::done(Message::SetToastKeyboardInteractivity(
            toast.window_id,
            keyboard_interactivity,
        ))
    }

    pub(crate) fn close_all_toasts(&mut self) -> Task<Message> {
        let tasks = self
            .toasts
            .drain(..)
            .map(|toast| {
                self.shell.registry.remove(toast.window_id);
                Task::done(Message::CloseToastLayer(toast.window_id))
            })
            .collect::<Vec<_>>();
        Task::batch(tasks).chain(Task::batch(self.toast_layout_tasks()))
    }

    pub(crate) fn toast_layout_tasks(&self) -> Vec<Task<Message>> {
        let config = &self.config.snapshot.host.toasts;
        let mut offsets = std::collections::BTreeMap::<Option<String>, i32>::new();
        let mut tasks = Vec::with_capacity(self.toasts.len());

        for toast in self.toasts.iter().rev() {
            let output = self
                .shell
                .registry
                .get(toast.window_id)
                .and_then(|record| record.output.clone());
            let offset = offsets.entry(output).or_insert(config.margin as i32);
            let placement = config.placement(*offset);
            tasks.extend([
                Task::done(Message::MoveToastLayer(toast.window_id, placement)),
                Task::done(Message::SetLayerMargin(toast.window_id, placement.margin)),
            ]);
            *offset = offset
                .saturating_add(toast.height as i32)
                .saturating_add(config.spacing as i32);
        }
        for (output, instance) in &self.osds.instances {
            if instance.is_music_toast() {
                let offset = offsets.get(output).copied().unwrap_or(config.margin as i32);
                let mut osd = self.config.snapshot.host.osd.clone();
                osd.placement = Some(config.placement(offset));
                tasks.extend([
                    Task::done(Message::ConfigureOsdLayer(instance.id, osd)),
                    Task::done(Message::SetLayerMargin(
                        instance.id,
                        config.placement(offset).margin,
                    )),
                ]);
            }
        }

        tasks
    }

    pub(crate) fn music_toast_placement(
        &self,
        output: &Option<String>,
    ) -> config::toasts::Placement {
        let config = &self.config.snapshot.host.toasts;
        let offset = self
            .toasts
            .iter()
            .filter(|toast| {
                self.shell
                    .registry
                    .get(toast.window_id)
                    .is_some_and(|record| &record.output == output)
            })
            .fold(config.margin as i32, |offset, toast| {
                offset
                    .saturating_add(toast.height as i32)
                    .saturating_add(config.spacing as i32)
            });
        config.placement(offset)
    }

    fn theme(&self, _: IcedId) -> Theme {
        self.theme.clone()
    }

    fn subscription(&self) -> Subscription<Message> {
        let mut subs: Vec<Subscription<Message>> = Vec::with_capacity(self.plugins.len() + 1);
        subs.push(event::listen_with(layer_surface_size_event));
        let refresh = crate::config::store::get_path(
            &self.config.snapshot.effective,
            "monitor.refresh_seconds",
        )
        .and_then(|value| value.as_integer())
        .unwrap_or(2) as u64;
        subs.push(Subscription::run_with(
            refresh,
            crate::monitor::watch_inventory,
        ));
        subs.push(event::listen_with(crate::shortcuts::event));
        if let Some(client) = &self.overview.client {
            subs.push(Subscription::run_with(
                (self.overview.generation, client.clone()),
                crate::overview::subscription,
            ));
            subs.push(event::listen_with(crate::overview::mouse_event));
        }
        if self.settings.window_id.is_some()
            || self.launcher.window_id.is_some()
            || self.control_center.window_id.is_some()
            || self.modal_window_id.is_some()
            || self.shell.session_window.is_some()
            || self.shell.oxipaste_window.is_some()
            || self.popup_open
            || self.shell.bars.values().any(|bar| bar.config.auto_hide)
        {
            subs.push(event::listen_with(crate::surfaces::overlay_mouse_event));
        }
        subs.push(Subscription::run(crate::compositor::watch));
        if self.desktop.draft.is_some() {
            subs.push(event::listen_with(crate::desktop::event));
        }
        subs.push(Subscription::run_with(
            self.config.endpoint.clone(),
            ipc::listen_endpoint,
        ));
        if !self.config.directory.as_os_str().is_empty() {
            subs.push(Subscription::run_with(
                self.config.watch_paths.clone(),
                config::watch_changes,
            ));
        }
        if let Some(controller) = &self.settings.monitors.controller {
            subs.push(Subscription::run_with(
                controller.clone(),
                crate::monitor::runtime::events,
            ));
        }
        subs.extend(
            self.plugins
                .iter()
                .filter(|(name, (_, funcs))| self.subscription_enabled(name, funcs))
                .map(|(plugin_id, (_model, funcs))| {
                    let sub_fn_addr = funcs.subscription as usize;
                    Subscription::run_with((plugin_id.clone(), sub_fn_addr), build_plugin_stream)
                        .with(plugin_id.clone())
                        .map(|(id, msg): (String, PluginMsg)| map_plugin_message(id, msg))
                }),
        );
        Subscription::batch(subs)
    }

    fn style(&self, _: &Theme) -> Style {
        layer_theme()
    }

    fn scale_factor(&self, _: IcedId) -> f32 {
        SCALE_FACTOR
    }

    pub(crate) fn popup_metrics(&self, plugin_id: &str) -> PopupMetrics {
        let menu_size = match plugin_id {
            "@control-center" => Some(self.config.snapshot.host.control_center_size),
            "@session" => Some((400, 540)),
            _ if plugin_id.eq_ignore_ascii_case("Notifications")
                && self.config.snapshot.host.menus.notifications_dropdown =>
            {
                Some(self.config.snapshot.host.menus.notification_size)
            }
            _ => None,
        };
        if let Some((width, height)) = menu_size {
            if let Some(bar) = self.shell.popup_bar.and_then(|id| self.shell.bar(id)) {
                return PopupMetrics::new((
                    width.min(
                        bar.size
                            .width
                            .saturating_sub(layout::POPUP_CONNECTOR_PADDING)
                            .max(1),
                    ),
                    height.min(bar.config.popup_height),
                ));
            }
            return PopupMetrics::new((width, height));
        }
        let dynamic = self.plugin_popup_metrics(plugin_id);
        let (width, height) = self.popup_size(plugin_id, dynamic);
        PopupMetrics::new(self.bounded_popup_size(width, height))
    }

    pub(crate) fn popup_input_metrics(&self, plugin_id: &str) -> PopupMetrics {
        if plugin_id.starts_with('@')
            || (plugin_id.eq_ignore_ascii_case("Notifications")
                && self.config.snapshot.host.menus.notifications_dropdown)
        {
            return self.popup_metrics(plugin_id);
        }
        let dynamic = self.plugin_popup_metrics(plugin_id);
        let visible = self.popup_metrics(plugin_id);
        let (width, height) = dynamic
            .and_then(|metrics| metrics.popup_input_size)
            .or_else(|| {
                self.plugins
                    .get(plugin_id)
                    .and_then(|(_, funcs)| funcs.metadata.popup_input_size)
            })
            .unwrap_or((visible.body_width, visible.height));
        PopupMetrics::new(
            self.bounded_popup_size(width.max(visible.body_width), height.max(visible.height)),
        )
    }

    fn bounded_popup_size(&self, width: u32, height: u32) -> (u32, u32) {
        self.shell
            .popup_bar
            .and_then(|id| self.shell.bar(id))
            .map_or((width, height), |bar| {
                (
                    width.min(
                        bar.size
                            .width
                            .saturating_sub(layout::POPUP_CONNECTOR_PADDING)
                            .max(1),
                    ),
                    height.min(bar.config.popup_height),
                )
            })
    }
}

fn build_plugin_stream(data: &(String, usize)) -> Pin<Box<PluginStream>> {
    let sub_fn: SubscriptionFn = unsafe { std::mem::transmute(data.1) };
    let raw = unsafe { sub_fn() };
    unsafe { Pin::new_unchecked(Box::from_raw(raw)) }
}

impl OxiBar {
    pub(crate) fn subscription_enabled(
        &self,
        name: &str,
        funcs: &crate::plugins::PluginFuncs,
    ) -> bool {
        if funcs.subscription_policy.is_none_or(
            |policy| unsafe { policy() } == oxibar_plugin_api::SubscriptionPolicy::Always,
        ) {
            return true;
        }
        let matches = |provider: &str| provider.eq_ignore_ascii_case(name);
        self.shell.bars.values().any(|bar| {
            bar.config
                .start
                .iter()
                .chain(&bar.config.center)
                .chain(&bar.config.end)
                .any(|provider| matches(provider))
        }) || self
            .osds
            .instances
            .values()
            .any(|instance| instance.plugin == name)
            || self.toasts.iter().any(|toast| toast.plugin_id == name)
            || (funcs.dock_entries.is_some() && !self.docks.instances.is_empty())
            || self
                .desktop
                .instances
                .values()
                .any(|widget| matches(&widget.spec.plugin))
            || (!self.desktop.editors.is_empty()
                && self.desktop.draft.as_ref().is_some_and(|draft| {
                    draft
                        .widgets
                        .iter()
                        .any(|widget| widget.enabled && matches(&widget.plugin))
                }))
            || (self.popup_open && self.popup_plugin.as_deref().is_some_and(matches))
            || (self.shell.oxipaste_window.is_some() && matches("OxiPaste"))
            || self.modal_plugin.as_deref().is_some_and(matches)
            || self.panel_plugin.as_deref().is_some_and(matches)
            || (self.control_center.window_id.is_some()
                && self.control_center.selected.as_deref().is_some_and(matches))
    }
}

fn layer_surface_size_event(
    event: iced::Event,
    _status: event::Status,
    id: IcedId,
) -> Option<Message> {
    let size = match event {
        iced::Event::Window(window::Event::Closed) => return Some(Message::SurfaceClosed(id)),
        iced::Event::Window(window::Event::Focused) => {
            return Some(Message::SurfaceFocus(id, true));
        }
        iced::Event::Window(window::Event::Unfocused) => {
            return Some(Message::SurfaceFocus(id, false));
        }
        iced::Event::Window(window::Event::Opened { size, .. })
        | iced::Event::Window(window::Event::Resized(size)) => size,
        _ => return None,
    };
    Some(Message::LayerSurfaceResized(
        id,
        size_to_u32(size.width),
        size_to_u32(size.height),
    ))
}

fn size_to_u32(value: f32) -> u32 {
    if value.is_finite() && value > 0.0 {
        value.round().min(i32::MAX as f32) as u32
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_bar() -> OxiBar {
        let mut bar = OxiBar {
            paste: crate::paste::PasteState::default(),
            overview: crate::overview::Overview::default(),
            appearance: crate::theme::ThemeRuntime::new(&Table::new(), 0, false),
            docks: crate::dock::Docks::default(),
            desktop: crate::desktop::Desktop::default(),
            compositor: None,
            compositor_error: None,
            compositor_action_error: None,
            osds: crate::osd::Osds::untimed(),
            shell: Shell::new(Err("Test".to_owned())),
            config: ConfigRuntime::new(ConfigStore::memory(Table::new(), Table::new())),
            settings: SettingsUi::new(&ConfigStore::memory(Table::new(), Table::new()).snapshot()),
            control_center: ControlCenter::default(),
            plugin_order: Vec::new(),
            plugin_failures: std::collections::BTreeMap::new(),
            plugin_runtime: crate::plugins::PluginRuntime::default(),
            launcher: Launcher::new(&Table::new()),
            theme: Theme::Dark,
            plugins: PluginMap::new(),
            transparent: false,
            bar_window_id: None,
            bar_size: layout::DEFAULT_BAR_SIZE,
            start_widgets: vec!["Clock".to_owned()],
            center_widgets: vec!["Network".to_owned()],
            end_widgets: vec!["Notifications".to_owned()],
            popup_plugin: None,
            popup_open: false,
            modal_plugin: None,
            modal_window_id: None,
            panel_plugin: None,
            panel_window_id: None,
            toasts: Vec::new(),
        };
        let _ = bar.reconcile_bars();
        bar
    }

    #[test]
    fn plugin_section_matches_config_case_insensitively() {
        let bar = empty_bar();
        assert!(matches!(bar.plugin_section("clock"), BarSection::Start));
        assert!(matches!(bar.plugin_section("NETWORK"), BarSection::Center));
        assert!(matches!(
            bar.plugin_section("Notifications"),
            BarSection::End
        ));
        assert!(matches!(bar.plugin_section("unknown"), BarSection::End));
    }

    #[test]
    fn update_setters_mutate_popup_state() {
        let mut bar = empty_bar();

        let _ = bar.update(Message::SetPopupPlugin(Some("Clock".to_owned())));
        let _ = bar.update(Message::SetPopupOpen(true));

        assert_eq!(bar.popup_plugin.as_deref(), Some("Clock"));
        assert!(bar.popup_open);
    }

    #[test]
    fn layer_resize_updates_bar_dimensions() {
        let mut bar = empty_bar();
        let main_window = bar.bar_window_id.unwrap();

        let _ = bar.update(Message::LayerSurfaceResized(main_window, 2560, 451));

        assert_eq!(
            bar.bar_size,
            BarDimensions {
                width: 2560,
                height: 31,
            }
        );

        let _ = bar.update(Message::LayerSurfaceResized(IcedId::unique(), 420, 144));

        assert_eq!(bar.bar_size.width, 2560);
    }

    #[test]
    fn auxiliary_resize_does_not_claim_bar_window() {
        let mut bar = empty_bar();
        let modal_window = IcedId::unique();
        let main_window = bar.bar_window_id.unwrap();
        bar.modal_window_id = Some(modal_window);

        let _ = bar.update(Message::LayerSurfaceResized(modal_window, 460, 300));
        let _ = bar.update(Message::LayerSurfaceResized(main_window, 1920, 451));

        assert_eq!(bar.bar_window_id, Some(main_window));
        assert_eq!(bar.bar_size.width, 1920);
    }

    #[test]
    fn compositor_readiness_requires_layer_shell() {
        let globals = [Global {
            name: 1,
            interface: "wl_compositor".to_owned(),
            version: 6,
        }];
        assert!(!has_required_layer_shell_globals(&globals));

        let globals = [
            globals[0].clone(),
            Global {
                name: 2,
                interface: "zwlr_layer_shell_v1".to_owned(),
                version: 5,
            },
        ];
        assert!(has_required_layer_shell_globals(&globals));
    }

    #[test]
    fn compositor_readiness_does_not_require_an_output() {
        // The bar binds no output when targeting the active monitor, so a
        // compositor advertising no wl_output yet must still count as ready.
        // Requiring it here caused an infinite wait on otherwise fine setups.
        let globals = [
            Global {
                name: 1,
                interface: "wl_compositor".to_owned(),
                version: 6,
            },
            Global {
                name: 2,
                interface: "zwlr_layer_shell_v1".to_owned(),
                version: 5,
            },
        ];
        assert!(has_required_layer_shell_globals(&globals));
    }

    #[test]
    fn input_region_refreshes_on_first_surface_ready() {
        // Regression test: the boot-time region task can be dropped while the
        // surface does not exist yet, so the first size event must re-apply
        // even when the size matches the default exactly.
        let size = layout::DEFAULT_BAR_SIZE;
        assert!(should_refresh_input_region(true, size, size));
        assert!(should_refresh_input_region(
            false,
            size,
            BarDimensions {
                width: size.width + 1,
                ..size
            }
        ));
        assert!(!should_refresh_input_region(false, size, size));
    }

    #[test]
    fn wayland_socket_discovery_lists_sorted_wayland_sockets() {
        let dir = std::env::temp_dir().join(format!(
            "oxibar-wayland-discovery-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        for name in ["wayland-1", "wayland-0", "oxibar.lock", "hypr"] {
            std::fs::write(dir.join(name), []).expect("create temp entry");
        }

        let candidates = wayland_socket_candidates_in(&dir);

        assert_eq!(
            candidates,
            vec![dir.join("wayland-0"), dir.join("wayland-1")]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wayland_socket_discovery_handles_missing_dir() {
        let dir = std::env::temp_dir().join(format!(
            "oxibar-wayland-discovery-missing-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(wayland_socket_candidates_in(&dir).is_empty());
        assert!(discover_wayland_connection(&dir).is_none());
    }

    #[test]
    fn connection_from_non_socket_path_returns_none() {
        let dir = std::env::temp_dir().join(format!(
            "oxibar-wayland-non-socket-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join("wayland-9");
        std::fs::write(&path, []).expect("create regular file");

        assert!(connection_from_socket_path(&path).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
