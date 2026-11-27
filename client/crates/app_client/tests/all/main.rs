//! The integration tests of `app_client`, as one binary: each file under `tests/` used
//! to be a binary of its own, and every one of them linked the crate and its dependencies again
//! after each edit.
//! `frame_allocation.rs` and `play_allocation.rs` stay binaries of their own,
//! because each installs a counting global allocator, and a binary has only one. The tests'
//! shared table and scripts, `common`, sit beside them.

#[path = "../../../../test_data.rs"]
mod test_data;
#[path = "../../../../test_draw_snapshot.rs"]
mod test_draw_snapshot;

fn reference() -> &'static game_table::table::Layout {
    use game_table::{profiles::Profile, table::Layout};
    use std::sync::OnceLock;

    static REFERENCE: OnceLock<Layout> = OnceLock::new();
    REFERENCE.get_or_init(|| {
        let data = test_data::data();
        Profile::for_window(&data.profiles, data.profiles.fallback().usual_window())
            .reference_layout()
            .unwrap()
    })
}

mod ability_printings;
mod bug_report;
mod checks;
#[path = "../common/mod.rs"]
mod common;
mod compile_in_parts;
mod contract;
mod creature_row;
mod drawn;
mod driver;
mod elements;
mod faces;
mod flat_layer;
mod flat_text;
mod game_compact;
mod game_over;
mod gesture;
mod hand_cost;
mod hud;
mod inspect;
mod lobby;
mod manual;
mod manual_assists;
mod manual_controls;
mod manual_drop;
mod manual_start;
mod manual_viewer;
mod notices;
mod object_states;
mod orb;
mod own;
mod piles;
mod pointer;
mod prompts;
mod screens;
mod screens_compact;
mod scroll;
mod scroll_compact;
mod settings_sheet;
mod shadows;
mod shortcuts;
mod table;
mod testing_screen;
mod text_field;
mod touch;
mod verbs_count;
mod verbs_fly;
mod verbs_fly_compact;
mod widgets;

const REGULAR: kit_base::data::profiles::SizeClass = kit_base::data::profiles::SizeClass::Regular;
const COMPACT: kit_base::data::profiles::SizeClass = kit_base::data::profiles::SizeClass::Compact;
