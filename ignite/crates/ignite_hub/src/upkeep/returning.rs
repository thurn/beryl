//! Giving a returning machine back the claims nobody else took.
//!
//! [`super::offline::release_claims`] releases every claim of a machine silent for two hours
//! and records who held each ticket ([`ReleasedOffline`]). A machine that was only away comes
//! back with its CLI workers still in their trees, and its heartbeat lists the ticket each one
//! holds a tree for. [`reconcile`] answers each such ticket that the hub released from that same
//! machine and session (see [`ignite_core::protocol::returning`]): it claims the ticket again
//! for the session and tree when nobody touched it, and reports it lost otherwise. A ticket with
//! no record, or with a record of another machine or session, is left to the usual checks.

use rusqlite::{Connection, TransactionBehavior};
use serde_json::{Value, json};

use ignite_core::protocol::hub::Heartbeat;
use ignite_core::protocol::returning::{
    RELEASED_OFFLINE_KEY, Reconciled, ReleasedOffline, ReturnOutcome,
};
use ignite_core::protocol::tickets::{BatchRequest, Operation, Precondition, Status, Ticket};
use ignite_core::time::Timestamp;
use ignite_store::Result;
use ignite_store::hub::batch::{self, BatchOutcome};
use ignite_store::hub::tickets;

use crate::routes::dispatch::set_columns;

/// Claims again, for the same session and tree, each ticket the heartbeat's workers list that
/// the hub released from that session for silence and nobody has touched since; reports the
/// others as lost. Tickets the hub did not release that way are not in the answer.
pub fn reconcile(
    conn: &mut Connection,
    beat: &Heartbeat,
    now: Timestamp,
) -> Result<Vec<Reconciled>> {
    let mut answers = Vec::new();
    for worker in &beat.workers {
        let Some(ticket) = &worker.ticket else {
            continue;
        };
        let mut tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let Some(current) = tickets::load(&tx, ticket.as_str())? else {
            continue;
        };
        let Some(record) = ReleasedOffline::of(&current) else {
            continue;
        };
        if record.machine != beat.machine || record.session != worker.session {
            continue;
        }
        if current.status == Status::InProgress
            && current.assignee.as_deref() == Some(worker.session.as_str())
        {
            // Claimed again already, by a start that raced this heartbeat.
            continue;
        }
        // A live reservation for another worker makes the claim below conflict.
        let untouched = current.status == Status::Open
            && current.assignee.is_none()
            && current.updated_at <= record.at;
        let outcome = if untouched && take_back(&mut tx, &current, &record, now)? {
            ReturnOutcome::Reclaimed
        } else {
            ReturnOutcome::Lost
        };
        tx.commit()?;
        answers.push(Reconciled {
            session: worker.session.clone(),
            ticket: ticket.clone(),
            outcome,
        });
    }
    Ok(answers)
}

/// Claims `ticket` for the session and tree of `record`, provided it is still unassigned, and
/// says so in a comment. Returns whether the claim was made.
fn take_back(
    tx: &mut rusqlite::Transaction,
    ticket: &Ticket,
    record: &ReleasedOffline,
    now: Timestamp,
) -> Result<bool> {
    let id = ticket.id.to_string();
    let name: String = tx
        .query_row(
            "SELECT name FROM machines WHERE id = ?1",
            [record.machine.as_str()],
            |row| row.get(0),
        )
        .unwrap_or_else(|_| record.machine.to_string());
    let tree = record
        .tree
        .as_ref()
        .map_or(String::new(), |tree| format!(" in {tree}"));
    let text = format!(
        "Claimed again: machine {name} ({}) reported again after the hub released its claims \
         at {} for silence. Nobody had taken or changed the ticket since, so {} continues it{tree}.",
        record.machine, record.at, record.session
    );
    let request = BatchRequest {
        preconditions: vec![Precondition::Assignee {
            id: ticket.id.clone(),
            assignee: None,
        }],
        ops: vec![
            Operation::Claim { id: id.clone() },
            Operation::Update {
                id: id.clone(),
                set: None,
                add_labels: Vec::new(),
                remove_labels: Vec::new(),
                set_meta: std::collections::BTreeMap::new(),
                unset_meta: vec![RELEASED_OFFLINE_KEY.to_owned()],
            },
            Operation::Comment {
                id: id.clone(),
                text,
            },
        ],
    };
    let actor = record.session.as_str();
    match batch::apply_in(tx, &request, actor, record.machine.as_str(), now)? {
        BatchOutcome::Applied(_) => {}
        BatchOutcome::Conflict(_) => return Ok(false),
    }
    let tree = record
        .tree
        .as_ref()
        .map_or(Value::Null, |tree| Value::from(tree.as_str()));
    set_columns(
        tx,
        &id,
        actor,
        &[
            ("session", Value::from(actor)),
            ("tree", tree),
            ("provider", json!(record.provider)),
        ],
        now,
    )?;
    Ok(true)
}

#[cfg(test)]
#[path = "returning_tests.rs"]
mod tests;
