//! The atlas (docs/beryl-3d.md, "The frame": three texture arrays): a 2D array texture of
//! single-channel pages, each `PAGE_SIZE` texels square, holding coverage. The glyph worker in
//! `beryl_assets` fills pages from page 0 up (`Renderer::upload_page`), and the last page holds
//! the shapes the renderer draws itself, such as the soft disc of a glow.

use beryl_assets::packer::PAGE_SIZE;
use beryl_assets::{Prepared, Upload};
use beryl_scene::frame::Sprite;

/// How many pages the atlas holds: `GLYPH_PAGES` for glyphs and one for shapes, 1 MB each.
const PAGES: u32 = 8;
/// How many pages glyphs may fill. The printable ASCII characters of every type style fill
/// about one.
pub const GLYPH_PAGES: u16 = PAGES as u16 - 1;
/// The page that holds the renderer's own shapes.
const SHAPES_PAGE: u16 = GLYPH_PAGES;
/// The side of the glow's disc, in texels.
const DISC: u16 = 128;
/// The soft disc that a glow draws, at the top left of the shapes page.
pub const GLOW_SPRITE: Sprite = Sprite {
    page: SHAPES_PAGE,
    rect: [0, 0, DISC, DISC],
};

/// The atlas texture and the view the shader samples.
pub(crate) struct Atlas {
    texture: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
}

impl Atlas {
    /// An atlas whose glyph pages are empty and whose shapes page holds the glow's disc.
    pub(crate) fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let side = u32::from(PAGE_SIZE);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("atlas"),
            size: wgpu::Extent3d {
                width: side,
                height: side,
                depth_or_array_layers: PAGES,
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
        let atlas = Self { texture, view };
        atlas.write(queue, SHAPES_PAGE, &disc(), u32::from(DISC));
        atlas
    }

    /// Writes a glyph page into its layer. Returns whether it was written: anything but a glyph
    /// page, a page past `GLYPH_PAGES` or one of the wrong size is not, with a log line.
    pub(crate) fn upload(&self, queue: &wgpu::Queue, prepared: &Prepared) -> bool {
        let Upload::GlyphPage { page, pixels } = &prepared.upload else {
            return false;
        };
        let side = usize::from(PAGE_SIZE);
        if *page >= GLYPH_PAGES || pixels.len() != side * side {
            eprintln!(
                "beryl_render: glyph page {page} of {} bytes does not fit the atlas's {GLYPH_PAGES} \
                 pages of {side} by {side}",
                pixels.len()
            );
            return false;
        }
        self.write(queue, *page, pixels, u32::from(PAGE_SIZE));
        true
    }

    /// Writes `pixels`, `side` texels square, at the top left of page `page`.
    fn write(&self, queue: &wgpu::Queue, page: u16, pixels: &[u8], side: u32) {
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

/// The disc of `GLOW_SPRITE`: full coverage at the centre, falling to nothing at the edge.
fn disc() -> Vec<u8> {
    let size = u32::from(DISC);
    let mut pixels = Vec::with_capacity((size * size) as usize);
    for y in 0..size {
        for x in 0..size {
            let dx = (x as f32 + 0.5) / size as f32 * 2.0 - 1.0;
            let dy = (y as f32 + 0.5) / size as f32 * 2.0 - 1.0;
            let distance = (dx * dx + dy * dy).sqrt().min(1.0);
            let coverage = (1.0 - distance) * (1.0 - distance);
            pixels.push((coverage * 255.0).round() as u8);
        }
    }
    pixels
}
