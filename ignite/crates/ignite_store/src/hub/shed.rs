//! Orders to shed build tickets, waiting for their machine's next heartbeat, and the record of
//! each shed in the fleet's recent changes (see `ignite_core::protocol::hub::shed`).

use rusqlite::{Connection, params};

use ignite_core::protocol::hub::shed::ShedOrder;
use ignite_core::time::Timestamp;

use super::{fleet_settings, meta};
use crate::{Result, StoreError};

fn key(machine: &str) -> String {
    format!("shed_order:{machine}")
}

/// Keeps `order` for the machine with ID `machine`, replacing an order it has not received.
pub fn order(conn: &Connection, machine: &str, order: &ShedOrder) -> Result<()> {
    meta::put(conn, &key(machine), order)
}

/// Removes and returns the order waiting for the machine with ID `machine`.
pub fn take(conn: &Connection, machine: &str) -> Result<Option<ShedOrder>> {
    let key = key(machine);
    let order = meta::get::<ShedOrder>(conn, &key)?;
    if order.is_some() {
        conn.execute("DELETE FROM meta WHERE key = ?1", [&key])?;
    }
    Ok(order)
}

/// Records a shed in the fleet's recent changes. The row keeps the settings in force, which a
/// shed does not change, so that the settings' history reads the same with it as without.
pub fn record(conn: &Connection, actor: &str, summary: &str, now: Timestamp) -> Result<()> {
    let settings = serde_json::to_string(&fleet_settings::settings(conn)?)
        .map_err(|error| StoreError::Invalid(format!("fleet settings: {error}")))?;
    conn.execute(
        "INSERT INTO fleet_changes (at, actor, summary, settings) VALUES (?1, ?2, ?3, ?4)",
        params![now.to_string(), actor, summary, settings],
    )?;
    Ok(())
}
