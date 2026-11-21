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
use kit_base::atlas::shadows::ShadowChain;
use kit_base::tokens::shadow;

pub use kit_base::atlas::shadows::{code, stage_pad, stage_side, stage_step};

/// Every chain of drop shadows (`Style::drop_shadow`) an icon is drawn with, beside the icons
/// drawn with it, each with the CSS rule it copies.
pub const CHAINS: [ShadowChain; 8] = [
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
