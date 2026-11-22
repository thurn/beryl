//! Fitting table regions and the tabletop shade to the web interface until the switch to Beryl 3D.

mod tuning;
mod web_fit;

use game_table::table::{RegionFit, RegionPolicy};
pub use web_fit::stage_width_and_scale;

/// The table's web projection, enabled by the app until the switch to Beryl 3D.
pub const FIT: RegionFit = RegionFit::new(&POLICY);

static POLICY: RegionPolicy = RegionPolicy {
    regions: web_fit::regions,
    screen: web_fit::screen,
    row_pose: web_fit::lean,
};
