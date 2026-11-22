//! Fitting rows and piles to the web interface's positions; deleted at the switch.

mod tuning;
mod web_fit;

use game_table::table::{RegionFit, RegionPolicy};

/// The table's web projection, enabled by the app until the switch to Beryl 3D.
pub const FIT: RegionFit = RegionFit::new(&POLICY);

static POLICY: RegionPolicy = RegionPolicy {
    regions: web_fit::regions,
    row_pose: web_fit::lean,
};
