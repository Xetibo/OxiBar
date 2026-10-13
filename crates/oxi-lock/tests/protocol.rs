use std::{
    collections::BTreeSet,
    fs::File,
    io::Read,
    os::{
        fd::OwnedFd,
        unix::{fs::FileExt, net::UnixStream},
    },
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use iced::{Color, Element};
use oxi_lock::backend::{Action, Application, Event, Exit, Locker, Output};
use wayland_client::Connection;
use wayland_protocols::{
    ext::session_lock::v1::server::{
        ext_session_lock_manager_v1 as manager, ext_session_lock_surface_v1 as lock_surface,
        ext_session_lock_v1 as lock,
    },
    wp::{
        fractional_scale::v1::server::{
            wp_fractional_scale_manager_v1 as fractional_manager,
            wp_fractional_scale_v1 as fractional,
        },
        viewporter::server::{wp_viewport as viewport, wp_viewporter as viewporter},
    },
};
use wayland_server::{
    Client, DataInit, Dispatch, Display, DisplayHandle, GlobalDispatch, New, Resource,
    backend::{ClientData, ClientId, DisconnectReason, GlobalId},
    protocol::{
        wl_buffer, wl_compositor, wl_output, wl_pointer, wl_region, wl_seat, wl_shm, wl_shm_pool,
        wl_surface,
    },
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Policy {
    Normal,
    Scaled,
    Fractional,
    Denied,
    Finished,
    Hotplug,
    Missing,
    Malformed,
    Pointer,
}

#[derive(Default)]
struct Record {
    requested: usize,
    created: Vec<usize>,
    commits: Vec<(usize, i32, i32)>,
    unlocked: usize,
    destroyed: usize,
    removed: usize,
    cursors: usize,
}
#[derive(Default)]
struct Controls {
    record: Mutex<Record>,
    stop: AtomicBool,
    disconnected: AtomicBool,
}
impl ClientData for Controls {
    fn initialized(&self, _: ClientId) {}
    fn disconnected(&self, _: ClientId, _: DisconnectReason) {
        self.disconnected.store(true, Ordering::Release);
    }
}
struct Fixture {
    client: Option<UnixStream>,
    controls: Arc<Controls>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Fixture {
    fn new(policy: Policy) -> Self {
        let (client, server) = UnixStream::pair().unwrap();
        let controls = Arc::new(Controls::default());
        let control = controls.clone();
        let thread = std::thread::spawn(move || {
            let mut display = Display::<Server>::new().unwrap();
            let mut handle = display.handle();
            handle.create_global::<Server, wl_compositor::WlCompositor, _>(6, ());
            handle.create_global::<Server, wl_shm::WlShm, _>(1, ());
            let initial = handle.create_global::<Server, wl_output::WlOutput, _>(4, 0usize);
            if policy != Policy::Missing {
                handle.create_global::<Server, manager::ExtSessionLockManagerV1, _>(1, ());
            }
            if policy == Policy::Fractional {
                handle.create_global::<Server, viewporter::WpViewporter, _>(1, ());
                handle.create_global::<Server, fractional_manager::WpFractionalScaleManagerV1, _>(
                    1,
                    (),
                );
            }
            if policy == Policy::Pointer {
                handle.create_global::<Server, wl_seat::WlSeat, _>(9, ());
            }
            handle.insert_client(server, control.clone()).unwrap();
            let mut state = Server {
                controls: control.clone(),
                policy,
                lock: None,
                locked: false,
                initial: Some(initial),
                hotplugged: false,
                pointer: None,
            };
            let start = Instant::now();
            while !control.stop.load(Ordering::Acquire) && start.elapsed() < Duration::from_secs(8)
            {
                display.dispatch_clients(&mut state).unwrap();
                display.flush_clients().unwrap();
                std::thread::sleep(Duration::from_millis(2));
            }
        });
        Self {
            client: Some(client),
            controls,
            thread: Some(thread),
        }
    }
    fn client(&mut self) -> Result<Locker, oxi_lock::backend::Error> {
        Locker::from_connection(Connection::from_socket(self.client.take().unwrap())?)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.controls.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

#[derive(Default)]
struct Surface {
    role: Option<usize>,
    acknowledged: u32,
    buffer: Option<wl_buffer::WlBuffer>,
    scale: i32,
    destination: Option<(i32, i32)>,
    cursor: bool,
}
type SurfaceData = Arc<Mutex<Surface>>;
struct LockData {
    surface: SurfaceData,
    index: usize,
}
struct BufferData {
    file: Arc<File>,
    offset: u64,
    width: i32,
    height: i32,
    stride: i32,
    format: wl_shm::Format,
}
struct Server {
    controls: Arc<Controls>,
    policy: Policy,
    lock: Option<lock::ExtSessionLockV1>,
    locked: bool,
    initial: Option<GlobalId>,
    hotplugged: bool,
    pointer: Option<wl_pointer::WlPointer>,
}

macro_rules! global {
    ($object:ty) => {
        impl GlobalDispatch<$object, ()> for Server {
            fn bind(
                _: &mut Self,
                _: &DisplayHandle,
                _: &Client,
                resource: New<$object>,
                _: &(),
                init: &mut DataInit<'_, Self>,
            ) {
                init.init(resource, ());
            }
        }
    };
}
macro_rules! noop {
    ($object:ty, $data:ty) => {
        impl Dispatch<$object, $data> for Server {
            fn request(
                _: &mut Self,
                _: &Client,
                _: &$object,
                _: <$object as Resource>::Request,
                _: &$data,
                _: &DisplayHandle,
                _: &mut DataInit<'_, Self>,
            ) {
            }
        }
    };
}
global!(wl_compositor::WlCompositor);
global!(manager::ExtSessionLockManagerV1);
global!(viewporter::WpViewporter);
global!(fractional_manager::WpFractionalScaleManagerV1);
noop!(wl_region::WlRegion, ());
noop!(wl_buffer::WlBuffer, BufferData);
noop!(fractional::WpFractionalScaleV1, ());

impl GlobalDispatch<wl_seat::WlSeat, ()> for Server {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<wl_seat::WlSeat>,
        _: &(),
        init: &mut DataInit<'_, Self>,
    ) {
        let seat = init.init(resource, ());
        seat.capabilities(wl_seat::Capability::Pointer);
        seat.name("private-pointer".into());
    }
}
impl Dispatch<wl_seat::WlSeat, ()> for Server {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &wl_seat::WlSeat,
        request: wl_seat::Request,
        _: &(),
        _: &DisplayHandle,
        init: &mut DataInit<'_, Self>,
    ) {
        if let wl_seat::Request::GetPointer { id } = request {
            state.pointer = Some(init.init(id, ()));
        }
    }
}
impl Dispatch<wl_pointer::WlPointer, ()> for Server {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &wl_pointer::WlPointer,
        request: wl_pointer::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        if let wl_pointer::Request::SetCursor {
            serial,
            surface: Some(surface),
            hotspot_x,
            hotspot_y,
        } = request
        {
            assert_eq!((serial, hotspot_x, hotspot_y), (42, 0, 0));
            surface
                .data::<SurfaceData>()
                .unwrap()
                .lock()
                .unwrap()
                .cursor = true;
            state.controls.record.lock().unwrap().cursors += 1;
        }
    }
}

impl GlobalDispatch<wl_shm::WlShm, ()> for Server {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<wl_shm::WlShm>,
        _: &(),
        init: &mut DataInit<'_, Self>,
    ) {
        let shm = init.init(resource, ());
        shm.format(wl_shm::Format::Xrgb8888);
        shm.format(wl_shm::Format::Argb8888);
    }
}
impl GlobalDispatch<wl_output::WlOutput, usize> for Server {
    fn bind(
        state: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<wl_output::WlOutput>,
        index: &usize,
        init: &mut DataInit<'_, Self>,
    ) {
        let output = init.init(resource, *index);
        output.name(format!("PRIVATE-{index}"));
        output.scale(if state.policy == Policy::Scaled { 2 } else { 1 });
        output.done();
    }
}
noop!(wl_output::WlOutput, usize);

impl Dispatch<wl_compositor::WlCompositor, ()> for Server {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &wl_compositor::WlCompositor,
        request: wl_compositor::Request,
        _: &(),
        _: &DisplayHandle,
        init: &mut DataInit<'_, Self>,
    ) {
        match request {
            wl_compositor::Request::CreateSurface { id } => {
                init.init(
                    id,
                    Arc::new(Mutex::new(Surface {
                        scale: 1,
                        ..Surface::default()
                    })),
                );
            }
            wl_compositor::Request::CreateRegion { id } => {
                init.init(id, ());
            }
            _ => {}
        }
    }
}
impl Dispatch<wl_shm::WlShm, ()> for Server {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &wl_shm::WlShm,
        request: wl_shm::Request,
        _: &(),
        _: &DisplayHandle,
        init: &mut DataInit<'_, Self>,
    ) {
        if let wl_shm::Request::CreatePool { id, fd, .. } = request {
            init.init(id, Arc::new(File::from(fd)));
        }
    }
}
impl Dispatch<wl_shm_pool::WlShmPool, Arc<File>> for Server {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &wl_shm_pool::WlShmPool,
        request: wl_shm_pool::Request,
        file: &Arc<File>,
        _: &DisplayHandle,
        init: &mut DataInit<'_, Self>,
    ) {
        if let wl_shm_pool::Request::CreateBuffer {
            id,
            offset,
            width,
            height,
            stride,
            format,
        } = request
        {
            let wayland_server::WEnum::Value(format) = format else {
                panic!("unknown pixel format");
            };
            assert!(matches!(
                format,
                wl_shm::Format::Xrgb8888 | wl_shm::Format::Argb8888
            ));
            init.init(
                id,
                BufferData {
                    file: file.clone(),
                    offset: offset as u64,
                    width,
                    height,
                    stride,
                    format,
                },
            );
        }
    }
}

