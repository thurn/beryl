//! `POST /emergency`: declares, renews, adds to, adopts or ends the emergency pause (see
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
        }
        | EmergencyChange::Adopt {
            machine, ticket, ..
        } => {
            same_machine(request, machine)?;
            *ticket = canonical(conn, ticket)?;
        }
        EmergencyChange::Allow { allowance, .. } => {
            allowance.ticket = canonical(conn, &allowance.ticket)?;
        }
        EmergencyChange::End { ticket, .. } => {
            if let Some(named) = ticket {
                *named = canonical(conn, named)?;
            }
        }
    }
    let before = emergency::in_force(conn, now).map_err(|e| store_failure(&e))?;
    let changed = emergency::change(conn, &parsed, now).map_err(|error| match error {
        StoreError::Rejected(message) => failure(409, &message),
        other => store_failure(&other),
    })?;
    if let Some(replaced) = before
        && let Some(changed) = &changed
        && (replaced.ticket != changed.ticket || replaced.session != changed.session)
    {
        // The new emergency is already in force. A failure to tell must not undo it.
        let told = if matches!(parsed, EmergencyChange::Adopt { .. }) {
            record_adoption(conn, &replaced, changed, now)
        } else {
            tell_replaced(conn, &replaced, changed, now)
        };
        if let Err(error) = told {
            eprintln!(
                "ignite: the hand-over of emergency {} was not recorded: {error}",
                replaced.ticket
            );
        }
    }
    Ok(Response::json(
        200,
        &json!(EmergencyReply { emergency: changed }),
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
    let text = format!(
        "Emergency {} was replaced at {now} by emergency {} that {} declared on {}. {} no longer \
         holds the emergency, so `ignite emergency allow` and `ignite emergency end` no \
         longer act on it. To end the new emergency anyway, run `ignite emergency end {}`.",
        replaced.ticket, by.ticket, by.session, by.machine, replaced.session, by.ticket
    );
    comment(conn, id, text, true, now)
}

/// Records on both tickets that `by` adopted `adopted`: the ticket it left is told, the way
/// [`tell_replaced`] tells it, and the adopting ticket keeps the record.
fn record_adoption(
    conn: &mut Connection,
    adopted: &Emergency,
    by: &Emergency,
    now: Timestamp,
) -> Result<(), StoreError> {
    let scope = match by.scope {
        ignite_core::protocol::emergency::EmergencyScope::All => "heavy verbs and hand-offs",
        ignite_core::protocol::emergency::EmergencyScope::Landings => "landings only",
    };
    let ends = match by.until_master_holds {
        Some(since) => format!(
            "It still ends by itself once `master` holds every landing made before {since}."
        ),
        None => format!("It lapses at {} unless renewed.", by.expires_at()),
    };
    let left = format!(
        "Emergency {} was adopted at {now} by {} on {}, onto ticket {}. It still pauses {scope} \
         and keeps its allowances, and this ticket is allowed every heavy verb. {ends} \
         `ignite emergency allow` and `ignite emergency end` now belong to {}.",
        adopted.ticket, by.session, by.machine, by.ticket, by.session
    );
    comment(conn, adopted.ticket.as_str(), left, true, now)?;
    let taken = format!(
        "{} adopted emergency {} (declared by {}) onto this ticket at {now}. It pauses {scope}. \
         {ends}",
        by.session, adopted.ticket, adopted.session
    );
    comment(conn, by.ticket.as_str(), taken, false, now)
}

/// Comments `text` on ticket `id`. With `tell`, it also sets [`TELL_KEY`] to the comment's
/// number, so the daemon of the machine holding the ticket delivers it to its session (the
/// mechanism `ignite tell` uses).
fn comment(
    conn: &mut Connection,
    id: &str,
    text: String,
    tell: bool,
    now: Timestamp,
) -> Result<(), StoreError> {
    let last: i64 = conn.query_row(
        "SELECT coalesce(max(n), 0) FROM comments WHERE ticket = ?1",
        params![id],
        |row| row.get(0),
    )?;
    let mut ops = vec![Operation::Comment {
        id: id.to_owned(),
        text,
    }];
    if tell {
        ops.push(Operation::Update {
            id: id.to_owned(),
            set: None,
            add_labels: Vec::new(),
            remove_labels: Vec::new(),
            set_meta: BTreeMap::from([(TELL_KEY.to_owned(), Value::from(last + 1))]),
            unset_meta: Vec::new(),
        });
    }
    let request = BatchRequest {
        preconditions: Vec::new(),
        ops,
    };
    batch::apply(conn, &request, UPKEEP.actor, UPKEEP.machine, now)?;
    Ok(())
}
