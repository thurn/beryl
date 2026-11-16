//! The card face arrays (docs/beryl-3d.md, "Card faces"): a board-size array with a layer for
//! every face of the match, and a full-size array of `FULL_LAYERS` layers for the cards being
//! read, both in the first compressed format the device samples.
//!
//! Until a face's layer arrives from `beryl_assets`' workers, the board-size array shows a
//! generated placeholder in that layer. The renderer has no texture encoder and the frame thread
//! must never run one, so the placeholder is made of shapes on the 4 by 4 pixel grid of a
//! compressed block, and each block is written as one solid color, which every format can say in
//! a few fixed bits.
//!
//! During play two kinds of upload remain, and both are paced (`Waiting`): a board-size face that
//! arrives late is uploaded only in a frame in which nothing animates, one layer a frame, and at
//! most one full-size face is uploaded a frame.

use std::collections::VecDeque;

use beryl_assets::{FULL_LAYERS, Format, Layer, Prepared, Upload, dimensions};
use beryl_scene::frame::Wake;
use beryl_scene::input::{AssetReady, FaceSize};

use crate::textures::{back, face};

/// The device features that let it sample a compressed format.
pub(crate) const COMPRESSION: wgpu::Features = wgpu::Features::TEXTURE_COMPRESSION_ASTC
    .union(wgpu::Features::TEXTURE_COMPRESSION_ETC2)
    .union(wgpu::Features::TEXTURE_COMPRESSION_BC);

/// The side of a compressed block, in pixels, and its size in bytes in all three formats.
const BLOCK: u32 = 4;
const BLOCK_BYTES: usize = 16;

/// The compressed formats a device with `features` samples, in the order they are preferred.
pub(crate) fn supported(features: wgpu::Features) -> Vec<Format> {
    Format::COMPRESSED
        .into_iter()
        .filter(|format| features.contains(feature(*format)))
        .collect()
}

fn feature(format: Format) -> wgpu::Features {
    match format {
        Format::Astc4x4 => wgpu::Features::TEXTURE_COMPRESSION_ASTC,
        Format::Etc2Rgba8 => wgpu::Features::TEXTURE_COMPRESSION_ETC2,
        Format::Bc7 => wgpu::Features::TEXTURE_COMPRESSION_BC,
        Format::Rgba8 => wgpu::Features::empty(),
    }
}

/// The texture format of an array in `format`. A face's bytes hold sRGB values, but they are
/// sampled as stored, without conversion, as every other color is (`Renderer::headless`).
fn texture_format(format: Format) -> wgpu::TextureFormat {
    match format {
        Format::Astc4x4 => wgpu::TextureFormat::Astc {
            block: wgpu::AstcBlock::B4x4,
            channel: wgpu::AstcChannel::Unorm,
        },
        Format::Etc2Rgba8 => wgpu::TextureFormat::Etc2Rgba8Unorm,
        Format::Bc7 => wgpu::TextureFormat::Bc7RgbaUnorm,
        Format::Rgba8 => wgpu::TextureFormat::Rgba8Unorm,
    }
}

/// One mipmap level's shape: its size in texels as the copy names it (whole blocks when
/// compressed), and its bytes per row of texels or blocks and number of such rows.
struct Level {
    width: u32,
    height: u32,
    bytes_per_row: u32,
    rows: u32,
}

impl Level {
    fn of(format: Format, (width, height): (u32, u32), level: u32) -> Self {
        let width = (width >> level).max(1);
        let height = (height >> level).max(1);
        if format == Format::Rgba8 {
            return Self {
                width,
                height,
                bytes_per_row: width * 4,
                rows: height,
            };
        }
        let (across, down) = (width.div_ceil(BLOCK), height.div_ceil(BLOCK));
        Self {
            width: across * BLOCK,
            height: down * BLOCK,
            bytes_per_row: across * BLOCK_BYTES as u32,
            rows: down,
        }
    }

    fn bytes(&self) -> usize {
        (self.bytes_per_row * self.rows) as usize
    }
}

/// One face array: a texture of `layers` layers at one face size in one format, with every
/// mipmap level down to 1 by 1, as `beryl_assets` makes a face.
pub(crate) struct FaceArray {
    texture: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
    format: Format,
    size: (u32, u32),
    levels: u32,
}

