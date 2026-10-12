use std::{
    collections::{BTreeMap, VecDeque},
    fs::File,
    io::Write,
    time::{Duration, Instant},
};

use iced::{Color, Element, Font, Pixels, Size, Theme, mouse};
use iced_runtime::{
    core::{clipboard, renderer},
    user_interface::{self, UserInterface},
};
use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle,
    protocol::{wl_buffer, wl_compositor, wl_output, wl_registry, wl_shm, wl_surface},
};
use wayland_protocols::{
    ext::session_lock::v1::client::{
        ext_session_lock_manager_v1, ext_session_lock_surface_v1, ext_session_lock_v1,
    },
    wp::{
        fractional_scale::v1::client::{wp_fractional_scale_manager_v1, wp_fractional_scale_v1},
        viewporter::client::{wp_viewport, wp_viewporter},
    },
};

use crate::{
    buffer::Buffer,
    input::Seat,
    lifecycle::{Finish, Lifecycle},
};

pub type Error = Box<dyn std::error::Error + Send + Sync>;

#[derive(Clone, Debug)]
pub struct Output {
    pub id: u32,
    pub name: String,
    pub size: (u32, u32),
    pub scale: f64,
}

pub enum Event<M> {
    Locked,
    OutputAdded(u32),
    OutputRemoved(u32),
    Input { output: u32, event: iced::Event },
    Message(M),
    Tick(Instant),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Redraw,
    AuthenticatedUnlock,
}

pub trait Application {
    type Message: Clone + 'static;
    fn update(&mut self, event: Event<Self::Message>) -> Action;
    fn view(&self, output: &Output) -> Element<'_, Self::Message>;
    fn theme(&self) -> Theme {
        Theme::Dark
    }
    fn background(&self) -> Color {
        Color::BLACK
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    Authenticated,
    Denied,
    ExternallyUnlocked,
}

pub struct Locker {
    connection: Connection,
    queue: EventQueue<State>,
    state: State,
}

impl Locker {
    pub fn connect() -> Result<Self, Error> {
        Self::from_connection(Connection::connect_to_env()?)
    }

    pub fn from_connection(connection: Connection) -> Result<Self, Error> {
        let mut queue = connection.new_event_queue();
        connection.display().get_registry(&queue.handle(), ());
        let mut state = State::default();
        queue.roundtrip(&mut state)?;
        if let Some(error) = state.error.take() {
            return Err(std::io::Error::other(error).into());
        }
        if state.manager.is_none() {
            return Err(
                std::io::Error::other("Compositor does not support ext-session-lock-v1").into(),
            );
        }
        if state.compositor.is_none() || state.shm.is_none() {
            return Err(std::io::Error::other("Compositor lacks lock rendering globals").into());
        }
        Ok(Self {
            connection,
            queue,
            state,
        })
    }

