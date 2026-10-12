use super::{
    Output, OutputMode, OutputSnapshot, configuration::DisplayConfig, controller::Backend,
    output::logical_size,
};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum,
    backend::ObjectId,
    protocol::{wl_callback, wl_output, wl_registry},
};
use wayland_protocols_wlr::output_management::v1::client::{
    zwlr_output_configuration_head_v1 as configuration_head,
    zwlr_output_configuration_v1 as configuration, zwlr_output_head_v1 as head,
    zwlr_output_manager_v1 as manager, zwlr_output_mode_v1 as mode,
};

#[derive(Default)]
struct Head {
    proxy: Option<head::ZwlrOutputHeadV1>,
    name: String,
    description: String,
    make: String,
    model: String,
    serial: String,
    enabled: bool,
    current: Option<ObjectId>,
    position: (i32, i32),
    scale: f64,
    transform: u32,
    vrr: Option<bool>,
    modes: Vec<mode::ZwlrOutputModeV1>,
}
#[derive(Default)]
struct Mode {
    width: u32,
    height: u32,
    hz: f64,
}
#[derive(Default)]
struct State {
    manager: Option<manager::ZwlrOutputManagerV1>,
    serial: Option<u32>,
    heads: HashMap<ObjectId, Head>,
    modes: HashMap<ObjectId, Mode>,
    synchronized: u64,
    finished: bool,
    pending: Option<ObjectId>,
    result: Option<Result<(), String>>,
}
pub(super) struct Control {
    connection: Connection,
    queue: EventQueue<State>,
    state: State,
}