impl FaceArray {
    /// An array of `layers` layers (at least two, because OpenGL cannot tell a one-layer array
    /// from a plain texture), `size` texels large, whose contents start as zeros.
    pub(crate) fn new(
        device: &wgpu::Device,
        format: Format,
        size: (u32, u32),
        layers: u32,
    ) -> Self {
        let levels = u32::BITS - size.0.max(size.1).leading_zeros();
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("faces"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: layers.max(2),
            },
            mip_level_count: levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: texture_format(format),
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        Self {
            texture,
            view,
            format,
            size,
            levels,
        }
    }

    /// Writes `layer`'s levels into layer `index`. Logs and writes nothing when the layer is of
    /// another format or size than the array, or the array has no such layer.
    pub(crate) fn write(&self, queue: &wgpu::Queue, index: u16, layer: &Layer) -> bool {
        let fits = layer.format == self.format
            && (layer.width, layer.height) == self.size
            && u32::from(index) < self.texture.depth_or_array_layers();
        if !fits {
            eprintln!(
                "beryl_render: a {} face {}x{} does not fit layer {index} of the {} {}x{} array",
                layer.format.name(),
                layer.width,
                layer.height,
                self.format.name(),
                self.size.0,
                self.size.1,
            );
            return false;
        }
        self.write_levels(queue, index, &layer.levels)
    }

    fn write_levels(&self, queue: &wgpu::Queue, index: u16, levels: &[Vec<u8>]) -> bool {
        for (level, bytes) in (0..self.levels).zip(levels) {
            let shape = Level::of(self.format, self.size, level);
            if bytes.len() != shape.bytes() {
                eprintln!(
                    "beryl_render: level {level} of a face has {} bytes, not {}",
                    bytes.len(),
                    shape.bytes()
                );
                return false;
            }
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.texture,
                    mip_level: level,
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: u32::from(index),
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                bytes,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(shape.bytes_per_row),
                    rows_per_image: Some(shape.rows),
                },
                wgpu::Extent3d {
                    width: shape.width,
                    height: shape.height,
                    depth_or_array_layers: 1,
                },
            );
        }
        true
    }

    /// Fills layer `index` with the placeholder `pixels`, RGBA at the array's size.
    fn placeholder(&self, queue: &wgpu::Queue, index: u16, pixels: &[u8]) {
        let levels: Vec<Vec<u8>> = (0..self.levels)
            .map(|level| solid_level(pixels, self.size, self.format, level))
            .collect();
        self.write_levels(queue, index, &levels);
    }
}

/// The two face arrays of a match whose faces are drawn on placeholders named `names`, in
/// `FaceId` order: the board-size array with one layer per name, each showing its placeholder
/// (layer 0, the card back's, shows the placeholder back), and the full-size array of
/// `FULL_LAYERS` layers, empty until faces are uploaded into it.
pub(crate) fn face_arrays(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    names: &[&str],
    format: Format,
) -> [FaceArray; 2] {
    let size = dimensions(FaceSize::Board);
    let layers = device.limits().max_texture_array_layers;
    if names.len() > layers as usize {
        eprintln!(
            "beryl_render: the match has {} faces but the device allows {layers} layers; the rest \
             draw from layer 0",
            names.len()
        );
    }
    let count = (names.len() as u32).clamp(1, layers);
    let board = FaceArray::new(device, format, size, count);
    for (index, name) in (0..count).zip(names.iter().copied().chain(std::iter::repeat(""))) {
        let pixels = if index == 0 {
            back(size)
        } else {
            face(name, size)
        };
        board.placeholder(queue, index as u16, &pixels);
    }
    let full = FaceArray::new(
        device,
        format,
        dimensions(FaceSize::Full),
        FULL_LAYERS as u32,
    );
    [board, full]
}

/// The face arrays of a renderer with no match yet: the board-size array holds the placeholder
/// back alone, uncompressed, and the full-size array is a token one that no face fits.
pub(crate) fn empty_arrays(device: &wgpu::Device, queue: &wgpu::Queue) -> [FaceArray; 2] {
    let size = dimensions(FaceSize::Board);
    let board = FaceArray::new(device, Format::Rgba8, size, 1);
    board.placeholder(queue, 0, &back(size));
    [
        board,
        FaceArray::new(device, Format::Rgba8, (BLOCK, BLOCK), 1),
    ]
}

