//! Opening the hub's own tickets once their automated source has workers.
//!
//! The hub files improvements, flaky-check investigations, exploration findings and other
//! upkeep tickets in `pending_approval`, which the dashboard shows as allocation zero. Each of
//! those tickets belongs to an automated source (`ignite_core::shares::Source`), and the source's
//! allocation is the operator's control over them: its own weight, or the General pool's when it
//! has none. A ticket the hub left pending opens here as soon as that allocation is positive, so
//! it inherits its source's allocation the way a ticket under an epic inherits the epic's. A
//! source set to zero keeps its tickets parked.
//!
//! A ticket that a person set to `pending_approval` stays there: only a ticket whose latest
//! status change came from its filers or the hub is opened.

use std::collections::BTreeSet;

use rusqlite::{Connection, TransactionBehavior};

use ignite_core::protocol::tickets::{BatchRequest, Operation, Status, TicketFields};
use ignite_core::shares::{DEFAULT_GENERAL_ALLOCATION, Source};
use ignite_core::time::Timestamp;
use ignite_store::hub::batch::{self, BatchOutcome};
use ignite_store::hub::{fleet_settings, tickets};
use ignite_store::{Result, StoreError};

use super::filing::{FILERS_KEY, SOURCE_FIELDS, UPKEEP, allocation_source, strings};

/// Opens every standalone ticket the hub left pending whose source has a positive allocation;
/// returns their IDs.
pub fn release(conn: &mut Connection, now: Timestamp) -> Result<Vec<String>> {
    let allocations = fleet_settings::settings(conn)?
        .allocations
        .unwrap_or_default();
    let allocation = |source: Source| {
        allocations
            .sources
            .get(&source)
            .copied()
            .or(allocations.general)
            .unwrap_or(DEFAULT_GENERAL_ALLOCATION)
    };
    let mut tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let candidates = {
        let mut select = tx.prepare(&format!(
            "SELECT id, {SOURCE_FIELDS} FROM tickets
             WHERE status = 'pending_approval' AND parent IS NULL ORDER BY id"
        ))?;
        select
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, allocation_source(row, 1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    let mut released = Vec::new();
    for (id, source) in candidates {
        if source.is_none_or(|source| allocation(source) == 0) || !left_pending_by_hub(&tx, &id)? {
            continue;
        }
        released.push(id);
    }
    if released.is_empty() {
        return Ok(released);
    }
    let request = BatchRequest {
        preconditions: Vec::new(),
        ops: released
            .iter()
            .map(|id| Operation::Update {
                id: id.clone(),
                set: Some(TicketFields {
                    status: Some(Status::Open),
                    ..TicketFields::default()
                }),
                add_labels: Vec::new(),
                remove_labels: Vec::new(),
                set_meta: std::collections::BTreeMap::default(),
                unset_meta: Vec::new(),
            })
            .collect(),
    };
    if let BatchOutcome::Conflict(conflict) =
        batch::apply_automatic_in(&mut tx, &request, UPKEEP.actor, UPKEEP.machine, now)?
    {
        return Err(StoreError::Invalid(format!(
            "opening tickets of sources with workers: {conflict:?}"
        )));
    }
    tx.commit()?;
    Ok(released)
}

/// Whether the latest change to the ticket's status was made by its filers or the hub, rather
/// than by a person parking it.
fn left_pending_by_hub(conn: &Connection, id: &str) -> Result<bool> {
    let Some(ticket) = tickets::load(conn, id)? else {
        return Ok(false);
    };
    let mut automatic: BTreeSet<String> =
        strings(&ticket.metadata, FILERS_KEY).into_iter().collect();
    automatic.insert(ticket.created_by.clone());
    automatic.insert(UPKEEP.actor.to_owned());
    automatic.insert(crate::rules::filing::ACTOR.to_owned());
    let history = tickets::history(conn, id)?;
    let setter = history
        .iter()
        .rev()
        .find(|row| row.change.get("status").is_some())
        .map(|row| row.actor.as_str());
    Ok(setter.is_none_or(|actor| automatic.contains(actor)))
}
