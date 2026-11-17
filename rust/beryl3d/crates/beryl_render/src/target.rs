//! What a frame is drawn into. An offscreen target is a texture whose pixels can be read back
//! as RGBA bytes and written as a PNG picture.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::renderer::Renderer;

/// A picture a frame is drawn into, `width` by `height` physical pixels.
pub struct Target {
    pub(crate) view: wgpu::TextureView,
    texture: wgpu::Texture,
    pub width: u32,
    pub height: u32,
}

impl Target {
    /// An offscreen target in the renderer's target format.
    #[must_use]
    pub fn offscreen(renderer: &Renderer, width: u32, height: u32) -> Self {
        let texture = renderer.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: renderer.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            view,
            texture,
            width,
            height,
        }
    }
}

impl Renderer {
    /// Waits for the GPU and returns `target`'s pixels, row by row from the top, four bytes
    /// each: red, green, blue and opacity.
    #[must_use]
    pub fn read(&self, target: &Target) -> Vec<u8> {
        let (width, height) = (target.width, target.height);
        // A copy's rows must start at multiples of 256 bytes.
        let row = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: u64::from(row * height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        encoder.copy_texture_to_buffer(
            target.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("the GPU finishes the copy");
        let mapped = slice.get_mapped_range().expect("the readback buffer maps");
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            let start = (y * row) as usize;
            pixels.extend_from_slice(&mapped[start..start + (width * 4) as usize]);
        }
        pixels
    }

    /// Starts copying `target`'s pixels to a buffer the CPU can map, and returns at once. The
    /// returned [`Readback`] gives the picture once the GPU has finished the copy, which a
    /// caller finds out by polling on a later frame, so that the frame thread never waits.
    #[must_use]
    pub fn read_back(&self, target: &Target) -> Readback {
        let (width, height) = (target.width, target.height);
        let row = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("live readback"),
            size: u64::from(row * height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        encoder.copy_texture_to_buffer(
            target.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);
        let mapped = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&mapped);
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                flag.store(result.is_ok(), Ordering::Release);
            });
        Readback {
            device: self.device.clone(),
            buffer,
            row,
            width,
            height,
            bgra: matches!(
                self.format,
                wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
            ),
            mapped,
        }
    }

    /// Writes `target`'s pixels to a PNG file at `path`.
    pub fn write_png(&self, target: &Target, path: &Path) -> Result<(), String> {
        write_rgba_png(path, &self.read(target), target.width, target.height)
    }
}

/// Writes `pixels`, `width` by `height` of RGBA bytes row by row from the top, to a PNG file at
/// `path`. It serves pictures built from several frames, which no target holds.
///
/// # Errors
///
/// When the file cannot be written; the message names it.
pub fn write_rgba_png(path: &Path, pixels: &[u8], width: u32, height: u32) -> Result<(), String> {
    image::save_buffer(path, pixels, width, height, image::ColorType::Rgba8)
        .map_err(|error| format!("{}: {error}", path.display()))
}

/// Reads the PNG file at `path` as RGBA bytes, row by row from the top, whatever its own
/// color type: `just b3d compare` puts the web interface's screenshot beside a frame with it.
///
/// # Errors
///
/// When the file cannot be read or decoded; the message names it.
pub fn read_rgba_png(path: &Path) -> Result<Picture, String> {
    let image = image::open(path)
        .map_err(|error| format!("{}: {error}", path.display()))?
        .into_rgba8();
    let (width, height) = image.dimensions();
    Ok(Picture {
        pixels: image.into_raw(),
        width,
        height,
    })
}

/// A picture read back from the GPU: `width` by `height` pixels of RGBA bytes, row by row from
/// the top.
pub struct Picture {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl Picture {
    /// Writes the picture to a PNG file at `path`.
    ///
    /// # Errors
    ///
    /// When the file cannot be written; the message names it.
    pub fn write_png(&self, path: &Path) -> Result<(), String> {
        write_rgba_png(path, &self.pixels, self.width, self.height)
    }

    /// The picture as the bytes of a PNG file.
    ///
    /// # Errors
    ///
    /// When the picture cannot be encoded.
    pub fn png(&self) -> Result<Vec<u8>, String> {
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::write_buffer_with_format(
            &mut bytes,
            &self.pixels,
            self.width,
            self.height,
            image::ColorType::Rgba8,
            image::ImageFormat::Png,
        )
        .map_err(|error| error.to_string())?;
        Ok(bytes.into_inner())
    }

    /// The part of the picture `width` by `height` pixels from (`x`, `y`), cut to the picture's
    /// edges.
    #[must_use]
    pub fn crop(&self, x: u32, y: u32, width: u32, height: u32) -> Picture {
        let x = x.min(self.width);
        let y = y.min(self.height);
        let width = width.min(self.width - x);
        let height = height.min(self.height - y);
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for row in y..y + height {
            let start = ((row * self.width + x) * 4) as usize;
            pixels.extend_from_slice(&self.pixels[start..start + (width * 4) as usize]);
        }
        Picture {
            pixels,
            width,
            height,
        }
    }
}

/// A copy of a target's pixels on its way to the CPU (`Renderer::read_back`).
pub struct Readback {
    device: wgpu::Device,
    buffer: wgpu::Buffer,
    /// Bytes from the start of one row to the next in the buffer.
    row: u32,
    width: u32,
    height: u32,
    /// Whether the target stores blue first, as a window's surface may.
    bgra: bool,
    mapped: Arc<AtomicBool>,
}

impl Readback {
    /// The picture, once the GPU has finished the copy, and `None` before. It never waits.
    #[must_use]
    pub fn poll(&self) -> Option<Picture> {
        // Lets wgpu run the mapping's callback if the copy has finished.
        let _ = self.device.poll(wgpu::PollType::Poll);
        if !self.mapped.load(Ordering::Acquire) {
            return None;
        }
        let range = self
            .buffer
            .slice(..)
            .get_mapped_range()
            .expect("the readback buffer is mapped");
        let mut pixels = Vec::with_capacity((self.width * self.height * 4) as usize);
        for y in 0..self.height {
            let start = (y * self.row) as usize;
            pixels.extend_from_slice(&range[start..start + (self.width * 4) as usize]);
        }
        if self.bgra {
            for pixel in pixels.as_chunks_mut::<4>().0 {
                pixel.swap(0, 2);
            }
        }
        Some(Picture {
            pixels,
            width: self.width,
            height: self.height,
        })
    }
}