    pub fn run<A: Application>(
        mut self,
        mut app: A,
        mut readiness: Option<File>,
    ) -> Result<Exit, Error> {
        let qh = self.queue.handle();
        self.state.lock = self
            .state
            .manager
            .as_ref()
            .map(|manager| manager.lock(&qh, ()));
        let outputs: Vec<_> = self.state.outputs.keys().copied().collect();
        for id in outputs {
            self.state.add_surface(id, &qh);
        }
        let mut last_tick = Instant::now();
        loop {
            self.queue.dispatch_pending(&mut self.state)?;
            if let Some(error) = self.state.error.take() {
                return Err(std::io::Error::other(error).into());
            }
            let mut action = Action::None;
            while let Some(event) = self.state.events.pop_front() {
                let event = match event {
                    Pending::Locked => {
                        if !self.state.lifecycle.ready() {
                            continue;
                        }
                        if let Some(mut file) = readiness.take() {
                            file.write_all(b"locked\n")?;
                        }
                        Event::Locked
                    }
                    Pending::Added(id) => Event::OutputAdded(id),
                    Pending::Removed(id) => Event::OutputRemoved(id),
                    Pending::Input(output, event) => Event::Input { output, event },
                };
                combine(&mut action, app.update(event));
            }
            let now = Instant::now();
            if now.duration_since(last_tick) >= Duration::from_millis(30) {
                last_tick = now;
                combine(&mut action, app.update(Event::Tick(now)));
            }
            if let Some(exit) = self.state.finished.take() {
                self.queue.roundtrip(&mut self.state)?;
                return Ok(exit);
            }
            if action == Action::AuthenticatedUnlock && self.state.lifecycle.authenticated_unlock()
            {
                if let Some(lock) = self.state.lock.take() {
                    lock.unlock_and_destroy();
                }
                // A final display sync ensures unlock reaches the compositor before process exit.
                self.queue.roundtrip(&mut self.state)?;
                return Ok(Exit::Authenticated);
            }
            if action == Action::Redraw {
                self.state
                    .outputs
                    .values_mut()
                    .for_each(|out| out.dirty = true);
            }
            for slot in self.state.outputs.values_mut() {
                if slot.pending.is_some() || slot.dirty || !slot.input.is_empty() {
                    let messages =
                        slot.render(&app, self.state.shm.as_ref().ok_or("Missing shm")?, &qh)?;
                    for message in messages {
                        combine(&mut action, app.update(Event::Message(message)));
                    }
                }
            }
            if action != Action::None {
                if action == Action::AuthenticatedUnlock
                    && self.state.lifecycle.authenticated_unlock()
                {
                    if let Some(lock) = self.state.lock.take() {
                        lock.unlock_and_destroy();
                    }
                    self.queue.roundtrip(&mut self.state)?;
                    return Ok(Exit::Authenticated);
                }
                self.state
                    .outputs
                    .values_mut()
                    .for_each(|out| out.dirty = true);
            }
            self.connection.flush()?;
            if let Some(guard) = self.queue.prepare_read() {
                let fd = guard.connection_fd();
                let mut fds = [rustix::event::PollFd::new(
                    &fd,
                    rustix::event::PollFlags::IN,
                )];
                let timeout = rustix::event::Timespec {
                    tv_sec: 0,
                    tv_nsec: 10_000_000,
                };
                match rustix::event::poll(&mut fds, Some(&timeout)) {
                    Ok(_) if !fds[0].revents().is_empty() => {
                        guard.read()?;
                    }
                    Ok(_) => drop(guard),
                    Err(rustix::io::Errno::INTR) => drop(guard),
                    Err(error) => return Err(error.into()),
                }
            }
        }
    }
}

fn combine(current: &mut Action, next: Action) {
    if next == Action::AuthenticatedUnlock || (*current == Action::None && next == Action::Redraw) {
        *current = next;
    }
}

enum Pending {
    Locked,
    Added(u32),
    Removed(u32),
    Input(u32, iced::Event),
}

#[derive(Default)]
pub(crate) struct State {
    compositor: Option<wl_compositor::WlCompositor>,
    shm: Option<wl_shm::WlShm>,
    manager: Option<ext_session_lock_manager_v1::ExtSessionLockManagerV1>,
    lock: Option<ext_session_lock_v1::ExtSessionLockV1>,
    viewporter: Option<wp_viewporter::WpViewporter>,
    fractional: Option<wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1>,
    pub(crate) outputs: BTreeMap<u32, Slot>,
    pub(crate) seats: BTreeMap<u32, Seat>,
    lifecycle: Lifecycle,
    events: VecDeque<Pending>,
    finished: Option<Exit>,
    error: Option<String>,
}

impl State {
    pub(crate) fn input_error(&mut self, message: &str) {
        self.error = Some(message.into());
    }
    fn add_surface(&mut self, id: u32, qh: &QueueHandle<Self>) {
        let (Some(compositor), Some(lock), Some(slot)) =
            (&self.compositor, &self.lock, self.outputs.get_mut(&id))
        else {
            return;
        };
        if slot.surface.is_some() {
            return;
        }
        let surface = compositor.create_surface(qh, ());
        slot.lock_surface = Some(lock.get_lock_surface(&surface, &slot.output, qh, id));
        if let Some(manager) = &self.viewporter {
            slot.viewport = Some(manager.get_viewport(&surface, qh, ()));
        }
        if slot.viewport.is_some()
            && let Some(manager) = &self.fractional
        {
            slot.fractional = Some(manager.get_fractional_scale(&surface, qh, id));
        }
        slot.surface = Some(surface);
        self.events.push_back(Pending::Added(id));
    }

    pub(crate) fn surface_id(&self, surface: &wl_surface::WlSurface) -> Option<u32> {
        self.outputs
            .iter()
            .find_map(|(id, out)| (out.surface.as_ref() == Some(surface)).then_some(*id))
    }

