//! Beryl 3D's assets: card pictures decoded and glyphs rasterized, and packed for the renderer.
//! Nothing here runs on the frame thread (docs/beryl-3d.md, "Card faces", "Preparation"): a
//! worker sends what it finishes as a [`Prepared`], whose pixels go to the renderer and whose
//! [`AssetReady`] goes to the client core.

pub mod glyphs;
pub mod packer;

use beryl_scene::input::AssetReady;

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
}

impl std::fmt::Debug for Upload {
    /// Names the page and leaves out its megabyte of texels.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::GlyphPage { page, pixels } => formatter
                .debug_struct("GlyphPage")
                .field("page", page)
                .field("bytes", &pixels.len())
                .finish(),
        }
    }
}
