//! Beryl 3D's assets: card pictures decoded and glyphs rasterized, and packed for the renderer.
//! Nothing here runs on the frame thread (docs/beryl-3d.md, "Card faces", "Preparation"): a
//! worker sends what it finishes as a [`Prepared`], whose pixels go to the renderer and whose
//! [`AssetReady`] goes to the client core.
//!
//! A match's faces are listed when it loads ([`MatchFaces`]). The face workers ([`Preparer`])
//! read each from the compressed cache ([`CacheKey`]) or, on a miss, decode its scan from
//! `beryl_art`'s scan cache, scale it to board or full size, compress it with its mipmaps
//! ([`Format`]) and cache the result. A face with no downloaded scan is drawn as its name on a
//! colored field.

mod cache;
mod encode;
mod faces;
pub mod glyphs;
pub mod packer;
mod placeholder;
mod prepare;

use beryl_scene::input::AssetReady;

pub use cache::CacheKey;
pub use encode::{ENCODER_VERSION, EncodeError, Format, Layer, decode, dimensions, encode};
pub use faces::{CARD_BACK, MatchFace, MatchFaces};
pub use placeholder::placeholder;
pub use prepare::{
    Config, FULL_LAYERS, Full, Preparer, Source, default_workers, faces_built_on_this_thread,
};

/// A finished asset: the pixels for the renderer to upload and what the client core learns of
/// them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prepared {
    pub upload: Upload,
    pub ready: AssetReady,
}

/// Pixels for the renderer to upload.
#[derive(Clone, PartialEq, Eq)]
pub enum Upload {
    /// The whole of one atlas page: [`packer::PAGE_SIZE`] rows of as many 8-bit coverage
    /// texels, top row first.
    GlyphPage { page: u16, pixels: Vec<u8> },
    /// One card face's layer, for the layer of the board-size or full-size array that the
    /// [`AssetReady`] names.
    Face { layer: Layer, source: Source },
}

impl std::fmt::Debug for Upload {
    /// Names the page or face and leaves out its texels.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::GlyphPage { page, pixels } => formatter
                .debug_struct("GlyphPage")
                .field("page", page)
                .field("bytes", &pixels.len())
                .finish(),
            Self::Face { layer, source } => formatter
                .debug_struct("Face")
                .field("format", &layer.format)
                .field("width", &layer.width)
                .field("height", &layer.height)
                .field("bytes", &layer.bytes())
                .field("source", source)
                .finish(),
        }
    }
}
