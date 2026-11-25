//! `POST /emergency`: declares, renews or adds to the emergency pause (see
//! `ignite_core::protocol::emergency`).

use std::collections::BTreeMap;

use rusqlite::{Connection, params};
use serde_json::{Value, json};

use ignite_core::protocol::emergency::{Emergency, EmergencyChange, EmergencyReply};
use ignite_core::protocol::tickets::{BatchRequest, Operation};
use ignite_core::time::Timestamp;
use ignite_store::StoreError;
use ignite_store::hub::batch;
use ignite_store::hub::emergency;

use super::{Answer, Request, Response, body, canonical, failure, same_machine, store_failure};
use crate::upkeep::filing::UPKEEP;

/// The ticket metadata key that holds the number of the comment `ignite tell` delivers.
const TELL_KEY: &str = "tell";

pub(super) fn change(conn: &mut Connection, request: &Request, now: Timestamp) -> Answer {
    // Tickets are stored by their canonical ID, which is how a daemon names the ticket a tree
    // holds.
    let mut parsed: EmergencyChange = body(request)?;
    match &mut parsed {
        EmergencyChange::Declare {
            machine, ticket, ..
        } => {
            same_machine(request, machine)?;
            *ticket = canonical(conn, ticket)?;
        }
        EmergencyChange::Allow { allowance, .. } => {
            allowance.ticket = canonical(conn, &allowance.ticket)?;
        }
    }
    let before = emergency::in_force(conn, now).map_err(|e| store_failure(&e))?;
    let changed = emergency::change(conn, &parsed, now).map_err(|error| match error {
        StoreError::Rejected(message) => failure(409, &message),
        other => store_failure(&other),
    })?;
    if let Some(replaced) = before
        && (replaced.ticket != changed.ticket || replaced.session != changed.session)
    {
        // The new emergency is already in force. A failure to tell must not undo it.
        if let Err(error) = tell_replaced(conn, &replaced, &changed, now) {
            eprintln!(
                "ignite: the declarer of emergency {} was not told it was replaced: {error}",
                replaced.ticket
            );
        }
    }
    Ok(Response::json(
        200,
        &json!(EmergencyReply {
            emergency: Some(changed),
        }),
    ))
}

/// Tells the session that declared `replaced` that `by` replaced it: a comment on its ticket,
/// with [`TELL_KEY`] set to the comment's number so the daemon of the machine holding the
/// ticket delivers it to the session (the mechanism `ignite tell` uses).
fn tell_replaced(
    conn: &mut Connection,
    replaced: &Emergency,
    by: &Emergency,
    now: Timestamp,
) -> Result<(), StoreError> {
    let id = replaced.ticket.as_str();
    let last: i64 = conn.query_row(
        "SELECT coalesce(max(n), 0) FROM comments WHERE ticket = ?1",
        params![id],
        |row| row.get(0),
    )?;
    let text = format!(
        "Emergency {} was replaced at {now} by emergency {} that {} declared on {}. {} no longer \
         holds the emergency, so `ignite emergency allow` and `ignite pool resume --all` no \
         longer act on it. To end the new emergency anyway, run `ignite pool resume --all \
         --emergency {}`.",
        replaced.ticket, by.ticket, by.session, by.machine, replaced.session, by.ticket
    );
    let request = BatchRequest {
        preconditions: Vec::new(),
        ops: vec![
            Operation::Comment {
                id: id.to_owned(),
                text,
            },
            Operation::Update {
                id: id.to_owned(),
                set: None,
                add_labels: Vec::new(),
                remove_labels: Vec::new(),
                set_meta: BTreeMap::from([(TELL_KEY.to_owned(), Value::from(last + 1))]),
                unset_meta: Vec::new(),
            },
        ],
    };
    batch::apply(conn, &request, UPKEEP.actor, UPKEEP.machine, now)?;
    Ok(())
}
