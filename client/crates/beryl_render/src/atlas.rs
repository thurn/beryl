//! The atlas (docs/beryl-3d.md, "The frame": three texture arrays): a 2D array texture of
//! single-channel pages, each `PAGE_SIZE` texels square, holding coverage. Page 0 holds the
//! shapes of light (`beryl_assets::light`), which a host uploads before its first frame, and the
//! glyph worker in `beryl_assets` fills the pages after it with glyphs and icons. Both arrive
//! through `Renderer::upload_page`.

pub use beryl_assets::packer::ATLAS_PAGES;
use beryl_assets::packer::PAGE_SIZE;
use beryl_assets::{Prepared, Upload};

/// The number of pages held at startup. Only requests for new pages can grow the texture.
const INITIAL_PAGES: u32 = 8;

/// The atlas texture and the view the shader samples.
pub(crate) struct Atlas {
    texture: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
}

impl Atlas {
    /// An atlas whose pages are all empty.
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        Self::with_pages(device, INITIAL_PAGES)
    }

    fn with_pages(device: &wgpu::Device, pages: u32) -> Self {
        let side = u32::from(PAGE_SIZE);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("atlas"),
            size: wgpu::Extent3d {
                width: side,
                height: side,
                depth_or_array_layers: pages,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        Self { texture, view }
    }

    pub(crate) fn pages(&self) -> u32 {
        self.texture.depth_or_array_layers()
    }

    /// Writes a whole page, growing if needed. Returns whether the page fits the atlas format
    /// and the device's array limit.
    pub(crate) fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        prepared: &Prepared,
    ) -> bool {
        let Upload::AtlasPage { page, pixels } = &prepared.upload else {
            return false;
        };
        let side = usize::from(PAGE_SIZE);
        let limit = u32::from(ATLAS_PAGES).min(device.limits().max_texture_array_layers);
        let needed = u32::from(*page) + 1;
        if needed > limit || pixels.len() != side * side {
            eprintln!(
                "beryl_render: atlas page {page} of {} bytes does not fit the atlas's {limit} \
                 pages of {side} by {side}",
                pixels.len()
            );
            return false;
        }
        if needed > self.pages() {
            self.grow(device, queue, needed.next_power_of_two().min(limit));
        }
        self.write(queue, *page, pixels);
        true
    }

    fn grow(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, pages: u32) {
        let larger = Self::with_pages(device, pages);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("grow coverage atlas"),
        });
        // The client keeps the coordinates of every cell it has received. Copy their pages
        // to the same array layers before any later draw samples the larger atlas.
        encoder.copy_texture_to_texture(
            self.texture.as_image_copy(),
            larger.texture.as_image_copy(),
            self.texture.size(),
        );
        queue.submit([encoder.finish()]);
        *self = larger;
    }

    /// Writes `pixels`, a whole page, into page `page`.
    fn write(&self, queue: &wgpu::Queue, page: u16, pixels: &[u8]) {
        let side = u32::from(PAGE_SIZE);
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: 0,
                    y: 0,
                    z: u32::from(page),
                },
                aspect: wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(side),
                rows_per_image: Some(side),
            },
            wgpu::Extent3d {
                width: side,
                height: side,
                depth_or_array_layers: 1,
            },
        );
    }
}
