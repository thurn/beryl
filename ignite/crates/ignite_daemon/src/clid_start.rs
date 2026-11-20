//! Stub. The ticket "CLI mode: start a turn exactly once and find it again after a restart" owns
//! this file and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use std::path::Path;

use ignite_core::ids::SessionId;

/// The variable in a turn's supervised command that names its session and turn, as
/// `<session>:<turn>`, so a restarted daemon finds the run of each recorded turn.
pub const TURN_VARIABLE: &str = "IGNITE_CLI_TURN";

/// What the runs in the Ignite home `home` say about turn `turn` of `session`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recovery {
    /// No run names the turn: it was recorded and never started, so it is started now.
    NotStarted,
    /// A run names it and its turn lock is held.
    Running,
    /// A run names it and wrote its result, with this exit code.
    Ended { exit: Option<i32> },
    /// A run names it, its lock is free and it wrote no result, as after a reboot.
    Interrupted,
}

/// Finds the run of turn `turn` of `session` among the supervised runs under `home`.
pub fn recover(home: &Path, session: &SessionId, turn: u32) -> Recovery {
    todo!()
}
