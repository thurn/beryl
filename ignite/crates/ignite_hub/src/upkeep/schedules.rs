//! Files the copies of meta-tickets that are due (see `ignite_core::schedule`).
//!
//! Each pass looks at every open meta-ticket with an interval, and for each one whose next copy
//! is due files a child ticket with the meta-ticket's title, text, notes, labels and priority, `open`, so the pool may be offered it at once. A copy is an ordinary task: it is claimed,
//! worked and closed like any other ticket, and closing it leaves the meta-ticket open.

use rusqlite::Connection;

use ignite_core::protocol::tickets::{
    BatchRequest, Operation, Precondition, Status, Ticket, TicketFields, TicketType,
};
use ignite_core::schedule::NextCopy;
use ignite_core::time::Timestamp;
use ignite_store::Result;
use ignite_store::hub::batch::{self, BatchOutcome};
use ignite_store::hub::schedules;

use super::filing::{Author, UPKEEP};

/// Who the copies are filed by, so a copy's history says the schedule filed it.
pub const SCHEDULE: Author<'static> = Author {
    actor: "ignite-schedule",
    machine: UPKEEP.machine,
};

/// Files each copy that is due and returns the IDs of the copies filed.
pub fn file_due(conn: &mut Connection, now: Timestamp) -> Result<Vec<String>> {
    let mut filed = Vec::new();
    for meta in schedules::active(conn)? {
        if schedules::next_copy(conn, &meta, now)? != Some(NextCopy::Now) {
            continue;
        }
        let request = BatchRequest {
            // A person may pause or edit the meta-ticket between the read and this write; the
            // copy is filed only if it is still the version that was read.
            preconditions: vec![Precondition::Version {
                id: meta.id.clone(),
                version: meta.version,
            }],
            ops: vec![copy_of(&meta)],
        };
        if let BatchOutcome::Applied(reply) =
            batch::apply(conn, &request, SCHEDULE.actor, SCHEDULE.machine, now)?
        {
            filed.extend(reply.results.iter().map(|result| result.id.to_string()));
        }
    }
    Ok(filed)
}

/// The create of one copy of `meta`, under it.
fn copy_of(meta: &Ticket) -> Operation {
    Operation::Create {
        fields: TicketFields {
            title: Some(meta.title.clone()),
            body: Some(meta.body.clone()),
            acceptance: Some(meta.acceptance.clone()),
            design: Some(meta.design.clone()),
            // The notes are what earlier copies left for this one: a copy writes them on the
            // meta-ticket, and the next copy starts with them.
            notes: Some(meta.notes.clone()),
            ticket_type: Some(TicketType::Task),
            status: Some(Status::Open),
            priority: Some(meta.priority),
            labels: Some(meta.labels.clone()),
            cores: meta.cores.map(Some),
            estimate_minutes: meta.estimate_minutes.map(Some),
            unfiled_minutes: meta.unfiled_minutes.map(Some),
            needs_os: meta.needs_os.map(Some),
            fallback: Some(meta.fallback),
            ..TicketFields::default()
        },
        parent: Some(meta.id.to_string()),
        deps: Vec::new(),
        request_key: None,
    }
}
