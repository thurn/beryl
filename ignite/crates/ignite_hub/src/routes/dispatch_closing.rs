//! Ticket changes shared by landing and closing.

use std::collections::BTreeMap;

use ignite_core::protocol::tickets::{
    CULPRIT_AFFINITY_KEY, LandedWork, Operation, REVERTED_KEY, Status, Ticket, TicketFields,
};

use rusqlite::Transaction;
use serde_json::json;

use ignite_core::protocol::hub::{Landed, LandedReply};
use ignite_core::time::Timestamp;
use ignite_store::hub::fix_outcomes::LandingFix;
use ignite_store::hub::{findings, fleet};

use super::{Response, SAVED_BRANCH, apply_in, load, queue, store_failure};

/// The operations that close `ticket` with `reason` and forget its saved branch, which the
/// closing session deletes from the remote.
pub(super) fn closing(ticket: &Ticket, reason: &str) -> Vec<Operation> {
    let mut ops = forgetting(ticket);
    ops.push(Operation::Close {
        id: ticket.id.as_str().to_owned(),
        reason: reason.to_owned(),
        include_ready: false,
    });
    ops
}

/// The operations that land `work` under the ticket of a landings freeze whose stall has not
/// ended (`crate::upkeep::freeze::holds_open`): they forget its saved branch as [`closing`]
/// does, but release the ticket and say why instead of closing it, so that the pool offers
/// it again while `master` still does not move.
pub(super) fn held_open(ticket: &Ticket, work: &LandedWork) -> Vec<Operation> {
    let id = ticket.id.as_str().to_owned();
    let mut ops = forgetting(ticket);
    ops.push(Operation::Update {
        id: id.clone(),
        set: Some(TicketFields {
            status: Some(Status::Open),
            assignee: Some(None),
            ..TicketFields::default()
        }),
        add_labels: Vec::new(),
        remove_labels: Vec::new(),
        set_meta: BTreeMap::new(),
        unset_meta: Vec::new(),
    });
    let commits: Vec<String> = work.commits.iter().map(ToString::to_string).collect();
    let commits = commits.join(", ");
    ops.push(Operation::Comment {
        id,
        text: format!(
            "Landed {commits}. This ticket stays open, and was released, because `master` \
             still lacks a landing made before landings froze. Upkeep closes it once `master` \
             catches up; until then the next worker continues from the mitigations recorded here."
        ),
    });
    ops
}

/// The operations that forget `ticket`'s saved branch and its reverted mark, if it has them.
fn forgetting(ticket: &Ticket) -> Vec<Operation> {
    let mut ops = Vec::new();
    // A landing applies the work again after a tip round reverted it, so `reverted` is done.
    let reverted = ticket.metadata.contains_key(REVERTED_KEY);
    let stale: Vec<String> = [SAVED_BRANCH, REVERTED_KEY]
        .into_iter()
        .chain(reverted.then_some(CULPRIT_AFFINITY_KEY))
        .filter(|key| ticket.metadata.contains_key(*key))
        .map(str::to_owned)
        .collect();
    if !stale.is_empty() {
        ops.push(Operation::Update {
            id: ticket.id.as_str().to_owned(),
            set: None,
            add_labels: Vec::new(),
            remove_labels: if reverted {
                crate::upkeep::culprit_affinity::added_label(ticket)
                    .into_iter()
                    .collect()
            } else {
                Vec::new()
            },
            set_meta: BTreeMap::new(),
            unset_meta: stale,
        });
    }
    ops
}

/// The answer to a repeat of a landing the hub already recorded, or `None` for a new landing.
/// A repeat returns the same reservation while it can still be used.
pub(super) fn repeat(
    tx: &Transaction,
    ticket: &Ticket,
    landed: &Landed,
    now: Timestamp,
) -> Result<Option<Response>, Response> {
    let id = landed.ticket.as_str();
    if ticket.status == Status::Closed && ticket.session.as_ref() == Some(&landed.session) {
        if ticket.landed.as_ref() != Some(&landed.work)
            || queue::chain_machine(tx, &landed.machine, now)
                .map_err(|e| store_failure(&e))?
                .is_none()
        {
            return Ok(Some(Response::json(
                200,
                &json!(LandedReply { next: None }),
            )));
        }
        let next = fleet::reservation_for_landing(
            tx,
            landed.machine.as_str(),
            landed.session.as_str(),
            id,
            now,
        )
        .map_err(|e| store_failure(&e))?;
        return Ok(Some(Response::json(200, &json!(LandedReply { next }))));
    }
    // A repeat of a landing that left a freeze's ticket open and released
    // (`closing::held_open`): the work is recorded and the ticket no longer has an assignee.
    if ticket.status != Status::Closed
        && ticket.assignee.is_none()
        && ticket.landed.as_ref() == Some(&landed.work)
    {
        return Ok(Some(Response::json(
            200,
            &json!(LandedReply { next: None }),
        )));
    }
    Ok(None)
}

/// Closes the landed ticket, unless it is the ticket of a landings freeze whose stall has not
/// ended: that one stays open and is released (`closing::held_open`).
pub(super) fn settle(
    tx: &mut Transaction,
    landed: &Landed,
    now: Timestamp,
    fix: &LandingFix,
) -> Result<(), Response> {
    let id = landed.ticket.as_str();
    if crate::upkeep::freeze::holds_open(tx, id).map_err(|e| store_failure(&e))? {
        let ops = held_open(&load(tx, id)?, &landed.work);
        apply_in(tx, ops, &landed.session, &landed.machine, now)?;
    } else {
        super::close_after_landing(tx, landed, now, fix)?;
        findings::close_ticket(tx, id, now).map_err(|e| store_failure(&e))?;
    }
    Ok(())
}