impl Dispatch<manager::ExtSessionLockManagerV1, ()> for Server {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &manager::ExtSessionLockManagerV1,
        request: manager::Request,
        _: &(),
        _: &DisplayHandle,
        init: &mut DataInit<'_, Self>,
    ) {
        if let manager::Request::Lock { id } = request {
            state.controls.record.lock().unwrap().requested += 1;
            let lock = init.init(id, ());
            if state.policy == Policy::Denied {
                lock.finished();
            }
            state.lock = Some(lock);
        }
    }
}
impl Dispatch<lock::ExtSessionLockV1, ()> for Server {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &lock::ExtSessionLockV1,
        request: lock::Request,
        _: &(),
        _: &DisplayHandle,
        init: &mut DataInit<'_, Self>,
    ) {
        match request {
            lock::Request::GetLockSurface {
                id,
                surface,
                output,
            } => {
                let index = *output.data::<usize>().unwrap();
                let data = surface.data::<SurfaceData>().unwrap().clone();
                assert!(data.lock().unwrap().role.replace(index).is_none());
                let object = init.init(
                    id,
                    LockData {
                        surface: data,
                        index,
                    },
                );
                object.configure(1, 64, 32);
                object.configure(
                    2,
                    if state.policy == Policy::Malformed {
                        0
                    } else {
                        80
                    },
                    40,
                );
                state.controls.record.lock().unwrap().created.push(index);
            }
            lock::Request::UnlockAndDestroy => {
                assert!(state.locked);
                state.controls.record.lock().unwrap().unlocked += 1;
            }
            lock::Request::Destroy => {
                assert!(!state.locked);
                state.controls.record.lock().unwrap().destroyed += 1;
            }
            _ => {}
        }
    }
}
impl Dispatch<lock_surface::ExtSessionLockSurfaceV1, LockData> for Server {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &lock_surface::ExtSessionLockSurfaceV1,
        request: lock_surface::Request,
        data: &LockData,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        match request {
            lock_surface::Request::AckConfigure { serial } => {
                assert_eq!(serial, 2);
                let mut surface = data.surface.lock().unwrap();
                assert_eq!(surface.acknowledged, 0);
                surface.acknowledged = serial;
            }
            lock_surface::Request::Destroy if data.index == 0 => {
                state.controls.record.lock().unwrap().removed += 1;
            }
            _ => {}
        }
    }
}
impl Dispatch<wl_surface::WlSurface, SurfaceData> for Server {
    fn request(
        state: &mut Self,
        _: &Client,
        proxy: &wl_surface::WlSurface,
        request: wl_surface::Request,
        data: &SurfaceData,
        handle: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        let mut surface = data.lock().unwrap();
        if surface.cursor {
            match request {
                wl_surface::Request::Attach { buffer, .. } => surface.buffer = buffer,
                wl_surface::Request::Commit => {
                    let buffer = surface.buffer.as_ref().unwrap();
                    let data = buffer.data::<BufferData>().unwrap();
                    assert_eq!(
                        (data.width, data.height, data.format),
                        (16, 24, wl_shm::Format::Argb8888)
                    );
                    let mut pixels = vec![0; (data.stride * data.height) as usize];
                    data.file.read_exact_at(&mut pixels, data.offset).unwrap();
                    assert!(
                        pixels
                            .chunks_exact(4)
                            .any(|pixel| u32::from_ne_bytes(pixel.try_into().unwrap()) == 0)
                    );
                    assert!(
                        pixels.chunks_exact(4).any(|pixel| u32::from_ne_bytes(
                            pixel.try_into().unwrap()
                        ) == 0xffffffff)
                    );
                    buffer.release();
                }
                _ => {}
            }
            return;
        }
        match request {
            wl_surface::Request::Attach { buffer, .. } => {
                assert_eq!(surface.acknowledged, 2);
                assert!(buffer.is_some());
                surface.buffer = buffer;
            }
            wl_surface::Request::SetBufferScale { scale } => surface.scale = scale,
            wl_surface::Request::Commit => {
                assert_eq!(surface.acknowledged, 2);
                let buffer = surface
                    .buffer
                    .as_ref()
                    .expect("A lock commit must never contain a NULL buffer");
                let data = buffer.data::<BufferData>().unwrap();
                assert_eq!(data.format, wl_shm::Format::Xrgb8888);
                let factor = match state.policy {
                    Policy::Scaled => 2.0,
                    Policy::Fractional => 1.25,
                    _ => 1.0,
                };
                assert_eq!(
                    (data.width, data.height),
                    ((80.0 * factor) as i32, (40.0 * factor) as i32)
                );
                if state.policy == Policy::Fractional {
                    assert_eq!(surface.destination, Some((80, 40)));
                    assert_eq!(surface.scale, 1);
                } else {
                    assert_eq!(
                        (data.width / surface.scale, data.height / surface.scale),
                        (80, 40)
                    );
                }
                let mut pixels = vec![0; (data.stride * data.height) as usize];
                data.file.read_exact_at(&mut pixels, data.offset).unwrap();
                assert_eq!(
                    u32::from_ne_bytes(pixels[..4].try_into().unwrap()),
                    0xff2266bb
                );
                assert!(
                    pixels
                        .chunks_exact(4)
                        .all(|pixel| u32::from_ne_bytes(pixel.try_into().unwrap()) >> 24 == 255)
                );
                let index = surface.role.unwrap();
                state.controls.record.lock().unwrap().commits.push((
                    index,
                    data.width,
                    data.height,
                ));
                buffer.release();
                if !state.locked {
                    state.locked = true;
                    state.lock.as_ref().unwrap().locked();
                    if state.policy == Policy::Pointer {
                        let pointer = state.pointer.as_ref().unwrap();
                        pointer.enter(42, proxy, 40.0, 20.0);
                        pointer.button(43, 1, 272, wl_pointer::ButtonState::Pressed);
                        pointer.button(44, 2, 272, wl_pointer::ButtonState::Released);
                        pointer.frame();
                    }
                    if state.policy == Policy::Finished {
                        state.lock.as_ref().unwrap().finished();
                    }
                    if state.policy == Policy::Hotplug {
                        handle.create_global::<Server, wl_output::WlOutput, _>(4, 1usize);
                        state.hotplugged = true;
                    }
                } else if state.hotplugged
                    && index == 1
                    && let Some(initial) = state.initial.take()
                {
                    handle.remove_global::<Server>(initial);
                }
            }
            _ => {}
        }
    }
}
impl Dispatch<viewporter::WpViewporter, ()> for Server {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &viewporter::WpViewporter,
        request: viewporter::Request,
        _: &(),
        _: &DisplayHandle,
        init: &mut DataInit<'_, Self>,
    ) {
        if let viewporter::Request::GetViewport { id, surface } = request {
            init.init(id, surface.data::<SurfaceData>().unwrap().clone());
        }
    }
}
impl Dispatch<viewport::WpViewport, SurfaceData> for Server {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &viewport::WpViewport,
        request: viewport::Request,
        data: &SurfaceData,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        if let viewport::Request::SetDestination { width, height } = request {
            data.lock().unwrap().destination = Some((width, height));
        }
    }
}
impl Dispatch<fractional_manager::WpFractionalScaleManagerV1, ()> for Server {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &fractional_manager::WpFractionalScaleManagerV1,
        request: fractional_manager::Request,
        _: &(),
        _: &DisplayHandle,
        init: &mut DataInit<'_, Self>,
    ) {
        if let fractional_manager::Request::GetFractionalScale { id, .. } = request {
            init.init(id, ()).preferred_scale(150);
        }
    }
}

