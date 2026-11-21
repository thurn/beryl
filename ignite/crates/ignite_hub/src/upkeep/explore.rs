//! Exploration findings share P2 tickets while they wait for approval. Each report keeps
//! its own check, commit, replay command, and output so a worker can investigate each one.

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
        "The exploration check {check} failed at commit {} on {}.\n\n\
         replay: {}\n\n\
         output (last {OUTPUT_LINES} lines):\n{}",
        find.commit,
        find.host,
        find.replay,
        tail(&find.output, OUTPUT_LINES)
    );
    let signature = Signature::of_output(family, "found", "", &find.output, &find.host);
    // Two runs of a check on one commit can fail in different ways.
    let evidence = format!("{}:{}@{}", signature.key(), find.replay, find.commit);
    let filing = Filing {
        signature,
        evidence,
        fields: TicketFields {
            title: Some(format!("Investigate {family} findings")),
            body: Some(body),
            acceptance: Some(
                "Review the finding in the description and each report in the comments. \
                 Record which findings reproduce, what was fixed, and any follow-up needed."
                    .to_owned(),
            ),
            ticket_type: Some(TicketType::Bug),
            priority: Some(2),
            labels: Some(vec![format!("found-by:{family}")]),
            ..TicketFields::default()
        },
        deps: Vec::new(),
        released: false,
    };
    filing::file(conn, author, &filing, now)
}

#[cfg(test)]
mod tests;
