//! `ignite plan check <epic>`: the shape of an epic's ticket graph: tickets ready at the start,
//! the longest chain, suspect edges, tickets that name no files and lanes whose tickets do not
//! depend on each other.
//!
//! Stub. The ticket Thrh4.37 (Plan check) owns this file and replaces the `todo!()` body.

use ignite_core::protocol::client::Reply;

use crate::args::PlanCommand;

pub fn run(command: &PlanCommand) -> Reply {
    let _ = command;
    todo!("Thrh4.37 (Plan check)")
}
