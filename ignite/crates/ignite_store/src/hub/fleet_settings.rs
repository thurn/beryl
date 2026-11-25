//! Versioned fleet settings and their change log.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use ignite_core::config::Os;
use ignite_core::fleet::{
    FleetChange, FleetSettings, MachineState, TreeSpace, TreeVolumes, absolute_trees_root, summary,
};
use ignite_core::protocol::tickets::FeedChange;
use ignite_core::time::Timestamp;

use super::rows;
use super::{feed, fleet, meta};
use crate::{Result, StoreError};

const KEY: &str = "fleet";
const STORAGE_STUCK_KEY: &str = "storage_stuck";

#[derive(Debug, PartialEq, Eq)]
pub enum PutOutcome {
    Stored(FleetSettings),
    Conflict(FleetSettings),
}

/// The current settings. Version zero means the fleet has yet to be seeded.
pub fn settings(conn: &Connection) -> Result<FleetSettings> {
    Ok(meta::get(conn, KEY)?.unwrap_or_default())
}

/// Each platform with registered machines, all of which are explicitly off.
pub fn off_machines(
    conn: &Connection,
    settings: &FleetSettings,
) -> Result<BTreeMap<Os, Vec<String>>> {
    let mut platforms: BTreeMap<Os, Vec<_>> = BTreeMap::new();
    for machine in fleet::machines(conn)? {
        let os = match machine.os.as_str() {
            "macos" => Os::Macos,
            "windows" => Os::Windows,
            _ => continue,
        };
        platforms.entry(os).or_default().push(machine.name);
    }
    platforms.retain(|_, machines| {
        machines.sort();
        machines
            .iter()
            .all(|name| settings.machine(name).state == MachineState::Off)
    });
    Ok(platforms)
}

/// A machine with no heartbeat for half an hour is also unavailable for dispatch fallback.
pub const ONLINE_SECONDS: f64 = 30.0 * 60.0;

/// Whether `machine` has sent a heartbeat within [`ONLINE_SECONDS`] of `now`.
pub fn online(machine: &rows::Machine, now: Timestamp) -> bool {
    machine
        .last_seen
        .parse::<Timestamp>()
        .is_ok_and(|seen| now.seconds_since(seen) < ONLINE_SECONDS)
}

/// Records that `machine`'s monitor found a stuck trees drive at `at`. While the machine has sent
/// no newer heartbeat, [`unavailable_platforms`] treats it as unavailable at once rather than
/// waiting [`ONLINE_SECONDS`] for heartbeat silence.
pub fn record_storage_stuck(conn: &Connection, machine: &str, at: Timestamp) -> Result<()> {
    let mut stuck: BTreeMap<String, Timestamp> =
        meta::get(conn, STORAGE_STUCK_KEY)?.unwrap_or_default();
    let keep = stuck.get(machine).is_none_or(|prev| *prev < at);
    if keep {
        stuck.insert(machine.to_owned(), at);
        meta::put(conn, STORAGE_STUCK_KEY, &stuck)?;
        feed::record(
            conn,
            &FeedChange {
                seq: feed::next_seq(conn)?,
                kind: "attention".to_owned(),
                ticket: None,
                name: Some(machine.to_owned()),
            },
        )?;
    }
    Ok(())
}

/// Whether `machine` has a stuck-storage report that no later heartbeat has superseded.
pub fn storage_stuck_active(stuck: &BTreeMap<String, Timestamp>, machine: &rows::Machine) -> bool {
    stuck.get(&machine.id).is_some_and(|at| {
        machine
            .last_seen
            .parse::<Timestamp>()
            .is_ok_and(|seen| seen <= *at)
    })
}

/// All recorded stuck-storage reports, keyed by machine ID.
pub fn storage_stuck_map(conn: &Connection) -> Result<BTreeMap<String, Timestamp>> {
    Ok(meta::get(conn, STORAGE_STUCK_KEY)?.unwrap_or_default())
}

/// Whether `machine` is available to verify tips and make progress at `now`.
pub fn machine_online_and_active(
    machine: &rows::Machine,
    settings: &FleetSettings,
    stuck: &BTreeMap<String, Timestamp>,
    now: Timestamp,
) -> bool {
    settings.machine(&machine.name).state != MachineState::Off
        && online(machine, now)
        && !storage_stuck_active(stuck, machine)
}

