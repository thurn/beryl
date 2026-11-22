//! The hub's provider holds for dispatch and heartbeat replies.

use std::collections::BTreeMap;

use rusqlite::Connection;

use ignite_core::dispatch::{Handoff, ReadyTicket, label};
use ignite_core::fleet::{FleetNotice, FleetSettings};
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
    Ok(FleetNotice {
        version: settings.version,
        state: machine.state,
        state_actor: fleet_settings::state_actor(conn, &name)?,
        disk_cap_gb: machine.disk_cap_gb,
        holds: provider_usage::held(conn, &settings, now)?,
        off_platforms: fleet_settings::off_machines(conn, &settings)?
            .into_keys()
            .collect(),
    })
}

fn blocking_hold<'a>(
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

/// Why a legacy daemon's idle worker cannot receive a new reservation.
pub(super) fn legacy_blocker(
    settings: &FleetSettings,
    holds: &BTreeMap<String, Vec<Hold>>,
    worker: &IdleWorker,
    ticket: &ReadyTicket,
) -> Option<String> {
    if !settings.provider(&worker.app).enabled {
        return Some(format!("{} is off", worker.app));
    }
    let hold = blocking_hold(holds, worker, ticket)?;
    Some(format!(
        "{} is on a usage hold ({})",
        worker.app,
        hold.describe()
    ))
}

/// Explains why a legacy worker that would otherwise receive the hand-off cannot take it.
pub(super) fn unavailable_reason(
    settings: &FleetSettings,
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
    legacy_blocker(settings, holds, worker, ticket)
}
