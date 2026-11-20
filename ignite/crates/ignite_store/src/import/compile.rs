//! The recorded per-crate compile timings.
//!
//! The old efficiency store's `compile_timings` table holds one row per crate and mode (`check`,
//! `clippy`, `test-build`, ...) of each build: `host`, `run` (the build), `time`, `crate`,
//! `mode`, `seconds` and `fresh` (1 when cargo did not rebuild the crate). Each build becomes
//! one hub record, verb `build` and class `agent`, made by Ignite version [`IMPORTED`], with one
//! compile unit per row. Cargo measured each unit's duration, not its CPU time, so `seconds`
//! goes into `cpu_s` as the nearest measure, and `instructions` stays empty. The record's own
//! `cpu_s` is the sum of its units'; its wall, queue and memory figures are unknown and zero.
//!
//! A build is identified by its machine and start time among imported records, so a second
//! import finds the records it made and writes nothing.

use std::collections::BTreeMap;
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};

use crate::Result;
use crate::import::{ImportReport, open_source, parse_time};

/// The `ignite` version of every record the import makes.
pub const IMPORTED: &str = "import";

struct Unit {
    crate_name: String,
    mode: String,
    seconds: f64,
    fresh: bool,
}

struct Build {
    host: String,
    /// The earliest time of its rows, as the hub stores times.
    at: String,
    units: Vec<Unit>,
}

pub fn import(hub: &Connection, timings: &Path) -> Result<ImportReport> {
    let old = open_source(timings)?;
    let mut report = ImportReport::new(&format!("compile timings ({})", timings.display()));
    let builds = read_builds(&old, &mut report)?;
    let mut find = hub.prepare(
        "SELECT id FROM records WHERE machine = ?1 AND at = ?2 AND verb = 'build' AND ignite = ?3",
    )?;
    let mut insert_record = hub.prepare(
        "INSERT INTO records (machine, verb, class, queued_s, wall_s, cpu_s, peak_mb, ignite, at)
         VALUES (?1, 'build', 'agent', 0, 0, ?2, 0, ?3, ?4)",
    )?;
    let mut upsert_unit = hub.prepare(
        "INSERT INTO compile_units (record, crate_name, mode, instructions, cpu_s, rebuilt)
         VALUES (?1, ?2, ?3, NULL, ?4, ?5)
         ON CONFLICT(record, crate_name, mode) DO UPDATE SET
           cpu_s = excluded.cpu_s, rebuilt = excluded.rebuilt
         WHERE cpu_s IS NOT excluded.cpu_s OR rebuilt IS NOT excluded.rebuilt",
    )?;
    for build in builds.values() {
        report.kind("builds").read += 1;
        let existing: Option<i64> = find
            .query_row(params![build.host, build.at, IMPORTED], |row| row.get(0))
            .optional()?;
        let record = if let Some(id) = existing {
            report.kind("builds").skipped += 1;
            id
        } else {
            let cpu: f64 = build.units.iter().map(|u| u.seconds).sum();
            insert_record.execute(params![build.host, cpu, IMPORTED, build.at])?;
            report.kind("builds").written += 1;
            hub.last_insert_rowid()
        };
        for unit in &build.units {
            let written = upsert_unit.execute(params![
                record,
                unit.crate_name,
                unit.mode,
                unit.seconds,
                i64::from(!unit.fresh)
            ])?;
            let count = report.kind("compile units");
            if written > 0 {
                count.written += 1;
            } else {
                count.skipped += 1;
            }
        }
    }
    Ok(report)
}

