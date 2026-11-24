//! Shadow cell dimensions and codes shared by layout and rasterization.

use crate::paint::{MOST_DROP_SHADOWS, Shadow};

use super::{IconId, artwork::texels};

/// A list of icons and the drop shadows they cast in order.
pub type ShadowChain = (&'static [IconId], &'static [Shadow]);

/// The `IconCell::shadow` of the cell of shadow `stage` of chain `chain`: never 0,
/// which is the icon's own cell.
#[must_use]
pub const fn code(chain: usize, stage: usize) -> u8 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the icon catalog has fewer than 64 chains of four shadows"
    )]
    let code = (1 + chain * MOST_DROP_SHADOWS + stage) as u8;
    code
}

/// How many standard deviations a shadow's cell reaches past what casts it: past three, a
/// Gaussian leaves under a level of 255.
const REACH_DEVIATIONS: u8 = 3;

/// The side, in physical pixels, of an icon held at `size` points at `scale`, as the glyph
/// worker rasterizes it.
#[must_use]
fn icon_pixels(size: u16, scale: f32) -> u16 {
    texels(f32::from(size) * scale)
}

/// How far, in physical pixels at `scale`, the cell of shadow `stage` of `shadows` reaches past
/// the icon's square on every side: for it and each shadow before it, `REACH_DEVIATIONS`
/// deviations of its blur and the longer of its offsets.
#[must_use]
pub fn stage_pad(shadows: &[Shadow], stage: usize, scale: f32) -> u16 {
    let points: f32 = (shadows.iter().take(stage + 1))
        .map(|shadow| {
            f32::from(REACH_DEVIATIONS) * shadow.drop_sigma() + shadow.x.abs().max(shadow.y.abs())
        })
        .sum();
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a cast from a float saturates, and a shadow reaches far less than 65,536 pixels"
    )]
    let pixels = (points * scale).ceil().max(0.0) as u16;
    pixels
}

/// The side, in physical pixels, of one texel of the cell of shadow `stage` of `shadows` at
/// `scale`: half the shadow's deviation, and at least one pixel. The cell is the shadow's blur
/// of what came before it, so it has no detail finer than that deviation, and the atlas's
/// linear filtering draws it smooth from fewer texels.
#[must_use]
pub fn stage_step(shadows: &[Shadow], stage: usize, scale: f32) -> u16 {
    let sigma = shadows.get(stage).map_or(0.0, Shadow::drop_sigma);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a cast from a float saturates, and a deviation is a few pixels"
    )]
    let step = (sigma * scale / 2.0).floor().max(1.0) as u16;
    step
}

/// The side, in texels, of the square cell of shadow `stage` of `shadows` cast by an icon held
/// at `size` points, at `scale`: the icon's square of pixels grown by `stage_pad` on every
/// side, in texels of `stage_step` pixels, rounded up.
#[must_use]
pub fn stage_side(size: u16, shadows: &[Shadow], stage: usize, scale: f32) -> u16 {
    let pixels = icon_pixels(size, scale).saturating_add(2 * stage_pad(shadows, stage, scale));
    pixels.div_ceil(stage_step(shadows, stage, scale))
}
