//! Checks: their state, their record of runs and failures by area, and their costs.

use rusqlite::{Connection, TransactionBehavior, params};

use ignite_core::protocol::hub::{AreaStats, CheckState, CheckStats, CheckStatsReply, FlakyReport};
use ignite_core::score;
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

/// Registers a check no family has listed, as an `on` regression check of the family named by
/// the part of its name before the first colon. A check already registered is left as it is.
/// A check first listed under Ignite has no row until its first result or state change, so
/// every reader of `checks` (flaky and stuck upkeep, catches, the tips page) would miss it.
pub fn register(conn: &Connection, check: &str) -> Result<()> {
    let family = check.split_once(':').map_or("", |(family, _)| family);
    conn.execute(
        "INSERT OR IGNORE INTO checks (name, family, kind, state, check_group)
         VALUES (?1, ?2, 'regression', 'on', ?2)",
        params![check, family],
    )?;
    Ok(())
}

/// Sets a check's state, registering the check first when it has no row.
pub fn set_state(conn: &Connection, check: &str, state: CheckState) -> Result<()> {
    register(conn, check)?;
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

pub(crate) fn median(values: &[f64]) -> Option<f64> {
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

/// How many of a family's newest commands [`startup`] fits its estimate to.
const STARTUP_RUNS: i64 = 200;

/// What starting one of a family's commands costs, estimated by [`score::startup`] from its
/// newest commands that stored a result, in each unit they were measured in.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Startup {
    /// Billions of instructions, from the commands whose machine counted them.
    pub billions: Option<f64>,
    pub cpu_s: Option<f64>,
}

impl Startup {
    /// The start-up in `unit`, `instructions` (billions) or `cpu_s`.
    pub fn in_unit(&self, unit: &str) -> Option<f64> {
        if unit == "cpu_s" {
            self.cpu_s
        } else {
            self.billions
        }
    }
}

/// Estimates `family`'s start-up from its newest commands. A command's size is the number of
/// results in the cache that name its record, which undercounts a command whose results a later
/// run replaced; the estimate's bound by the cheap commands' cost keeps that from inflating it.
/// The results are found by the family prefix of their check names, which the primary key
/// orders, so only the family's own results are read.
pub fn startup(conn: &Connection, family: &str) -> Result<Startup> {
    let mut select = conn.prepare_cached(
        "SELECT r.instructions, r.cpu_s, n.checks FROM records r
         JOIN (SELECT record, count(*) AS checks FROM results
               WHERE check_name > ?1 || ':' AND check_name < ?1 || ';'
               GROUP BY record) n ON n.record = r.id
         WHERE r.family = ?1
         ORDER BY r.id DESC LIMIT ?2",
    )?;
    let runs = select
        .query_map(params![family, STARTUP_RUNS], |row| {
            Ok((
                row.get::<_, Option<f64>>(0)?,
                row.get::<_, f64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let size = |checks: i64| u32::try_from(checks).unwrap_or(u32::MAX);
    let billions: Vec<(u32, f64)> = runs
        .iter()
        .filter_map(|(instructions, _, checks)| Some((size(*checks), (*instructions)? / 1e9)))
        .collect();
    let cpu_s: Vec<(u32, f64)> = runs
        .iter()
        .map(|(_, cpu_s, checks)| (size(*checks), *cpu_s))
        .collect();
    Ok(Startup {
        billions: score::startup(&billions),
        cpu_s: score::startup(&cpu_s),
    })
}

/// The least part of a share that taking the start-up out leaves. The oracle family's start-up
/// is about 12 times what its 50 checks add, so a share loses up to about 92 percent of itself.
const LEAST_SHARE_LEFT: f64 = 0.05;

/// A check's share of a command with the family's start-up taken out: `share - start-up /
/// shared_by`, where `shared_by` checks shared the start-up. Never less than
/// [`LEAST_SHARE_LEFT`] of the share, so that a command cheaper than the start-up estimate (one
/// of the cheapest tenth, see [`score::startup`]) cannot make its checks look free; the share
/// as it is when the start-up or `shared_by` is unknown.
pub fn without_startup(share: f64, startup: Option<f64>, shared_by: u32) -> f64 {
    match startup {
        Some(startup) if shared_by > 0 => {
            (share - startup / f64::from(shared_by)).max(share * LEAST_SHARE_LEFT)
        }
        _ => share,
    }
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
///
/// A check is one of the family's when the `checks` table names it, or when a cost or an area
/// count is recorded under a name with the family's prefix (`<family>:`). Only the import of
/// the old systems' history and the flaky switch write `checks`, so a family first listed under
/// Ignite has its record only in `check_costs` and `check_stats`. Reading `checks` alone made
/// every such check look never run to the gate, which then planned it as certain to fail at a
/// cost of one billion instructions: two `allocations` checks that cost 3,041 billion were
/// chosen within a budget of 470. A check `checks` does not name is on.
pub fn stats(conn: &Connection, family: &str) -> Result<CheckStatsReply> {
    let mut select = conn.prepare(
        "WITH names AS (
           SELECT name FROM checks WHERE family = ?1
           UNION SELECT check_name FROM check_costs
             WHERE check_name > ?1 || ':' AND check_name < ?1 || ';'
           UNION SELECT check_name FROM check_stats
             WHERE check_name > ?1 || ':' AND check_name < ?1 || ';')
         SELECT n.name, COALESCE(c.state, 'on'), k.median, k.unit FROM names n
         LEFT JOIN checks c ON c.name = n.name
         LEFT JOIN check_costs k ON k.check_name = n.name
         ORDER BY n.name",
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
    let startup = startup(conn, family)?;
    Ok(CheckStatsReply {
        family: family.to_owned(),
        family_rate,
        checks,
        startup_billions: startup.billions,
        startup_cpu_s: startup.cpu_s,
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

    /// Stores a command of `family` that cost `billions` and stored `checks` results.
    fn command(conn: &Connection, family: &str, id: i64, checks: u32, billions: f64) {
        conn.execute(
            "INSERT INTO records (id, machine, verb, class, family, queued_s, wall_s, cpu_s,
                                  instructions, peak_mb, ignite, at)
             VALUES (?1, 'm-1', 'tip', 'tip', ?2, 0, 1, ?3, ?4, 0, 'x', '2026-11-01T00:00:00Z')",
            params![id, family, billions / 17.0, billions * 1e9],
        )
        .unwrap();
        for n in 0..checks {
            conn.execute(
                "INSERT INTO results (check_name, hash, platform, outcome, machine, record, at,
                                      last_lookup)
                 VALUES (?1, ?2, '', 'pass', 'm-1', ?3, 'x', 'x')",
                params![format!("{family}:c{n}"), format!("{id}"), id],
            )
            .unwrap();
        }
    }

    /// The oracle family's commands each start two Java programs: about 720 billion
    /// instructions before the first scenario, then about 1.2 billion a scenario.
    #[test]
    fn a_family_start_up_is_estimated_from_commands_of_several_sizes() {
        let conn = hub::open_in_memory().unwrap();
        for (id, checks) in [(1, 30), (2, 50), (3, 30), (4, 50), (5, 40), (6, 50)] {
            command(&conn, "oracle", id, checks, 720.0 + 1.2 * f64::from(checks));
        }
        // Another family whose name starts the same is not counted.
        command(&conn, "oracle-generated", 7, 3, 1.0);
        let startup = startup(&conn, "oracle").unwrap();
        let billions = startup.billions.unwrap();
        assert!((billions - 720.0).abs() < 1e-6, "{billions}");
        assert!((startup.cpu_s.unwrap() - 720.0 / 17.0).abs() < 1e-6);
        assert_eq!(startup.in_unit("instructions"), startup.billions);

        let reply = stats(&conn, "oracle").unwrap();
        assert_eq!(reply.startup_billions, startup.billions);
        assert_eq!(stats(&conn, "rust").unwrap().startup_billions, None);
    }

    #[test]
    fn a_share_without_its_start_up_keeps_a_twentieth_of_itself() {
        // 50 checks shared a command that cost 780: 720 to start and 1.2 a check.
        assert!((without_startup(15.6, Some(720.0), 50) - 1.2).abs() < 1e-9);
        assert!((without_startup(15.6, Some(2000.0), 50) - 0.78).abs() < 1e-9);
        assert!((without_startup(15.6, None, 50) - 15.6).abs() < 1e-9);
        assert!((without_startup(15.6, Some(720.0), 0) - 15.6).abs() < 1e-9);
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
    fn a_check_only_its_runs_and_costs_name_is_in_its_family() {
        // Results of a family first listed under Ignite record costs and area counts, but no
        // row in `checks`.
        let mut conn = hub::open_in_memory().unwrap();
        add_cost(&conn, "alloc:a", 1500.0, "instructions").unwrap();
        count_run(&mut conn, "alloc:a", &["crates/x".to_owned()], false).unwrap();
        count_run(&mut conn, "alloc:b", &["crates/x".to_owned()], true).unwrap();
        // Another family whose name starts the same is not counted.
        add_cost(&conn, "alloc-x:c", 1.0, "instructions").unwrap();
        let reply = stats(&conn, "alloc").unwrap();
        let names: Vec<&str> = reply.checks.iter().map(|c| c.check.as_str()).collect();
        assert_eq!(names, ["alloc:a", "alloc:b"]);
        assert_eq!(reply.checks[0].median_cost, Some(1500.0));
        assert_eq!(reply.checks[0].unit, "instructions");
        assert_eq!(reply.checks[0].state, CheckState::On);
        assert_eq!(reply.checks[1].median_cost, None);
        assert!((reply.family_rate - 0.5).abs() < 1e-9);
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