struct App {
    hotplug: bool,
    crash: bool,
    external: bool,
    outputs: BTreeSet<u32>,
    ready: Option<UnixStream>,
    pointer: bool,
}
impl Application for App {
    type Message = ();
    fn update(&mut self, event: Event<()>) -> Action {
        match event {
            Event::Locked => {
                if let Some(file) = &mut self.ready {
                    let mut data = [0; 7];
                    file.read_exact(&mut data).unwrap();
                    assert_eq!(&data, b"locked\n");
                }
                assert!(
                    !self.crash,
                    "Private fixture simulates a locker crash after Locked"
                );
                if !self.hotplug && !self.external && !self.pointer {
                    return Action::AuthenticatedUnlock;
                }
            }
            Event::OutputAdded(id) => {
                self.outputs.insert(id);
                if !self.hotplug && !self.external && !self.pointer {
                    return Action::AuthenticatedUnlock;
                }
            }
            Event::OutputRemoved(id) => {
                self.outputs.remove(&id);
                if self.hotplug {
                    return Action::AuthenticatedUnlock;
                }
            }
            Event::Message(()) if self.pointer => return Action::AuthenticatedUnlock,
            _ => {}
        }
        Action::None
    }
    fn view(&self, _: &Output) -> Element<'_, ()> {
        if self.pointer {
            return iced::widget::container(iced::widget::button("Click").on_press(()))
                .center(iced::Fill)
                .into();
        }
        iced::widget::container(iced::widget::text("Private lock fixture"))
            .center(iced::Fill)
            .into()
    }
    fn background(&self) -> Color {
        Color::from_rgb8(0x22, 0x66, 0xbb)
    }
}
fn app() -> App {
    App {
        hotplug: false,
        crash: false,
        external: false,
        outputs: BTreeSet::new(),
        ready: None,
        pointer: false,
    }
}

