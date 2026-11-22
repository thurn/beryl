//! Closing the tickets the hub filed by itself once the fault they report has cleared.
//!
//! Upkeep notices each fault on every pass, and without this it never noticed one clearing, so
//! tickets for faults long gone piled up for a person to check by hand, and a released one could
//! send a worker after a fault that no longer existed.
//!
//! For faults reported from sightings, upkeep asks [`clearing_kinds::verdict`] whether the
//! fault still holds and applies the waiting period below. Regression tickets use verified
//! check results instead and are settled by [`regressions::settle`].
//!
//! - A ticket in progress gets a comment when its fault first clears.
//! - A ticket that anyone but its filers commented on, edited or claimed is left alone.
//!   A release from pending approval does not count.
//! - Otherwise the fault must stay clear for [`HOLD_SECONDS`], or [`RETURNED_HOLD_SECONDS`]
//!   if it has come back since an automatic close.
//! - A fault that returns within [`filing::REOPEN_SECONDS`] reopens the ticket.
//!
//! Tickets filed before tickets recorded their fault get it here when their signature is one
//! upkeep can rebuild from a subject it can list (an epic, a ticket, a machine or a fixed
//! subject); the rest are marked as not watched.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::Connection;
use serde_json::{Value, json};

use ignite_core::protocol::tickets::{BatchRequest, Operation, Precondition, Status, Ticket};
use ignite_core::time::Timestamp;
use ignite_store::Result;
use ignite_store::hub::batch::{self, BatchOutcome};
use ignite_store::hub::{fleet, tickets};

use super::clearing_kinds::{self, Fault, Verdict, World};
use super::filing::{
    self, AUTO_CLOSED_KEY, CLEARED_SINCE_KEY, FAULT_KEY, FILERS_KEY, RETURNS_KEY,
    STATUS_BEFORE_KEY, UPKEEP, strings,
};
use super::signature::Signature;
use super::{justiciar, regressions};
use crate::routes::tip_rounds as rounds_route;

/// How long a fault must stay clear before its ticket closes.
pub const HOLD_SECONDS: f64 = 3_600.0;

/// How long a fault that came back once after an automatic close must stay clear.
pub const RETURNED_HOLD_SECONDS: f64 = 86_400.0;

/// The ticket metadata key that records that a worker's ticket was told its fault cleared.
pub const NOTED_KEY: &str = "ignite_cleared_noted";

/// The words every automatic close reason starts with.
pub const CLOSE_PREFIX: &str = "Ignite closed this ticket because";

/// Closes each watched ticket whose fault has stayed clear long enough; returns the tickets
/// closed.
pub fn close_cleared(conn: &mut Connection, now: Timestamp) -> Result<Vec<String>> {
    backfill(conn, now)?;
    let world = World::load(conn, now)?;
    let mut closed = regressions::settle(conn, now)?;
    for id in watched(conn)? {
        let Some(ticket) = tickets::load(conn, &id)? else {
            continue;
        };
        let verdict = match Fault::of(&ticket.metadata) {
            Some(fault) => clearing_kinds::verdict(conn, &world, &fault, &ticket)?,
            None => Verdict::Unwatched,
        };
        // A worker in the middle of the ticket is told, whoever else touched it.
        let working = ticket.status == Status::InProgress
            && matches!(verdict, Verdict::Cleared(_) | Verdict::ConfirmedClear(_));
        if verdict == Verdict::Unwatched || (!working && touched(conn, &ticket)?) {
            continue;
        }
        if act(conn, &ticket, verdict, now)? {
            closed.push(id);
        }
    }
    Ok(closed)
}

/// Every open ticket that records a fault upkeep watches.
fn watched(conn: &Connection) -> Result<Vec<String>> {
    let mut select = conn.prepare(
        "SELECT id FROM tickets
         WHERE status != 'closed' AND json_extract(metadata, '$.ignite_fault.category') != ''
         ORDER BY id",
    )?;
    let ids = select
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    Ok(ids)
}

/// Whether anyone but the ticket's filers and upkeep commented on, edited or claimed it. A
/// release, which only moves it from `pending_approval` to `open`, does not count.
fn touched(conn: &Connection, ticket: &Ticket) -> Result<bool> {
    let mut automatic: BTreeSet<String> =
        strings(&ticket.metadata, FILERS_KEY).into_iter().collect();
    if automatic.is_empty() {
        automatic.insert(ticket.created_by.clone());
    }
    automatic.insert(UPKEEP.actor.to_owned());
    let commented = tickets::comments(conn, ticket.id.as_str())?
        .iter()
        .any(|comment| !automatic.contains(&comment.author));
    if commented {
        return Ok(true);
    }
    Ok(tickets::history(conn, ticket.id.as_str())?
        .iter()
        .any(|row| !automatic.contains(&row.actor) && !is_release(&row.change)))
}

/// Whether a history row's change only released the ticket.
fn is_release(change: &Value) -> bool {
    let Some(fields) = change.as_object() else {
        return false;
    };
    fields.get("status") == Some(&json!([Status::PendingApproval, Status::Open]))
        && fields
            .keys()
            .all(|key| matches!(key.as_str(), "status" | "updated_at" | "version"))
}

