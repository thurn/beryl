//! The card face arrays (docs/beryl-3d.md, "Card faces"): a board-size array with a layer for
//! every face of the match, and a full-size array of `FULL_LAYERS` layers for the cards being
//! read, both in the first compressed format the device samples.
//!
//! Until a face's layer arrives from `beryl_assets`' workers, the board-size array shows a
//! generated placeholder in that layer (`beryl_assets::Placeholders`). The workers draw the
//! placeholders too, so the frame thread only copies their levels into the array.
//!
//! During play two kinds of upload remain, and both are paced (`Waiting`): a board-size face that
//! arrives late is uploaded only in a frame in which nothing animates, one layer a frame, and at
//! most one full-size face is uploaded a frame.

use std::collections::VecDeque;

use beryl_assets::{FULL_LAYERS, Format, Layer, Level, Placeholders, Prepared, Upload, dimensions};
use kit_base::atlas::{AssetReady, FaceSize};
use kit_frame::frame::Wake;

/// The layer of the face arrays that holds the card back: the card back is always the first
/// face supplied by the host.
pub const BACK_LAYER: u16 = 0;

/// The device features that let it sample a compressed format.
pub(crate) const COMPRESSION: wgpu::Features = wgpu::Features::TEXTURE_COMPRESSION_ASTC
    .union(wgpu::Features::TEXTURE_COMPRESSION_ETC2)
    .union(wgpu::Features::TEXTURE_COMPRESSION_BC);

/// The side of a compressed block, in pixels.
const BLOCK: u32 = 4;

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

/// One face array: a texture of `layers` layers at one face size in one format, with every
/// mipmap level down to 1 by 1, as `beryl_assets` makes a face.
pub(crate) struct FaceArray {
    texture: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
    pub(crate) format: Format,
    pub(crate) size: (u32, u32),
    levels: u32,
    /// By layer: whether a face's layer has been written into it (placeholders do not count).
    filled: Vec<bool>,
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
        let levels = Level::count(size);
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
            // `COPY_SRC` so that `grown` can copy the layers into a larger array.
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
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
            filled: vec![false; layers.max(2) as usize],
        }
    }

    /// Writes `layer`'s levels into layer `index`. Logs and writes nothing when the layer is of
    /// another format or size than the array, or the array has no such layer.
    pub(crate) fn write(&mut self, queue: &wgpu::Queue, index: u16, layer: &Layer) -> bool {
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
        let written = self.write_levels(queue, index, &layer.levels);
        if written {
            self.filled[usize::from(index)] = true;
        }
        written
    }

    /// How many layers hold a face written by `write`.
    pub(crate) fn filled(&self) -> usize {
        self.filled.iter().filter(|filled| **filled).count()
    }

    pub(crate) fn layer_uploaded(&self, layer: u16) -> bool {
        self.filled
            .get(usize::from(layer))
            .copied()
            .unwrap_or(false)
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

    /// A copy of this array with `layers` layers: its first `kept` layers are copied on the
    /// GPU, each with its filled mark, and the layers from `kept` on hold `placeholders`, in
    /// order, unfilled. Faces added during play grow the board-size array this way, so the faces
    /// already uploaded are never replaced by placeholders and never uploaded again.
    pub(crate) fn grown(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        kept: u32,
        layers: u32,
        placeholders: &[Layer],
    ) -> Self {
        let mut grown = Self::new(device, self.format, self.size, layers);
        let kept = kept.min(self.layers()).min(grown.layers());
        if kept > 0 {
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("grow faces"),
            });
            for level in 0..self.levels {
                let shape = Level::of(self.format, self.size, level);
                let at = |texture| wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: level,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                };
                encoder.copy_texture_to_texture(
                    at(&self.texture),
                    at(&grown.texture),
                    wgpu::Extent3d {
                        width: shape.width,
                        height: shape.height,
                        depth_or_array_layers: kept,
                    },
                );
            }
            queue.submit([encoder.finish()]);
        }
        let kept = kept as usize;
        grown.filled[..kept].copy_from_slice(&self.filled[..kept]);
        for (index, layer) in (kept..grown.filled.len()).zip(placeholders) {
            grown.placeholder(queue, index as u16, layer);
        }
        grown
    }

    /// How many layers the array has.
    pub(crate) fn layers(&self) -> u32 {
        self.texture.depth_or_array_layers()
    }

    /// Copies the placeholder `layer` into layer `index`, which it does not mark as filled.
    fn placeholder(&self, queue: &wgpu::Queue, index: u16, layer: &Layer) {
        if layer.format == self.format && (layer.width, layer.height) == self.size {
            self.write_levels(queue, index, &layer.levels);
        } else {
            eprintln!(
                "beryl_render: a {} placeholder does not fit the {} array",
                layer.format.name(),
                self.format.name()
            );
        }
    }
}

/// The two face arrays of a match: the board-size array with one layer per placeholder in
/// `placeholders`, in `FaceId` order, each showing it, and the full-size array of `FULL_LAYERS`
/// layers, empty until faces are uploaded into it. The placeholders are only copied: none is
/// drawn here.
pub(crate) fn face_arrays(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    placeholders: &Placeholders,
) -> [FaceArray; 2] {
    let format = placeholders.format;
    let size = dimensions(FaceSize::Board);
    let layers = device.limits().max_texture_array_layers;
    let faces = placeholders.layers.len();
    if faces > layers as usize {
        eprintln!(
            "beryl_render: the match has {faces} faces but the device allows {layers} layers; the \
             rest draw from layer 0"
        );
    }
    let count = (faces as u32).clamp(1, layers);
    let board = FaceArray::new(device, format, size, count);
    for (index, layer) in (0..count).zip(&placeholders.layers) {
        board.placeholder(queue, index as u16, layer);
    }
    [board, full_array(device, format)]
}

/// The full-size array of `FULL_LAYERS` empty layers in `format`.
pub(crate) fn full_array(device: &wgpu::Device, format: Format) -> FaceArray {
    FaceArray::new(
        device,
        format,
        dimensions(FaceSize::Full),
        FULL_LAYERS as u32,
    )
}

/// A two-layer array of 4 by 4 texels that no face fits: what stands bound in place of the
/// full-size array while there is none.
pub(crate) fn token_array(device: &wgpu::Device) -> FaceArray {
    FaceArray::new(device, Format::Rgba8, (BLOCK, BLOCK), 1)
}

/// The face arrays of a renderer with no match yet: the board-size array holds the placeholder
/// back alone, uncompressed, and the full-size array is a token one that no face fits.
pub(crate) fn empty_arrays(device: &wgpu::Device, queue: &wgpu::Queue) -> [FaceArray; 2] {
    let back = Placeholders::draw::<&str>(&[], Format::Rgba8);
    let board = FaceArray::new(device, Format::Rgba8, dimensions(FaceSize::Board), 1);
    board.placeholder(queue, 0, &back.layers[0]);
    [board, token_array(device)]
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

    /// Drops the full-size faces still waiting: they were asked for before memory ran low.
    pub(crate) fn drop_full(&mut self) {
        self.full.clear();
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
