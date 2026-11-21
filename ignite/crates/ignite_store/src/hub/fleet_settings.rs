//! Versioned fleet settings and their change log.

use rusqlite::{Connection, TransactionBehavior, params};

use ignite_core::fleet::{FleetChange, FleetSettings, summary};
use ignite_core::time::Timestamp;

use super::owners;
use super::rows;
use crate::{Result, StoreError};

const KEY: &str = "fleet";

#[derive(Debug, PartialEq, Eq)]
pub enum PutOutcome {
    Stored(FleetSettings),
    Conflict(FleetSettings),
}

/// The current settings. Version zero means the fleet has yet to be seeded.
pub fn settings(conn: &Connection) -> Result<FleetSettings> {
    Ok(owners::get(conn, KEY)?.unwrap_or_default())
}

/// Applies a change and records it together, or returns the settings that replaced the
/// caller's version.
pub fn put(
    conn: &mut Connection,
    expected_version: u64,
    mut proposed: FleetSettings,
    actor: &str,
    now: Timestamp,
) -> Result<PutOutcome> {
    let transaction = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let before = settings(&transaction)?;
    if expected_version != before.version {
        return Ok(PutOutcome::Conflict(before));
    }
    proposed.validate().map_err(StoreError::Rejected)?;
    proposed.version = before.version;
    if before.version != 0 && before == proposed {
        return Ok(PutOutcome::Stored(before));
    }
    proposed.version = before
        .version
        .checked_add(1)
        .ok_or_else(|| StoreError::Invalid("fleet settings version overflow".to_owned()))?;
    let settings_json = serde_json::to_string(&proposed)
        .map_err(|error| StoreError::Invalid(format!("fleet settings: {error}")))?;
    owners::put(&transaction, KEY, &proposed)?;
    transaction.execute(
        "INSERT INTO fleet_changes (at, actor, summary, settings) VALUES (?1, ?2, ?3, ?4)",
        params![
            now.to_string(),
            actor,
            summary(&before, &proposed),
            settings_json
        ],
    )?;
    transaction.commit()?;
    Ok(PutOutcome::Stored(proposed))
}

/// The most recent changes, newest first.
pub fn changes(conn: &Connection, limit: u32) -> Result<Vec<FleetChange>> {
    let mut statement = conn.prepare(
        "SELECT seq, at, actor, summary, settings FROM fleet_changes ORDER BY seq DESC LIMIT ?1",
    )?;
    statement
        .query_map([limit], |row| {
            Ok(rows::FleetChange {
                seq: row.get(0)?,
                at: row.get(1)?,
                actor: row.get(2)?,
                summary: row.get(3)?,
                settings: row.get(4)?,
            })
        })?
        .map(|row| {
            let row = row?;
            Ok(FleetChange {
                seq: row.seq,
                at: Timestamp::parse(&row.at)
                    .map_err(|error| StoreError::Invalid(error.to_string()))?,
                actor: row.actor,
                summary: row.summary,
                settings: serde_json::from_str(&row.settings)
                    .map_err(|error| StoreError::Invalid(format!("fleet settings: {error}")))?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::open_in_memory;
    use ignite_core::fleet::{MachineSetting, MachineState, ProviderSetting};

    #[test]
    fn settings_and_change_log_round_trip_with_version_checks() {
        let mut conn = open_in_memory().unwrap();
        let now = Timestamp::EPOCH;
        assert_eq!(settings(&conn).unwrap(), FleetSettings::default());
        let mut proposed = FleetSettings {
            version: 98,
            ..FleetSettings::default()
        };
        proposed.machines.insert(
            "host".to_owned(),
            MachineSetting {
                state: MachineState::ChecksOnly,
                disk_cap_gb: Some(200),
            },
        );
        proposed.providers.insert(
            "codex".to_owned(),
            ProviderSetting {
                share: 50,
                ..ProviderSetting::default()
            },
        );
        let mut written = proposed.clone();
        written.version = 1;
        assert_eq!(
            put(&mut conn, 0, proposed.clone(), "operator", now).unwrap(),
            PutOutcome::Stored(written.clone())
        );
        assert_eq!(settings(&conn).unwrap(), written);
        assert_eq!(
            put(&mut conn, 0, FleetSettings::default(), "stale", now).unwrap(),
            PutOutcome::Conflict(written.clone())
        );
        assert_eq!(
            put(&mut conn, 1, proposed, "operator", now).unwrap(),
            PutOutcome::Stored(written.clone())
        );
        assert_eq!(changes(&conn, 20).unwrap(), [FleetChange {
            seq: 1,
            at: now,
            actor: "operator".to_owned(),
            summary: "machine host: checks-only, disk cap 200 GB | provider codex: on, share 50, stop at 99%".to_owned(),
            settings: written.clone(),
        }]);

        let mut second = written;
        second.providers.get_mut("codex").unwrap().enabled = false;
        assert!(matches!(
            put(&mut conn, 1, second, "operator", now).unwrap(),
            PutOutcome::Stored(_)
        ));
        let latest = changes(&conn, 1).unwrap();
        assert_eq!(latest.len(), 1);
        assert_eq!(latest[0].seq, 2);
        assert_eq!(latest[0].settings, settings(&conn).unwrap());
    }

    #[test]
    fn invalid_settings_leave_version_and_audit_log_untouched() {
        let mut conn = open_in_memory().unwrap();
        let mut proposed = FleetSettings::default();
        proposed.providers.insert(
            "codex".to_owned(),
            ProviderSetting {
                share: 1_001,
                ..ProviderSetting::default()
            },
        );
        assert!(matches!(
            put(&mut conn, 0, proposed, "operator", Timestamp::EPOCH),
            Err(StoreError::Rejected(_))
        ));
        assert_eq!(settings(&conn).unwrap(), FleetSettings::default());
        assert!(changes(&conn, 20).unwrap().is_empty());
    }
}