/// Acts on the verdict of a ticket nobody touched; returns whether it closed.
fn act(conn: &mut Connection, ticket: &Ticket, verdict: Verdict, now: Timestamp) -> Result<bool> {
    let id = ticket.id.to_string();
    let since = ticket
        .metadata
        .get(CLEARED_SINCE_KEY)
        .and_then(Value::as_str)
        .and_then(|at| Timestamp::parse(at).ok());
    let (reason, confirmed) = match verdict {
        Verdict::Cleared(reason) => (reason, false),
        Verdict::ConfirmedClear(reason) => (reason, true),
        Verdict::Present | Verdict::Unwatched => {
            if since.is_some() {
                apply(
                    conn,
                    ticket,
                    vec![meta(&id, BTreeMap::new(), &[CLEARED_SINCE_KEY])],
                    now,
                )?;
            }
            return Ok(false);
        }
    };
    match ticket.status {
        Status::InProgress => {
            if !ticket.metadata.contains_key(NOTED_KEY) {
                let ops = vec![
                    Operation::Comment {
                        id: id.clone(),
                        text: format!(
                            "The fault this ticket reports has cleared: {reason} Ignite leaves \
                             a ticket in progress to its worker; close it if nothing is left \
                             to do."
                        ),
                    },
                    meta(
                        &id,
                        BTreeMap::from([(NOTED_KEY, json!(now.to_string()))]),
                        &[],
                    ),
                ];
                apply(conn, ticket, ops, now)?;
            }
            Ok(false)
        }
        Status::Open | Status::PendingApproval => {
            if !confirmed {
                let Some(since) = since else {
                    let set = BTreeMap::from([(CLEARED_SINCE_KEY, json!(now.to_string()))]);
                    apply(conn, ticket, vec![meta(&id, set, &[])], now)?;
                    return Ok(false);
                };
                let returned = ticket
                    .metadata
                    .get(RETURNS_KEY)
                    .and_then(Value::as_u64)
                    .is_some_and(|returns| returns > 0);
                let hold = if returned {
                    RETURNED_HOLD_SECONDS
                } else {
                    HOLD_SECONDS
                };
                if now.seconds_since(since) < hold {
                    return Ok(false);
                }
            }
            let set = BTreeMap::from([
                (AUTO_CLOSED_KEY, json!(now.to_string())),
                (STATUS_BEFORE_KEY, json!(ticket.status)),
            ]);
            let ops = vec![
                meta(&id, set, &[CLEARED_SINCE_KEY, NOTED_KEY]),
                Operation::Close {
                    id: id.clone(),
                    reason: format!("{CLOSE_PREFIX} its fault cleared: {reason}"),
                    include_ready: false,
                },
            ];
            apply(conn, ticket, ops, now)
        }
        // A deferred or blocked ticket was put there by someone, who decides.
        _ => Ok(false),
    }
}

/// An update that only sets and unsets metadata.
pub(super) fn meta(id: &str, set: BTreeMap<&str, Value>, unset: &[&str]) -> Operation {
    Operation::Update {
        id: id.to_owned(),
        set: None,
        add_labels: Vec::new(),
        remove_labels: Vec::new(),
        set_meta: set
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
        unset_meta: unset.iter().map(|key| (*key).to_owned()).collect(),
    }
}

/// Applies `ops` if the ticket has not changed since it was read; returns whether they applied.
/// A ticket that changed is looked at again on the next pass.
fn apply(
    conn: &mut Connection,
    ticket: &Ticket,
    ops: Vec<Operation>,
    now: Timestamp,
) -> Result<bool> {
    let request = BatchRequest {
        preconditions: vec![Precondition::Version {
            id: ticket.id.clone(),
            version: ticket.version,
        }],
        ops,
    };
    Ok(matches!(
        batch::apply(conn, &request, UPKEEP.actor, UPKEEP.machine, now)?,
        BatchOutcome::Applied(_)
    ))
}

/// Gives each open ticket filed before tickets recorded their fault the fault its signature
/// stands for, when upkeep can rebuild that signature, and an empty category otherwise, which
/// is never watched.
fn backfill(conn: &mut Connection, now: Timestamp) -> Result<()> {
    let mut select = conn.prepare(
        "SELECT id, json_extract(metadata, '$.ignite_signature') FROM tickets
         WHERE status != 'closed' AND json_extract(metadata, '$.ignite_signature') IS NOT NULL
           AND json_extract(metadata, '$.ignite_fault') IS NULL",
    )?;
    let missing = select
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(select);
    if missing.is_empty() {
        return Ok(());
    }
    let mut known: BTreeMap<String, Signature> = BTreeMap::new();
    let mut add = |signature: Signature| {
        known.insert(signature.key(), signature);
    };
    let ids = {
        let mut select = conn.prepare("SELECT id FROM tickets")?;
        select
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    for id in &ids {
        add(Signature::of_subject(id, "stuck-epic"));
        add(Signature::of_subject(id, "held"));
    }
    let mut systems: BTreeSet<String> = ["macos", "windows"].map(str::to_owned).into();
    for machine in fleet::machines(conn)? {
        add(Signature::of_subject(&machine.id, "unreachable"));
        systems.insert(machine.os);
    }
    for os in systems {
        add(Signature::of_subject(&format!("os:{os}"), "unreachable"));
    }
    add(Signature::of_subject(
        justiciar::MASTER_LAG_SUBJECT,
        justiciar::LABEL,
    ));
    add(Signature::of_subject(
        rounds_route::SUBJECT,
        rounds_route::CATEGORY,
    ));
    for (id, key) in missing {
        let fault = known
            .get(&key)
            .map_or_else(|| json!({ "category": "" }), filing::fault_value);
        let op = meta(&id, BTreeMap::from([(FAULT_KEY, fault)]), &[]);
        let request = BatchRequest {
            preconditions: Vec::new(),
            ops: vec![op],
        };
        batch::apply(conn, &request, UPKEEP.actor, UPKEEP.machine, now)?;
    }
    Ok(())
}
