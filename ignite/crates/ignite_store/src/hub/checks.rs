//! Checks: their state, their record of runs and failures by area, and their costs.

use rusqlite::{Connection, TransactionBehavior, params};

use ignite_core::protocol::hub::{AreaStats, CheckState, CheckStats, CheckStatsReply, FlakyReport};
use ignite_core::time::Timestamp;

use crate::hub::results::parse;
use crate::hub::rows;
use crate::{Result, StoreError};

/// How many costs a check keeps.
const COSTS_KEPT: usize = 20;

/// A second flaky record on a different commit within this many days switches a check off.
pub const FLAKY_WINDOW_DAYS: f64 = 7.0;

pub(crate) fn state_text(state: CheckState) -> &'static str {
    match state {
        CheckState::On => "on",
        CheckState::Off => "off",
        CheckState::Uncached => "uncached",
    }
}

pub(crate) fn state_from(text: &str) -> Result<CheckState> {
    match text {
        "on" => Ok(CheckState::On),
        "off" => Ok(CheckState::Off),
        "uncached" => Ok(CheckState::Uncached),
        other => Err(StoreError::Invalid(format!("stored check state {other:?}"))),
    }
}

/// Adds or updates the checks a family lists. An existing check keeps its state, so listing a
/// family again never switches back on a check that was switched off.
pub fn upsert(conn: &mut Connection, checks: &[rows::Check]) -> Result<()> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    {
        let mut insert = tx.prepare_cached(
            "INSERT INTO checks (name, family, kind, state, check_group)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(name) DO UPDATE SET
               family = excluded.family, kind = excluded.kind, check_group = excluded.check_group",
        )?;
        for check in checks {
            insert.execute(params![
                check.name,
                check.family,
                check.kind,
                check.state,
                check.check_group
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

pub fn set_state(conn: &Connection, check: &str, state: CheckState) -> Result<()> {
    conn.execute(
        "UPDATE checks SET state = ?2 WHERE name = ?1",
        params![check, state_text(state)],
    )?;
    Ok(())
}

/// Counts one run (and maybe a failure) after a change to each of `areas`.
pub fn count_run(conn: &mut Connection, check: &str, areas: &[String], failed: bool) -> Result<()> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    {
        let mut count = tx.prepare_cached(
            "INSERT INTO check_stats (check_name, area, runs, failures) VALUES (?1, ?2, 1, ?3)
             ON CONFLICT(check_name, area) DO UPDATE SET
               runs = runs + 1, failures = failures + excluded.failures",
        )?;
        for area in areas {
            count.execute(params![check, area, i64::from(failed)])?;
        }
    }
    tx.commit()?;
    Ok(())
}

fn median(values: &[f64]) -> Option<f64> {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let middle = sorted.len() / 2;
    match sorted.len() {
        0 => None,
        n if n % 2 == 1 => Some(sorted[middle]),
        _ => Some(f64::midpoint(sorted[middle - 1], sorted[middle])),
    }
}

/// Adds a cost, keeping the last 20 and their median. When `unit` differs from the unit of the
/// costs kept, those costs are in another measure and are dropped.
pub fn add_cost(conn: &Connection, check: &str, cost: f64, unit: &str) -> Result<()> {
    let existing: Option<(String, String)> = {
        let mut select =
            conn.prepare_cached("SELECT costs, unit FROM check_costs WHERE check_name = ?1")?;
        let mut rows = select.query(params![check])?;
        match rows.next()? {
            Some(row) => Some((row.get(0)?, row.get(1)?)),
            None => None,
        }
    };
    let mut costs: Vec<f64> = match existing {
        Some((json, kept_unit)) if kept_unit == unit => serde_json::from_str(&json)
            .map_err(|error| StoreError::Invalid(format!("stored costs of {check}: {error}")))?,
        _ => Vec::new(),
    };
    costs.push(cost);
    if costs.len() > COSTS_KEPT {
        costs.drain(..costs.len() - COSTS_KEPT);
    }
    let json = serde_json::to_string(&costs).expect("numbers serialize");
    conn.execute(
        "INSERT INTO check_costs (check_name, costs, median, unit) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(check_name) DO UPDATE SET
           costs = excluded.costs, median = excluded.median, unit = excluded.unit",
        params![check, json, median(&costs), unit],
    )?;
    Ok(())
}

/// The families that hold regression checks, in name order.
pub fn regression_families(conn: &Connection) -> Result<Vec<String>> {
    let mut select = conn
        .prepare("SELECT DISTINCT family FROM checks WHERE kind = 'regression' ORDER BY family")?;
    let families = select
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    Ok(families)
}

/// Every check of `family` with its state, area counts and median cost, and the family's
/// failure rate over all those counts (zero before any run).
pub fn stats(conn: &Connection, family: &str) -> Result<CheckStatsReply> {
    let mut select = conn.prepare(
        "SELECT c.name, c.state, k.median, k.unit FROM checks c
         LEFT JOIN check_costs k ON k.check_name = c.name
         WHERE c.family = ?1 ORDER BY c.name",
    )?;
    let listed = select
        .query_map(params![family], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<f64>>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut areas = conn.prepare(
        "SELECT area, runs, failures FROM check_stats WHERE check_name = ?1 ORDER BY area",
    )?;
    let (mut all_runs, mut all_failures) = (0_u64, 0_u64);
    let mut checks = Vec::with_capacity(listed.len());
    for (name, state, median_cost, unit) in listed {
        let areas = areas
            .query_map(params![name], |row| {
                Ok(AreaStats {
                    area: row.get(0)?,
                    runs: row.get(1)?,
                    failures: row.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for area in &areas {
            all_runs += u64::from(area.runs);
            all_failures += u64::from(area.failures);
        }
        checks.push(CheckStats {
            check: parse("check name", &name)?,
            family: family.to_owned(),
            state: state_from(&state)?,
            areas,
            median_cost,
            unit: unit.unwrap_or_default(),
        });
    }
    let family_rate = if all_runs == 0 {
        0.0
    } else {
        all_failures as f64 / all_runs as f64
    };
    Ok(CheckStatsReply {
        family: family.to_owned(),
        family_rate,
        checks,
    })
}

/// When this many different checks flaked in one command run, the cause is the machine, such
/// as a full disk or a stuck browser page, and none of those records counts toward switching a
/// check off.
pub const MACHINE_FAULT_CHECKS: i64 = 3;

/// The SQL condition on a `flaky` row for it to count toward switching its check off: no
/// ticket was filed for it yet, and it is not from a command in which [`MACHINE_FAULT_CHECKS`]
/// or more checks flaked.
pub fn counting_flaky() -> String {
    format!("filed IS NULL AND {}", not_machine_fault())
}

/// The SQL condition on a `flaky` row for it to be about its own check rather than about the
/// machine: it is not from a command in which [`MACHINE_FAULT_CHECKS`] or more checks flaked.
pub fn not_machine_fault() -> String {
    format!(
        "(command = '' OR command NOT IN (
           SELECT command FROM flaky WHERE command != ''
           GROUP BY command HAVING COUNT(DISTINCT check_name) >= {MACHINE_FAULT_CHECKS}))"
    )
}

/// Records that `check` failed and then passed on the same input hash at `report.commit`.
/// Reporting the same (check, hash, commit) again changes nothing.
pub fn record_flaky(conn: &Connection, report: &FlakyReport, now: Timestamp) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO flaky (check_name, hash, commit_sha, machine, at, always, command, output)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            report.check.as_str(),
            report.hash.as_str(),
            report.commit.as_str(),
            report.machine.as_str(),
            now.to_string(),
            report.always,
            report.command,
            report.output,
        ],
    )?;
    Ok(())
}

/// Whether `check` has two flaky records that count ([`counting_flaky`]) on different commits
/// no more than 7 days apart.
pub fn flaky_repeated(conn: &Connection, check: &str) -> Result<bool> {
    let mut select = conn.prepare_cached(&format!(
        "SELECT commit_sha, at FROM flaky WHERE check_name = ?1 AND {} ORDER BY at",
        counting_flaky()
    ))?;
    let records = select
        .query_map(params![check], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut parsed = Vec::with_capacity(records.len());
    for (commit, at) in records {
        parsed.push((commit, parse::<Timestamp>("time", &at)?));
    }
    let window = FLAKY_WINDOW_DAYS * 86_400.0;
    Ok(parsed.iter().enumerate().any(|(i, (commit, at))| {
        parsed[i + 1..]
            .iter()
            .any(|(other, later)| other != commit && later.seconds_since(*at) <= window)
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub;
    use ignite_core::ids::{CheckName, Commit, InputHash, MachineId};

    fn check(name: &str, family: &str) -> rows::Check {
        rows::Check {
            name: name.to_owned(),
            family: family.to_owned(),
            kind: "regression".to_owned(),
            state: "on".to_owned(),
            check_group: "group".to_owned(),
        }
    }

    fn flaky_report(commit: &str) -> FlakyReport {
        FlakyReport {
            check: CheckName::new("rust:a").unwrap(),
            hash: InputHash::new("1".repeat(64)).unwrap(),
            commit: Commit::new(commit).unwrap(),
            machine: MachineId::new("m-1").unwrap(),
            always: false,
            command: String::new(),
            output: String::new(),
        }
    }

    fn day(n: u32) -> Timestamp {
        format!("2026-11-{n:02}T12:00:00.000Z").parse().unwrap()
    }

    #[test]
    fn statistics_add_up_over_batches_and_the_family_rate_follows() {
        let mut conn = hub::open_in_memory().unwrap();
        upsert(
            &mut conn,
            &[
                check("rust:a", "rust"),
                check("rust:b", "rust"),
                check("ui:c", "ui"),
            ],
        )
        .unwrap();
        let areas = vec!["crates/x".to_owned(), "crates/y".to_owned()];
        count_run(&mut conn, "rust:a", &areas, false).unwrap();
        count_run(&mut conn, "rust:a", &areas[..1], true).unwrap();
        count_run(&mut conn, "rust:a", &areas[..1], false).unwrap();
        count_run(&mut conn, "rust:b", &areas[1..], false).unwrap();
        count_run(&mut conn, "ui:c", &areas[..1], true).unwrap();

        let reply = stats(&conn, "rust").unwrap();
        assert_eq!(reply.family, "rust");
        assert_eq!(reply.checks.len(), 2);
        let a = &reply.checks[0];
        assert_eq!(a.check.as_str(), "rust:a");
        assert_eq!(
            a.areas,
            vec![
                AreaStats {
                    area: "crates/x".to_owned(),
                    runs: 3,
                    failures: 1
                },
                AreaStats {
                    area: "crates/y".to_owned(),
                    runs: 1,
                    failures: 0
                },
            ]
        );
        // 5 counted runs in the family, 1 failure; the other family's failure is not counted.
        assert!((reply.family_rate - 0.2).abs() < 1e-9);
        assert!(stats(&conn, "none").unwrap().checks.is_empty());
        assert!(stats(&conn, "none").unwrap().family_rate.abs() < 1e-9);
    }

    #[test]
    fn listing_a_family_again_keeps_a_check_switched_off() {
        let mut conn = hub::open_in_memory().unwrap();
        upsert(&mut conn, &[check("rust:a", "rust")]).unwrap();
        set_state(&conn, "rust:a", CheckState::Off).unwrap();
        let mut moved = check("rust:a", "rust");
        moved.check_group = "other".to_owned();
        upsert(&mut conn, &[moved]).unwrap();
        let reply = stats(&conn, "rust").unwrap();
        assert_eq!(reply.checks[0].state, CheckState::Off);
        let group: String = conn
            .query_row("SELECT check_group FROM checks", [], |row| row.get(0))
            .unwrap();
        assert_eq!(group, "other");
        set_state(&conn, "rust:a", CheckState::Uncached).unwrap();
        assert_eq!(
            stats(&conn, "rust").unwrap().checks[0].state,
            CheckState::Uncached
        );
    }

    #[test]
    fn costs_keep_the_last_twenty_and_their_median() {
        let mut conn = hub::open_in_memory().unwrap();
        upsert(&mut conn, &[check("rust:a", "rust")]).unwrap();
        for cost in 1..=3 {
            add_cost(&conn, "rust:a", f64::from(cost), "cpu_s").unwrap();
        }
        assert_eq!(
            stats(&conn, "rust").unwrap().checks[0].median_cost,
            Some(2.0)
        );
        add_cost(&conn, "rust:a", 10.0, "cpu_s").unwrap();
        // 1, 2, 3, 10: the median is between 2 and 3.
        assert_eq!(
            stats(&conn, "rust").unwrap().checks[0].median_cost,
            Some(2.5)
        );
        for cost in 100..120 {
            add_cost(&conn, "rust:a", f64::from(cost), "cpu_s").unwrap();
        }
        let json: String = conn
            .query_row("SELECT costs FROM check_costs", [], |row| row.get(0))
            .unwrap();
        let kept: Vec<f64> = serde_json::from_str(&json).unwrap();
        assert_eq!(kept.len(), 20);
        assert!((kept[0] - 100.0).abs() < 1e-9);
        let reply = stats(&conn, "rust").unwrap();
        assert_eq!(reply.checks[0].median_cost, Some(109.5));
        assert_eq!(reply.checks[0].unit, "cpu_s");
    }

    #[test]
    fn a_new_unit_starts_the_costs_over() {
        let mut conn = hub::open_in_memory().unwrap();
        upsert(&mut conn, &[check("rust:a", "rust")]).unwrap();
        add_cost(&conn, "rust:a", 50.0, "cpu_s").unwrap();
        add_cost(&conn, "rust:a", 2.0, "instructions").unwrap();
        let reply = stats(&conn, "rust").unwrap();
        assert_eq!(reply.checks[0].median_cost, Some(2.0));
        assert_eq!(reply.checks[0].unit, "instructions");
    }

    #[test]
    fn a_second_flaky_record_on_a_different_commit_within_a_week_is_repeated() {
        let conn = hub::open_in_memory().unwrap();
        assert!(!flaky_repeated(&conn, "rust:a").unwrap());
        record_flaky(&conn, &flaky_report("aaaa"), day(1)).unwrap();
        assert!(!flaky_repeated(&conn, "rust:a").unwrap());
        // The same hash and commit again is the same record.
        record_flaky(&conn, &flaky_report("aaaa"), day(2)).unwrap();
        assert!(!flaky_repeated(&conn, "rust:a").unwrap());
        // A different commit eight days after the first is outside the window.
        record_flaky(&conn, &flaky_report("bbbb"), day(9)).unwrap();
        assert!(!flaky_repeated(&conn, "rust:a").unwrap());
        // A third, a different commit one day after the second, is inside it.
        record_flaky(&conn, &flaky_report("cccc"), day(10)).unwrap();
        assert!(flaky_repeated(&conn, "rust:a").unwrap());
        assert!(!flaky_repeated(&conn, "rust:other").unwrap());
    }
}
