//! Durable platform verification coverage. The daemon compares commits with git, and the hub
//! only writes the replacement if no other round has changed that platform meanwhile.

use std::collections::BTreeMap;

use ignite_core::config::Os;
use ignite_core::protocol::tickets::FeedChange;
use ignite_core::tipround::{PlatformVerification, TipCoverage, TipCoverageUpdate};
use rusqlite::{Connection, TransactionBehavior};

use super::{feed, fleet_settings, meta};
use crate::{Result, StoreError};

const KEY: &str = "tip_coverage";

fn platforms(conn: &Connection) -> Result<BTreeMap<Os, PlatformVerification>> {
    Ok(meta::get(conn, KEY)?.unwrap_or_default())
}

pub fn get(conn: &Connection) -> Result<TipCoverage> {
    let settings = fleet_settings::settings(conn)?;
    Ok(TipCoverage {
        platforms: platforms(conn)?,
        off_machines: fleet_settings::off_machines(conn, &settings)?,
    })
}

pub fn update(conn: &mut Connection, update: &TipCoverageUpdate) -> Result<bool> {
    let transaction = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut platforms = platforms(&transaction)?;
    if platforms.get(&update.platform) != update.expected.as_ref() {
        return Ok(false);
    }
    if update.replacement.deferred.is_some() {
        let settings = fleet_settings::settings(&transaction)?;
        let off = fleet_settings::off_machines(&transaction, &settings)?;
        if !off.contains_key(&update.platform) {
            return Err(StoreError::Rejected(format!(
                "{} can verify tips again",
                update.platform
            )));
        }
    }
    platforms.insert(update.platform, update.replacement.clone());
    meta::put(&transaction, KEY, &platforms)?;
    feed::record(
        &transaction,
        &FeedChange {
            seq: feed::next_seq(&transaction)?,
            kind: "attention".to_owned(),
            ticket: None,
            name: Some(update.platform.to_string()),
        },
    )?;
    transaction.commit()?;
    Ok(true)
}
