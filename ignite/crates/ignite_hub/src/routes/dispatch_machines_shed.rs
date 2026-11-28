//! `POST /shed`, `POST /shed/done`, and what a heartbeat's reply says about the machine's build
//! tickets (see `ignite_core::protocol::hub::shed`).

use rusqlite::Connection;

use ignite_core::protocol::hub::Heartbeat;
use ignite_core::protocol::hub::shed::{BuildHoldings, ShedDone, ShedOrder};
use ignite_core::protocol::machine_state::MachineSample;
use ignite_core::time::Timestamp;
use ignite_store::hub::{fleet, shed};

use super::super::{Answer, Request, done, failure, same_machine, store_failure};

/// The machine's build tickets and limit, or `None` when they cannot be counted. A failure
/// must not cost the machine its heartbeat, so it is logged and the next sample tries again.
pub(in crate::routes) fn holdings(
    conn: &Connection,
    beat: &Heartbeat,
    sample: &MachineSample,
    now: Timestamp,
) -> Option<BuildHoldings> {
    let counted = super::super::queue::shed::holdings(
        conn,
        beat.machine.as_str(),
        &sample.pressure,
        f64::from(sample.admit_cores),
        now,
    );
    counted.unwrap_or_else(|error| {
        eprintln!(
            "ignite: build tickets of {} not counted: {error}",
            beat.machine
        );
        None
    })
}

/// Removes and returns the order waiting for the beat's machine.
pub(in crate::routes) fn take_order(conn: &Connection, beat: &Heartbeat) -> Option<ShedOrder> {
    shed::take(conn, beat.machine.as_str()).unwrap_or_else(|error| {
        eprintln!("ignite: shed order for {} not read: {error}", beat.machine);
        None
    })
}

/// Keeps an order for the named machine until its next heartbeat with a sample.
pub(in crate::routes) fn order(conn: &Connection, order: &ShedOrder) -> Answer {
    let machines = fleet::machines(conn).map_err(|e| store_failure(&e))?;
    let wanted = order.machine.as_str();
    let Some(machine) = machines
        .iter()
        .find(|m| m.id == wanted || m.name.eq_ignore_ascii_case(wanted))
    else {
        return Err(failure(
            404,
            &format!("no machine is registered as {wanted}"),
        ));
    };
    shed::order(conn, &machine.id, order).map_err(|e| store_failure(&e))?;
    Ok(done())
}

/// Records what a machine shed in the fleet's recent changes, as the machine that shed.
pub(in crate::routes) fn record(
    conn: &Connection,
    request: &Request,
    shed_done: &ShedDone,
    now: Timestamp,
) -> Answer {
    same_machine(request, &shed_done.machine)?;
    let actor = fleet::name_of(conn, shed_done.machine.as_str())
        .map_err(|e| store_failure(&e))?
        .unwrap_or_else(|| shed_done.machine.to_string());
    shed::record(conn, &actor, &shed_done.summary, now).map_err(|e| store_failure(&e))?;
    Ok(done())
}
