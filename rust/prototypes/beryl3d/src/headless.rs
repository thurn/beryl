//! Drawing with no window: the same renderer into a texture, and pictures read back from it.

use std::path::Path;

use crate::faces::FaceImages;
use crate::render::Renderer;

pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// A device on the default adapter. With `limits`, the device is created with those limits,
/// as a check that everything the renderer does fits them.
pub fn device(limits: Option<wgpu::Limits>) -> (wgpu::Adapter, wgpu::Device, wgpu::Queue, bool) {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    }))
    .expect("a GPU adapter");
    let timestamps =
        limits.is_none() && adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: None,
        required_features: if timestamps {
            wgpu::Features::TIMESTAMP_QUERY
        } else {
            wgpu::Features::empty()
        },
        required_limits: limits.unwrap_or_default(),
        ..Default::default()
    }))
    .expect("a GPU device");
    (adapter, device, queue, timestamps)
}

pub struct Offscreen {
    pub renderer: Renderer,
    pub target: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub width: u32,
    pub height: u32,
}

impl Offscreen {
    pub fn new(
        width: u32,
        height: u32,
        faces: &FaceImages,
        limits: Option<wgpu::Limits>,
    ) -> Offscreen {
        let (_adapter, device, queue, timestamps) = device(limits);
        Offscreen::with_device(device, queue, width, height, faces, timestamps)
    }

    pub fn with_device(
        device: wgpu::Device,
        queue: wgpu::Queue,
        width: u32,
        height: u32,
        faces: &FaceImages,
        timestamps: bool,
    ) -> Offscreen {
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let renderer = Renderer::new(device, queue, FORMAT, faces, timestamps);
        Offscreen {
            renderer,
            target,
            view,
            width,
            height,
        }
    }

    /// Writes what was last drawn to a PNG file.
    pub fn png(&self, path: &Path) {
        let device = &self.renderer.device;
        let row = (self.width * 4).div_ceil(256) * 256;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(row * self.height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
        self.renderer.queue.submit([encoder.finish()]);
        buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("the GPU finishes");
        let mapped = buffer
            .slice(..)
            .get_mapped_range()
            .expect("the buffer maps");
        let mut pixels = Vec::with_capacity((self.width * self.height * 4) as usize);
        for y in 0..self.height {
            let start = (y * row) as usize;
            pixels.extend_from_slice(&mapped[start..start + (self.width * 4) as usize]);
        }
        image::save_buffer(
            path,
            &pixels,
            self.width,
            self.height,
            image::ColorType::Rgba8,
        )
        .expect("the picture is written");
    }
}
