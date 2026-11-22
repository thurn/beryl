//! The integration tests of `beryl_inspect`, as one binary: each file under `tests/` used
//! to be a binary of its own, and every one of them linked the crate and its dependencies again
//! after each edit.
//! `recorded_allocation.rs` stays a binary of its own, because it installs a counting global
//! allocator, and a binary has only one.

#[path = "../../../../test_data.rs"]
mod test_data;

mod chrome;
mod cli;
mod conventions;
mod differential;
mod explain;
mod faults;
mod frame_allocation;
mod furniture;
mod gate;
mod hud;
mod log;
mod manual_table;
mod map;
mod opening;
mod pack;
mod parity;
mod pointer_held;
#[path = "profiles/prompt_placement.rs"]
mod prompt_placement;
mod prompts;
mod prompts_declare;
mod prompts_divide;
mod prompts_order;
mod prompts_pick;
mod prompts_split;
mod recorded;
mod replay;
mod runner;
mod screens;
mod specs;
mod stress;
mod tiers;

mod stories;