/// Level `level` of the placeholder `pixels`, RGBA `size` texels large, in `format`: each texel,
/// or each block of a compressed format, takes the color of the full-size pixel at its centre.
fn solid_level(pixels: &[u8], size: (u32, u32), format: Format, level: u32) -> Vec<u8> {
    let shape = Level::of(format, size, level);
    let step = if format == Format::Rgba8 { 1 } else { BLOCK };
    let mut bytes = Vec::with_capacity(shape.bytes());
    for y in (0..shape.height).step_by(step as usize) {
        for x in (0..shape.width).step_by(step as usize) {
            let centre =
                |at: u32, side: u32| ((at << level) + ((step << level) >> 1)).min(side - 1);
            let i = ((centre(y, size.1) * size.0 + centre(x, size.0)) * 4) as usize;
            let rgb = [pixels[i], pixels[i + 1], pixels[i + 2]];
            match format {
                Format::Rgba8 => bytes.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]),
                _ => bytes.extend_from_slice(&solid_block(format, rgb)),
            }
        }
    }
    bytes
}

/// One opaque 4 by 4 block of the single color `rgb` in a compressed format.
fn solid_block(format: Format, [r, g, b]: [u8; 3]) -> [u8; BLOCK_BYTES] {
    let mut block = [0; BLOCK_BYTES];
    match format {
        // A void-extent block (ASTC specification, "Void-Extent Blocks"): block mode 0x1FC,
        // LDR, the two reserved bits set and every extent coordinate all ones, then the color as
        // four 16-bit UNORM values.
        Format::Astc4x4 => {
            block[..8].copy_from_slice(&[0xFC, 0xFD, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
            for (at, value) in [r, g, b, 255].into_iter().enumerate() {
                let unorm = u16::from(value) * 257;
                block[8 + at * 2..10 + at * 2].copy_from_slice(&unorm.to_le_bytes());
            }
        }
        // EAC alpha, then an ETC1 block in individual mode (Khronos Data Format, "ETC2"). The
        // alpha's base is 255, with multiplier 1 and table 0, and every pixel index 4 adds 2,
        // which stays 255. The color is 4 bits a channel for both halves, and every pixel index
        // 0 adds table 0's smallest modifier, 2.
        Format::Etc2Rgba8 => {
            block[..8].copy_from_slice(&[255, 0x10, 0x92, 0x49, 0x24, 0x92, 0x49, 0x24]);
            for (at, value) in [r, g, b].into_iter().enumerate() {
                let four = ((u16::from(value.saturating_sub(2)) * 15 + 127) / 255) as u8;
                block[8 + at] = four << 4 | four;
            }
        }
        // BC7 mode 6 (Khronos Data Format, "BC7"): both endpoints the same color in 7 bits a
        // channel with a shared low bit of 1, and every index 0.
        Format::Bc7 => {
            let mut bits: u128 = 1 << 6;
            let mut at = 7;
            for value in [r, r, g, g, b, b, 255, 255] {
                bits |= u128::from(value >> 1) << at;
                at += 7;
            }
            bits |= 0b11 << 63;
            block = bits.to_le_bytes();
        }
        Format::Rgba8 => unreachable!("an uncompressed format has no blocks"),
    }
    block
}

/// Faces that arrived during play, waiting for a frame in which they may be uploaded.
#[derive(Default)]
pub(crate) struct Waiting {
    board: VecDeque<Prepared>,
    full: VecDeque<Prepared>,
}

impl Waiting {
    /// Holds `prepared` until `next` lets it through. Anything but a face is dropped.
    pub(crate) fn push(&mut self, prepared: Prepared) {
        match prepared.ready {
            AssetReady::Face {
                size: FaceSize::Board,
                ..
            } => self.board.push_back(prepared),
            AssetReady::Face {
                size: FaceSize::Full,
                ..
            } => self.full.push_back(prepared),
            AssetReady::Glyphs { .. } | AssetReady::Icons { .. } => {}
        }
    }

    /// The faces this frame may upload: the oldest full-size face, and the oldest board-size
    /// face when the client is not animating (`wake` is not `Now`), so that a card never changes
    /// its picture in the middle of a flight.
    pub(crate) fn next(&mut self, wake: Wake) -> [Option<Prepared>; 2] {
        let board = if wake == Wake::Now {
            None
        } else {
            self.board.pop_front()
        };
        [self.full.pop_front(), board]
    }

    pub(crate) fn len(&self) -> usize {
        self.board.len() + self.full.len()
    }
}

/// The face layer and its index in the array for its size, when `prepared` is a face.
pub(crate) fn face_upload(prepared: &Prepared) -> Option<(FaceSize, u16, &Layer)> {
    match (&prepared.upload, &prepared.ready) {
        (
            Upload::Face { layer, .. },
            AssetReady::Face {
                size, layer: index, ..
            },
        ) => Some((*size, *index, layer)),
        _ => None,
    }
}
