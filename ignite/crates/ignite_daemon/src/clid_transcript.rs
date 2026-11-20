//! Stub. The ticket "CLI mode: send transcript digests to the hub and `ignite pool watch`" owns
//! this file and replaces the `todo!()` body.
#![allow(unused_variables)]

use ignite_core::ids::{SessionId, TicketId};
use ignite_core::protocol::cli::TranscriptLine;

/// The digest of turn `turn` of `session`, from the text of its `output.log` as `app` prints it.
pub fn digest(
    app: &str,
    output: &str,
    session: &SessionId,
    ticket: Option<&TicketId>,
    turn: u32,
) -> Vec<TranscriptLine> {
    todo!()
}
