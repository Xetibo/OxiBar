use super::{
    capture,
    protocols::server_mapping::{
        hyprland_toplevel_mapping_manager_v1 as mapping,
        hyprland_toplevel_window_mapping_handle_v1 as mapped,
    },
};
use iced::futures::{
    StreamExt,
    executor::block_on,
    future::{Either, select},
    stream::BoxStream,
};
use std::{
    collections::HashMap,
    fs::File,
    os::unix::net::UnixListener,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use wayland_protocols::ext::{
    foreign_toplevel_list::v1::server::{
        ext_foreign_toplevel_handle_v1 as foreign, ext_foreign_toplevel_list_v1 as list,
    },
    image_capture_source::v1::server::{
        ext_foreign_toplevel_image_capture_source_manager_v1 as sources,
        ext_image_capture_source_v1 as source,
    },
    image_copy_capture::v1::server::{
        ext_image_copy_capture_frame_v1 as frame, ext_image_copy_capture_manager_v1 as copies,
        ext_image_copy_capture_session_v1 as session,
    },
};
use wayland_protocols_wlr::foreign_toplevel::v1::server::{
    zwlr_foreign_toplevel_handle_v1 as native, zwlr_foreign_toplevel_manager_v1 as wlr,
};
use wayland_server::{
    Client, DataInit, Dispatch, Display, DisplayHandle, GlobalDispatch, New, Resource,
    protocol::{wl_buffer, wl_output, wl_seat, wl_shm, wl_shm_pool},
};

enum Change {
    Resize,
    Deny,
    Recycle,
    ReplaceExt,
}
#[derive(Default)]
struct Controls {
    stop: AtomicBool,
    changes: Mutex<Vec<Change>>,
    activated: Mutex<Vec<usize>>,
    max_frames: AtomicUsize,
}
struct Fixture {
    root: PathBuf,
    socket: PathBuf,
    controls: Arc<Controls>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Fixture {
    fn new() -> Self {
        use std::os::unix::fs::DirBuilderExt;
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("oxibar-ext-{}-{nonce}", std::process::id()));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&root)
            .unwrap();
        let socket = root.join("wayland");
        let listener = UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let controls = Arc::new(Controls::default());
        let control = controls.clone();
        let thread = std::thread::spawn(move || {
            let mut display = Display::<Server>::new().unwrap();
            let mut handle = display.handle();
            let mut state = Server {
                controls: control.clone(),
                outputs: Vec::new(),
                foreign: HashMap::new(),
                native: HashMap::new(),
                sessions: Vec::new(),
                active: HashMap::new(),
                sizes: [(64, 32); 3],
                list_manager: None,
                native_manager: None,
            };
            handle.create_global::<Server, wl_shm::WlShm, _>(1, ());
            handle.create_global::<Server, wl_seat::WlSeat, _>(1, ());
            handle.create_global::<Server, wl_output::WlOutput, _>(4, 0usize);
            handle.create_global::<Server, wl_output::WlOutput, _>(4, 1usize);
            handle.create_global::<Server, mapping::HyprlandToplevelMappingManagerV1, _>(1, ());
            handle
                .create_global::<Server, sources::ExtForeignToplevelImageCaptureSourceManagerV1, _>(
                    1,
                    (),
                );
            handle.create_global::<Server, copies::ExtImageCopyCaptureManagerV1, _>(1, ());
            handle.create_global::<Server, list::ExtForeignToplevelListV1, _>(1, ());
            handle.create_global::<Server, wlr::ZwlrForeignToplevelManagerV1, _>(3, ());
            while !control.stop.load(Ordering::Acquire) {
                if let Ok((stream, _)) = listener.accept() {
                    handle.insert_client(stream, Arc::new(())).unwrap();
                }
                display.dispatch_clients(&mut state).unwrap();
                state.changes(&handle);
                display.flush_clients().unwrap();
                std::thread::sleep(Duration::from_millis(2));
            }
        });
        Self {
            root,
            socket,
            controls,
            thread: Some(thread),
        }
    }
    fn change(&self, change: Change) {
        self.controls.changes.lock().unwrap().push(change);
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.controls.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
struct Server {
    controls: Arc<Controls>,
    outputs: Vec<(usize, wl_output::WlOutput)>,
    foreign: HashMap<usize, foreign::ExtForeignToplevelHandleV1>,
    native: HashMap<usize, native::ZwlrForeignToplevelHandleV1>,
    sessions: Vec<(usize, session::ExtImageCopyCaptureSessionV1)>,
    active: HashMap<wayland_server::backend::ObjectId, wayland_server::backend::ObjectId>,
    sizes: [(u32, u32); 3],
    list_manager: Option<list::ExtForeignToplevelListV1>,
    native_manager: Option<wlr::ZwlrForeignToplevelManagerV1>,
}
fn address(index: usize) -> u64 {
    0x1234_abcd_0000_0001 + (index % 2) as u64
}
impl Server {
    fn foreign_window(
        &mut self,
        client: &Client,
        handle: &DisplayHandle,
        manager: &list::ExtForeignToplevelListV1,
        index: usize,
    ) {
        let window = client
            .create_resource::<foreign::ExtForeignToplevelHandleV1, usize, Self>(handle, 1, index)
            .unwrap();
        manager.toplevel(&window);
        window.identifier(format!("incarnation-{index}"));
        window.title("Identical title".into());
        window.app_id(format!("org.oxibar.modern.{index}"));
        window.done();
        self.foreign.insert(index, window);
    }
    fn native_window(
        &mut self,
        client: &Client,
        handle: &DisplayHandle,
        manager: &wlr::ZwlrForeignToplevelManagerV1,
        index: usize,
    ) {
        let window = client
            .create_resource::<native::ZwlrForeignToplevelHandleV1, usize, Self>(handle, 3, index)
            .unwrap();
        manager.toplevel(&window);
        window.title("Identical title".into());
        window.app_id(format!("org.oxibar.modern.{index}"));
        for (_, output) in self.outputs.iter().filter(|(slot, _)| *slot == index % 2) {
            window.output_enter(output);
        }
        window.state(Vec::new());
        window.done();
        self.native.insert(index, window);
    }
    fn constraints(&self, object: &session::ExtImageCopyCaptureSessionV1, index: usize) {
        // The protocol permits formats before dimensions in every constraint batch.
        object.shm_format(wl_shm::Format::Argb8888);
        let (width, height) = self.sizes[index];
        object.buffer_size(width, height);
        object.done();
    }
    fn changes(&mut self, handle: &DisplayHandle) {
        let changes = std::mem::take(&mut *self.controls.changes.lock().unwrap());
        for change in changes {
            match change {
                Change::Resize => {
                    self.sizes[0] = (128, 48);
                    for (_, object) in self
                        .sessions
                        .iter()
                        .filter(|(index, object)| *index == 0 && object.is_alive())
                    {
                        self.constraints(object, 0);
                    }
                }
                Change::Deny => {
                    for (_, object) in self
                        .sessions
                        .iter()
                        .filter(|(index, object)| *index == 1 && object.is_alive())
                    {
                        object.stopped();
                    }
                }
                Change::Recycle => {
                    let old = self.native.remove(&0).unwrap();
                    let client = old.client().unwrap();
                    old.closed();
                    let manager = self.native_manager.as_ref().unwrap().clone();
                    self.native_window(&client, handle, &manager, 2);
                    // Keep the old ext handle alive: it must not acquire the recycled activation handle.
                }
                Change::ReplaceExt => {
                    let old = self.foreign.remove(&0).unwrap();
                    let client = old.client().unwrap();
                    old.closed();
                    let manager = self.list_manager.as_ref().unwrap().clone();
                    self.foreign_window(&client, handle, &manager, 2);
                }
            }
        }
    }
}

macro_rules! global {
    ($object:path) => {
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
macro_rules! inert {
    ($object:path, $data:ty) => {
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
global!(mapping::HyprlandToplevelMappingManagerV1);
global!(sources::ExtForeignToplevelImageCaptureSourceManagerV1);
global!(copies::ExtImageCopyCaptureManagerV1);
global!(wl_seat::WlSeat);
inert!(wl_seat::WlSeat, ());
inert!(wl_output::WlOutput, usize);
inert!(wl_buffer::WlBuffer, SharedBuffer);
inert!(foreign::ExtForeignToplevelHandleV1, usize);
inert!(source::ExtImageCaptureSourceV1, usize);
inert!(mapped::HyprlandToplevelWindowMappingHandleV1, ());
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
        output.name(format!("MOCK-{index}"));
        output.done();
        state.outputs.push((*index, output));
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
        init.init(resource, ()).format(wl_shm::Format::Argb8888);
    }
}
impl GlobalDispatch<list::ExtForeignToplevelListV1, ()> for Server {
    fn bind(
        state: &mut Self,
        handle: &DisplayHandle,
        client: &Client,
        resource: New<list::ExtForeignToplevelListV1>,
        _: &(),
        init: &mut DataInit<'_, Self>,
    ) {
        let manager = init.init(resource, ());
        state.list_manager = Some(manager.clone());
        for index in 0..2 {
            state.foreign_window(client, handle, &manager, index);
        }
    }
}
inert!(list::ExtForeignToplevelListV1, ());
impl GlobalDispatch<wlr::ZwlrForeignToplevelManagerV1, ()> for Server {
    fn bind(
        state: &mut Self,
        handle: &DisplayHandle,
        client: &Client,
        resource: New<wlr::ZwlrForeignToplevelManagerV1>,
        _: &(),
        init: &mut DataInit<'_, Self>,
    ) {
        let manager = init.init(resource, ());
        state.native_manager = Some(manager.clone());
        for index in 0..2 {
            state.native_window(client, handle, &manager, index);
        }
    }
}
inert!(wlr::ZwlrForeignToplevelManagerV1, ());
impl Dispatch<native::ZwlrForeignToplevelHandleV1, usize> for Server {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &native::ZwlrForeignToplevelHandleV1,
        request: native::Request,
        index: &usize,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        if let native::Request::Activate { .. } = request {
            state.controls.activated.lock().unwrap().push(*index);
        }
    }
}
impl Dispatch<mapping::HyprlandToplevelMappingManagerV1, ()> for Server {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &mapping::HyprlandToplevelMappingManagerV1,
        request: mapping::Request,
        _: &(),
        _: &DisplayHandle,
        init: &mut DataInit<'_, Self>,
    ) {
        let (object, index) = match request {
            mapping::Request::GetWindowForToplevel { handle, toplevel } => {
                (handle, *toplevel.data::<usize>().unwrap())
            }
            mapping::Request::GetWindowForToplevelWlr { handle, toplevel } => {
                (handle, *toplevel.data::<usize>().unwrap())
            }
            _ => return,
        };
        let object = init.init(object, ());
        let address = address(index);
        object.window_address((address >> 32) as u32, address as u32);
    }
}
impl Dispatch<sources::ExtForeignToplevelImageCaptureSourceManagerV1, ()> for Server {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &sources::ExtForeignToplevelImageCaptureSourceManagerV1,
        request: sources::Request,
        _: &(),
        _: &DisplayHandle,
        init: &mut DataInit<'_, Self>,
    ) {
        if let sources::Request::CreateSource {
            source,
            toplevel_handle,
        } = request
        {
            init.init(source, *toplevel_handle.data::<usize>().unwrap());
        }
    }
}
impl Dispatch<copies::ExtImageCopyCaptureManagerV1, ()> for Server {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &copies::ExtImageCopyCaptureManagerV1,
        request: copies::Request,
        _: &(),
        _: &DisplayHandle,
        init: &mut DataInit<'_, Self>,
    ) {
        if let copies::Request::CreateSession {
            session, source, ..
        } = request
        {
            let index = *source.data::<usize>().unwrap();
            let object = init.init(session, index);
            state.constraints(&object, index);
            state.sessions.push((index, object));
        }
    }
}
struct FrameData {
    index: usize,
    session: wayland_server::backend::ObjectId,
    buffer: Mutex<Option<wl_buffer::WlBuffer>>,
}
impl Dispatch<session::ExtImageCopyCaptureSessionV1, usize> for Server {
    fn request(
        state: &mut Self,
        _: &Client,
        object: &session::ExtImageCopyCaptureSessionV1,
        request: session::Request,
        index: &usize,
        _: &DisplayHandle,
        init: &mut DataInit<'_, Self>,
    ) {
        if let session::Request::CreateFrame { frame } = request {
            assert!(
                !state.active.values().any(|session| *session == object.id()),
                "one frame per capture session"
            );
            let frame = init.init(
                frame,
                FrameData {
                    index: *index,
                    session: object.id(),
                    buffer: Mutex::new(None),
                },
            );
            state.active.insert(frame.id(), object.id());
            state
                .controls
                .max_frames
                .fetch_max(state.active.len(), Ordering::Relaxed);
        }
    }
}
struct SharedBuffer {
    file: Arc<File>,
    offset: usize,
    width: usize,
    height: usize,
    stride: usize,
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
            ..
        } = request
        {
            assert_eq!(
                format,
                wayland_server::WEnum::Value(wl_shm::Format::Argb8888)
            );
            init.init(
                id,
                SharedBuffer {
                    file: file.clone(),
                    offset: offset as usize,
                    width: width as usize,
                    height: height as usize,
                    stride: stride as usize,
                },
            );
        }
    }
}
impl Dispatch<frame::ExtImageCopyCaptureFrameV1, FrameData> for Server {
    fn request(
        state: &mut Self,
        _: &Client,
        object: &frame::ExtImageCopyCaptureFrameV1,
        request: frame::Request,
        data: &FrameData,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        match request {
            frame::Request::AttachBuffer { buffer } => *data.buffer.lock().unwrap() = Some(buffer),
            frame::Request::Capture => {
                let buffer = data.buffer.lock().unwrap();
                let buffer = buffer.as_ref().unwrap().data::<SharedBuffer>().unwrap();
                let (width, height) = state.sizes[data.index];
                if (buffer.width, buffer.height) != (width as usize, height as usize) {
                    object.failed(frame::FailureReason::BufferConstraints);
                    return;
                }
                let size = buffer.offset + buffer.height * buffer.stride;
                assert!(size as u64 <= buffer.file.metadata().unwrap().len());
                // The fixture owns the passed memfd and writes only the validated buffer range.
                let mut memory =
                    unsafe { memmap2::MmapOptions::new().len(size).map_mut(&*buffer.file) }
                        .unwrap();
                let color = match data.index {
                    0 => [0, 0, 255, 255],
                    1 => [255, 0, 0, 255],
                    _ => [0, 255, 0, 255],
                };
                for y in 0..buffer.height {
                    for x in 0..buffer.width {
                        let at = buffer.offset + y * buffer.stride + x * 4;
                        memory[at..at + 4].copy_from_slice(&color);
                    }
                }
                object.transform(wl_output::Transform::Normal);
                object.damage(0, 0, width as i32, height as i32);
                object.presentation_time(0, 1, 0);
                object.ready();
            }
            _ => {}
        }
    }
    fn destroyed(
        state: &mut Self,
        _: wayland_server::backend::ClientId,
        object: &frame::ExtImageCopyCaptureFrameV1,
        data: &FrameData,
    ) {
        assert_eq!(
            state.active.remove(&object.id()),
            Some(data.session.clone())
        );
    }
}