#[test]
fn native_pointer_sets_visible_cursor_and_click_reaches_widgets_on_lock_surface() {
    let mut fixture = Fixture::new(Policy::Pointer);
    let mut app = app();
    app.pointer = true;
    assert_eq!(
        fixture.client().unwrap().run(app, None).unwrap(),
        Exit::Authenticated
    );
    let record = fixture.controls.record.lock().unwrap();
    assert_eq!(record.cursors, 1);
    assert_eq!(record.unlocked, 1);
    assert!(record.commits.len() >= 2);
}

#[test]
fn latest_configure_opaque_buffers_integer_and_fractional_scaling_authenticated_unlock_and_sync() {
    for policy in [Policy::Normal, Policy::Scaled, Policy::Fractional] {
        let mut fixture = Fixture::new(policy);
        let (reader, writer) = UnixStream::pair().unwrap();
        reader
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut app = app();
        app.ready = Some(reader);
        let exit = fixture
            .client()
            .unwrap()
            .run(app, Some(File::from(OwnedFd::from(writer))))
            .unwrap();
        assert_eq!(exit, Exit::Authenticated);
        let record = fixture.controls.record.lock().unwrap();
        assert_eq!(record.unlocked, 1);
        assert_eq!(record.destroyed, 0);
        assert_eq!(record.created, [0]);
        assert!(!record.commits.is_empty());
    }
}

