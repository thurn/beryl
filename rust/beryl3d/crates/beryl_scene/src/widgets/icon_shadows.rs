//! The drop shadows of icons (docs/beryl-3d.md, "Matching the web interface's look", bullet
//! "Shadows"). A Baseline material takes one texture sample, so an icon's drop shadows cannot
//! blur as they draw: the glyph worker rasterizes them into cells of their own, which the flat
//! layer draws under the icon with the `Icon` material, each in its shadow's color
//! (`layout::shadows::write_icon_shadows`).
//!
//! Icons are rasterized once per scale before the first frame, not on request as glyphs are, so
//! every chain of drop shadows an icon is drawn with is listed here (`CHAINS`). An element that
//! gives an icon a chain not listed draws no shadow, and the "drawn" check reports it.
//!
//! CSS applies each `drop-shadow()` of a `filter` to the result of the ones before it. Because
//! the chains are known ahead, the worker computes them as CSS does, unlike text's
//! `DropLayers`: the coverage of shadow `k` is the coverage of the icon and every shadow before
//! it, composited, blurred by shadow `k`'s deviation and moved by its offset. Drawing the cells
//! bottom first, last shadow first, then the icon, gives CSS's result, since compositing over is
//! associative. A chain of `n` shadows is `n` cells.

use super::IconId;
use super::icons::{FULL_CONTROL, PASS_ON, PHASES, STEPS, STEPS_OUTLINED};
use crate::element::{MOST_DROP_SHADOWS, Shadow};
use crate::tokens::shadow;

/// Every chain of drop shadows (`Style::drop_shadow`) an icon is drawn with, beside the icons
/// drawn with it, each with the CSS rule it copies.
pub const CHAINS: [(&[IconId], &[Shadow]); 8] = [
    // `.step-icon` and `.step-current .step-icon` (phase-tracker.css), filled and outlined.
    (&STEPS, &shadow::STEP_ICON),
    (&STEPS_OUTLINED, &shadow::STEP_ICON),
    (&STEPS, &shadow::CURRENT_GLOW),
    (&STEPS_OUTLINED, &shadow::CURRENT_GLOW),
    // `.phase-icon` and `.phase-current .phase-icon`, in the tracker and on the phone.
    (&PHASES, &shadow::PHASE_ICON),
    (&PHASES, &shadow::CURRENT_GLOW),
    // `.full-control svg` (controls.css).
    (&[FULL_CONTROL], &shadow::FULL_CONTROL),
    // `.pass-on-face > svg` (controls.css).
    (&[PASS_ON], &shadow::PASS_ON),
];

/// The `IconCell::shadow` of the cell of shadow `stage` of chain `chain` of `CHAINS`: never 0,
/// which is the icon's own cell.
#[must_use]
pub const fn code(chain: usize, stage: usize) -> u8 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "eight chains of four shadows"
    )]
    let code = (1 + chain * MOST_DROP_SHADOWS + stage) as u8;
    code
}

/// The shadows of `chain` the cells are drawn for: at most `MOST_DROP_SHADOWS`.
#[must_use]
pub fn shadows(chain: usize) -> &'static [Shadow] {
    let all = CHAINS[chain].1;
    &all[..all.len().min(MOST_DROP_SHADOWS)]
}

/// The chain of `CHAINS` that draws `shadows` under `icon`, if one does.
#[must_use]
pub fn chain_of(icon: IconId, shadows: &[Shadow]) -> Option<usize> {
    CHAINS
        .iter()
        .position(|(icons, chain)| *chain == shadows && icons.contains(&icon))
}

/// Every icon and `IconCell::shadow` the worker rasterizes a cell of: each shadow of each chain,
/// for each icon drawn with it.
pub fn cells() -> impl Iterator<Item = (IconId, u8)> {
    (0..CHAINS.len()).flat_map(|chain| {
        let icons = CHAINS[chain].0;
        (0..shadows(chain).len())
            .flat_map(move |stage| icons.iter().map(move |&icon| (icon, code(chain, stage))))
    })
}

/// The chain and shadow of a cell's `IconCell::shadow`, if it names one.
#[must_use]
pub fn decode(shadow: u8) -> Option<(usize, usize)> {
    let index = usize::from(shadow.checked_sub(1)?);
    let (chain, stage) = (index / MOST_DROP_SHADOWS, index % MOST_DROP_SHADOWS);
    (chain < CHAINS.len() && stage < shadows(chain).len()).then_some((chain, stage))
}

/// How many standard deviations a shadow's cell reaches past what casts it: past three, a
/// Gaussian leaves under a level of 255.
const REACH_DEVIATIONS: u8 = 3;

/// The side, in physical pixels, of an icon held at `size` points at `scale`, as the glyph
/// worker rasterizes it.
#[must_use]
pub fn icon_pixels(size: u16, scale: f32) -> u16 {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a cast from a float saturates, and an icon is far narrower than 65,536 pixels"
    )]
    let pixels = (f32::from(size) * scale).round().max(1.0) as u16;
    pixels
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_cell_code_names_its_chain_and_shadow() {
        for chain in 0..CHAINS.len() {
            for stage in 0..shadows(chain).len() {
                assert_eq!(decode(code(chain, stage)), Some((chain, stage)));
            }
        }
        assert_eq!(decode(0), None);
    }

    #[test]
    fn the_current_steps_glow_is_found_for_a_filled_step_icon() {
        let chain = chain_of(STEPS[3], &shadow::CURRENT_GLOW).expect("the glow is listed");
        assert_eq!(shadows(chain).len(), 3);
        assert_eq!(chain_of(STEPS[3], &shadow::PASS_ON), None);
    }
}