async fn next(stream: &mut BoxStream<'static, capture::Snapshot>) -> capture::Snapshot {
    match select(
        stream.next(),
        Box::pin(async_io::Timer::after(Duration::from_secs(5))),
    )
    .await
    {
        Either::Left((Some(snapshot), _)) => {
            assert!(snapshot.error.is_none(), "{:?}", snapshot.error);
            snapshot
        }
        _ => panic!("private modern capture deadline"),
    }
}
#[test]
fn modern_protocol_real_shm_any_order_constraints_resize_denial_and_exact_native_activation() {
    let fixture = Fixture::new();
    let client = capture::Client::for_socket(fixture.socket.clone());
    let mut stream = capture::events(client.clone());
    let initial = block_on(async {
        loop {
            let state = next(&mut stream).await;
            if state.windows.len() == 2
                && state
                    .windows
                    .iter()
                    .all(|w| w.image.is_some() && w.activatable)
            {
                break state;
            }
        }
    });
    assert_eq!(initial.backend, "ext-image-copy-capture");
    let first = initial
        .windows
        .iter()
        .find(|w| w.app_id.ends_with('0'))
        .unwrap();
    let second = initial
        .windows
        .iter()
        .find(|w| w.app_id.ends_with('1'))
        .unwrap();
    assert_eq!(first.title, second.title);
    assert_eq!(first.outputs, ["MOCK-0"]);
    assert_eq!(second.outputs, ["MOCK-1"]);
    let id = first.id;
    match first.image.as_ref().unwrap() {
        iced::widget::image::Handle::Rgba { pixels, .. } => {
            assert_eq!(&pixels[..4], [255, 0, 0, 255])
        }
        _ => panic!("real RGBA pixels required"),
    }
    block_on(client.activate(second.id)).unwrap();
    assert_eq!(*fixture.controls.activated.lock().unwrap(), [1]);
    fixture.change(Change::Resize);
    block_on(async {
        loop {
            let state = next(&mut stream).await;
            if state.windows.iter().any(|w| {
                w.id == id
                    && matches!(
                        &w.image,
                        Some(iced::widget::image::Handle::Rgba {
                            width: 128,
                            height: 48,
                            ..
                        })
                    )
            }) {
                break;
            }
        }
    });
    fixture.change(Change::Deny);
    block_on(async {
        loop {
            let state = next(&mut stream).await;
            if state
                .windows
                .iter()
                .any(|w| w.id == second.id && w.image.is_none() && w.error.is_some())
            {
                break;
            }
        }
    });
    fixture.change(Change::Recycle);
    block_on(async {
        loop {
            let state = next(&mut stream).await;
            if state.windows.iter().any(|w| w.id == id && !w.activatable) {
                break;
            }
        }
    });
    assert!(
        block_on(client.activate(id)).is_err(),
        "old ext handle must not activate a recycled native address"
    );
    fixture.change(Change::ReplaceExt);
    let replacement = block_on(async {
        loop {
            let state = next(&mut stream).await;
            if let Some(window) = state
                .windows
                .iter()
                .find(|w| w.app_id.ends_with('2') && w.image.is_some() && w.activatable)
            {
                break window.clone();
            }
        }
    });
    assert_ne!(replacement.id, id);
    block_on(client.activate(replacement.id)).unwrap();
    assert_eq!(*fixture.controls.activated.lock().unwrap(), [1, 2]);
    assert!(block_on(client.activate(id)).is_err());
    assert!(fixture.controls.max_frames.load(Ordering::Relaxed) <= 2);
    let before = Instant::now();
    drop(stream);
    assert!(before.elapsed() < Duration::from_secs(1));
    assert!(block_on(client.activate(id)).is_err());
}
