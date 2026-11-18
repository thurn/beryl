//! The daemon's entry points: wiring the real implementations into a [`crate::server::Context`],
//! registering every verb, and serving. The `ignite` executable calls these from its hidden
//! subcommands.
//!
//! Stub. The ticket Thrh4.49 (Assemble the daemon and prove one ticket end to end) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use std::path::Path;

/// The hidden `ignite daemon` subcommand: runs the daemon for the checkout at `repo` until the
/// process ends. Returns the exit code.
pub fn run_daemon(repo: &Path) -> i32 {
    todo!("Thrh4.49 (Assemble the daemon and prove one ticket end to end)")
}