/// The builds of the old table, by host and run.
fn read_builds(
    old: &Connection,
    report: &mut ImportReport,
) -> Result<BTreeMap<(String, String), Build>> {
    let mut select = old.prepare(
        "SELECT host, run, time, crate, mode, seconds, fresh FROM compile_timings
         ORDER BY host, run, time, crate, mode",
    )?;
    let mut rows = select.query([])?;
    let mut builds: BTreeMap<(String, String), Build> = BTreeMap::new();
    while let Some(row) = rows.next()? {
        report.kind("compile units").read += 1;
        let host: String = row.get(0)?;
        let run: String = row.get(1)?;
        let time: String = row.get(2)?;
        let Some(at) = parse_time(&time) else {
            report.kind("compile units").skipped += 1;
            report.problem(format!("build {run} on {host}: time {time:?}"));
            continue;
        };
        let at = at.to_string();
        let build = builds.entry((host.clone(), run)).or_insert_with(|| Build {
            host,
            at: at.clone(),
            units: Vec::new(),
        });
        if at < build.at {
            build.at = at;
        }
        build.units.push(Unit {
            crate_name: row.get(3)?,
            mode: row.get(4)?,
            seconds: row.get(5)?,
            fresh: row.get::<_, i64>(6)? != 0,
        });
    }
    Ok(builds)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub;
    use crate::import::{Sources, run};

    fn timings(name: &str) -> std::path::PathBuf {
        let dir = ignite_core::test_files::temp_dir()
            .join(format!("ignite-compile-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("store.sqlite");
        let _ = std::fs::remove_file(&path);
        let old = Connection::open(&path).unwrap();
        old.execute_batch(
            "CREATE TABLE compile_timings (host TEXT NOT NULL, run TEXT NOT NULL,
               time TEXT NOT NULL, crate TEXT NOT NULL, mode TEXT NOT NULL,
               seconds REAL NOT NULL, fresh INTEGER NOT NULL,
               PRIMARY KEY (host, run, crate, mode));
             INSERT INTO compile_timings VALUES
               ('m1', 'r1', '2026-11-17T17:21:55.120Z', 'alpha', 'clippy', 0.0, 1),
               ('m1', 'r1', '2026-11-17T17:21:55.120Z', 'beta', 'clippy', 4.5, 0),
               ('m1', 'r2', '2026-11-17T18:00:00.000Z', 'beta', 'test-build', 7.0, 0);",
        )
        .unwrap();
        path
    }

    fn import_timings(hub: &mut Connection, path: &Path, dry_run: bool) -> ImportReport {
        let sources = Sources {
            compile_timings: Some(path.to_owned()),
            ..Sources::default()
        };
        run(hub, &sources, dry_run).unwrap().remove(0)
    }

    #[test]
    fn each_build_becomes_a_record_with_its_units_once() {
        let path = timings("once");
        let mut hub = hub::open_in_memory().unwrap();
        let first = import_timings(&mut hub, &path, false);
        assert_eq!(first.get("builds").unwrap().written, 2);
        assert_eq!(first.get("compile units").unwrap().written, 3);
        let (cpu, at): (f64, String) = hub
            .query_row(
                "SELECT cpu_s, at FROM records WHERE machine = 'm1' ORDER BY at LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((cpu, at.as_str()), (4.5, "2026-11-17T17:21:55.120Z"));
        let rebuilt: Vec<(String, i64)> = hub
            .prepare(
                "SELECT crate_name, rebuilt FROM compile_units WHERE mode = 'clippy' ORDER BY 1",
            )
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(|row| row.unwrap())
            .collect();
        assert_eq!(
            rebuilt,
            vec![("alpha".to_owned(), 0), ("beta".to_owned(), 1)]
        );

        let second = import_timings(&mut hub, &path, false);
        assert!(!second.wrote(), "{second}");
    }

    #[test]
    fn a_dry_run_writes_no_record() {
        let path = timings("dry");
        let mut hub = hub::open_in_memory().unwrap();
        assert!(import_timings(&mut hub, &path, true).wrote());
        let records: i64 = hub
            .query_row("SELECT count(*) FROM records", [], |r| r.get(0))
            .unwrap();
        assert_eq!(records, 0);
    }
}
