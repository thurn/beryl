//! The hub's provider holds for dispatch and heartbeat replies.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::Connection;

use ignite_core::config::Os;
use ignite_core::dispatch::{Handoff, ReadyTicket, label};
use ignite_core::fleet::{FleetNotice, MachineState};
use ignite_core::plan_usage::{Hold, model_for_tier};
use ignite_core::protocol::hub::{IdleWorker, MachineFree};
use ignite_core::time::Timestamp;
use ignite_store::Result;
use ignite_store::hub::{fleet, fleet_settings, provider_usage};

pub(super) fn holds(conn: &Connection, now: Timestamp) -> Result<BTreeMap<String, Vec<Hold>>> {
    provider_usage::held(conn, &fleet_settings::settings(conn)?, now)
}

pub(super) fn notice(conn: &Connection, machine: &str, now: Timestamp) -> Result<FleetNotice> {
    let settings = fleet_settings::settings(conn)?;
    let name = fleet::name_of(conn, machine)?.unwrap_or_else(|| machine.to_owned());
    let machine = settings.machine(&name);
    let mut platforms = BTreeSet::new();
    let mut enabled_platforms = BTreeSet::new();
    for known in fleet::machines(conn)? {
        let os = match known.os.as_str() {
            "macos" => Os::Macos,
            "windows" => Os::Windows,
            _ => continue,
        };
        platforms.insert(os);
        if settings.machine(&known.name).state != MachineState::Off {
            enabled_platforms.insert(os);
        }
    }
    Ok(FleetNotice {
        version: settings.version,
        state: machine.state,
        state_actor: fleet_settings::state_actor(conn, &name)?,
        disk_cap_gb: machine.disk_cap_gb,
        holds: provider_usage::held(conn, &settings, now)?,
        off_platforms: &platforms - &enabled_platforms,
    })
}

pub(super) fn blocking_hold<'a>(
    holds: &'a BTreeMap<String, Vec<Hold>>,
    worker: &IdleWorker,
    ticket: &ReadyTicket,
) -> Option<&'a Hold> {
    let windows = holds.get(&worker.app)?;
    let model = model_for_tier(
        &worker.app,
        label(&ticket.labels, "tier:"),
        &worker.model_ids,
        worker.default_model.as_deref(),
    );
    windows.iter().find(|hold| hold.holds_model(model))
}

/// Explains why the hand-off dispatch would have made without provider holds is blocked.
pub(super) fn hold_reason(
    holds: &BTreeMap<String, Vec<Hold>>,
    free: &MachineFree,
    ready: &[ReadyTicket],
    handoff: &Handoff,
) -> Option<String> {
    let worker = free
        .idle_workers
        .iter()
        .find(|worker| worker.session == handoff.session)?;
    let ticket = ready.iter().find(|ticket| ticket.id == handoff.ticket)?;
    let hold = blocking_hold(holds, worker, ticket)?;
    Some(format!(
        "{} is on a usage hold ({})",
        worker.app,
        hold.describe()
    ))
}
