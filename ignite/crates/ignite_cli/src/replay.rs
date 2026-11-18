//! `ignite gate replay`: the largest threshold at which the gate would still have run 95% of the
//! regression checks that caught something in the last days.
//!
//! Stub. The ticket Thrh4.38 (Gate replay) owns this file and replaces the `todo!()` body.

use ignite_core::protocol::client::Reply;

use crate::args::GateCommand;

pub fn run(command: &GateCommand) -> Reply {
    let _ = command;
    todo!("Thrh4.38 (Gate replay)")
}