#[test]
fn denial_never_reports_readiness_or_commits_and_uses_unlocked_destroy() {
    let mut fixture = Fixture::new(Policy::Denied);
    let (mut reader, writer) = UnixStream::pair().unwrap();
    reader
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    assert_eq!(
        fixture
            .client()
            .unwrap()
            .run(app(), Some(File::from(OwnedFd::from(writer))))
            .unwrap(),
        Exit::Denied
    );
    assert_eq!(reader.read(&mut [0]).unwrap(), 0);
    let record = fixture.controls.record.lock().unwrap();
    assert_eq!(record.destroyed, 1);
    assert_eq!(record.unlocked, 0);
    assert!(record.commits.is_empty());
}

#[test]
fn externally_finished_after_locked_uses_unlock_destructor_without_authentication() {
    let mut fixture = Fixture::new(Policy::Finished);
    let mut app = app();
    app.external = true;
    assert_eq!(
        fixture.client().unwrap().run(app, None).unwrap(),
        Exit::ExternallyUnlocked
    );
    let record = fixture.controls.record.lock().unwrap();
    assert_eq!(record.unlocked, 1);
    assert_eq!(record.destroyed, 0);
}

#[test]
fn hotplug_covers_new_output_and_removal_retires_its_surface_before_unlock() {
    let mut fixture = Fixture::new(Policy::Hotplug);
    let mut app = app();
    app.hotplug = true;
    assert_eq!(
        fixture.client().unwrap().run(app, None).unwrap(),
        Exit::Authenticated
    );
    let record = fixture.controls.record.lock().unwrap();
    assert_eq!(record.created, [0, 1]);
    assert!(record.commits.iter().any(|&(index, _, _)| index == 1));
    assert!(record.removed >= 1);
    assert_eq!(record.unlocked, 1);
}

#[test]
fn crash_leaves_lock_owned_by_compositor_and_missing_protocol_has_no_overlay_fallback() {
    let mut fixture = Fixture::new(Policy::Normal);
    let mut app = app();
    app.crash = true;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        fixture.client().unwrap().run(app, None)
    }));
    assert!(result.is_err());
    let start = Instant::now();
    while !fixture.controls.disconnected.load(Ordering::Acquire)
        && start.elapsed() < Duration::from_secs(2)
    {
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(fixture.controls.disconnected.load(Ordering::Acquire));
    assert_eq!(fixture.controls.record.lock().unwrap().unlocked, 0);
    let mut missing = Fixture::new(Policy::Missing);
    assert!(missing.client().is_err());
    assert_eq!(missing.controls.record.lock().unwrap().requested, 0);

    let mut malformed = Fixture::new(Policy::Malformed);
    assert!(malformed.client().unwrap().run(self::app(), None).is_err());
    let record = malformed.controls.record.lock().unwrap();
    assert_eq!(record.unlocked, 0);
    assert!(record.commits.is_empty());
}
