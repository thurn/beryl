//! What a frame is drawn into. An offscreen target is a texture whose pixels can be read back
//! as RGBA bytes and written as a PNG picture.

use std::path::Path;

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

    /// Writes `target`'s pixels to a PNG file at `path`.
    pub fn write_png(&self, target: &Target, path: &Path) -> Result<(), String> {
        image::save_buffer(
            path,
            &self.read(target),
            target.width,
            target.height,
            image::ColorType::Rgba8,
        )
        .map_err(|error| format!("{}: {error}", path.display()))
    }
}
