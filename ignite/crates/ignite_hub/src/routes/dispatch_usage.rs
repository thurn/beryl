//! The hub's provider holds for dispatch and heartbeat replies.

use std::collections::BTreeMap;

use rusqlite::Connection;

use ignite_core::dispatch::{ReadyTicket, label};
use ignite_core::fleet::FleetNotice;
use ignite_core::plan_usage::Hold;
use ignite_core::protocol::hub::IdleWorker;
use ignite_core::time::Timestamp;
use ignite_store::Result;
use ignite_store::hub::{fleet, fleet_settings, provider_usage};

pub(super) fn holds(conn: &Connection, now: Timestamp) -> Result<BTreeMap<String, Vec<Hold>>> {
    provider_usage::held(conn, &fleet_settings::settings(conn)?, now)
}

pub(super) fn notice(conn: &Connection, machine: &str, now: Timestamp) -> Result<FleetNotice> {
    let settings = fleet_settings::settings(conn)?;
    let name = fleet::name_of(conn, machine)?.unwrap_or_default();
    let machine = settings.machine(&name);
    Ok(FleetNotice {
        version: settings.version,
        state: machine.state,
        disk_cap_gb: machine.disk_cap_gb,
        holds: provider_usage::held(conn, &settings, now)?,
        ..FleetNotice::default()
    })
}

pub(super) fn blocking_hold<'a>(
    holds: &'a BTreeMap<String, Vec<Hold>>,
    worker: &IdleWorker,
    ticket: &ReadyTicket,
) -> Option<&'a Hold> {
    let windows = holds.get(&worker.app)?;
    let model = if let Some(tier) = label(&ticket.labels, "tier:") {
        let fallback = match (worker.app.as_str(), tier) {
            ("claude", "light") => "sonnet",
            _ => tier,
        };
        worker.model_ids.get(tier).map_or(fallback, String::as_str)
    } else {
        worker.default_model.as_deref().unwrap_or({
            // Older daemons did not advertise model IDs. Claude's untiered tickets use Sonnet.
            if worker.app == "claude" { "sonnet" } else { "" }
        })
    };
    windows.iter().find(|hold| hold.holds_model(model))
}
