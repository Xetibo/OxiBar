use super::capture::{self, Snapshot};
use iced::futures::{StreamExt, executor::block_on};
use std::{
    fs::File,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use wayland_client::{
    Connection, Dispatch, QueueHandle,
    protocol::{wl_buffer, wl_compositor, wl_registry, wl_shm, wl_shm_pool, wl_surface},
};
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};

struct Surface {
    surface: wl_surface::WlSurface,
    top: xdg_toplevel::XdgToplevel,
    _xdg: xdg_surface::XdgSurface,
    buffer: Option<wl_buffer::WlBuffer>,
    color: [u8; 4],
    configured: bool,
    size: (u32, u32),
}
#[derive(Default)]
struct Fixture {
    compositor: Option<wl_compositor::WlCompositor>,
    shm: Option<wl_shm::WlShm>,
    shell: Option<xdg_wm_base::XdgWmBase>,
    windows: Vec<Surface>,
}
impl Fixture {
    fn draw(&mut self, index: usize, qh: &QueueHandle<Self>) {
        let w = &mut self.windows[index];
        let (width, height) = w.size;
        let size = width as usize * height as usize * 4;
        let file = File::from(
            rustix::fs::memfd_create(c"oxibar-private-window", rustix::fs::MemfdFlags::CLOEXEC)
                .unwrap(),
        );
        file.set_len(size as u64).unwrap();
        // This fixture owns the memfd and completes every write before committing its buffer.
        let mut map = unsafe { memmap2::MmapOptions::new().len(size).map_mut(&file) }.unwrap();
        for y in 0..height {
            for x in 0..width {
                let offset = (y as usize * width as usize + x as usize) * 4;
                let pixel = if (x / 32 + y / 32) % 2 == 0 {
                    w.color
                } else {
                    [255, 255, 255, 255]
                };
                map[offset..offset + 4].copy_from_slice(&pixel);
            }
        }
        use std::os::fd::AsFd;
        let pool = self
            .shm
            .as_ref()
            .unwrap()
            .create_pool(file.as_fd(), size as i32, qh, ());
        let buffer = pool.create_buffer(
            0,
            width as i32,
            height as i32,
            width as i32 * 4,
            wl_shm::Format::Xrgb8888,
            qh,
            (),
        );
        pool.destroy();
        w.surface.attach(Some(&buffer), 0, 0);
        w.surface.damage_buffer(0, 0, width as i32, height as i32);
        w.surface.commit();
        if let Some(old) = w.buffer.replace(buffer) {
            old.destroy()
        }
    }
}
impl Dispatch<wl_registry::WlRegistry, ()> for Fixture {
    fn event(
        s: &mut Self,
        p: &wl_registry::WlRegistry,
        e: wl_registry::Event,
        _: &(),
        _: &Connection,
        q: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = e
        {
            match interface.as_str() {
                "wl_compositor" => s.compositor = Some(p.bind(name, version.min(6), q, ())),
                "wl_shm" => s.shm = Some(p.bind(name, 1, q, ())),
                "xdg_wm_base" => s.shell = Some(p.bind(name, version.min(6), q, ())),
                _ => {}
            }
        }
    }
}
impl Dispatch<xdg_wm_base::XdgWmBase, ()> for Fixture {
    fn event(
        _: &mut Self,
        p: &xdg_wm_base::XdgWmBase,
        e: xdg_wm_base::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = e {
            p.pong(serial)
        }
    }
}
impl Dispatch<xdg_surface::XdgSurface, usize> for Fixture {
    fn event(
        s: &mut Self,
        p: &xdg_surface::XdgSurface,
        e: xdg_surface::Event,
        id: &usize,
        _: &Connection,
        q: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = e {
            p.ack_configure(serial);
            s.windows[*id].configured = true;
            s.draw(*id, q);
        }
    }
}
impl Dispatch<xdg_toplevel::XdgToplevel, usize> for Fixture {
    fn event(
        s: &mut Self,
        _: &xdg_toplevel::XdgToplevel,
        e: xdg_toplevel::Event,
        id: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_toplevel::Event::Configure { width, height, .. } = e
            && width > 0
            && height > 0
        {
            s.windows[*id].size = (width as u32, height as u32);
        }
    }
}
wayland_client::delegate_noop!(Fixture:ignore wl_compositor::WlCompositor);
wayland_client::delegate_noop!(Fixture:ignore wl_shm::WlShm);
wayland_client::delegate_noop!(Fixture:ignore wl_shm_pool::WlShmPool);
wayland_client::delegate_noop!(Fixture:ignore wl_buffer::WlBuffer);
wayland_client::delegate_noop!(Fixture:ignore wl_surface::WlSurface);
struct Windows {
    stop: Arc<AtomicBool>,
    changed: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Windows {
    fn new() -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let changed = Arc::new(AtomicBool::new(false));
        let phase = changed.clone();
        let flag = stop.clone();
        let thread = std::thread::spawn(move || {
            let connection = Connection::connect_to_env().unwrap();
            let mut queue = connection.new_event_queue();
            let qh = queue.handle();
            let mut state = Fixture::default();
            connection.display().get_registry(&qh, ());
            queue.roundtrip(&mut state).unwrap();
            for (index, color) in [[0, 0, 255, 255], [255, 0, 0, 255]].into_iter().enumerate() {
                let surface = state.compositor.as_ref().unwrap().create_surface(&qh, ());
                let xdg = state
                    .shell
                    .as_ref()
                    .unwrap()
                    .get_xdg_surface(&surface, &qh, index);
                let top = xdg.get_toplevel(&qh, index);
                top.set_title("Identical private preview title".into());
                top.set_app_id(format!("org.oxibar.preview.{index}"));
                top.set_min_size(320, 240);
                state.windows.push(Surface {
                    surface: surface.clone(),
                    top,
                    _xdg: xdg,
                    buffer: None,
                    color,
                    configured: false,
                    size: (320, 240),
                });
                surface.commit();
            }
            let mut updated = false;
            while !flag.load(Ordering::Acquire) {
                if phase.load(Ordering::Acquire) && !updated {
                    updated = true;
                    for index in 0..state.windows.len() {
                        state.windows[index].color = if index == 0 {
                            [255, 0, 0, 255]
                        } else {
                            [0, 0, 255, 255]
                        };
                        if state.windows[index].configured {
                            state.draw(index, &qh);
                        }
                    }
                }
                queue.dispatch_pending(&mut state).unwrap();
                connection.flush().unwrap();
                let Some(guard) = queue.prepare_read() else {
                    continue;
                };
                let backend = connection.backend();
                let fd = backend.poll_fd();
                let readable = {
                    let mut poll = [rustix::event::PollFd::new(
                        &fd,
                        rustix::event::PollFlags::IN,
                    )];
                    rustix::event::poll(
                        &mut poll,
                        Some(&rustix::event::Timespec {
                            tv_sec: 0,
                            tv_nsec: 100_000_000,
                        }),
                    )
                    .unwrap();
                    !poll[0].revents().is_empty()
                };
                if readable {
                    guard.read().unwrap();
                } else {
                    drop(guard)
                }
            }
            for w in state.windows {
                w.top.destroy();
                w._xdg.destroy();
                w.surface.destroy();
                if let Some(buffer) = w.buffer {
                    buffer.destroy()
                }
            }
            let _ = connection.flush();
        });
        Self {
            stop,
            changed,
            thread: Some(thread),
        }
    }
}
impl Drop for Windows {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
async fn next(stream: &mut iced::futures::stream::BoxStream<'static, Snapshot>) -> Snapshot {
    use iced::futures::{
        FutureExt,
        future::{Either, select},
    };
    match select(
        stream.next().boxed(),
        iced::futures::FutureExt::boxed(async_io::Timer::after(Duration::from_secs(5))),
    )
    .await
    {
        Either::Left((Some(value), _)) => value,
        _ => panic!("No native overview snapshot within deadline"),
    }
}

#[test]
#[ignore = "Requires the owned isolated Hyprland compositor"]
fn private_overview_real_pixels_exact_handles_and_owned_cleanup() {
    assert_eq!(std::env::var("OXIBAR_TEST_ISOLATED").as_deref(), Ok("1"));
    let windows = Windows::new();
    let client = capture::Client::new(8, 512, Default::default()).unwrap();
    let mut stream = capture::events(client.clone());
    let snapshot = block_on(async {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let snapshot = next(&mut stream).await;
            let ours: Vec<_> = snapshot
                .windows
                .iter()
                .filter(|w| w.app_id.starts_with("org.oxibar.preview."))
                .collect();
            if ours.len() == 2 && ours.iter().all(|w| w.image.is_some() && w.frames >= 4) {
                break snapshot;
            }
            assert!(Instant::now() < deadline, "{snapshot:?}");
        }
    });
    assert!(snapshot.staging_bytes <= super::pixels::STAGING_LIMIT);
    assert!(snapshot.in_flight <= 2);
    eprintln!("Native capture backend: {}", snapshot.backend);
    let ours: Vec<_> = snapshot
        .windows
        .iter()
        .filter(|w| w.app_id.starts_with("org.oxibar.preview."))
        .collect();
    assert_eq!(ours[0].title, ours[1].title);
    assert_ne!(ours[0].id, ours[1].id);
    for window in &ours {
        let iced::widget::image::Handle::Rgba { pixels, .. } = window.image.as_ref().unwrap()
        else {
            panic!("Expected captured RGBA")
        };
        let red = pixels
            .chunks_exact(4)
            .any(|p| p[0] > 240 && p[1] < 15 && p[2] < 15);
        let blue = pixels
            .chunks_exact(4)
            .any(|p| p[2] > 240 && p[1] < 15 && p[0] < 15);
        assert_eq!(
            (red, blue),
            if window.app_id.ends_with('0') {
                (true, false)
            } else {
                (false, true)
            },
            "{}: first pixels {:?}",
            window.app_id,
            &pixels[..pixels.len().min(40)]
        );
    }
    let id = ours[0].id;
    client.metadata(crate::compositor::read_snapshot().unwrap());
    block_on(async {
        loop {
            let state = next(&mut stream).await;
            if state
                .windows
                .iter()
                .any(|w| w.id == id && w.activatable && !w.outputs.is_empty())
            {
                break;
            }
        }
    });
    assert!(block_on(client.activate(id)).is_ok());
    let (_, latest) = block_on(async {
        iced::futures::join!(client.activate(ours[0].id), client.activate(ours[1].id))
    });
    assert!(latest.is_ok());
    let clients = std::process::Command::new("hyprctl")
        .args(["-j", "clients"])
        .output()
        .unwrap();
    let clients: serde_json::Value = serde_json::from_slice(&clients.stdout).unwrap();
    let addresses: Vec<_> = clients
        .as_array()
        .unwrap()
        .iter()
        .filter(|w| {
            w["class"]
                .as_str()
                .is_some_and(|class| class.starts_with("org.oxibar.preview."))
        })
        .map(|w| w["address"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(addresses.len(), 2);
    // Only the owned fixture compositor is touched. Both windows overlap before one moves off-workspace.
    if !crate::compositor::read_snapshot().unwrap().lua_dispatch {
        for address in &addresses {
            for command in [
                format!("setfloating address:{address}"),
                format!("resizewindowpixel exact 640 480,address:{address}"),
                format!("movewindowpixel exact 64 64,address:{address}"),
            ] {
                assert!(
                    std::process::Command::new("hyprctl")
                        .args(["dispatch", &command])
                        .status()
                        .unwrap()
                        .success()
                );
            }
        }
        let command = format!("movetoworkspacesilent 99,address:{}", addresses[0]);
        assert!(
            std::process::Command::new("hyprctl")
                .args(["dispatch", &command])
                .status()
                .unwrap()
                .success()
        );
    }
    windows.changed.store(true, Ordering::Release);
    block_on(async {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let state = next(&mut stream).await;
            let ours: Vec<_> = state
                .windows
                .iter()
                .filter(|w| w.app_id.starts_with("org.oxibar.preview."))
                .collect();
            let changed = ours.len() == 2
                && ours.iter().all(|w| {
                    let Some(iced::widget::image::Handle::Rgba { pixels, .. }) = &w.image else {
                        return false;
                    };
                    pixels.chunks_exact(4).any(|p| {
                        if w.app_id.ends_with('0') {
                            p[2] > 240 && p[1] < 15 && p[0] < 15
                        } else {
                            p[0] > 240 && p[1] < 15 && p[2] < 15
                        }
                    })
                });
            if changed {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "Occluded/off-workspace content failed to update: {state:?}"
            );
        }
    });
    drop(windows);
    block_on(async {
        loop {
            let snapshot = next(&mut stream).await;
            if !snapshot.windows.iter().any(|w| w.id == id) {
                break;
            }
        }
    });
    assert!(block_on(client.activate(id)).is_err());
    let started = Instant::now();
    drop(stream);
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
#[ignore = "Owned overview GUI fixture process; requires explicit private fixture directory"]
fn private_overview_window_fixture() {
    assert_eq!(std::env::var("OXIBAR_TEST_ISOLATED").as_deref(), Ok("1"));
    let Some(directory) = std::env::var_os("OXIBAR_TEST_OVERVIEW_FIXTURE") else {
        return;
    };
    let directory = std::path::PathBuf::from(directory);
    assert!(directory.is_dir());
    let windows = Windows::new();
    std::fs::write(directory.join("ready"), std::process::id().to_string()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(120);
    while !directory.join("stop").exists() && Instant::now() < deadline {
        windows
            .changed
            .store(directory.join("change").exists(), Ordering::Release);
        std::thread::sleep(Duration::from_millis(25));
    }
    drop(windows);
}
