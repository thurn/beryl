//! Exploration finds: a failed exploration check is product work. It files an ordinary P2 bug
//! labelled `found-by:<family>` with the steps to replay it, or adds a comment to the open
//! ticket with the same signature.
//!
//! The signature's subject is the family, not the check: an exploration check is one seed or
//! scenario, and two seeds that hit the same fault are one bug.

use rusqlite::Connection;

use ignite_core::protocol::hub::ExplorationFind;
use ignite_core::protocol::tickets::{TicketFields, TicketType};
use ignite_core::time::Timestamp;
use ignite_store::Result;

use super::filing::{self, Author, Filed, Filing};
use super::regressions::OUTPUT_LINES;
use super::signature::Signature;
use super::tail;

pub fn find(
    conn: &mut Connection,
    author: Author,
    find: &ExplorationFind,
    now: Timestamp,
) -> Result<Filed> {
    let check = find.check.as_str();
    let family = find.check.family();
    let body = format!(
        "The exploration check {check} failed at commit {}.\n\n\
         replay: {}\n\n\
         output (last {OUTPUT_LINES} lines):\n{}",
        find.commit,
        find.replay,
        tail(&find.output, OUTPUT_LINES)
    );
    let filing = Filing {
        signature: Signature::of_output(family, "found", "", &find.output, &find.host),
        evidence: format!("{}@{}", find.replay, find.commit),
        fields: TicketFields {
            title: Some(format!("Found by {family}: {check}")),
            body: Some(body),
            ticket_type: Some(TicketType::Bug),
            priority: Some(2),
            labels: Some(vec![format!("found-by:{family}")]),
            ..TicketFields::default()
        },
        deps: Vec::new(),
    };
    filing::file(conn, author, &filing, now)
}
