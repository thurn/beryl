//! The integration tests of `beryl_inspect`, as one binary: each file under `tests/` used
//! to be a binary of its own, and every one of them linked the crate and its dependencies again
//! after each edit.
//! `recorded_allocation.rs` stays a binary of its own, because it installs a counting global
//! allocator, and a binary has only one.

mod cli;
mod differential;
mod frame_allocation;
mod gate;
mod pack;
mod recorded;
mod runner;
mod stress;
