//! Routes repairs to the machine and provider that hold a CLI landing's transcript.
//!
//! A reopened landing is pinned there with a `machine:` label that Ignite removes when it lands
//! again. A regression holds `master` back, so its affinity is only a preference in its
//! metadata, which dispatch weighs against the speed of the machines that are free
//! (`routes::dispatch_speed`).

use std::collections::BTreeMap;

use ignite_core::config::Os;
use ignite_core::pins::machine_label;
use ignite_core::protocol::tickets::{
    CLI_LANDING_KEY, CULPRIT_AFFINITY_KEY, CliLanding, CulpritAffinity, Ticket, cli_provider,
};
use ignite_store::Result;
use ignite_store::hub::fleet;
use rusqlite::Connection;
use serde_json::{Value, json};

/// Only old, closed tickets need to recover the landing from their last claim.
pub fn landing(ticket: &Ticket) -> Option<CliLanding> {
    if let Some(value) = ticket.metadata.get(CLI_LANDING_KEY) {
        return serde_json::from_value(value.clone()).ok();
    }
    ticket.landed.as_ref()?;
    let session = ticket.session.as_ref()?;
    (ticket.status == ignite_core::protocol::tickets::Status::Closed).then_some(CliLanding {
        machine: ticket.machine.clone()?,
        provider: cli_provider(session)?.to_owned(),
    })
}

/// The affinity of `repair` for `culprit`'s landing. With `preference`, no label is added.
pub fn for_repair(
    conn: &Connection,
    culprit: &Ticket,
    repair: Option<&Ticket>,
    needed_os: Option<Os>,
    preference: bool,
) -> Result<Option<CulpritAffinity>> {
    let Some(landing) = landing(culprit) else {
        return Ok(None);
    };
    let Some(machine) = fleet::machines(conn)?
        .into_iter()
        .find(|machine| machine.id == landing.machine.as_str())
    else {
        return Ok(None);
    };
    if needed_os.is_some_and(|os| os.as_str() != machine.os) {
        return Ok(None);
    }
    let machine_label = machine_label(&machine.name);
    let already_added = repair
        .and_then(affinity)
        .is_some_and(|old| old.added_label && old.machine_label == machine_label);
    let already_labelled = repair.is_some_and(|ticket| ticket.labels.contains(&machine_label));
    Ok(Some(CulpritAffinity {
        landing,
        machine_label,
        added_label: !preference && (already_added || !already_labelled),
    }))
}

pub fn affinity(ticket: &Ticket) -> Option<CulpritAffinity> {
    serde_json::from_value(ticket.metadata.get(CULPRIT_AFFINITY_KEY)?.clone()).ok()
}

pub fn added_label(ticket: &Ticket) -> Option<String> {
    added_label_of(&affinity(ticket)?)
}

fn added_label_of(pin: &CulpritAffinity) -> Option<String> {
    pin.added_label.then(|| pin.machine_label.clone())
}

/// Replaces the temporary routing while preserving the ticket's own labels.
pub fn update(
    ticket: &Ticket,
    pin: Option<CulpritAffinity>,
    set_meta: &mut BTreeMap<String, Value>,
) -> (Vec<String>, Vec<String>, Vec<String>) {
    let previous = added_label(ticket);
    let remove = previous
        .filter(|label| {
            pin.as_ref()
                .is_none_or(|pin| !pin.added_label || pin.machine_label != *label)
        })
        .into_iter()
        .collect();
    let Some(pin) = pin else {
        return (Vec::new(), remove, vec![CULPRIT_AFFINITY_KEY.to_owned()]);
    };
    let add = added_label_of(&pin).into_iter().collect();
    set_meta.insert(CULPRIT_AFFINITY_KEY.to_owned(), json!(pin));
    (add, remove, Vec::new())
}
