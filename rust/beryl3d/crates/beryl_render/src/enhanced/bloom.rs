//! The Enhanced tier's bloom on glows and streaks (docs/beryl-3d.md, "The Enhanced tier":
//! "Bloom"), applied to the resolved picture before the flat layer is drawn over it.
//!
//! Not drawn yet: it records nothing.

use crate::draw::Draws;

/// Bloom's textures and pipelines.
pub(crate) struct Bloom;

impl Bloom {
    pub(crate) fn new() -> Self {
        Self
    }

    /// Records bloom for the frame `draws` describes into `encoder`, over the resolved picture
    /// in `view`, and returns how many draw calls it took. `Frame::bands` gives the glows'
    /// records to draw again. It records nothing yet.
    #[expect(
        clippy::unused_self,
        reason = "bloom is not drawn yet; drawing it reads its own textures"
    )]
    pub(crate) fn apply(
        &mut self,
        _encoder: &mut wgpu::CommandEncoder,
        _draws: &Draws,
        _view: &wgpu::TextureView,
    ) -> u32 {
        0
    }
}
