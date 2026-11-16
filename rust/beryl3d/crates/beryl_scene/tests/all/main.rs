//! The integration tests of `beryl_scene`, as one binary: each file under `tests/` used
//! to be a binary of its own, and every one of them linked the crate and its dependencies again
//! after each edit.
//! `frame_allocation.rs`, `order.rs` and `play_allocation.rs` stay binaries of their own,
//! because each installs a counting global allocator, and a binary has only one. The tests'
//! shared table and scripts, `common`, sit beside them.

mod agent;
mod checks;
#[path = "../common/mod.rs"]
mod common;
mod contract;
mod driver;
mod effects;
mod effects_card;
mod effects_light;
mod elements;
mod faces;
mod flat_layer;
mod flat_text;
mod game_over;
mod inspect;
mod light;
mod motion;
mod notices;
mod object_states;
mod own;
mod piles;
mod play;
mod pointer;
mod relay;
mod screens;
mod scroll;
mod settings_sheet;
mod shadows;
mod sound;
mod table;
mod testing_screen;
mod text_field;
mod tokens;
mod under;
mod verbs_count;
mod verbs_fly;
mod verbs_fly_draw;
mod verbs_leaving;
mod verbs_lift;
mod verbs_mark;
mod verbs_point;
mod verbs_shown;
mod verbs_streak;
mod verbs_strike;
mod verbs_tap;
mod widgets;
