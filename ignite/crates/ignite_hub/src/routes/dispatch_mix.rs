//! The fleet's provider load and the provider choices for a room-based hand-off.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::Connection;

use ignite_core::dispatch::{ReadyTicket, label};
use ignite_core::fleet::FleetSettings;
use ignite_core::mix::{self, ProviderLoad};
use ignite_core::plan_usage::{Hold, model_for_tier};
use ignite_core::protocol::hub::Room;
use ignite_core::time::Timestamp;
use ignite_store::Result;
use ignite_store::hub::{fleet_settings, machine_asks, provider_usage};

use super::queue::held;

/// Include every known provider, even one absent from this machine's offers, so
/// dispatch can explain why it was unavailable.
pub(super) fn loads(
    conn: &Connection,
    settings: &FleetSettings,
    holds: &BTreeMap<String, Vec<Hold>>,
    room: &Room,
    now: Timestamp,
) -> Result<Vec<ProviderLoad>> {
    // Claims and reservations made before the provider column was populated
    // can still contribute when the worker reported its provider in a heartbeat.
    let mut statement = conn.prepare(
        "SELECT provider, count(*) FROM (
             SELECT coalesce(t.provider, w.app) AS provider
             FROM tickets t LEFT JOIN workers w USING (machine, session)
             WHERE t.status = 'in_progress'
             UNION ALL
             SELECT coalesce(r.provider, w.app) AS provider
             FROM reservations r LEFT JOIN workers w USING (machine, session)
             WHERE r.expires_at > ?1
         ) WHERE provider IS NOT NULL GROUP BY provider",
    )?;
    let counts = statement
        .query_map([now.to_string()], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<BTreeMap<String, u32>>>()?;
    let mut names = settings.providers.keys().collect::<BTreeSet<_>>();
    names.extend(holds.keys());
    names.extend(counts.keys());
    names.extend(room.providers.iter().map(|offer| &offer.name));
    Ok(names
        .into_iter()
        .map(|name| ProviderLoad {
            name: name.clone(),
            setting: settings.provider(name),
            in_progress: counts.get(name).copied().unwrap_or(0),
            holds: holds.get(name).cloned().unwrap_or_default(),
        })
        .collect())
}

pub(super) fn pending(conn: &Connection, machine: &str, now: Timestamp) -> Result<u32> {
    Ok(conn.query_row(
        "SELECT count(*) FROM reservations WHERE machine = ?1 AND expires_at > ?2",
        (machine, now.to_string()),
        |row| row.get(0),
    )?)
}

pub(super) fn choose<'a>(
    loads: &'a [ProviderLoad],
    room: &Room,
    pending: u32,
    ticket: &ReadyTicket,
) -> std::result::Result<&'a str, String> {
    if room.memory_pressed {
        return Err("waits for memory pressure to ease".to_owned());
    }
    if pending >= room.start && !held::holds_master(ticket) {
        return Err("waits for room to start another worker".to_owned());
    }
    mix::choose(
        loads,
        &room.providers,
        label(&ticket.labels, "tier:"),
        ticket.pins.provider.as_deref(),
    )
}

/// A lane already has a worker, so it doesn't take room or compete for shares.
/// Use the latest room offer's tiers and model IDs when available. An older
/// daemon without those details can choose another provider at its next ask.
pub(super) fn continuation_blocked(
    conn: &Connection,
    machine: &str,
    provider: &str,
    ticket: &ReadyTicket,
    now: Timestamp,
) -> Result<bool> {
    let settings = fleet_settings::settings(conn)?;
    if !settings.provider(provider).enabled {
        return Ok(true);
    }
    let mut holds = provider_usage::held(conn, &settings, now)?;
    let holds = holds.remove(provider).unwrap_or_default();
    let offer = machine_asks::latest(conn, machine)?
        .and_then(|ask| ask.free.room)
        .and_then(|room| {
            room.providers
                .into_iter()
                .find(|offer| offer.name == provider)
        });
    let Some(offer) = offer else {
        return Ok(!holds.is_empty());
    };
    let tier = label(&ticket.labels, "tier:");
    let model = model_for_tier(
        provider,
        tier,
        &offer.model_ids,
        offer.default_model.as_deref(),
    );
    Ok(
        tier.is_some_and(|tier| !offer.tiers.iter().any(|available| available == tier))
            || holds.iter().any(|hold| hold.holds_model(model)),
    )
}
