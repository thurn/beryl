//! The atlas (docs/beryl-3d.md, "The frame": three texture arrays): a 2D array texture of
//! single-channel pages, each `PAGE_SIZE` texels square, holding coverage. Page 0 holds the
//! shapes of light (`beryl_assets::light`), which a host uploads before its first frame, and the
//! glyph worker in `beryl_assets` fills the pages after it. Both arrive through
//! `Renderer::upload_page`.

use beryl_assets::packer::PAGE_SIZE;
use beryl_assets::{Prepared, Upload};

/// How many pages the atlas holds, 1 MB each: the shapes of light and seven of glyphs. The
/// printable ASCII characters of every type style fill about one.
pub const ATLAS_PAGES: u16 = 8;

/// The atlas texture and the view the shader samples.
pub(crate) struct Atlas {
    texture: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
}

impl Atlas {
    /// An atlas whose pages are all empty.
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let side = u32::from(PAGE_SIZE);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("atlas"),
            size: wgpu::Extent3d {
                width: side,
                height: side,
                depth_or_array_layers: u32::from(ATLAS_PAGES),
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        Self { texture, view }
    }

    /// Writes a whole page into its layer. Returns whether it was written: anything but a page,
    /// a page past `ATLAS_PAGES` or one of the wrong size is not, with a log line.
    pub(crate) fn upload(&self, queue: &wgpu::Queue, prepared: &Prepared) -> bool {
        let Upload::AtlasPage { page, pixels } = &prepared.upload else {
            return false;
        };
        let side = usize::from(PAGE_SIZE);
        if *page >= ATLAS_PAGES || pixels.len() != side * side {
            eprintln!(
                "beryl_render: atlas page {page} of {} bytes does not fit the atlas's {ATLAS_PAGES} \
                 pages of {side} by {side}",
                pixels.len()
            );
            return false;
        }
        self.write(queue, *page, pixels);
        true
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
