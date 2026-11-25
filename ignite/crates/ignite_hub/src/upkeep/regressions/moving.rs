//! Moving one check off a regression ticket onto another ticket (`POST /regressions/move`).
//!
//! The hub groups failing checks onto few regression tickets, so one ticket can hold a check
//! that a different landing broke. A worker who splits such a check out by filing a ticket of
//! its own must also move the check's row in `regressions`: while the row still named the old
//! ticket, each later failure of the check reopened the old ticket ("The check still fails
//! after the fix") and the landing gate listed the check as known-failing under it, so the old
//! ticket could never stay closed.
//!
//! A move points the row at the target and records the move on both tickets. The target gets
//! the `regression` and `check:<family>` labels, so dispatch treats it as a regression, and
//! [`ADDED_KEY`], so that only tips seen after the move can settle it (see [`super::resolution`]).
//! The source keeps its other checks and settles once they pass. Moving a check onto the ticket
//! that already holds it changes nothing, so a repeated request is harmless.

use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde_json::json;

use ignite_core::ids::{CheckName, TicketId};
use ignite_core::protocol::tickets::{BatchRequest, Operation, Status};
use ignite_core::time::Timestamp;
use ignite_store::hub::batch::{self, BatchOutcome};
use ignite_store::hub::tickets;
use ignite_store::{Result, StoreError};

use super::ADDED_KEY;
use crate::upkeep::filing::Author;

/// What a move did: the ticket the check was on and the ticket it is on now. They are equal
/// when the check was already on the target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Moved {
    pub from: TicketId,
    pub to: TicketId,
}

/// Moves `check`'s regression onto the ticket `target` names (an ID or an alias). Refused when
/// the check has no regression, the target does not exist, or the target is closed: a closed
/// ticket would not hold the check back from `master`'s gate, so reopen it first.
pub fn move_check(
    conn: &mut Connection,
    author: Author,
    check: &CheckName,
    target: &str,
    now: Timestamp,
) -> Result<Moved> {
    let mut tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let from: String = tx
        .query_row(
            "SELECT ticket FROM regressions WHERE check_name = ?1",
            params![check.as_str()],
            |row| row.get(0),
        )
        .optional()?
        .ok_or_else(|| StoreError::Rejected(format!("{check} has no regression ticket")))?;
    let to = tickets::resolve(&tx, target)?
        .ok_or_else(|| StoreError::Rejected(format!("no ticket {target}")))?;
    let from: TicketId = from
        .parse()
        .map_err(|error| StoreError::Invalid(format!("ticket ID {from:?}: {error}")))?;
    if from == to {
        return Ok(Moved { from, to });
    }
    let loaded = tickets::load(&tx, to.as_str())?
        .ok_or_else(|| StoreError::Rejected(format!("no ticket {to}")))?;
    if loaded.status == Status::Closed {
        return Err(StoreError::Rejected(format!(
            "{to} is closed; reopen it before moving {check} onto it"
        )));
    }
    tx.execute(
        "UPDATE regressions SET ticket = ?2 WHERE check_name = ?1",
        params![check.as_str(), to.as_str()],
    )?;
    let remaining = remaining(&tx, from.as_str())?;
    let left = if remaining.is_empty() {
        "No other failing check is on this ticket.".to_owned()
    } else {
        format!(
            "This ticket is done when its other checks pass: {}.",
            remaining.join(", ")
        )
    };
    let ops = vec![
        Operation::Comment {
            id: to.to_string(),
            text: format!(
                "{actor} moved the failing check {check} here from {from}. A later failure of \
                 {check} reopens this ticket, not {from}.",
                actor = author.actor
            ),
        },
        Operation::Update {
            id: to.to_string(),
            set: None,
            add_labels: vec!["regression".to_owned(), format!("check:{}", check.family())],
            remove_labels: Vec::new(),
            set_meta: BTreeMap::from([(ADDED_KEY.to_owned(), json!(now.to_string()))]),
            unset_meta: Vec::new(),
        },
        Operation::Comment {
            id: from.to_string(),
            text: format!(
                "{actor} moved the failing check {check} to {to}. {left}",
                actor = author.actor
            ),
        },
    ];
    let request = BatchRequest {
        preconditions: Vec::new(),
        ops,
    };
    match batch::apply_in(&mut tx, &request, author.actor, author.machine, now)? {
        BatchOutcome::Applied(_) => {}
        BatchOutcome::Conflict(conflict) => {
            return Err(StoreError::Rejected(format!(
                "moving {check} to {to} conflicted at operation {}",
                conflict.failed
            )));
        }
    }
    tx.commit()?;
    Ok(Moved { from, to })
}

/// The checks still on `ticket` whose regression is not fixed.
fn remaining(conn: &Connection, ticket: &str) -> Result<Vec<String>> {
    let mut select = conn.prepare(
        "SELECT check_name FROM regressions WHERE ticket = ?1 AND state != ?2
         ORDER BY check_name",
    )?;
    let names = select
        .query_map(params![ticket, ignite_store::hub::tips::FIXED], |row| {
            row.get(0)
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(names)
}

#[cfg(test)]
#[path = "moving_tests.rs"]
mod tests;
