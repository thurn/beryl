//! Versioned fleet settings and their change log.

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use ignite_core::fleet::{FleetChange, FleetSettings, MachineState, summary};
use ignite_core::time::Timestamp;

use super::rows;
use super::{fleet, meta};
use crate::{Result, StoreError};

const KEY: &str = "fleet";

#[derive(Debug, PartialEq, Eq)]
pub enum PutOutcome {
    Stored(FleetSettings),
    Conflict(FleetSettings),
}

/// The current settings. Version zero means the fleet has yet to be seeded.
pub fn settings(conn: &Connection) -> Result<FleetSettings> {
    Ok(meta::get(conn, KEY)?.unwrap_or_default())
}

/// Applies a change and records it together, or returns the settings that replaced the
/// caller's version.
pub fn put(
    conn: &mut Connection,
    expected_version: u64,
    proposed: FleetSettings,
    actor: &str,
    now: Timestamp,
) -> Result<PutOutcome> {
    let transaction = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let before = settings(&transaction)?;
    if expected_version != before.version {
        return Ok(PutOutcome::Conflict(before));
    }
    let stored = write(&transaction, &before, proposed, actor, now)?;
    transaction.commit()?;
    Ok(PutOutcome::Stored(stored))
}

/// Changes only the state, for older daemons still using the per-machine `POST /pause`.
pub fn set_machine_state(
    conn: &mut Connection,
    machine: &str,
    state: MachineState,
    actor: &str,
    now: Timestamp,
) -> Result<()> {
    let transaction = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let name = fleet::name_of(&transaction, machine)?.unwrap_or_else(|| machine.to_owned());
    let before = settings(&transaction)?;
    if before.machine(&name).state != state {
        let mut proposed = before.clone();
        proposed.machines.entry(name).or_default().state = state;
        write(&transaction, &before, proposed, actor, now)?;
    }
    transaction.commit()?;
    Ok(())
}

/// Imports legacy pauses after seeding, in the same transaction that removes their rows.
pub fn migrate_pauses(conn: &mut Connection, now: Timestamp) -> Result<()> {
    let transaction = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let names = {
        let mut statement = transaction.prepare(
            "SELECT coalesce(m.name, p.machine) FROM pauses p
             LEFT JOIN machines m ON p.machine = m.id WHERE p.machine != ?1",
        )?;
        statement
            .query_map([fleet::ALL_MACHINES], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?
    };
    if !names.is_empty() {
        let before = settings(&transaction)?;
        let mut proposed = before.clone();
        for name in names {
            let setting = proposed.machines.entry(name).or_default();
            // A legacy pause allowed tip verification. An explicit Off is more restrictive.
            if setting.state == MachineState::Active {
                setting.state = MachineState::ChecksOnly;
            }
        }
        write(&transaction, &before, proposed, "legacy pool pause", now)?;
        transaction.execute(
            "DELETE FROM pauses WHERE machine != ?1",
            [fleet::ALL_MACHINES],
        )?;
    }
    transaction.commit()?;
    Ok(())
}

/// Who last changed this machine's state. Disk cap and provider edits do not affect attribution.
pub fn state_actor(conn: &Connection, name: &str) -> Result<Option<String>> {
    Ok(conn.query_row(
        "SELECT actor FROM (
             SELECT seq, actor, state, lag(state, 1, 'active') OVER (ORDER BY seq) AS previous
             FROM (
                 SELECT seq, actor, coalesce(settings -> 'machines' -> ?1 ->> 'state', 'active') AS state
                 FROM fleet_changes
             )
         ) WHERE state != previous ORDER BY seq DESC LIMIT 1",
        [name],
        |row| row.get(0),
    ).optional()?)
}

fn write(
    conn: &Connection,
    before: &FleetSettings,
    mut proposed: FleetSettings,
    actor: &str,
    now: Timestamp,
) -> Result<FleetSettings> {
    proposed.validate().map_err(StoreError::Rejected)?;
    proposed.version = before.version;
    if before.version != 0 && *before == proposed {
        return Ok(before.clone());
    }
    proposed.version = before
        .version
        .checked_add(1)
        .ok_or_else(|| StoreError::Invalid("fleet settings version overflow".to_owned()))?;
    let settings_json = serde_json::to_string(&proposed)
        .map_err(|error| StoreError::Invalid(format!("fleet settings: {error}")))?;
    meta::put(conn, KEY, &proposed)?;
    conn.execute(
        "INSERT INTO fleet_changes (at, actor, summary, settings) VALUES (?1, ?2, ?3, ?4)",
        params![
            now.to_string(),
            actor,
            summary(before, &proposed),
            settings_json
        ],
    )?;
    Ok(proposed)
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
    fn legacy_pauses_move_into_settings_and_leave_the_global_pause_in_force() {
        let mut conn = open_in_memory().unwrap();
        let now = Timestamp::EPOCH;
        conn.execute_batch(
            "INSERT INTO machines (id, name, os, cores, memory_mb, families, version, last_seen)
             VALUES ('m-1', 'host', 'macos', 8, 16000, '[]', 'aaaa000', '2026-11-17T12:00:00.000Z');
             INSERT INTO pauses (machine, at) VALUES ('m-1', 'now'), ('*', 'now');",
        )
        .unwrap();
        let mut initial = FleetSettings::default();
        initial.machines.insert(
            "host".to_owned(),
            MachineSetting {
                state: MachineState::Active,
                disk_cap_gb: Some(150),
            },
        );
        put(&mut conn, 0, initial, "ignite.toml", now).unwrap();

        migrate_pauses(&mut conn, now).unwrap();
        let mut stored = settings(&conn).unwrap();
        assert_eq!(stored.version, 2);
        assert_eq!(
            stored.machine("host"),
            MachineSetting {
                state: MachineState::ChecksOnly,
                disk_cap_gb: Some(150),
            }
        );
        assert_eq!(
            state_actor(&conn, "host").unwrap().as_deref(),
            Some("legacy pool pause")
        );
        let pause: String = conn
            .query_row("SELECT machine FROM pauses", [], |row| row.get(0))
            .unwrap();
        assert_eq!(pause, "*");
        migrate_pauses(&mut conn, now).unwrap();
        assert_eq!(settings(&conn).unwrap(), stored);

        stored.machines.get_mut("host").unwrap().disk_cap_gb = Some(100);
        put(&mut conn, 2, stored, "disk operator", now).unwrap();
        assert_eq!(
            state_actor(&conn, "host").unwrap().as_deref(),
            Some("legacy pool pause")
        );
        fleet::set_paused_everywhere(&conn, false, now).unwrap();
        assert!(!fleet::paused_everywhere(&conn).unwrap());
        assert_eq!(
            settings(&conn).unwrap().machine("host").state,
            MachineState::ChecksOnly
        );
    }

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
