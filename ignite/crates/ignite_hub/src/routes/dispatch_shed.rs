//! The build tickets a machine holds and its limit, counted as `POST /next` counts them, for
//! the reply to each heartbeat that carries a machine sample (see
//! `ignite_core::protocol::hub::shed`).

use rusqlite::Connection;

use ignite_core::dispatch_pressure::{Budget, DispatchPressure};
use ignite_core::ids::TicketId;
use ignite_core::protocol::hub::shed::{BuildHoldings, HeldBuild};
use ignite_core::time::Timestamp;
use ignite_store::hub::tickets;
use ignite_store::{Result, StoreError};

use super::{Requirements, capacity, held, history, landed_files};

/// The machine's build tickets, newest claim first, and its limit; `None` when it reported no
/// cores to set a limit by.
pub fn holdings(
    conn: &Connection,
    machine: &str,
    pressure: &DispatchPressure,
    admit_cores: f64,
    now: Timestamp,
) -> Result<Option<BuildHoldings>> {
    let requirements = Requirements::load(conn)?;
    let capacity = capacity::read(
        conn,
        machine,
        &history(conn, machine)?,
        &landed_files(conn)?,
        now,
        &requirements,
        Budget::new(Some(pressure), Some(admit_cores), now),
    )?;
    let Some(limit) = capacity.budget.build_limit() else {
        return Ok(None);
    };
    let mut held_builds = Vec::with_capacity(capacity.builds.len());
    for (id, since, claimed) in capacity.builds {
        let session = if claimed {
            tickets::load(conn, &id)?.and_then(|ticket| ticket.session)
        } else {
            None
        };
        held_builds.push(HeldBuild {
            master_blocker: held::holds_master_id(conn, &id)?,
            ticket: TicketId::new(&id).map_err(|e| StoreError::Invalid(e.to_string()))?,
            session,
            since,
        });
    }
    held_builds.sort_by_key(|held| std::cmp::Reverse(held.since));
    Ok(Some(BuildHoldings {
        limit,
        tickets: held_builds,
    }))
}
