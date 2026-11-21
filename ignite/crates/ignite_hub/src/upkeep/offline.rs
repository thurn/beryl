//! Claims held by a machine that has gone offline.
//!
//! A machine that stops reporting (powered off, asleep, off the network) cannot release its
//! tickets. Without this step they stay `in_progress` for good and count against their epic's
//! allocation of workers. [`release_claims`] gives back every claim of a machine whose last
//! heartbeat is more than [`OFFLINE_SECONDS`] old. The ticket returns to the queue with a
//! comment, and a daemon that comes back finds its claim lost, which `/landed` and `/release`
//! answer with 409 (exit code 3).
//!
//! [`release_unheld`] gives back a ticket that is in progress although no Ignite session or
//! machine holds it, once it has gone [`OFFLINE_SECONDS`] without a change. Such a ticket was
//! claimed outside Ignite: by the tracker it was imported from, or by a status set by hand. No
//! heartbeat ever ends that claim, and the stall rule covers only Ignite's own workers, so
//! without this step the ticket would count against its epic's allocation for good.

use rusqlite::{Connection, TransactionBehavior};
use serde_json::Value;

use ignite_core::protocol::tickets::{BatchRequest, Operation, Precondition, Status, TicketFields};
use ignite_core::time::Timestamp;
use ignite_store::hub::batch::{self, BatchOutcome};
use ignite_store::{Result, StoreError};

use super::parse_time;
use crate::routes::dispatch::set_columns;

/// How long a machine may go without a heartbeat before its claims are released: 2 hours.
pub const OFFLINE_SECONDS: f64 = 7_200.0;

/// The actor that the release is recorded as.
const ACTOR: &str = super::filing::UPKEEP.actor;

