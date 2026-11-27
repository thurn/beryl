//! Artwork for the game HUD, in its own drawing coordinates.

pub mod orb;
mod orb_coords;
pub mod spinner;
pub mod turn;

use kit_base::atlas::artwork::{CoverageLayer, LayerCell};
use kit_base::data::profiles::Profiles;

/// The game HUD's atlas cells, also supplied by the headless stub.
pub fn atlas_cells(profiles: &Profiles) -> impl Iterator<Item = LayerCell> {
    orb::atlas_cells(profiles)
        .chain(turn::atlas_cells())
        .chain(spinner::atlas_cells())
}

/// The game HUD's coverage artwork rasterized by the host.
#[must_use]
pub fn atlas_artwork(profiles: &Profiles) -> Vec<CoverageLayer> {
    let mut artwork = orb::atlas_artwork(profiles);
    artwork.extend(turn::atlas_artwork());
    artwork.extend(spinner::atlas_artwork());
    artwork
}
