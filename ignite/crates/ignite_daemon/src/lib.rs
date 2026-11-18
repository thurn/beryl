//! The daemon: one long-running process per machine. It hands out cores and build trees, runs
//! every heavy command under a supervisor, keeps the result cache, lands tickets, verifies tips,
//! drives the pool of workers and records what everything cost.
//!
//! Each verb the client sends is a [`server::Verb`] registered with [`server::Registry`].

// `main` is the module that wires the daemon together, not a binary: `autobins = false` in
// Cargo.toml keeps Cargo from building it as one. The lint fires before attributes on the item
// are seen, so it is allowed for the crate.
#![allow(special_module_name)]

pub mod build;
pub mod cache;
pub mod checks;
pub mod claim;
pub mod crash;
pub mod dispatchd;
pub mod doctor;
pub mod handles;
pub mod hubclient;
pub mod idle;
pub mod justiciar;
pub mod land;
pub mod loop_health;
pub mod machine_lock;
pub mod main;
pub mod metrics;
pub mod monitor;
pub mod outbox;
pub mod pool;
pub mod sched;
pub mod server;
pub mod supervise;
#[cfg(test)]
mod testhub;
pub mod tip;
pub mod trees;
pub mod update;
pub mod wrap;