/// Releases the claims of machines silent for [`OFFLINE_SECONDS`]. Returns the tickets released.
pub fn release_claims(conn: &mut Connection, now: Timestamp) -> Result<Vec<String>> {
    let held = {
        let mut select = conn.prepare(
            "SELECT t.id, t.assignee, t.tree, m.id, m.name, m.last_seen
             FROM tickets t JOIN machines m ON m.id = t.machine
             WHERE t.status != 'closed' AND t.assignee IS NOT NULL ORDER BY t.id",
        )?;
        select
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    let mut released = Vec::new();
    for (id, assignee, tree, machine, name, last_seen) in held {
        let silent = parse_time(&last_seen)?;
        if now.seconds_since(silent) < OFFLINE_SECONDS {
            continue;
        }
        let tree = tree.map_or(String::new(), |tree| format!(" ({tree})"));
        let text = format!(
            "Released by the hub: machine {name} ({machine}) has not reported since {last_seen}. \
             Any unsaved work stays in that machine's tree{tree}, and in its saved work on \
             the remote if any was pushed. The ticket is back in the queue."
        );
        let ops = vec![
            Operation::Update {
                id: id.clone(),
                set: Some(TicketFields {
                    status: Some(Status::Open),
                    assignee: Some(None),
                    ..TicketFields::default()
                }),
                add_labels: Vec::new(),
                remove_labels: Vec::new(),
                set_meta: std::collections::BTreeMap::new(),
                unset_meta: Vec::new(),
            },
            Operation::Comment {
                id: id.clone(),
                text,
            },
        ];
        let request = BatchRequest {
            preconditions: vec![Precondition::Assignee {
                id: id
                    .parse()
                    .map_err(|e| StoreError::Invalid(format!("{e}")))?,
                assignee: Some(assignee.clone()),
            }],
            ops,
        };
        let mut tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        match batch::apply_in(&mut tx, &request, ACTOR, super::filing::UPKEEP.machine, now)? {
            BatchOutcome::Applied(_) => {}
            // Someone changed the ticket since it was read; the next pass looks again.
            BatchOutcome::Conflict(_) => continue,
        }
        let cleared = ["session", "tree", "machine", "claimed_at", "provider"]
            .map(|column| (column, Value::Null));
        set_columns(&tx, &id, ACTOR, &cleared, now)?;
        tx.execute(
            "UPDATE workers SET ticket = NULL, state = 'idle' WHERE machine = ?1 AND ticket = ?2",
            rusqlite::params![machine, id],
        )?;
        tx.commit()?;
        released.push(id);
    }
    Ok(released)
}

/// Releases each ticket in progress that no session and no machine holds and that has not
/// changed for [`OFFLINE_SECONDS`]. Returns the tickets released.
pub fn release_unheld(conn: &mut Connection, now: Timestamp) -> Result<Vec<String>> {
    let unheld = {
        let mut select = conn.prepare(
            "SELECT id, assignee, updated_at FROM tickets
             WHERE status = 'in_progress' AND session IS NULL AND machine IS NULL ORDER BY id",
        )?;
        select
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    let mut released = Vec::new();
    for (id, assignee, updated_at) in unheld {
        if now.seconds_since(parse_time(&updated_at)?) < OFFLINE_SECONDS {
            continue;
        }
        let holder = assignee
            .as_deref()
            .map_or_else(|| "nobody".to_owned(), |a| format!("`{a}`"));
        let text = format!(
            "Released by the hub: this ticket was in progress, held by {holder}, but no Ignite \
             session or machine holds it, and it has not changed since {updated_at}. Work from \
             before Ignite may be saved on a branch of the remote that names the ticket. The \
             ticket is back in the queue."
        );
        let request = BatchRequest {
            preconditions: vec![Precondition::Assignee {
                id: id
                    .parse()
                    .map_err(|e| StoreError::Invalid(format!("{e}")))?,
                assignee,
            }],
            ops: vec![
                Operation::Update {
                    id: id.clone(),
                    set: Some(TicketFields {
                        status: Some(Status::Open),
                        assignee: Some(None),
                        ..TicketFields::default()
                    }),
                    add_labels: Vec::new(),
                    remove_labels: Vec::new(),
                    set_meta: std::collections::BTreeMap::new(),
                    unset_meta: Vec::new(),
                },
                Operation::Comment {
                    id: id.clone(),
                    text,
                },
            ],
        };
        let mut tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        match batch::apply_in(&mut tx, &request, ACTOR, super::filing::UPKEEP.machine, now)? {
            BatchOutcome::Applied(_) => {}
            // Someone changed the ticket since it was read; the next pass looks again.
            BatchOutcome::Conflict(_) => continue,
        }
        tx.commit()?;
        released.push(id);
    }
    Ok(released)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::upkeep::tests::apply;
    use ignite_core::config::Os;
    use ignite_core::ids::MachineId;
    use ignite_core::protocol::hub::MachineRegister;
    use ignite_store::hub::{self, fleet, tickets};

    fn at(seconds: f64) -> Timestamp {
        Timestamp::parse("2026-11-17T12:00:00Z")
            .unwrap()
            .plus_seconds(seconds)
    }

    #[test]
    fn a_claim_of_a_machine_silent_for_two_hours_returns_to_the_queue_with_a_comment() {
        let mut conn = hub::open_in_memory().unwrap();
        let register = MachineRegister {
            machine: MachineId::new("m-1").unwrap(),
            name: "jarvis".to_owned(),
            os: Os::Macos,
            cores: 16,
            memory_mb: 65_536,
            families: Vec::new(),
            version: "v".to_owned(),
        };
        fleet::register(&conn, &register, at(0.0)).unwrap();
        let ids = apply(
            &mut conn,
            vec![
                crate::upkeep::tests::create(
                    "work",
                    ignite_core::protocol::tickets::TicketType::Task,
                    None,
                    &[],
                ),
                Operation::Claim { id: "$0".into() },
            ],
            at(0.0),
        );
        conn.execute(
            "UPDATE tickets SET session = 'local_a', tree = 'tree-03' WHERE id = ?1",
            [&ids[0]],
        )
        .unwrap();

        // Under two hours since the last heartbeat: nothing happens.
        assert!(release_claims(&mut conn, at(7_199.0)).unwrap().is_empty());
        let held = tickets::load(&conn, &ids[0]).unwrap().unwrap();
        assert_eq!(held.status, Status::InProgress);

        // A heartbeat keeps the claim.
        fleet::register(&conn, &register, at(3_600.0)).unwrap();
        assert!(release_claims(&mut conn, at(10_000.0)).unwrap().is_empty());

        let released = release_claims(&mut conn, at(3_600.0 + 7_200.0)).unwrap();
        assert_eq!(released, [ids[0].clone()]);
        let ticket = tickets::load(&conn, &ids[0]).unwrap().unwrap();
        assert_eq!(ticket.status, Status::Open);
        assert_eq!(ticket.assignee, None);
        assert_eq!(ticket.session, None);
        assert_eq!(ticket.tree, None);
        let comments = tickets::comments(&conn, &ids[0]).unwrap();
        assert!(comments[0].text.contains("jarvis"), "{}", comments[0].text);
        assert!(
            comments[0].text.contains("unsaved work"),
            "{}",
            comments[0].text
        );
        // Nothing is left to release on the next pass.
        assert!(release_claims(&mut conn, at(20_000.0)).unwrap().is_empty());
    }

    #[test]
    fn a_ticket_in_progress_that_no_ignite_session_holds_returns_to_the_queue() {
        let mut conn = hub::open_in_memory().unwrap();
        let ids = apply(
            &mut conn,
            vec![
                crate::upkeep::tests::create(
                    "imported",
                    ignite_core::protocol::tickets::TicketType::Task,
                    None,
                    &[],
                ),
                crate::upkeep::tests::create(
                    "worked",
                    ignite_core::protocol::tickets::TicketType::Task,
                    None,
                    &[],
                ),
            ],
            at(0.0),
        );
        // The first is held as the old tracker left it: an assignee, but no session or
        // machine. The second is held by an Ignite session on a machine.
        conn.execute(
            "UPDATE tickets SET status = 'in_progress', assignee = 'old-run' WHERE id = ?1",
            [&ids[0]],
        )
        .unwrap();
        conn.execute(
            "UPDATE tickets SET status = 'in_progress', assignee = 'local_a', session = 'local_a',
               machine = 'm-1' WHERE id = ?1",
            [&ids[1]],
        )
        .unwrap();

        // Under two hours since the last change: nothing happens.
        assert!(release_unheld(&mut conn, at(7_199.0)).unwrap().is_empty());

        let released = release_unheld(&mut conn, at(7_200.0)).unwrap();
        assert_eq!(released, [ids[0].clone()]);
        let ticket = tickets::load(&conn, &ids[0]).unwrap().unwrap();
        assert_eq!(ticket.status, Status::Open);
        assert_eq!(ticket.assignee, None);
        let comments = tickets::comments(&conn, &ids[0]).unwrap();
        assert!(
            comments[0].text.contains("`old-run`"),
            "{}",
            comments[0].text
        );
        let worked = tickets::load(&conn, &ids[1]).unwrap().unwrap();
        assert_eq!(worked.status, Status::InProgress);
        // Nothing is left to release on the next pass.
        assert!(release_unheld(&mut conn, at(20_000.0)).unwrap().is_empty());
    }
}
