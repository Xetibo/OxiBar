use std::fs::File;

use iced::{Color, Rectangle, Size};
use iced_tiny_skia::graphics::Viewport;
use memmap2::{MmapMut, MmapOptions};
use wayland_client::{
    Proxy, QueueHandle,
    protocol::{wl_buffer, wl_compositor, wl_pointer, wl_shm, wl_shm_pool, wl_surface},
};

use crate::backend::{Error, State};

const MAX_BUFFER_BYTES: usize = 256 * 1024 * 1024;

pub(crate) struct Buffer {
    pub proxy: wl_buffer::WlBuffer,
    pub busy: bool,
    map: MmapMut,
    pixels: Vec<u8>,
    mask: tiny_skia::Mask,
    size: (u32, u32),
}

impl Buffer {
    pub fn new(
        shm: &wl_shm::WlShm,
        qh: &QueueHandle<State>,
        output: u32,
        size: (u32, u32),
    ) -> Result<Self, Error> {
        let bytes = (size.0 as usize)
            .checked_mul(size.1 as usize)
            .and_then(|n| n.checked_mul(4))
            .filter(|&n| n > 0 && n <= MAX_BUFFER_BYTES)
            .ok_or_else(|| std::io::Error::other("Lock surface exceeds rendering limits"))?;
        let file = File::from(rustix::fs::memfd_create(
            c"oxilock",
            rustix::fs::MemfdFlags::CLOEXEC,
        )?);
        file.set_len(bytes as u64)?;
        // The private memfd remains this length; no other client can truncate it.
        let map = unsafe { MmapOptions::new().len(bytes).map_mut(&file)? };
        use std::os::fd::AsFd;
        let pool = shm.create_pool(file.as_fd(), bytes as i32, qh, ());
        let proxy = pool.create_buffer(
            0,
            size.0 as i32,
            size.1 as i32,
            size.0 as i32 * 4,
            wl_shm::Format::Xrgb8888,
            qh,
            output,
        );
        pool.destroy();
        let mask = tiny_skia::Mask::new(size.0, size.1)
            .ok_or_else(|| std::io::Error::other("Cannot allocate lock mask"))?;
        Ok(Self {
            proxy,
            busy: false,
            map,
            pixels: vec![0; bytes],
            mask,
            size,
        })
    }

    pub fn draw(
        &mut self,
        renderer: &mut iced::Renderer,
        scale: f64,
        background: Color,
    ) -> Result<(), Error> {
        let iced::Renderer::Secondary(renderer) = renderer else {
            return Err(
                std::io::Error::other("Lock renderer must use the audited CPU path").into(),
            );
        };
        let viewport =
            Viewport::with_physical_size(Size::new(self.size.0, self.size.1), scale as f32);
        let mut pixels =
            tiny_skia::PixmapMut::from_bytes(&mut self.pixels, self.size.0, self.size.1)
                .ok_or_else(|| std::io::Error::other("Invalid lock pixel map"))?;
        renderer.draw(
            &mut pixels,
            &mut self.mask,
            &viewport,
            &[Rectangle::with_size(Size::new(
                self.size.0 as f32,
                self.size.1 as f32,
            ))],
            Color {
                a: 1.0,
                ..background
            },
        );
        for (source, target) in self
            .pixels
            .chunks_exact(4)
            .zip(self.map.chunks_exact_mut(4))
        {
            // Iced's CPU renderer emits BGRA; Wayland XRGB uses native-endian words.
            let pixel = 0xff000000
                | (u32::from(source[2]) << 16)
                | (u32::from(source[1]) << 8)
                | u32::from(source[0]);
            target.copy_from_slice(&pixel.to_ne_bytes());
        }
        Ok(())
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        if self.proxy.is_alive() {
            self.proxy.destroy();
        }
    }
}

pub(crate) struct Cursor {
    surface: wl_surface::WlSurface,
    buffer: wl_buffer::WlBuffer,
    _map: MmapMut,
}
impl Cursor {
    pub fn new(
        compositor: &wl_compositor::WlCompositor,
        shm: &wl_shm::WlShm,
        qh: &QueueHandle<State>,
    ) -> Result<Self, Error> {
        const WIDTH: usize = 16;
        const HEIGHT: usize = 24;
        let file = File::from(rustix::fs::memfd_create(
            c"oxilock-cursor",
            rustix::fs::MemfdFlags::CLOEXEC,
        )?);
        file.set_len((WIDTH * HEIGHT * 4) as u64)?;
        // Static cursor pixels are never rewritten while the compositor uses them.
        let mut map = unsafe { MmapOptions::new().map_mut(&file)? };
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let edge = y < 16 && x <= y / 2;
                let stem = (12..22).contains(&y) && (4..7).contains(&x);
                let pixel: u32 = if (edge && (x == 0 || x == y / 2 || y == 15))
                    || (stem && (x == 4 || x == 6 || y == 21))
                {
                    0xff000000
                } else if edge || stem {
                    0xffffffff
                } else {
                    0
                };
                let offset = (y * WIDTH + x) * 4;
                map[offset..offset + 4].copy_from_slice(&pixel.to_ne_bytes());
            }
        }
        use std::os::fd::AsFd;
        let pool = shm.create_pool(file.as_fd(), map.len() as i32, qh, ());
        let buffer = pool.create_buffer(
            0,
            WIDTH as i32,
            HEIGHT as i32,
            WIDTH as i32 * 4,
            wl_shm::Format::Argb8888,
            qh,
            (),
        );
        pool.destroy();
        Ok(Self {
            surface: compositor.create_surface(qh, ()),
            buffer,
            _map: map,
        })
    }
    pub fn show(&self, pointer: &wl_pointer::WlPointer, serial: u32) {
        pointer.set_cursor(serial, Some(&self.surface), 0, 0);
        self.surface.attach(Some(&self.buffer), 0, 0);
        self.surface.damage(0, 0, 16, 24);
        self.surface.commit();
    }
}
impl Drop for Cursor {
    fn drop(&mut self) {
        self.surface.destroy();
        self.buffer.destroy();
    }
}

wayland_client::delegate_noop!(State: ignore wl_shm_pool::WlShmPool);
wayland_client::delegate_noop!(State: ignore wl_buffer::WlBuffer);