/// Each platform with registered machines, every one of which is off, has reported a stuck trees
/// drive without a newer heartbeat, or has sent no heartbeat for [`ONLINE_SECONDS`]. Tip
/// verification defers such a platform, as it does one whose machines are all off, so that
/// `master` does not wait on a machine that cannot answer. A machine whose storage or daemon
/// hangs stops sending heartbeats without anyone setting it off: on 2026-11-25 `master` waited
/// on a hung Windows machine until a person turned it off. The platform comes back by itself
/// with the machine's next heartbeat.
pub fn unavailable_platforms(
    conn: &Connection,
    settings: &FleetSettings,
    now: Timestamp,
) -> Result<BTreeSet<Os>> {
    let stuck = storage_stuck_map(conn)?;
    let mut available = BTreeSet::new();
    let mut registered = BTreeSet::new();
    for machine in fleet::machines(conn)? {
        let os = match machine.os.as_str() {
            "macos" => Os::Macos,
            "windows" => Os::Windows,
            _ => continue,
        };
        registered.insert(os);
        if machine_online_and_active(&machine, settings, &stuck, now) {
            available.insert(os);
        }
    }
    Ok(registered.difference(&available).copied().collect())
}

/// Platforms that can run checks but have no online active verifier.
pub fn checks_only_platforms(
    conn: &Connection,
    settings: &FleetSettings,
    now: Timestamp,
) -> Result<BTreeSet<Os>> {
    let mut active = BTreeSet::new();
    let mut checks_only = BTreeSet::new();
    for machine in fleet::machines(conn)? {
        let os = match machine.os.as_str() {
            "macos" => Os::Macos,
            "windows" => Os::Windows,
            _ => continue,
        };
        if online(&machine, now) {
            match settings.machine(&machine.name).state {
                MachineState::Active => {
                    active.insert(os);
                }
                MachineState::ChecksOnly => {
                    checks_only.insert(os);
                }
                MachineState::Off => {}
            }
        }
    }
    checks_only.retain(|os| !active.contains(os));
    Ok(checks_only)
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
    validate_tree_settings(&transaction, &before, &proposed, now)?;
    let stored = write(&transaction, &before, proposed, actor, now)?;
    transaction.commit()?;
    Ok(PutOutcome::Stored(stored))
}

/// An older daemon omits this field. Its heartbeat must not refresh a previous report.
pub(super) fn record_tree_volumes(
    conn: &Connection,
    machine: &str,
    now: Timestamp,
    volumes: &TreeVolumes,
) -> Result<()> {
    let volumes =
        serde_json::to_string(volumes).map_err(|error| StoreError::Invalid(error.to_string()))?;
    conn.execute(
        "INSERT INTO machine_tree_volumes (machine, observed_at, volumes) VALUES (?1, ?2, ?3)
         ON CONFLICT(machine) DO UPDATE SET observed_at = excluded.observed_at, volumes = excluded.volumes",
        params![machine, now.to_string(), volumes],
    )?;
    Ok(())
}

