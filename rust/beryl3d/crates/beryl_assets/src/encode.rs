//! Turning a card picture into a texture layer: decoding a scan, scaling it to one of the two
//! face sizes and compressing it, with its mipmaps, into the format the GPU reads
//! (docs/beryl-3d.md, "Card faces").

use std::fmt;

use beryl_scene::input::FaceSize;
use ctt::encoders::Encoder;
use ctt::{
    AlphaMode, ColorSpace, Container, ConvertSettings, FormatDesc, ImageRef, PipelineOutput,
    Quality, SurfaceRef, TargetFormat, TextureKind,
};
use image::imageops::FilterType;

/// The version of everything that decides a layer's bytes: the encoders and their settings, the
/// scaling filter, the mipmap filter and the cache file's layout. Raise it whenever one of them
/// changes, so that faces cached by an older build are made again rather than read.
pub const ENCODER_VERSION: u32 = 1;

/// A texture format a face can be stored in. The compressed three store blocks of 4 by 4 pixels
/// at 8 bits a pixel, a quarter of [`Format::Rgba8`]'s size.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Format {
    /// ASTC with 4 by 4 blocks: iOS and Apple silicon.
    Astc4x4,
    /// ETC2 with EAC alpha: every OpenGL ES 3.0 device, so every Android phone.
    Etc2Rgba8,
    /// BC7: desktop GPUs.
    Bc7,
    /// Uncompressed 8-bit RGBA, the last resort.
    Rgba8,
}

impl Format {
    /// The compressed formats in the order they are preferred.
    pub const COMPRESSED: [Self; 3] = [Self::Astc4x4, Self::Etc2Rgba8, Self::Bc7];

    /// The first of [`Format::COMPRESSED`] that the device supports, or [`Format::Rgba8`].
    #[must_use]
    pub fn choose(supported: &[Self]) -> Self {
        Self::COMPRESSED
            .into_iter()
            .find(|format| supported.contains(format))
            .unwrap_or(Self::Rgba8)
    }

    /// The format's name in cache paths and logs.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Astc4x4 => "astc4x4",
            Self::Etc2Rgba8 => "etc2",
            Self::Bc7 => "bc7",
            Self::Rgba8 => "rgba8",
        }
    }

    /// The Khronos format of the layer's bytes. Faces hold sRGB color.
    #[must_use]
    pub fn khronos(self) -> ctt::Format {
        match self {
            Self::Astc4x4 => ctt::Format::ASTC_4x4_SRGB_BLOCK,
            Self::Etc2Rgba8 => ctt::Format::ETC2_R8G8B8A8_SRGB_BLOCK,
            Self::Bc7 => ctt::Format::BC7_SRGB_BLOCK,
            Self::Rgba8 => ctt::Format::R8G8B8A8_SRGB,
        }
    }

    /// The encoder preset. astcenc is far slower than the other encoders: measured on a
    /// board-size scan on an Apple M5 Max at load 55, its quality 35 (`Quality::Fast`) took
    /// 190 ms, its `fast` preset 110 ms and its `fastest` preset 77 ms, where etcpak took 3 ms
    /// and bc7f 19 ms at `Quality::Fast`. So ASTC uses `fastest`.
    fn quality(self) -> Quality {
        match self {
            Self::Astc4x4 => Quality::UltraFast,
            Self::Etc2Rgba8 | Self::Bc7 | Self::Rgba8 => Quality::Fast,
        }
    }

    pub(crate) fn code(self) -> u8 {
        self as u8
    }

    pub(crate) fn from_code(code: u8) -> Option<Self> {
        [Self::Astc4x4, Self::Etc2Rgba8, Self::Bc7, Self::Rgba8]
            .into_iter()
            .find(|format| format.code() == code)
    }
}

/// A face size's pixel dimensions: board size for every face in the match, full size for the
/// cards being read. Both are multiples of 4, the side of a compressed block.
#[must_use]
pub fn dimensions(size: FaceSize) -> (u32, u32) {
    match size {
        FaceSize::Board => (256, 360),
        FaceSize::Full => (672, 936),
    }
}

/// One texture layer: a face's picture at one size in one format, with its whole mipmap chain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layer {
    pub format: Format,
    /// The largest level's width and height in pixels.
    pub width: u32,
    pub height: u32,
    /// Each mipmap level's bytes, largest first. A compressed level is whole blocks row by row,
    /// so a level whose side is not a multiple of 4 is padded to one.
    pub levels: Vec<Vec<u8>>,
}

impl Layer {
    /// The bytes of every level together.
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.levels.iter().map(Vec::len).sum()
    }
}

#[derive(Debug)]
pub struct EncodeError(String);

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Decodes a JPEG or PNG scan and scales it to `size`, as RGBA pixels.
pub fn decode(bytes: &[u8], size: FaceSize) -> Result<Vec<u8>, EncodeError> {
    let picture = image::load_from_memory(bytes).map_err(|e| EncodeError(e.to_string()))?;
    let (width, height) = dimensions(size);
    let picture = if picture.width() == width && picture.height() == height {
        picture
    } else {
        picture.resize_exact(width, height, FilterType::Triangle)
    };
    Ok(picture.to_rgba8().into_raw())
}

/// Builds the mipmap chain of `pixels`, RGBA at `size`, and compresses every level into
/// `format`. Card faces are opaque, so the alpha channel is marked as carrying nothing.
pub fn encode(pixels: &[u8], size: FaceSize, format: Format) -> Result<Layer, EncodeError> {
    let (width, height) = dimensions(size);
    let surface = SurfaceRef {
        data: pixels,
        width,
        height,
        depth: 1,
        stride: width * 4,
        slice_stride: 0,
    };
    let image = ImageRef {
        surfaces: vec![vec![surface]],
        kind: TextureKind::Texture2D,
        desc: FormatDesc {
            format: ctt::Format::R8G8B8A8_SRGB,
            color_space: ColorSpace::Srgb,
            alpha: AlphaMode::Opaque,
        },
    };
    let target = match format {
        Format::Rgba8 => TargetFormat::Uncompressed(format.khronos()),
        _ => TargetFormat::Compressed {
            format: format.khronos(),
            encoder: Encoder::Auto,
        },
    };
    let settings = ConvertSettings {
        format: Some(target),
        container: Container::Raw,
        quality: format.quality(),
        mipmap: true,
        ..ConvertSettings::default()
    };
    let output = ctt::convert(image, settings).map_err(|e| EncodeError(e.to_string()))?;
    let PipelineOutput::Raw(mut image) = output else {
        return Err(EncodeError(
            "the encoder returned a file, not levels".into(),
        ));
    };
    let levels = image
        .surfaces
        .pop()
        .ok_or_else(|| EncodeError("the encoder returned no image".into()))?
        .into_iter()
        .map(|surface| surface.data)
        .collect();
    Ok(Layer {
        format,
        width,
        height,
        levels,
    })
}
