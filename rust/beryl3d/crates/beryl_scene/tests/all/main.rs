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
mod faces;
mod flat_layer;
mod light;
mod motion;
mod play;
mod table;
mod tokens;
mod verbs_count;
mod verbs_strike;