    pub(crate) fn input(&mut self, output: u32, event: iced::Event) {
        if let Some(out) = self.outputs.get_mut(&output) {
            if let iced::Event::Mouse(mouse::Event::CursorMoved { position }) = &event {
                out.cursor = mouse::Cursor::Available(*position);
            }
            if matches!(event, iced::Event::Mouse(mouse::Event::CursorLeft)) {
                out.cursor = mouse::Cursor::Unavailable;
            }
            out.input.push(event.clone());
            self.events.push_back(Pending::Input(output, event));
        }
    }
}

pub(crate) struct Slot {
    output: wl_output::WlOutput,
    name: String,
    integer_scale: i32,
    preferred_scale: Option<u32>,
    surface: Option<wl_surface::WlSurface>,
    lock_surface: Option<ext_session_lock_surface_v1::ExtSessionLockSurfaceV1>,
    viewport: Option<wp_viewport::WpViewport>,
    fractional: Option<wp_fractional_scale_v1::WpFractionalScaleV1>,
    pending: Option<(u32, u32, u32)>,
    size: (u32, u32),
    buffer_size: (u32, u32),
    buffers: Vec<Buffer>,
    renderer: iced::Renderer,
    cache: user_interface::Cache,
    input: Vec<iced::Event>,
    cursor: mouse::Cursor,
    dirty: bool,
}

impl Slot {
    fn new(output: wl_output::WlOutput, id: u32) -> Self {
        Self {
            output,
            name: format!("Output {id}"),
            integer_scale: 1,
            preferred_scale: None,
            surface: None,
            lock_surface: None,
            viewport: None,
            fractional: None,
            pending: None,
            size: (0, 0),
            buffer_size: (0, 0),
            buffers: Vec::new(),
            renderer: iced::Renderer::Secondary(iced_tiny_skia::Renderer::new(
                Font::DEFAULT,
                Pixels(16.0),
            )),
            cache: user_interface::Cache::default(),
            input: Vec::new(),
            cursor: mouse::Cursor::Unavailable,
            dirty: true,
        }
    }

    fn render<A: Application>(
        &mut self,
        app: &A,
        shm: &wl_shm::WlShm,
        qh: &QueueHandle<State>,
    ) -> Result<Vec<A::Message>, Error> {
        let (Some(surface), Some(lock_surface)) = (&self.surface, &self.lock_surface) else {
            return Ok(Vec::new());
        };
        if let Some((serial, width, height)) = self.pending.take() {
            if width == 0 || height == 0 {
                return Err(std::io::Error::other("Invalid lock configure dimensions").into());
            }
            self.size = (width, height);
            lock_surface.ack_configure(serial);
        }
        if self.size.0 == 0 {
            return Ok(Vec::new());
        }
        let scale = if self.viewport.is_some() {
            self.preferred_scale
                .map(|value| f64::from(value) / 120.0)
                .unwrap_or(f64::from(self.integer_scale))
        } else {
            f64::from(self.integer_scale)
        };
        let physical = (
            (f64::from(self.size.0) * scale).ceil() as u32,
            (f64::from(self.size.1) * scale).ceil() as u32,
        );
        if physical != self.buffer_size {
            self.buffers.clear();
            self.buffer_size = physical;
            for _ in 0..2 {
                self.buffers.push(Buffer::new(
                    shm,
                    qh,
                    self.output
                        .data::<u32>()
                        .copied()
                        .ok_or("Missing output identity")?,
                    physical,
                )?);
            }
        }
        let output = Output {
            id: *self.output.data::<u32>().ok_or("Missing output identity")?,
            name: self.name.clone(),
            size: self.size,
            scale,
        };
        let mut ui = UserInterface::build(
            app.view(&output),
            Size::new(self.size.0 as f32, self.size.1 as f32),
            std::mem::take(&mut self.cache),
            &mut self.renderer,
        );
        let mut messages = Vec::new();
        ui.update(
            &std::mem::take(&mut self.input),
            self.cursor,
            &mut self.renderer,
            &mut clipboard::Null,
            &mut messages,
        );
        if let Some(buffer) = self.buffers.iter_mut().find(|buffer| !buffer.busy) {
            ui.draw(
                &mut self.renderer,
                &app.theme(),
                &renderer::Style {
                    text_color: Color::WHITE,
                },
                self.cursor,
            );
            buffer.draw(&mut self.renderer, scale, app.background())?;
            if let Some(viewport) = &self.viewport {
                surface.set_buffer_scale(1);
                viewport.set_destination(self.size.0 as i32, self.size.1 as i32);
            } else {
                surface.set_buffer_scale(self.integer_scale);
            }
            surface.attach(Some(&buffer.proxy), 0, 0);
            if surface.version() >= 4 {
                surface.damage_buffer(0, 0, physical.0 as i32, physical.1 as i32);
            } else {
                surface.damage(0, 0, self.size.0 as i32, self.size.1 as i32);
            }
            surface.commit();
            buffer.busy = true;
            self.dirty = false;
        }
        self.cache = ui.into_cache();
        Ok(messages)
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        if let Some(scale) = &self.fractional {
            scale.destroy();
        }
        if let Some(viewport) = &self.viewport {
            viewport.destroy();
        }
        if let Some(lock) = &self.lock_surface {
            lock.destroy();
        }
        if let Some(surface) = &self.surface {
            surface.destroy();
        }
        if self.output.version() >= 3 {
            self.output.release();
        }
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
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => match interface.as_str() {
                "wl_compositor" => {
                    state.compositor = Some(registry.bind(name, version.min(6), qh, ()))
                }
                "wl_shm" => state.shm = Some(registry.bind(name, 1, qh, ())),
                "ext_session_lock_manager_v1" => {
                    state.manager = Some(registry.bind(name, 1, qh, ()))
                }
                "wp_viewporter" => state.viewporter = Some(registry.bind(name, 1, qh, ())),
                "wp_fractional_scale_manager_v1" => {
                    state.fractional = Some(registry.bind(name, 1, qh, ()))
                }
                "wl_output" => {
                    state.outputs.insert(
                        name,
                        Slot::new(registry.bind(name, version.min(4), qh, name), name),
                    );
                    state.add_surface(name, qh);
                }
                "wl_seat" => {
                    state.seats.insert(
                        name,
                        Seat::new(registry.bind(name, version.min(9), qh, name)),
                    );
                }
                _ => {}
            },
            wl_registry::Event::GlobalRemove { name } => {
                if state.outputs.remove(&name).is_some() {
                    self::State::input_removed(state, name);
                    state.events.push_back(Pending::Removed(name));
                }
                state.seats.remove(&name);
            }
            _ => {}
        }
    }
}

