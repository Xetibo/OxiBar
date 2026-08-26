use std::{
    panic::{self, AssertUnwindSafe},
    pin::Pin,
    time::Duration,
};

use iced::{Font, Subscription, Task, Theme, event, theme::Style, window};
use iced_layershell::reexport::{IcedId, KeyboardInteractivity};
use once_cell::sync::Lazy;
use oxibar_plugin_api::{
    HostToastRequest, PluginMsg, PluginPopupMetrics, PluginStream, SubscriptionFn,
};
use oxiced::{theme::theme_impl::get_derived_iced_theme, widgets::oxi_layer::layer_theme};
use toml::Table;
use tracing::error;
use wayland_client::{
    Connection, Dispatch, QueueHandle,
    globals::{Global, GlobalListContents, registry_queue_init},
    protocol::wl_registry,
};

use crate::{
    config::{self, get_config},
    font,
    layout::{
        self, BarDimensions, BarSection, DEFAULT_POPUP_SIZE, PopupMetrics, SCALE_FACTOR,
        TOAST_MARGIN_TOP, TOAST_SPACING,
    },
    messages::{Message, map_plugin_message},
    plugins::{PluginMap, dispatch_update, drain_errors, load_plugins, query_plugin_popup_metrics},
    single_instance::{self, SingleInstanceError},
};

pub(crate) static CONFIG: Lazy<Table> = Lazy::new(get_config);

