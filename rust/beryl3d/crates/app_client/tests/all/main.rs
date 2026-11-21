//! The integration tests of `app_client`, as one binary: each file under `tests/` used
//! to be a binary of its own, and every one of them linked the crate and its dependencies again
//! after each edit.
//! `frame_allocation.rs` and `play_allocation.rs` stay binaries of their own,
//! because each installs a counting global allocator, and a binary has only one. The tests'
//! shared table and scripts, `common`, sit beside them.

mod bug_report;
mod checks;
#[path = "../common/mod.rs"]
mod common;
mod compile_in_parts;
mod contract;
mod drawn;
mod driver;
mod elements;
mod faces;
mod flat_layer;
mod flat_text;
mod game_over;
mod game_phone;
mod gesture;
mod hud;
mod inspect;
mod lobby;
mod manual;
mod manual_assists;
mod manual_controls;
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
mod screens_phone;
mod scroll;
mod scroll_phone;
mod settings_sheet;
mod shadows;
mod table;
mod testing_screen;
mod text_field;
mod touch;
mod verbs_count;
mod verbs_fly;
mod widgets;
