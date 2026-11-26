//! Counting releases in `POST /release`, and holding back a ticket that different sessions
//! released (see `ignite_store::hub::releases`).
//!
//! A release is counted when it came from a worker giving up or from Ignite taking the ticket
//! back (`ReleaseCause::counts`) and names nothing the ticket needs. A release with `needs`
//! sends the ticket to a person, an operating system or a machine, so the next worker who is
//! offered it does not meet the same obstacle. A justiciar ticket is never counted, because
//! holding back an emergency would stop the work that ends it. Nor is a ticket that is
//! deferred to a later time when it is released: its worker found it was waiting on a clock
//! (a block ticket of a model trial whose pairs are still running, say), deferred it and gave
//! it back, and the next worker meets no obstacle once the deferral ends. The same holds for a
//! ticket with an open dependency: its worker found the work cannot start until the blocking
//! ticket closes, so a later worker meets no obstacle once it does.

use rusqlite::{Connection, TransactionBehavior, params};
use serde_json::Value;

use ignite_core::config::Os;

use ignite_core::dispatch::JUSTICIAR;
use ignite_core::pins::machine_label;
use ignite_core::protocol::hub::Release;
use ignite_core::protocol::tickets::{Operation, Status, Ticket, TicketFields};
use ignite_core::time::Timestamp;
use ignite_store::StoreError;
use ignite_store::hub::releases::{self, Releases};
use ignite_store::hub::tickets;

use super::{
    Answer, SAVED_BRANCH, apply_in, done, failure, free_worker, holds, load, set_columns,
    sql_failure, store_failure,
};
use crate::upkeep::regressions;

/// Gives a ticket back to the queue with the note as a comment, records the branch its work was
/// saved to and, with `needs`, records what it needs: `person` labels it `needs:person`, which
/// sets it aside for a person; an operating system sets the system it needs; a machine's name
/// pins the ticket to that machine. A session may also give back a
/// reservation it has not claimed yet. A ticket released by different sessions is held back
/// for a justiciar (see `dispatch_releases.rs`).
pub(super) fn release(conn: &mut Connection, release: &Release, now: Timestamp) -> Answer {
    let id = release.ticket.as_str();
    let mut tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_failure)?;
    let ticket = load(&tx, id)?;
    if ticket.assignee.as_deref() != Some(release.session.as_str()) {
        let dropped = tx
            .execute(
                "DELETE FROM reservations WHERE ticket = ?1 AND session = ?2",
                params![id, release.session.as_str()],
            )
            .map_err(sql_failure)?;
        if dropped > 0 {
            if ticket.assignee.is_none()
                && let Some(passing) =
                    regressions::passing(&tx, &ticket).map_err(|e| store_failure(&e))?
            {
                passing
                    .close(&mut tx, &ticket, now)
                    .map_err(|e| store_failure(&e))?;
            }
            tx.commit().map_err(sql_failure)?;
            return Ok(done());
        }
    }
    holds(&ticket, &release.session)?;
    let mut set = TicketFields {
        status: Some(Status::Open),
        assignee: Some(None),
        ..TicketFields::default()
    };
    let mut add_labels = Vec::new();
    let mut remove_labels = Vec::new();
    match release.needs.as_deref().map(str::trim) {
        None => {}
        Some("person") => add_labels.push("needs:person".to_owned()),
        Some(needs) => {
            if let Ok(os) = serde_json::from_value::<Os>(Value::String(needs.to_owned())) {
                set.needs_os = Some(Some(os));
            } else {
                if needs.is_empty()
                    || !needs
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
                {
                    return Err(failure(
                        400,
                        "write a machine name, an operating system or person",
                    ));
                }
                let label = machine_label(needs);
                remove_labels.extend(
                    ticket
                        .labels
                        .iter()
                        .filter(|old| old.starts_with("machine:") && **old != label)
                        .cloned(),
                );
                add_labels.push(label);
            }
        }
    }
    let set_meta = release
        .branch
        .iter()
        .map(|branch| (SAVED_BRANCH.to_owned(), Value::from(branch.as_str())))
        .collect();
    let ops = vec![
        Operation::Update {
            id: id.to_owned(),
            set: Some(set),
            add_labels,
            remove_labels,
            set_meta,
            unset_meta: Vec::new(),
        },
        Operation::Comment {
            id: id.to_owned(),
            text: format!("Released: {}", release.reason),
        },
    ];
    apply_in(&mut tx, ops, &release.session, &release.machine, now)?;
    let cleared = ["session", "tree", "machine", "claimed_at", "provider"]
        .map(|column| (column, Value::Null));
    set_columns(&tx, id, release.session.as_str(), &cleared, now).map_err(|e| store_failure(&e))?;
    free_worker(&tx, &release.machine, &release.session, None)?;
    let held =
        if let Some(passing) = regressions::passing(&tx, &ticket).map_err(|e| store_failure(&e))? {
            passing
                .close(&mut tx, &ticket, now)
                .map_err(|e| store_failure(&e))?;
            None
        } else {
            count(&tx, &ticket, release, now).map_err(|e| store_failure(&e))?
        };
    tx.commit().map_err(sql_failure)?;
    if let Some(releases) = held {
        file_after(conn, &ticket, &releases, now);
    }
    Ok(done())
}

/// Records the release in the release's transaction when it counts, and returns the ticket's
/// releases when it is now held back, for [`file_after`] once the transaction commits.
fn count(
    tx: &Connection,
    ticket: &Ticket,
    release: &Release,
    now: Timestamp,
) -> Result<Option<Releases>, StoreError> {
    let waiting = ticket.defer_until.is_some_and(|until| until > now);
    // Rule 1 is the ticket's own state, which is in progress while it is released, so only the
    // later rules (a dependency that has not closed) mean it is waiting on another ticket.
    let blocked = tickets::readiness(tx, std::slice::from_ref(ticket), now)?
        .remove(0)
        .iter()
        .any(|blocker| blocker.rule != 1);
    let counted = release.cause.counts()
        && release.needs.is_none()
        && !waiting
        && !blocked
        && !ticket.labels.iter().any(|label| label == JUSTICIAR);
    if !counted {
        return Ok(None);
    }
    let releases = releases::record(
        tx,
        ticket.id.as_str(),
        release.session.as_str(),
        release.cause,
        &release.reason,
        now,
    )?;
    Ok(releases.held_back().then_some(releases))
}

/// Files or comments on the justiciar ticket for a ticket that is held back. The release has
/// already committed, so a failure here is logged and leaves the ticket held back: it is not
/// offered either way, and the next release of it files again.
fn file_after(conn: &mut Connection, ticket: &Ticket, releases: &Releases, now: Timestamp) {
    let filed = crate::upkeep::justiciar::released_repeatedly(
        conn,
        ticket.id.as_str(),
        &ticket.title,
        releases,
        now,
    );
    if let Err(error) = filed {
        eprintln!(
            "ignite-hub: {} is held back after repeated releases, but its justiciar ticket was \
             not filed: {error}",
            ticket.id
        );
    }
}
