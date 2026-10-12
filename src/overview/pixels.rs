use wayland_client::{WEnum, protocol::wl_shm};

pub(super) const STAGING_LIMIT: usize = 64 * 1024 * 1024;
pub(super) const CACHE_LIMIT: usize = 96 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Shape {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: wl_shm::Format,
}

impl Shape {
    pub fn new(
        width: u32,
        height: u32,
        stride: u32,
        format: WEnum<wl_shm::Format>,
    ) -> Result<Self, String> {
        let WEnum::Value(format) = format else {
            return Err("Unknown capture pixel format".into());
        };
        if !matches!(
            format,
            wl_shm::Format::Argb8888
                | wl_shm::Format::Xrgb8888
                | wl_shm::Format::Abgr8888
                | wl_shm::Format::Xbgr8888
        ) {
            return Err("No supported 32-bit shared-memory capture format".into());
        }
        let shape = Self {
            width,
            height,
            stride,
            format,
        };
        if width == 0
            || height == 0
            || width > 16384
            || height > 16384
            || stride < width.checked_mul(4).ok_or("Capture width overflow")?
            || shape.bytes()? > STAGING_LIMIT
        {
            return Err("Window exceeds the shared-memory capture budget".into());
        }
        Ok(shape)
    }
    pub fn bytes(self) -> Result<usize, String> {
        (self.stride as usize)
            .checked_mul(self.height as usize)
            .ok_or_else(|| "Capture size overflow".into())
    }
    pub fn thumbnail(
        self,
        bytes: &[u8],
        maximum: u32,
        inverted: bool,
        transform: u32,
    ) -> Result<(u32, u32, Vec<u8>), String> {
        if bytes.len() < self.bytes()? || transform > 7 {
            return Err("Invalid captured buffer".into());
        }
        let rotated = transform % 2 == 1;
        let (sw, sh) = if rotated {
            (self.height, self.width)
        } else {
            (self.width, self.height)
        };
        let ratio = (maximum.clamp(64, 640) as f64 / sw.max(sh) as f64).min(1.0);
        let (width, height) = (
            (sw as f64 * ratio).round().max(1.0) as u32,
            (sh as f64 * ratio).round().max(1.0) as u32,
        );
        let mut pixels = vec![0; width as usize * height as usize * 4];
        for y in 0..height {
            for x in 0..width {
                let (u, v) = (x * sw / width, y * sh / height);
                let (mut sx, mut sy) = match transform % 4 {
                    0 => (u, v),
                    1 => (self.width - 1 - v, u),
                    2 => (self.width - 1 - u, self.height - 1 - v),
                    _ => (v, self.height - 1 - u),
                };
                if transform >= 4 {
                    sx = self.width - 1 - sx
                }
                if inverted {
                    sy = self.height - 1 - sy
                }
                let offset = sy as usize * self.stride as usize + sx as usize * 4;
                let source = &bytes[offset..offset + 4];
                let mut rgba = if matches!(
                    self.format,
                    wl_shm::Format::Abgr8888 | wl_shm::Format::Xbgr8888
                ) {
                    [source[0], source[1], source[2], 255]
                } else {
                    [source[2], source[1], source[0], 255]
                };
                if matches!(
                    self.format,
                    wl_shm::Format::Argb8888 | wl_shm::Format::Abgr8888
                ) {
                    let alpha = source[3];
                    for channel in &mut rgba[..3] {
                        *channel = if alpha == 0 {
                            0
                        } else {
                            ((*channel as u32 * 255 / alpha as u32).min(255)) as u8
                        };
                    }
                    rgba[3] = alpha;
                }
                let target = (y as usize * width as usize + x as usize) * 4;
                pixels[target..target + 4].copy_from_slice(&rgba);
            }
        }
        Ok((width, height, pixels))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capture_pixels_preserve_stride_orientation_and_budget() {
        let shape = Shape::new(2, 2, 12, WEnum::Value(wl_shm::Format::Xrgb8888)).unwrap();
        let pixels = [
            0, 0, 255, 0, 0, 255, 0, 0, 9, 9, 9, 9, 255, 0, 0, 0, 255, 255, 255, 0, 9, 9, 9, 9,
        ];
        let (_, _, rgba) = shape.thumbnail(&pixels, 64, false, 0).unwrap();
        assert_eq!(&rgba[..8], &[255, 0, 0, 255, 0, 255, 0, 255]);
        assert_eq!(
            &shape.thumbnail(&pixels, 64, true, 0).unwrap().2[..4],
            &[0, 0, 255, 255]
        );
        assert_eq!(
            &shape.thumbnail(&pixels, 64, false, 1).unwrap().2[..4],
            &[0, 255, 0, 255]
        );
        assert!(Shape::new(10000, 10000, 40000, WEnum::Value(wl_shm::Format::Argb8888)).is_err());
        assert!(Shape::new(2, 2, 7, WEnum::Value(wl_shm::Format::Argb8888)).is_err());
        assert!(shape.thumbnail(&pixels[..10], 64, false, 0).is_err());
        assert!(shape.thumbnail(&pixels, 64, false, 8).is_err());
    }
}
