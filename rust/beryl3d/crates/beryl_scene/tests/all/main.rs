//! The integration tests of `beryl_scene`, as one binary: each file under `tests/` used
//! to be a binary of its own, and every one of them linked the crate and its dependencies again
//! after each edit.
//! `frame_allocation.rs`, `order.rs` and `play_allocation.rs` stay binaries of their own,
//! because each installs a counting global allocator, and a binary has only one. The tests'
//! shared table and scripts, `common`, sit beside them.

mod agent;
mod badges_manual;
mod box_shadows;
mod bug_report;
mod checks;
#[path = "../common/mod.rs"]
mod common;
mod compile_in_parts;
mod contract;
mod drawn;
mod driver;
mod effects;
mod effects_card;
mod effects_light;
mod elements;
mod faces;
mod flat_layer;
mod flat_text;
mod game_over;
mod game_phone;
mod gesture;
mod hand_order;
mod hud;
mod inspect;
mod keep_only;
mod light;
mod lobby;
mod manual;
mod manual_assists;
mod manual_controls;
mod manual_start;
mod manual_viewer;
mod motion;
mod notices;
mod object_states;
mod orb;
mod own;
mod paints;
mod particles;
mod piles;
mod play;
mod pointer;
mod prompts;
mod relay;
mod screens;
mod screens_phone;
mod scroll;
mod scroll_phone;
mod serif;
mod settings_sheet;
mod shadows;
mod sound;
mod table;
mod table_phone;
mod testing_screen;
mod text_field;
mod tokens;
mod touch;
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
mod web_fit;
mod widgets;
