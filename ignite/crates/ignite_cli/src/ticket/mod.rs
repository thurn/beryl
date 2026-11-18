//! `ignite ticket`: the ticket verbs, which talk to the hub directly so they work while the
//! daemon is down.
//!
//! Stub. The ticket Thrh4.36 (Ticket commands) owns this folder and replaces the `todo!()` body.

use ignite_core::protocol::client::Reply;

use crate::args::TicketCommand;

pub fn run(command: &TicketCommand) -> Reply {
    let _ = command;
    todo!("Thrh4.36 (Ticket commands)")
}