impl Control {
    pub fn new() -> Result<Self, String> {
        let connection =
            crate::app::connect_wayland().ok_or("Unable to connect to the Wayland compositor")?;
        let queue = connection.new_event_queue();
        let qh = queue.handle();
        connection.display().get_registry(&qh, ());
        let mut control = Self {
            connection,
            queue,
            state: State::default(),
        };
        control.synchronize()?;
        control.synchronize()?;
        if control.state.manager.is_none() {
            return Err("This compositor does not advertise wlr-output-management; monitor controls are unavailable".into());
        }
        Ok(control)
    }
    fn synchronize(&mut self) -> Result<(), String> {
        let before = self.state.synchronized;
        self.connection.display().sync(&self.queue.handle(), ());
        self.wait(|state| state.synchronized > before)
    }
    fn wait(&mut self, done: impl Fn(&State) -> bool) -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            self.queue
                .dispatch_pending(&mut self.state)
                .map_err(|error| error.to_string())?;
            if self.state.finished {
                return Err("The output-management service ended".into());
            }
            if done(&self.state) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(
                    "The compositor did not answer the display configuration request".into(),
                );
            }
            self.connection.flush().map_err(|error| error.to_string())?;
            let Some(guard) = self.queue.prepare_read() else {
                continue;
            };
            let backend = self.connection.backend();
            let fd = backend.poll_fd();
            let flags = {
                let mut fds = [rustix::event::PollFd::new(
                    &fd,
                    rustix::event::PollFlags::IN,
                )];
                let timeout = rustix::event::Timespec {
                    tv_sec: 0,
                    tv_nsec: 100_000_000,
                };
                rustix::event::poll(&mut fds, Some(&timeout)).map_err(|error| error.to_string())?;
                fds[0].revents()
            };
            if flags.intersects(
                rustix::event::PollFlags::IN
                    | rustix::event::PollFlags::HUP
                    | rustix::event::PollFlags::ERR,
            ) {
                guard.read().map_err(|error| error.to_string())?;
            } else {
                drop(guard);
            }
        }
    }
    fn configure(&mut self, outputs: &[DisplayConfig], test: bool) -> Result<(), String> {
        self.synchronize()?;
        let manager = self
            .state
            .manager
            .as_ref()
            .ok_or("Output management unavailable")?;
        let serial = self
            .state
            .serial
            .ok_or("The compositor has not published a complete display inventory")?;
        let qh = self.queue.handle();
        let configuration = manager.create_configuration(serial, &qh, ());
        for output in outputs {
            let data = self
                .state
                .heads
                .values()
                .find(|head| head.name == output.name)
                .ok_or("A display disappeared before configuration")?;
            let proxy = data.proxy.as_ref().ok_or("Missing output head")?;
            if !output.enabled {
                configuration.disable_head(proxy);
                continue;
            }
            let enabled = configuration.enable_head(proxy, &qh, ());
            let selected = data
                .modes
                .iter()
                .find(|proxy| {
                    self.state.modes.get(&proxy.id()).is_some_and(|mode| {
                        mode.width == output.mode.width
                            && mode.height == output.mode.height
                            && (mode.hz - output.mode.refresh_hz).abs() <= 0.02
                    })
                })
                .ok_or("The requested mode is no longer available")?;
            enabled.set_mode(selected);
            enabled.set_position(output.position.0, output.position.1);
            enabled.set_scale(output.scale);
            enabled.set_transform(match output.transform {
                0 => wl_output::Transform::Normal,
                1 => wl_output::Transform::_90,
                2 => wl_output::Transform::_180,
                3 => wl_output::Transform::_270,
                4 => wl_output::Transform::Flipped,
                5 => wl_output::Transform::Flipped90,
                6 => wl_output::Transform::Flipped180,
                7 => wl_output::Transform::Flipped270,
                _ => return Err("Invalid display transform".into()),
            });
            if enabled.version() >= 4
                && let Some(vrr) = output.vrr
            {
                enabled.set_adaptive_sync(if vrr {
                    head::AdaptiveSyncState::Enabled
                } else {
                    head::AdaptiveSyncState::Disabled
                });
            }
        }
        self.state.pending = Some(configuration.id());
        self.state.result = None;
        if test {
            configuration.test();
        } else {
            configuration.apply();
        }
        let wait = self.wait(|state| state.result.is_some());
        configuration.destroy();
        self.state.pending = None;
        wait?;
        self.state.result.take().expect("configuration result")
    }
}
impl Backend for Control {
    fn label(&self) -> &str {
        "wlr-output-management"
    }
    fn inventory(&mut self) -> Result<OutputSnapshot, String> {
        self.synchronize()?;
        let mut outputs = self
            .state
            .heads
            .values()
            .filter(|head| !head.name.is_empty())
            .map(|head| {
                let mode = head
                    .current
                    .as_ref()
                    .and_then(|id| self.state.modes.get(id));
                let size = mode
                    .map(|mode| (mode.width, mode.height))
                    .unwrap_or_default();
                let scale = if head.scale.is_finite() && head.scale > 0.0 {
                    head.scale
                } else {
                    1.0
                };
                Output {
                    name: head.name.clone(),
                    description: head.description.clone(),
                    make: head.make.clone(),
                    model: head.model.clone(),
                    serial: head.serial.clone(),
                    enabled: head.enabled,
                    focused: false,
                    physical_size: size,
                    logical_size: logical_size(size, scale, head.transform),
                    position: head.position,
                    scale,
                    transform: head.transform,
                    refresh_hz: mode.map(|mode| mode.hz).unwrap_or(0.0),
                    vrr: head.vrr,
                    modes: head
                        .modes
                        .iter()
                        .filter_map(|mode| self.state.modes.get(&mode.id()))
                        .filter(|mode| mode.width > 0 && mode.height > 0 && mode.hz >= 0.0)
                        .map(|mode| OutputMode {
                            width: mode.width,
                            height: mode.height,
                            refresh_hz: mode.hz,
                        })
                        .collect(),
                }
            })
            .collect::<Vec<_>>();
        outputs.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(OutputSnapshot {
            backend: self.label().into(),
            outputs,
        })
    }
    fn apply(&mut self, config: &[DisplayConfig]) -> Result<(), String> {
        let inventory = self.inventory()?;
        let config = super::configuration::complete(config, &inventory)?;
        // Test and apply use distinct one-shot configurations, each with the latest serial.
        self.configure(&config, true)?;
        self.configure(&config, false)
    }
}
impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
            && interface == "zwlr_output_manager_v1"
        {
            state.manager = Some(registry.bind(name, version.min(4), qh, ()));
        }
    }
}
impl Dispatch<manager::ZwlrOutputManagerV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &manager::ZwlrOutputManagerV1,
        event: manager::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            manager::Event::Head { head } => {
                state.heads.insert(
                    head.id(),
                    Head {
                        proxy: Some(head),
                        scale: 1.0,
                        ..Head::default()
                    },
                );
            }
            manager::Event::Done { serial } => state.serial = Some(serial),
            manager::Event::Finished => {
                state.finished = true;
                state.manager = None;
            }
            _ => {}
        }
    }
    wayland_client::event_created_child!(State, manager::ZwlrOutputManagerV1, [0 => (head::ZwlrOutputHeadV1, ())]);
}
impl Dispatch<head::ZwlrOutputHeadV1, ()> for State {
    fn event(
        state: &mut Self,
        proxy: &head::ZwlrOutputHeadV1,
        event: head::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if matches!(event, head::Event::Finished) {
            state.heads.remove(&proxy.id());
            if proxy.version() >= 3 {
                proxy.release();
            }
            return;
        }
        let Some(data) = state.heads.get_mut(&proxy.id()) else {
            return;
        };
        match event {
            head::Event::Name { name } => data.name = name,
            head::Event::Description { description } => data.description = description,
            head::Event::Make { make } => data.make = make,
            head::Event::Model { model } => data.model = model,
            head::Event::SerialNumber { serial_number } => data.serial = serial_number,
            head::Event::Enabled { enabled } => data.enabled = enabled != 0,
            head::Event::Position { x, y } => data.position = (x, y),
            head::Event::Scale { scale } => data.scale = scale,
            head::Event::Transform {
                transform: WEnum::Value(transform),
            } => data.transform = transform as u32,
            head::Event::AdaptiveSync {
                state: WEnum::Value(vrr),
            } => data.vrr = Some(vrr == head::AdaptiveSyncState::Enabled),
            head::Event::CurrentMode { mode } => data.current = Some(mode.id()),
            head::Event::Mode { mode } => {
                state.modes.insert(mode.id(), Mode::default());
                data.modes.push(mode);
            }
            _ => {}
        }
    }
    wayland_client::event_created_child!(State, head::ZwlrOutputHeadV1, [3 => (mode::ZwlrOutputModeV1, ())]);
}
impl Dispatch<mode::ZwlrOutputModeV1, ()> for State {
    fn event(
        state: &mut Self,
        proxy: &mode::ZwlrOutputModeV1,
        event: mode::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if matches!(event, mode::Event::Finished) {
            state.modes.remove(&proxy.id());
            for head in state.heads.values_mut() {
                head.modes.retain(|mode| mode.id() != proxy.id());
            }
            if proxy.version() >= 3 {
                proxy.release();
            }
            return;
        }
        let data = state.modes.entry(proxy.id()).or_default();
        match event {
            mode::Event::Size { width, height } => {
                data.width = u32::try_from(width).unwrap_or(0);
                data.height = u32::try_from(height).unwrap_or(0);
            }
            mode::Event::Refresh { refresh } => data.hz = f64::from(refresh) / 1000.0,
            _ => {}
        }
    }
}
impl Dispatch<configuration::ZwlrOutputConfigurationV1, ()> for State {
    fn event(
        state: &mut Self,
        proxy: &configuration::ZwlrOutputConfigurationV1,
        event: configuration::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if state.pending.as_ref() != Some(&proxy.id()) {
            return;
        }
        state.result = match event {
            configuration::Event::Succeeded => Some(Ok(())),
            configuration::Event::Failed => Some(Err(
                "The compositor rejected this display configuration".into(),
            )),
            configuration::Event::Cancelled => Some(Err(
                "Displays changed while testing this configuration; refresh and try again".into(),
            )),
            _ => None,
        };
    }
}
impl Dispatch<configuration_head::ZwlrOutputConfigurationHeadV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &configuration_head::ZwlrOutputConfigurationHeadV1,
        _: configuration_head::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
impl Dispatch<wl_callback::WlCallback, ()> for State {
    fn event(
        state: &mut Self,
        _: &wl_callback::WlCallback,
        _: wl_callback::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        state.synchronized += 1;
    }
}
