//! Stub. The ticket "CLI mode: read a running turn and apply what follows its end" owns this
//! file and replaces the `todo!()` body.
#![allow(unused_variables)]

use std::path::Path;

use ignite_core::cli::{CliApp, TurnEvents};

/// What a turn's `output.log` at `output` says so far, read from its start through `app`.
pub fn read_turn(app: &dyn CliApp, output: &Path) -> TurnEvents {
    todo!()
}