fn validate_tree_settings(
    conn: &Connection,
    before: &FleetSettings,
    proposed: &FleetSettings,
    now: Timestamp,
) -> Result<()> {
    // Reports are sampled every minute. Allow one delayed sample before requiring a new one.
    const TREE_VOLUME_REPORT_MAX_AGE_S: f64 = 120.0;
    for (name, machine) in &proposed.machines {
        let previous = before.machine(name).trees;
        let trees = &machine.trees;
        if trees.trees_root == previous.trees_root && trees.tree_space_gb == previous.tree_space_gb
        {
            continue;
        }
        let registered: Option<(String, Option<String>, Option<String>)> = conn
            .query_row(
                "SELECT m.os, v.observed_at, v.volumes FROM machines m
             LEFT JOIN machine_tree_volumes v ON v.machine = m.id
             WHERE m.name = ?1 ORDER BY m.last_seen DESC, m.id LIMIT 1",
                [name],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let os = registered
            .as_ref()
            .and_then(|(os, _, _)| match os.as_str() {
                "macos" => Some(Os::Macos),
                "windows" => Some(Os::Windows),
                _ => None,
            });
        let root = trees.trees_root.as_deref();
        if root.is_some_and(|root| !absolute_trees_root(root, os)) {
            return Err(StoreError::Rejected(format!(
                "machine {name}: trees root must be an absolute path for its operating system"
            )));
        }
        if !matches!(trees.tree_space_gb, Some(TreeSpace::Automatic(_))) {
            continue;
        }
        let report = registered
            .and_then(|(_, at, volumes)| Some((Timestamp::parse(&at?).ok()?, volumes?)))
            .filter(|(at, _)| now.seconds_since(*at) <= TREE_VOLUME_REPORT_MAX_AGE_S)
            .and_then(|(_, volumes)| serde_json::from_str::<TreeVolumes>(&volumes).ok());
        let (Some(report), Some(os)) = (report, os) else {
            return Err(StoreError::Rejected(format!(
                "machine {name}: auto requires a recent tree volume report from the daemon"
            )));
        };
        if !report.allows_auto(root, os) {
            return Err(StoreError::Rejected(format!(
                "machine {name}: the daemon has not reported a dedicated volume for {}; set tree space to a number",
                root.unwrap_or(&report.default_root)
            )));
        }
    }
    Ok(())
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

/// Re-encode the current settings using the tree-space fields and discard the retired cap.
pub(super) fn migrate_disk_cap(conn: &rusqlite::Transaction<'_>) -> Result<()> {
    if let Some(settings) = meta::get::<FleetSettings>(conn, KEY)? {
        meta::put(conn, KEY, &settings)?;
    }
    Ok(())
}

/// Who last changed this machine's state. Tree and provider edits do not affect attribution.
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
    if proposed.allocations.is_none() {
        proposed.allocations.clone_from(&before.allocations);
    }
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
    if before.allocations != proposed.allocations {
        // The Tickets page refreshes its category headings when a pool weight changes.
        feed::record(
            conn,
            &FeedChange {
                seq: feed::next_seq(conn)?,
                kind: "allocation".to_owned(),
                ticket: None,
                name: None,
            },
        )?;
    }
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
    use ignite_core::fleet::{MachineSetting, MachineState, ProviderSetting, TreeSettings};

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
                trees: TreeSettings {
                    tree_ceiling_gb: 150,
                    ..TreeSettings::default()
                },
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
                trees: TreeSettings {
                    tree_ceiling_gb: 150,
                    ..TreeSettings::default()
                }
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

        stored
            .machines
            .get_mut("host")
            .unwrap()
            .trees
            .tree_ceiling_gb = 100;
        put(&mut conn, 2, stored, "tree operator", now).unwrap();
        assert_eq!(
            state_actor(&conn, "host").unwrap().as_deref(),
            Some("legacy pool pause")
        );
        fleet::set_paused_everywhere(&conn, false, now).unwrap();
        assert!(!fleet::paused_everywhere(&conn, now).unwrap());
        assert_eq!(
            settings(&conn).unwrap().machine("host").state,
            MachineState::ChecksOnly
        );
    }

    #[test]
    fn a_platform_whose_machines_are_all_off_or_silent_is_unavailable() {
        let conn = open_in_memory().unwrap();
        let start = Timestamp::parse("2026-11-25T05:00:00Z").unwrap();
        conn.execute_batch(
            "INSERT INTO machines (id, name, os, cores, memory_mb, families, version, last_seen)
             VALUES ('m-w', 'cygnus', 'windows', 12, 32000, '[]', 'aaaa000', '2026-11-25T05:00:00.000Z'),
                    ('m-1', 'jarvis', 'macos', 16, 64000, '[]', 'aaaa000', '2026-11-25T05:00:00.000Z'),
                    ('m-2', 'multivac', 'macos', 8, 16000, '[]', 'aaaa000', '2026-11-25T05:00:00.000Z');",
        )
        .unwrap();
        let mut settings = FleetSettings::default();
        let just_under = start.plus_seconds(ONLINE_SECONDS - 1.0);
        assert!(
            unavailable_platforms(&conn, &settings, just_under)
                .unwrap()
                .is_empty()
        );

        // Half an hour without a heartbeat takes every machine out; a Mac that is still
        // reporting keeps its platform.
        conn.execute(
            "UPDATE machines SET last_seen = '2026-11-25T05:29:00.000Z' WHERE id = 'm-2'",
            [],
        )
        .unwrap();
        let silent = start.plus_seconds(ONLINE_SECONDS);
        assert_eq!(
            unavailable_platforms(&conn, &settings, silent).unwrap(),
            BTreeSet::from([Os::Windows])
        );

        // An answering machine that is off counts as unavailable too.
        settings.machines.insert(
            "multivac".to_owned(),
            ignite_core::fleet::MachineSetting {
                state: MachineState::Off,
                ..Default::default()
            },
        );
        assert_eq!(
            unavailable_platforms(&conn, &settings, silent).unwrap(),
            BTreeSet::from([Os::Macos, Os::Windows])
        );

        // The next heartbeat brings the platform back.
        conn.execute(
            "UPDATE machines SET last_seen = '2026-11-25T05:30:00.000Z' WHERE id = 'm-w'",
            [],
        )
        .unwrap();
        assert_eq!(
            unavailable_platforms(&conn, &settings, silent).unwrap(),
            BTreeSet::from([Os::Macos])
        );

        // Reporting stuck storage defers the platform immediately without waiting 30 minutes,
        // and a newer heartbeat brings it back.
        record_storage_stuck(&conn, "m-w", silent).unwrap();
        assert_eq!(
            unavailable_platforms(&conn, &settings, silent).unwrap(),
            BTreeSet::from([Os::Macos, Os::Windows])
        );
        let recovered = silent.plus_seconds(10.0);
        conn.execute(
            "UPDATE machines SET last_seen = ?1 WHERE id = 'm-w'",
            [recovered.to_string()],
        )
        .unwrap();
        assert_eq!(
            unavailable_platforms(&conn, &settings, recovered).unwrap(),
            BTreeSet::from([Os::Macos])
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
                trees: TreeSettings {
                    tree_ceiling_gb: 200,
                    ..TreeSettings::default()
                },
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
            summary: "machine host: checks-only, tree ceiling 200 GB | provider codex: on, share 50, stop at 99%".to_owned(),
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
    fn auto_requires_a_recent_daemon_report_for_the_selected_root() {
        use ignite_core::fleet::{Auto, TreeVolume};

        let mut conn = open_in_memory().unwrap();
        let now = Timestamp::EPOCH;
        conn.execute_batch(
            "INSERT INTO machines (id, name, os, cores, memory_mb, families, version, last_seen)
             VALUES ('m-1', 'host', 'macos', 8, 16000, '[]', 'aaaa000', '2026-11-17T12:00:00.000Z');",
        )
        .unwrap();
        let mut proposed = FleetSettings::default();
        let machine = proposed.machines.entry("host".to_owned()).or_default();
        machine.trees.tree_space_gb = Some(TreeSpace::Automatic(Auto::Auto));
        machine.trees.trees_root = Some("/Volumes/Shared/trees".to_owned());
        let volumes = TreeVolumes {
            default_root: "/repo/.worktrees".to_owned(),
            dedicated: vec![TreeVolume {
                mount: "/Volumes/Builds".to_owned(),
                root: Some("/Volumes/Builds/trees".to_owned()),
            }],
        };
        record_tree_volumes(&conn, "m-1", now, &volumes).unwrap();
        let result = put(&mut conn, 0, proposed.clone(), "operator", now);
        assert!(
            matches!(result, Err(StoreError::Rejected(message)) if message.contains("dedicated volume"))
        );
        assert_eq!(settings(&conn).unwrap().version, 0);

        proposed.machines.get_mut("host").unwrap().trees.trees_root =
            Some("/Volumes/Builds/trees".to_owned());
        let result = put(
            &mut conn,
            0,
            proposed.clone(),
            "operator",
            now.plus_seconds(121.0),
        );
        assert!(
            matches!(result, Err(StoreError::Rejected(message)) if message.contains("recent tree volume report"))
        );

        let result = put(&mut conn, 0, proposed, "operator", now).unwrap();
        let PutOutcome::Stored(stored) = result else {
            panic!("an observed dedicated volume was rejected")
        };
        let automatic = stored.machine("host").trees;
        assert_eq!(
            automatic.tree_space_gb,
            Some(TreeSpace::Automatic(Auto::Auto))
        );
        assert_eq!(automatic.tree_ceiling_gb, 60);
    }

    #[test]
    fn fleet_migration_drops_the_cap_and_keeps_the_other_settings() {
        let mut conn = Connection::open_in_memory().unwrap();
        let migrations = super::super::MIGRATIONS;
        let before_cap_removal = migrations
            .iter()
            .position(|entry| entry.version == 47)
            .unwrap();
        crate::migrate::apply(&mut conn, &migrations[..before_cap_removal]).unwrap();
        let retired_key = concat!("disk_", "cap_gb");
        let old = serde_json::json!({
            "version": 2, "machines": {"host": {
                "state": "checks-only", (retired_key): 200, "tree_space_gb": 350
            }}, "providers": {"codex": {"enabled": true, "share": 3, "stop_percent": 99}}
        });
        meta::put(&conn, KEY, &old).unwrap();

        crate::migrate::apply(&mut conn, migrations).unwrap();
        let stored: serde_json::Value = meta::get(&conn, KEY).unwrap().unwrap();
        assert!(stored["machines"]["host"].get(retired_key).is_none());
        let saved = settings(&conn).unwrap();
        assert_eq!(saved.version, 2);
        assert_eq!(saved.machine("host").state, MachineState::ChecksOnly);
        assert_eq!(
            saved.machine("host").trees.tree_space_gb,
            Some(TreeSpace::Gigabytes(350))
        );
        assert_eq!(
            saved.machine("host").trees.tree_ceiling_gb,
            ignite_core::fleet::DEFAULT_TREE_CEILING_GB
        );
        assert_eq!(saved.provider("codex").share, 3);
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
