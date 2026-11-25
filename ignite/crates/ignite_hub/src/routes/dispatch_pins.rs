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

/// A blocker pinned by culprit affinity can run elsewhere after ten minutes ready and
/// unclaimed, even while its machine still reports. A pinned machine with a free slot takes the
/// ticket within one dispatch wait, so ten minutes unclaimed means its slots are busy with other
/// work. The culprit's transcript saves a repair a few minutes of reading at most, which is less
/// than what `master` loses while it waits longer.
const BUSY_FALLBACK_SECONDS: f64 = 10.0 * 60.0;

/// Resolve each inherited pin. Only the machine restriction can expire.
pub(super) fn effective(
    conn: &Connection,
    ticket: &Ticket,
    heard: &HashSet<String>,
    now: Timestamp,
) -> Result<Pins> {
    let mut pins = pins::of(conn, ticket.id.as_str())?;
    if let Some(affinity) = crate::upkeep::culprit_affinity::affinity(ticket)
        && ticket.labels.contains(&affinity.machine_label)
        && let Some(machine) = affinity.machine_label.strip_prefix("machine:")
    {
        let live = heard.contains(&machine.to_ascii_lowercase())
            && !(ticket.fallback
                && now.seconds_since(ready_since(conn, ticket)?) >= BUSY_FALLBACK_SECONDS);
        if live {
            pins.machine = Some(machine.to_owned());
            pins.provider = Some(affinity.landing.provider);
        } else {
            // The machine is silent, or busy past `BUSY_FALLBACK_SECONDS` while `master` waits.
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

/// When the ticket last became claimable: its last release, or its creation if it was never
/// released. A release clears `claimed_at`, and the history records that change.
fn ready_since(conn: &Connection, ticket: &Ticket) -> Result<Timestamp> {
    let released: Option<String> = conn.query_row(
        "SELECT max(at) FROM history WHERE ticket = ?1 \
         AND json_type(change, '$.claimed_at[1]') = 'null'",
        [ticket.id.as_str()],
        |row| row.get(0),
    )?;
    Ok(released
        .and_then(|at| at.parse::<Timestamp>().ok())
        .map_or(ticket.created_at, |at| at.max(ticket.created_at)))
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
    if ticket.needs_os.is_none() && ticket.families.is_empty() {
        return Ok(true);
    }
    Ok(fleet::dispatch_capabilities(conn, machine.as_str())?
        .is_some_and(|(_, os, families)| ticket.platform_fits(os, &families)))
}
