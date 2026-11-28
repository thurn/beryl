//! Each machine's measured speed, kept in `meta` and refreshed from the check records once an
//! hour (`ignite_core::machine_speed`).

use std::collections::BTreeMap;

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use ignite_core::fleet::FleetSettings;
use ignite_core::machine_speed::{self, Run, Speed};
use ignite_core::time::Timestamp;

use super::meta;
use crate::Result;

const KEY: &str = "machine_speeds";

/// How long a measurement stands before upkeep takes another.
pub const REFRESH_SECONDS: f64 = 3_600.0;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Measured {
    at: Option<Timestamp>,
    /// By machine name.
    speeds: BTreeMap<String, Speed>,
}

/// Measures again when the last measurement is older than [`REFRESH_SECONDS`]. A check here is
/// one family's command of one verb, such as `land` or `tip`, because a record is one command
/// and names its family, not its checks. Returns whether it measured.
pub fn refresh(conn: &Connection, now: Timestamp) -> Result<bool> {
    let old: Measured = meta::get(conn, KEY)?.unwrap_or_default();
    if old
        .at
        .is_some_and(|at| now.seconds_since(at) < REFRESH_SECONDS)
    {
        return Ok(false);
    }
    let from = now.plus_seconds(-machine_speed::WINDOW_DAYS * 86_400.0);
    let mut select = conn.prepare(
        "SELECT r.family || ' ' || r.verb, coalesce(m.name, r.machine), r.wall_s
         FROM records r LEFT JOIN machines m ON m.id = r.machine
         WHERE r.family IS NOT NULL AND r.wall_s > 0 AND r.at >= ?1",
    )?;
    let runs = select
        .query_map(params![from.to_string()], |row| {
            Ok(Run {
                check: row.get(0)?,
                machine: row.get(1)?,
                seconds: row.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let measured = Measured {
        at: Some(now),
        speeds: machine_speed::measure(&runs),
    };
    meta::put(conn, KEY, &measured)?;
    Ok(true)
}

/// The speed of each machine named in the measurement or the settings: the operator's setting
/// where there is one, else the measurement.
pub fn speeds(conn: &Connection, settings: &FleetSettings) -> Result<BTreeMap<String, Speed>> {
    let Measured { mut speeds, .. } = meta::get(conn, KEY)?.unwrap_or_default();
    for (name, setting) in &settings.machines {
        if let Some(speed) = setting.speed {
            speeds.insert(name.clone(), speed);
        }
    }
    Ok(speeds)
}

/// The speed dispatch assumes for `name`: its own, else 1.0, so an unmeasured machine is not
/// passed over.
pub fn of(speeds: &BTreeMap<String, Speed>, name: &str) -> Speed {
    speeds
        .iter()
        .find(|(known, _)| known.eq_ignore_ascii_case(name))
        .map_or(Speed::ONE, |(_, speed)| *speed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(conn: &Connection, machine: &str, family: &str, wall_s: f64, at: &str) {
        conn.execute(
            "INSERT INTO records (machine, verb, class, family, queued_s, wall_s, cpu_s,
                                  peak_mb, ignite, at)
             VALUES (?1, 'check', 'check', ?2, 0, ?3, ?3, 0, 'v', ?4)",
            params![machine, family, wall_s, at],
        )
        .unwrap();
    }

    /// Records seeded for two machines give jarvis 1.0 and multivac 0.4. A record older than
    /// the window is not read, and an operator's speed replaces the measurement.
    #[test]
    fn speeds_come_from_recent_records_and_the_operator_overrides_them() {
        let conn = crate::hub::open_in_memory().unwrap();
        conn.execute(
            "INSERT INTO machines (id, name, os, cores, memory_mb, version, last_seen)
             VALUES ('m-1', 'jarvis', 'macos', 20, 1, 'v', ''),
                    ('m-2', 'multivac', 'macos', 20, 1, 'v', '')",
            [],
        )
        .unwrap();
        let now = Timestamp::parse("2026-11-27T12:00:00Z").unwrap();
        record(&conn, "m-1", "rust", 100.0, "2026-11-27T10:00:00Z");
        record(&conn, "m-2", "rust", 250.0, "2026-11-27T11:00:00Z");
        record(&conn, "m-1", "client", 40.0, "2026-11-26T10:00:00Z");
        record(&conn, "m-2", "client", 100.0, "2026-11-26T11:00:00Z");
        record(&conn, "m-1", "client", 1.0, "2026-11-01T11:00:00Z");
        assert!(refresh(&conn, now).unwrap());
        assert!(!refresh(&conn, now.plus_seconds(60.0)).unwrap());
        let mut settings = FleetSettings::default();
        let measured = speeds(&conn, &settings).unwrap();
        assert_eq!(of(&measured, "jarvis"), Speed::ONE);
        assert_eq!(of(&measured, "Multivac"), "0.4".parse().unwrap());
        assert_eq!(of(&measured, "cygnus"), Speed::ONE);
        settings
            .machines
            .entry("multivac".to_owned())
            .or_default()
            .speed = Some("0.9".parse().unwrap());
        let set = speeds(&conn, &settings).unwrap();
        assert_eq!(of(&set, "multivac"), "0.9".parse().unwrap());
    }
}