pub fn run() -> Result<(), iced_layershell::Error> {
    init_tracing();

    let _instance_guard = match single_instance::acquire(&config::instance_policy(&CONFIG)) {
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

    let policy = config::startup_retry_policy(&CONFIG);
    let mut attempts: u32 = 0;
    let mut backoff: u64 = policy.initial_delay_ms;

    loop {
        let is_last = !policy.can_attempt_more(attempts);
        let ready = !policy.enabled || compositor_ready();

        if ready || is_last {
            let outcome = panic::catch_unwind(AssertUnwindSafe(run_bar));

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

fn compositor_ready() -> bool {
    let Ok(connection) = Connection::connect_to_env() else {
        return false;
    };
    let Ok((globals, _event_queue)) = registry_queue_init::<CompositorReadiness>(&connection)
    else {
        return false;
    };

    globals
        .contents()
        .with_list(has_required_layer_shell_globals)
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
    ["wl_compositor", "wl_output", "zwlr_layer_shell_v1"]
        .into_iter()
        .all(|required| globals.iter().any(|global| global.interface == required))
}

fn run_bar() -> Result<(), iced_layershell::Error> {
    let mut app =
        iced_layershell::daemon(OxiBar::new, OxiBar::namespace, OxiBar::update, OxiBar::view)
            .subscription(OxiBar::subscription)
            .settings(layout::layer_shell_settings(&CONFIG))
            .theme(OxiBar::theme)
            .default_font(Font::with_name(font::bar_font(&CONFIG)))
            .style(OxiBar::style)
            .scale_factor(OxiBar::scale_factor);
    if let Some(bytes) = font::bar_font_bytes(&CONFIG) {
        app = app.font(bytes);
    }
    app.run()
}

pub(crate) struct OxiBar {
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

fn initial_input_region_task(bar_size: BarDimensions) -> Task<Message> {
    Task::perform(
        async {
            std::thread::sleep(std::time::Duration::from_millis(50));
        },
        move |_| Message::SetPopupInputRegion {
            open: false,
            section: BarSection::End,
            width: 0,
            height: 0,
            bar_size,
        },
    )
}

impl OxiBar {
    fn new() -> (Self, Task<Message>) {
        let loaded = load_plugins(&CONFIG);
        let bar_table = CONFIG.get("bar").and_then(|v| v.as_table());
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
        let bar_size = layout::bar_size_from_config(&CONFIG);
        let start_widgets =
            if start_widgets.is_empty() && center_widgets.is_empty() && end_widgets.is_empty() {
                loaded.order.clone()
            } else {
                start_widgets
            };
        let bar = Self {
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
        let mut tasks = loaded.tasks;
        tasks.push(initial_input_region_task(bar.bar_size));
        (bar, Task::batch(tasks))
    }

    fn namespace() -> String {
        String::from("OxiBar")
    }

    pub(crate) fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Exit => std::process::exit(0),
            Message::SetPopupInputRegion { .. }
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
                    error!("message for unknown plugin `{plugin_id}`");
                    return Task::none();
                };
                let task = dispatch_update(funcs, model.clone(), msg);
                drain_errors(funcs, model);
                let refresh = if self.popup_open
                    && self.popup_plugin.as_deref() == Some(plugin_id.as_str())
                {
                    Task::done(Message::RefreshPopupInputRegion(plugin_id.clone()))
                } else {
                    Task::none()
                };
                match task {
                    Some(task) => {
                        let id = plugin_id.clone();
                        refresh.chain(task.map(move |msg| map_plugin_message(id.clone(), msg)))
                    }
                    None => refresh,
                }
            }
        }
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

        let section = self.plugin_section(plugin_id);
        let metrics = self.popup_input_metrics(plugin_id);
        Task::done(Message::SetPopupInputRegion {
            open: true,
            section,
            width: metrics.connector_width,
            height: metrics.height,
            bar_size: self.bar_size,
        })
    }

    fn update_bar_size(
        &mut self,
        id: IcedId,
        layer_width: u32,
        layer_height: u32,
    ) -> Task<Message> {
        if self.is_auxiliary_window(id) {
            return Task::none();
        }

        if let Some(bar_window_id) = self.bar_window_id {
            if bar_window_id != id {
                return Task::none();
            }
        } else {
            self.bar_window_id = Some(id);
        }

        let previous = self.bar_size;
        self.bar_size = layout::bar_size_from_layer_surface(previous, layer_width, layer_height);
        if self.bar_size == previous {
            return Task::none();
        }
        self.current_input_region_task()
    }

    fn is_auxiliary_window(&self, id: IcedId) -> bool {
        self.modal_window_id == Some(id)
            || self.panel_window_id == Some(id)
            || self.toasts.iter().any(|toast| toast.window_id == id)
    }

    fn current_input_region_task(&self) -> Task<Message> {
        if let Some(plugin_id) = self.popup_plugin.as_deref()
            && self.popup_open
        {
            return self.refresh_popup_input_region(plugin_id);
        }

        Task::done(Message::SetPopupInputRegion {
            open: false,
            section: BarSection::End,
            width: 0,
            height: 0,
            bar_size: self.bar_size,
        })
    }

    fn plugin_popup_metrics(&self, plugin_id: &str) -> Option<PluginPopupMetrics> {
        let (model, funcs) = self.plugins.get(plugin_id)?;
        query_plugin_popup_metrics(funcs, model)
    }

    fn popup_size(&self, plugin_id: &str, dynamic: Option<PluginPopupMetrics>) -> (u32, u32) {
        layout::popup_size_from_config(&CONFIG, plugin_id)
            .or_else(|| dynamic.and_then(|metrics| metrics.popup_size))
            .or_else(|| {
                self.plugins
                    .get(plugin_id)
                    .and_then(|(_, funcs)| funcs.metadata.popup_size)
            })
            .unwrap_or(DEFAULT_POPUP_SIZE)
    }

    fn toggle_plugin_popup(&mut self, plugin_id: String) -> Task<Message> {
        let Some((_model, funcs)) = self.plugins.get(&plugin_id) else {
            tracing::warn!(plugin = plugin_id, "unknown plugin requested popup");
            return Task::none();
        };
        if funcs.popup_view.is_none() {
            tracing::warn!(
                plugin = plugin_id,
                "plugin requested popup but has no popup_view"
            );
            return Task::none();
        }

        let section = self.plugin_section(&plugin_id);
        if self.popup_open && self.popup_plugin.as_deref() == Some(plugin_id.as_str()) {
            Task::done(Message::SetPopupOpen(false)).chain(Task::done(
                Message::SetPopupInputRegion {
                    open: false,
                    section,
                    width: 0,
                    height: 0,
                    bar_size: self.bar_size,
                },
            ))
        } else {
            let metrics = self.popup_input_metrics(&plugin_id);
            Task::done(Message::SetPopupPlugin(Some(plugin_id)))
                .chain(Task::done(Message::SetPopupInputRegion {
                    open: true,
                    section,
                    width: metrics.connector_width,
                    height: metrics.height,
                    bar_size: self.bar_size,
                }))
                .chain(Task::done(Message::SetPopupOpen(true)))
        }
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
        let close_existing = self.modal_window_id.take();
        self.modal_plugin = Some(plugin_id);
        self.modal_window_id = Some(id);

        if let Some(existing) = close_existing {
            Task::done(Message::CloseModalLayer(existing))
                .chain(Task::done(Message::OpenModalLayer(id)))
        } else {
            Task::done(Message::OpenModalLayer(id))
        }
    }

    fn toggle_plugin_panel(&mut self, plugin_id: String) -> Task<Message> {
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
        self.panel_plugin = Some(plugin_id);
        self.panel_window_id = Some(id);
        let close_toasts = self.close_all_toasts();

        if let Some(existing) = close_existing {
            close_toasts
                .chain(Task::done(Message::ClosePanelLayer(existing)))
                .chain(Task::done(Message::OpenPanelLayer(id)))
        } else {
            close_toasts.chain(Task::done(Message::OpenPanelLayer(id)))
        }
    }

    fn close_panel(&mut self) -> Task<Message> {
        self.panel_plugin = None;
        let Some(id) = self.panel_window_id.take() else {
            return Task::none();
        };
        Task::done(Message::ClosePanelLayer(id))
    }

    fn close_plugin_modal(&mut self, plugin_id: String) -> Task<Message> {
        if self.modal_plugin.as_deref() != Some(plugin_id.as_str()) {
            return Task::none();
        }
        self.close_modal()
    }

    fn close_modal(&mut self) -> Task<Message> {
        self.modal_plugin = None;
        let Some(id) = self.modal_window_id.take() else {
            return Task::none();
        };
        Task::done(Message::CloseModalLayer(id))
    }

    fn show_plugin_toast(&mut self, plugin_id: String, request: HostToastRequest) -> Task<Message> {
        if self.panel_plugin.is_some() {
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
        self.toasts.push(ToastEntry {
            plugin_id,
            toast_id: request.toast_id,
            window_id,
            width: request.width,
            height: request.height,
        });
        Task::done(Message::OpenToastLayer(
            window_id,
            request.width,
            request.height,
            TOAST_MARGIN_TOP,
        ))
        .chain(Task::batch(self.toast_layout_tasks()))
    }

    fn close_plugin_toast(&mut self, plugin_id: &str, toast_id: &str) -> Task<Message> {
        let Some(index) = self
            .toasts
            .iter()
            .position(|toast| toast.plugin_id == plugin_id && toast.toast_id == toast_id)
        else {
            return Task::none();
        };
        let toast = self.toasts.remove(index);
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

    fn close_all_toasts(&mut self) -> Task<Message> {
        let tasks = self
            .toasts
            .drain(..)
            .map(|toast| Task::done(Message::CloseToastLayer(toast.window_id)))
            .collect::<Vec<_>>();
        Task::batch(tasks)
    }

    fn toast_layout_tasks(&self) -> Vec<Task<Message>> {
        let mut top = TOAST_MARGIN_TOP;
        let mut tasks = Vec::with_capacity(self.toasts.len());

        for toast in self.toasts.iter().rev() {
            tasks.push(Task::done(Message::MoveToastLayer(toast.window_id, top)));
            top += toast.height as i32 + TOAST_SPACING;
        }

        tasks
    }

    fn theme(&self, _: IcedId) -> Theme {
        self.theme.clone()
    }

    fn subscription(&self) -> Subscription<Message> {
        let mut subs: Vec<Subscription<Message>> = Vec::with_capacity(self.plugins.len() + 1);
        subs.push(event::listen_with(layer_surface_size_event));
        subs.extend(self.plugins.iter().map(|(plugin_id, (_model, funcs))| {
            let sub_fn_addr = funcs.subscription as usize;
            Subscription::run_with((plugin_id.clone(), sub_fn_addr), build_plugin_stream)
                .with(plugin_id.clone())
                .map(|(id, msg): (String, PluginMsg)| map_plugin_message(id, msg))
        }));
        Subscription::batch(subs)
    }

    fn style(&self, _: &Theme) -> Style {
        layer_theme()
    }

    fn scale_factor(&self, _: IcedId) -> f32 {
        SCALE_FACTOR
    }

    pub(crate) fn popup_metrics(&self, plugin_id: &str) -> PopupMetrics {
        let dynamic = self.plugin_popup_metrics(plugin_id);
        PopupMetrics::new(self.popup_size(plugin_id, dynamic))
    }

    pub(crate) fn popup_input_metrics(&self, plugin_id: &str) -> PopupMetrics {
        let dynamic = self.plugin_popup_metrics(plugin_id);
        let visible_size = self.popup_size(plugin_id, dynamic);
        let visible = PopupMetrics::new(visible_size);
        let (width, height) = dynamic
            .and_then(|metrics| metrics.popup_input_size)
            .or_else(|| {
                self.plugins
                    .get(plugin_id)
                    .and_then(|(_, funcs)| funcs.metadata.popup_input_size)
            })
            .unwrap_or((visible.body_width, visible.height));
        PopupMetrics::new((width.max(visible.body_width), height.max(visible.height)))
    }
}

fn build_plugin_stream(data: &(String, usize)) -> Pin<Box<PluginStream>> {
    let sub_fn: SubscriptionFn = unsafe { std::mem::transmute(data.1) };
    let raw = unsafe { sub_fn() };
    unsafe { Pin::new_unchecked(Box::from_raw(raw)) }
}

fn layer_surface_size_event(
    event: iced::Event,
    _status: event::Status,
    id: IcedId,
) -> Option<Message> {
    let size = match event {
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
        OxiBar {
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
        }
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
        let main_window = IcedId::unique();

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
        let main_window = IcedId::unique();
        bar.modal_window_id = Some(modal_window);

        let _ = bar.update(Message::LayerSurfaceResized(modal_window, 460, 300));
        let _ = bar.update(Message::LayerSurfaceResized(main_window, 1920, 451));

        assert_eq!(bar.bar_window_id, Some(main_window));
        assert_eq!(bar.bar_size.width, 1920);
    }

    #[test]
    fn compositor_readiness_requires_layer_shell_and_an_output() {
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
        assert!(!has_required_layer_shell_globals(&globals));

        let globals = [
            globals[0].clone(),
            globals[1].clone(),
            Global {
                name: 3,
                interface: "wl_output".to_owned(),
                version: 4,
            },
        ];
        assert!(has_required_layer_shell_globals(&globals));
    }
}
