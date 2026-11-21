//! Pin resolution for dispatch, including the fallback for work holding master back.

use std::collections::HashSet;

use rusqlite::Connection;

use ignite_core::pins::Pins;
use ignite_core::protocol::tickets::Ticket;
use ignite_core::time::Timestamp;
use ignite_store::Result;
use ignite_store::hub::{fleet, pins};

/// A blocker pinned to a machine can run elsewhere after half an hour of silence.
const FALLBACK_SECONDS: f64 = 30.0 * 60.0;

/// Resolve each inherited pin. Only the machine restriction can expire.
pub(super) fn effective(
    conn: &Connection,
    ticket: &Ticket,
    heard: &HashSet<String>,
) -> Result<Pins> {
    let mut pins = pins::of(conn, ticket.id.as_str())?;
    if let Some(affinity) = crate::upkeep::culprit_affinity::affinity(ticket)
        && ticket.labels.contains(&affinity.machine_label)
        && let Some(machine) = affinity.machine_label.strip_prefix("machine:")
    {
        if heard.contains(&machine.to_ascii_lowercase()) {
            pins.machine = Some(machine.to_owned());
            pins.provider = Some(affinity.landing.provider);
        } else {
            // The transcript stays on the original machine. Any fallback worker starts fresh.
            pins.machine = None;
        }
        return Ok(pins);
    }
    if ticket.fallback
        && pins
            .machine
            .as_ref()
            .is_some_and(|name| !heard.contains(&name.to_ascii_lowercase()))
    {
        pins.machine = None;
    }
    Ok(pins)
}

/// The names, in lower case, of the machines the hub heard from in the last
/// [`FALLBACK_SECONDS`].
pub(super) fn heard_machines(conn: &Connection, now: Timestamp) -> Result<HashSet<String>> {
    Ok(fleet::machines(conn)?
        .into_iter()
        .filter(|machine| {
            machine
                .last_seen
                .parse::<Timestamp>()
                .is_ok_and(|seen| now.seconds_since(seen) < FALLBACK_SECONDS)
        })
        .map(|machine| machine.name.to_ascii_lowercase())
        .collect())
}

pub(super) fn allows_continuation(
    conn: &Connection,
    ticket: &ignite_core::dispatch::ReadyTicket,
    machine: &ignite_core::ids::MachineId,
    name: &str,
    app: &str,
) -> Result<bool> {
    if !ticket.pins.admits_machine(name) || !ticket.pins.admits_provider(app) {
        return Ok(false);
    }
    let Some(needed) = ticket.needs_os else {
        return Ok(true);
    };
    Ok(fleet::machines(conn)?
        .iter()
        .any(|row| row.id == machine.as_str() && row.os == needed.as_str()))
}
