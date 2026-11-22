//! Fitting table regions and the tabletop shade to the web interface until the switch to Beryl 3D.

mod tuning;
mod web_fit;

use game_table::table::{RegionFit, RegionPolicy};

/// The table's web projection, enabled by the app until the switch to Beryl 3D.
pub const FIT: RegionFit = RegionFit::new(&POLICY);

static POLICY: RegionPolicy = RegionPolicy {
    regions: web_fit::regions,
    row_pose: web_fit::lean,
};
