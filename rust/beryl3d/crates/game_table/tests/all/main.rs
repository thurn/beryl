//! Integration tests for the table's face list, placement and drawing.

use game_table::profiles::Profile;
use game_table::table::Layout;

fn reference() -> Layout {
    let data = test_data::data();
    Profile::for_window(&data.profiles, data.profiles.fallback().usual_window())
        .reference_layout()
        .unwrap()
}

#[path = "../../../../test_data.rs"]
mod test_data;

mod box_shadows;
mod clearance;
mod faces;
mod paints;
mod profiles;
mod under;