impl State {
    fn input_removed(&mut self, output: u32) {
        self.seats
            .values_mut()
            .for_each(|seat| seat.remove_output(output));
    }
}

impl Dispatch<wl_output::WlOutput, u32> for State {
    fn event(
        state: &mut Self,
        _: &wl_output::WlOutput,
        event: wl_output::Event,
        id: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let Some(out) = state.outputs.get_mut(id) {
            match event {
                wl_output::Event::Name { name } => out.name = name,
                wl_output::Event::Scale { factor } => {
                    out.integer_scale = factor.max(1);
                    out.dirty = true;
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<ext_session_lock_surface_v1::ExtSessionLockSurfaceV1, u32> for State {
    fn event(
        state: &mut Self,
        _: &ext_session_lock_surface_v1::ExtSessionLockSurfaceV1,
        event: ext_session_lock_surface_v1::Event,
        id: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_session_lock_surface_v1::Event::Configure {
            serial,
            width,
            height,
        } = event
            && let Some(out) = state.outputs.get_mut(id)
        {
            out.pending = Some((serial, width, height));
            out.dirty = true;
        }
    }
}

impl Dispatch<ext_session_lock_v1::ExtSessionLockV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ext_session_lock_v1::ExtSessionLockV1,
        event: ext_session_lock_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ext_session_lock_v1::Event::Locked => {
                if state.lifecycle.locked() {
                    state.events.push_back(Pending::Locked);
                }
            }
            ext_session_lock_v1::Event::Finished => match state.lifecycle.finished() {
                Finish::DeniedDestroy => {
                    if let Some(lock) = state.lock.take() {
                        lock.destroy();
                    }
                    state.finished = Some(Exit::Denied);
                }
                Finish::UnlockedDestroy => {
                    if let Some(lock) = state.lock.take() {
                        lock.unlock_and_destroy();
                    }
                    state.finished = Some(Exit::ExternallyUnlocked);
                }
                Finish::None => {}
            },
            _ => {}
        }
    }
}

impl Dispatch<wl_buffer::WlBuffer, u32> for State {
    fn event(
        state: &mut Self,
        proxy: &wl_buffer::WlBuffer,
        event: wl_buffer::Event,
        id: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if matches!(event, wl_buffer::Event::Release)
            && let Some(out) = state.outputs.get_mut(id)
            && let Some(buffer) = out.buffers.iter_mut().find(|buffer| buffer.proxy == *proxy)
        {
            buffer.busy = false;
        }
    }
}

impl Dispatch<wp_fractional_scale_v1::WpFractionalScaleV1, u32> for State {
    fn event(
        state: &mut Self,
        _: &wp_fractional_scale_v1::WpFractionalScaleV1,
        event: wp_fractional_scale_v1::Event,
        id: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event
            && let Some(out) = state.outputs.get_mut(id)
        {
            out.preferred_scale = Some(scale.max(1));
            out.dirty = true;
        }
    }
}

wayland_client::delegate_noop!(State: ignore wl_compositor::WlCompositor);
wayland_client::delegate_noop!(State: ignore wl_shm::WlShm);
wayland_client::delegate_noop!(State: ignore wl_surface::WlSurface);
wayland_client::delegate_noop!(State: ignore ext_session_lock_manager_v1::ExtSessionLockManagerV1);
wayland_client::delegate_noop!(State: ignore wp_viewporter::WpViewporter);
wayland_client::delegate_noop!(State: ignore wp_viewport::WpViewport);
wayland_client::delegate_noop!(State: ignore wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1);
