//! Fitting table regions and the tabletop shade to the web interface until the switch to Beryl 3D.

mod tuning;
mod web_fit;

use game_table::profiles::Profile;
use game_table::table::{RegionFit, RegionPolicy, Window};
use kit_base::geometry::Rect;
pub use web_fit::stage_width_and_scale;

/// The table's web projection, enabled by the app until the switch to Beryl 3D.
pub const FIT: RegionFit = RegionFit::new(&POLICY);

/// The mode chooser's tray on the fitted desktop web stage, and its points per stage pixel.
#[must_use]
pub fn ways_tray(profile: Profile<'_>, window: Window) -> Option<(Rect, f32)> {
    web_fit::ways_tray(profile, window)
}

static POLICY: RegionPolicy = RegionPolicy {
    regions: web_fit::regions,
    screen: web_fit::screen,
    row_pose: web_fit::lean,
};
