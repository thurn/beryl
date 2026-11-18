//! The old run log and verdicts on check failures, as each check's runs, costs and failures, so
//! the gate has scores on the first day.
//!
//! The run log is a folder of `*.jsonl` files, one JSON object per line for each run of each
//! check: `time`, `host`, `commit`, `script`, `place` (the branch it ran on), `lane`, `check`,
//! `outcome` (`pass`, `fail`, `skip`, ...), `timeout` (set when the run could not complete) and
//! `cpu_seconds`. From it the importer writes:
//!
//! - `checks`: one row per check, in the family its name starts with and the group of its old
//!   lane. A check the hub already lists keeps its row.
//! - `check_stats`: each check's runs and failures after changes to each area. A run counts
//!   once in each area its change touched. Failures come from the verdicts (see
//!   [`super::verdicts`]): a `stale`, `catch` or `mixed` verdict is a failure that a later change
//!   made pass again, the kind the score counts, and it is counted in the areas of the run that
//!   failed. Flaky failures, failures without a verdict, and verdicts whose failing run is not in
//!   the log are not counted.
//! - `check_costs`: the CPU seconds of each check's last 20 runs, in unit `cpu_s`.
//! - `flaky`: one row for each `flaky` verdict, with the tree it failed and passed on as the
//!   hash.
//!
//! Left out of everything: runs that were skipped or could not complete, the reruns of a
//! bisection at older commits (script `bisect`), and runs and verdicts in exploration lanes.
//!
//! A run's change is the set of paths between the commit it ran on and where that change
//! started. A run on the staging branch started at the previous staging commit seen in the log;
//! a run elsewhere started at the latest staging commit seen before it, or, when the log has
//! none, at the previous commit seen on its own branch. A run whose change is unknown (the
//! commit is not in the repository, or nothing came before it) or empty counts in the area `""`,
//! which no change names, so it adds only to the family's failure rate.
//!
//! The rows are computed from the whole log and then written over the hub's rows for the same
//! keys, so a second import of the same log writes nothing.

use std::borrow::Cow;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, params};
use serde::Deserialize;
use serde::de::IgnoredAny;

use ignite_core::admission::median;
use ignite_core::ids::{CheckName, Commit};
use ignite_core::inputs;
use ignite_core::time::Timestamp;

use crate::import::verdicts::{self, Verdict};
use crate::import::{ImportReport, RunLogSource, parse_time};
use crate::{Result, StoreError};

/// How many costs a check keeps, as `hub::checks` keeps them.
const COSTS_KEPT: usize = 20;
/// The script of a bisection's reruns, which say nothing of a check's own record.
const BISECT_SCRIPT: &str = "bisect";
/// The verdicts that count as a failure in a check's record.
const FAILURE_VERDICTS: &[&str] = &["stale", "catch", "mixed"];

#[derive(Deserialize)]
struct Line<'a> {
    #[serde(borrow)]
    time: Cow<'a, str>,
    #[serde(borrow, default)]
    commit: Cow<'a, str>,
    #[serde(borrow, default)]
    script: Cow<'a, str>,
    #[serde(borrow, default)]
    place: Cow<'a, str>,
    #[serde(borrow, default)]
    lane: Cow<'a, str>,
    #[serde(borrow)]
    check: Cow<'a, str>,
    #[serde(borrow)]
    outcome: Cow<'a, str>,
    #[serde(default)]
    timeout: Option<IgnoredAny>,
    #[serde(default)]
    cpu_seconds: Option<f64>,
}

/// One branch and commit some check ran on, and when it was first seen.
struct Change {
    place: String,
    commit: String,
    first: Timestamp,
    areas: Vec<u32>,
}

struct CheckInfo {
    name: String,
    lane: String,
    /// Recent (time, CPU seconds); trimmed to the newest [`COSTS_KEPT`] at the end.
    costs: Vec<(Timestamp, f64)>,
}

#[derive(Default)]
struct Log {
    changes: Vec<Change>,
    /// Change index by place, then commit.
    change_index: HashMap<String, HashMap<String, u32>>,
    checks: Vec<CheckInfo>,
    check_index: HashMap<String, u32>,
    /// The first change at which a check failed, by check and commit.
    failed_at: HashMap<(u32, String), u32>,
    /// Interned area names.
    areas: Vec<String>,
    area_index: HashMap<String, u32>,
}

/// A counted line: which check ran, on which change, whether it failed.
struct Run {
    check: u32,
    change: u32,
}

pub fn import(hub: &Connection, source: &RunLogSource) -> Result<ImportReport> {
    let mut report = ImportReport::new(&format!("run log ({})", source.folder.display()));
    let exploration: HashSet<&str> = source.exploration.iter().map(String::as_str).collect();
    let (files, mut log) = read_log(source, &exploration, &mut report)?;
    // The second read counts each run in its change's areas, which only the first read could
    // work out. It reports nothing new about the lines.
    let mut stats: HashMap<(u32, u32), (u32, u32)> = HashMap::new();
    let mut quiet = ImportReport::default();
    read_lines(&files, &exploration, &mut quiet, &mut |line, _, _| {
        let run = log.lookup(line);
        for &area in &log.changes[run.change as usize].areas {
            stats.entry((run.check, area)).or_default().0 += 1;
        }
    })?;
    let verdicts = verdicts::read(hub)?;
    count_verdicts(hub, &verdicts, &log, &exploration, &mut stats, &mut report)?;
    write_checks(hub, &log, &mut report)?;
    write_stats(hub, &log, &stats, &mut report)?;
    write_costs(hub, &mut log, &mut report)?;
    Ok(report)
}

/// One failure that a later change made pass again (a `stale`, `catch` or `mixed` verdict): a
/// check that caught something.
#[derive(Clone, Debug, PartialEq)]
pub struct Catch {
    pub check: String,
    /// When the change that failed was first seen in the log.
    pub at: Timestamp,
    /// The areas of that change; `[""]` when they are unknown.
    pub areas: Vec<String>,
}

/// Every catch in the run log since `since`, oldest first, read the way [`import`] counts
/// failures: verdicts from the hub, matched to the failing run in the log, leaving out flaky
/// failures and exploration lanes. Writes nothing.
pub fn catches(
    hub: &Connection,
    source: &RunLogSource,
    since: Timestamp,
) -> Result<(Vec<Catch>, ImportReport)> {
    let mut report = ImportReport::new(&format!("run log ({})", source.folder.display()));
    let exploration: HashSet<&str> = source.exploration.iter().map(String::as_str).collect();
    let (_, log) = read_log(source, &exploration, &mut report)?;
    let mut found = Vec::new();
    for verdict in verdicts::read(hub)? {
        let Some((check, change)) = counted_failure(&log, &exploration, &verdict) else {
            continue;
        };
        let change = &log.changes[change as usize];
        if change.first < since {
            continue;
        }
        found.push(Catch {
            check: log.checks[check as usize].name.clone(),
            at: change.first,
            areas: change
                .areas
                .iter()
                .map(|&area| log.areas[area as usize].clone())
                .collect(),
        });
    }
    found.sort_by(|a, b| (a.at, &a.check).cmp(&(b.at, &b.check)));
    report.kind("catches").read = found.len() as u64;
    Ok((found, report))
}

/// The first read of the log: every counted run interned, and each change's areas.
fn read_log(
    source: &RunLogSource,
    exploration: &HashSet<&str>,
    report: &mut ImportReport,
) -> Result<(Vec<PathBuf>, Log)> {
    let files = log_files(&source.folder)?;
    let mut log = Log::default();
    read_lines(&files, exploration, report, &mut |line, time, failed| {
        log.intern(line, time, failed);
    })?;
    find_areas(&mut log, source, report);
    Ok((files, log))
}

/// The `*.jsonl` files in the folder, in name order, which is time order for hour files.
fn log_files(folder: &Path) -> Result<Vec<PathBuf>> {
    let entries = std::fs::read_dir(folder)
        .map_err(|e| StoreError::NotFound(format!("{}: {e}", folder.display())))?;
    let mut files: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .collect();
    files.sort();
    Ok(files)
}

/// Reads every line and calls `counted` with each one that counts as a run, with its time and
/// whether it failed. Reports the lines on `report`.
fn read_lines(
    files: &[PathBuf],
    exploration: &HashSet<&str>,
    report: &mut ImportReport,
    counted: &mut dyn FnMut(&Line, Timestamp, bool),
) -> Result<()> {
    let mut buffer = Vec::new();
    for path in files {
        let file = File::open(path)
            .map_err(|e| StoreError::Invalid(format!("{}: {e}", path.display())))?;
        let mut reader = BufReader::with_capacity(1 << 20, file);
        let mut number = 0_u64;
        loop {
            buffer.clear();
            let read = reader
                .read_until(b'\n', &mut buffer)
                .map_err(|e| StoreError::Invalid(format!("{}: {e}", path.display())))?;
            if read == 0 {
                break;
            }
            number += 1;
            if buffer.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            report.kind("lines").read += 1;
            let line = match serde_json::from_slice::<Line>(&buffer) {
                Ok(line) => line,
                Err(error) => {
                    report.kind("lines").skipped += 1;
                    report.problem(format!("{}:{number}: {error}", path.display()));
                    continue;
                }
            };
            let failed = match line.outcome.as_ref() {
                "pass" => false,
                "fail" => true,
                _ => {
                    report.kind("lines").skipped += 1;
                    continue;
                }
            };
            if line.timeout.is_some()
                || line.script == BISECT_SCRIPT
                || exploration.contains(line.lane.as_ref())
            {
                report.kind("lines").skipped += 1;
                continue;
            }
            let Some(time) = parse_time(&line.time) else {
                report.kind("lines").skipped += 1;
                report.problem(format!("{}:{number}: time {:?}", path.display(), line.time));
                continue;
            };
            counted(&line, time, failed);
        }
    }
    Ok(())
}

impl Log {
    fn intern(&mut self, line: &Line, time: Timestamp, failed: bool) {
        let check = if let Some(&index) = self.check_index.get(line.check.as_ref()) {
            index
        } else {
            let index = self.checks.len() as u32;
            self.checks.push(CheckInfo {
                name: line.check.to_string(),
                lane: line.lane.to_string(),
                costs: Vec::new(),
            });
            self.check_index.insert(line.check.to_string(), index);
            index
        };
        let known = self
            .change_index
            .get(line.place.as_ref())
            .and_then(|by_commit| by_commit.get(line.commit.as_ref()))
            .copied();
        let change = if let Some(index) = known {
            let first = &mut self.changes[index as usize].first;
            *first = (*first).min(time);
            index
        } else {
            let index = self.changes.len() as u32;
            self.changes.push(Change {
                place: line.place.to_string(),
                commit: line.commit.to_string(),
                first: time,
                areas: Vec::new(),
            });
            self.change_index
                .entry(line.place.to_string())
                .or_default()
                .insert(line.commit.to_string(), index);
            index
        };
        if failed {
            self.failed_at
                .entry((check, line.commit.to_string()))
                .or_insert(change);
        }
        if let Some(cpu) = line.cpu_seconds {
            let costs = &mut self.checks[check as usize].costs;
            costs.push((time, cpu));
            if costs.len() >= 2 * COSTS_KEPT {
                keep_newest(costs);
            }
        }
    }

    /// The run of a line the first read interned.
    fn lookup(&self, line: &Line) -> Run {
        Run {
            check: self.check_index[line.check.as_ref()],
            change: self.change_index[line.place.as_ref()][line.commit.as_ref()],
        }
    }

    fn area(&mut self, name: &str) -> u32 {
        if let Some(&index) = self.area_index.get(name) {
            return index;
        }
        let index = self.areas.len() as u32;
        self.areas.push(name.to_owned());
        self.area_index.insert(name.to_owned(), index);
        index
    }
}

/// Keeps the newest [`COSTS_KEPT`] costs, oldest first.
fn keep_newest(costs: &mut Vec<(Timestamp, f64)>) {
    costs.sort_by_key(|(time, _)| *time);
    let excess = costs.len().saturating_sub(COSTS_KEPT);
    costs.drain(..excess);
}

/// Works out each change's areas from the paths between it and where it started.
fn find_areas(log: &mut Log, source: &RunLogSource, report: &mut ImportReport) {
    let mut order: Vec<u32> = (0..log.changes.len() as u32).collect();
    order.sort_by_key(|&i| (log.changes[i as usize].first, i));
    let mut last_staging: Option<String> = None;
    let mut last_on_place: HashMap<String, String> = HashMap::new();
    let mut paths_between: HashMap<(String, String), Option<BTreeSet<String>>> = HashMap::new();
    let unknown = log.area("");
    for index in order {
        let change = &log.changes[index as usize];
        let (place, commit) = (change.place.clone(), change.commit.clone());
        let from = if place == source.staging {
            last_staging.clone()
        } else {
            last_staging
                .clone()
                .or_else(|| last_on_place.get(&place).cloned())
        };
        let areas = from.and_then(|from| {
            paths_between
                .entry((from.clone(), commit.clone()))
                .or_insert_with(|| changed_areas(source, &from, &commit, report))
                .clone()
        });
        let count = report.kind("changes");
        count.read += 1;
        let ids: Vec<u32> = match areas {
            Some(areas) if !areas.is_empty() => areas.iter().map(|area| log.area(area)).collect(),
            _ => {
                count.skipped += 1;
                vec![unknown]
            }
        };
        log.changes[index as usize].areas = ids;
        if place == source.staging {
            last_staging = Some(commit.clone());
        }
        last_on_place.insert(place, commit);
    }
}

/// The areas of the paths that differ between two commits; `None` when git cannot say.
fn changed_areas(
    source: &RunLogSource,
    from: &str,
    to: &str,
    report: &mut ImportReport,
) -> Option<BTreeSet<String>> {
    let (Ok(from_commit), Ok(to_commit)) = (Commit::new(from), Commit::new(to)) else {
        return None;
    };
    match source.git.changed(&from_commit, &to_commit) {
        Ok(paths) => Some(
            paths
                .iter()
                .map(|path| inputs::area(path, &source.graph).to_string())
                .collect(),
        ),
        Err(error) => {
            report.problem(format!("paths between {from} and {to}: {error}"));
            None
        }
    }
}

/// Counts each failure verdict in the areas of the run that failed, and records each flaky
/// verdict.
fn count_verdicts(
    hub: &Connection,
    verdicts: &[Verdict],
    log: &Log,
    exploration: &HashSet<&str>,
    stats: &mut HashMap<(u32, u32), (u32, u32)>,
    report: &mut ImportReport,
) -> Result<()> {
    let mut insert_flaky = hub.prepare(
        "INSERT OR IGNORE INTO flaky (check_name, hash, commit_sha, machine, at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    for verdict in verdicts {
        report.kind("verdicts").read += 1;
        if in_exploration(log, exploration, verdict) {
            report.kind("verdicts").skipped += 1;
            continue;
        }
        if verdict.kind == "flaky" {
            let at = parse_time(&verdict.time);
            if CheckName::new(verdict.check.as_str()).is_err() || at.is_none() {
                report.kind("verdicts").skipped += 1;
                continue;
            }
            let hash = verdict.fixed.strip_prefix("tree ").unwrap_or_default();
            let at = at.map(|t| t.to_string());
            let written = insert_flaky.execute(params![
                verdict.check,
                hash,
                verdict.commit,
                verdict.host,
                at
            ])?;
            let count = report.kind("flaky");
            count.read += 1;
            if written > 0 {
                count.written += 1;
            } else {
                count.skipped += 1;
            }
            continue;
        }
        let Some((check, change)) = counted_failure(log, exploration, verdict) else {
            report.kind("verdicts").skipped += 1;
            continue;
        };
        for &area in &log.changes[change as usize].areas {
            stats.entry((check, area)).or_default().1 += 1;
        }
    }
    Ok(())
}

/// Whether a verdict is on a check of an exploration lane.
fn in_exploration(log: &Log, exploration: &HashSet<&str>, verdict: &Verdict) -> bool {
    let lane = log
        .check_index
        .get(&verdict.check)
        .map_or(verdict.lane.as_str(), |&c| {
            log.checks[c as usize].lane.as_str()
        });
    exploration.contains(lane) || exploration.contains(verdict.lane.as_str())
}

/// The check and the change of the failing run of a verdict that counts as a failure: one of
/// [`FAILURE_VERDICTS`], outside the exploration lanes, whose failing run is in the log.
fn counted_failure(
    log: &Log,
    exploration: &HashSet<&str>,
    verdict: &Verdict,
) -> Option<(u32, u32)> {
    if !FAILURE_VERDICTS.contains(&verdict.kind.as_str())
        || in_exploration(log, exploration, verdict)
    {
        return None;
    }
    let check = *log.check_index.get(&verdict.check)?;
    let change = *log.failed_at.get(&(check, verdict.commit.clone()))?;
    Some((check, change))
}

/// The family of a check: the part of its name before the first `:`.
fn family(name: &str) -> &str {
    name.split_once(':').map_or(name, |(family, _)| family)
}

fn write_checks(hub: &Connection, log: &Log, report: &mut ImportReport) -> Result<()> {
    let mut insert = hub.prepare(
        "INSERT INTO checks (name, family, kind, state, check_group)
         VALUES (?1, ?2, 'regression', 'on', ?3)
         ON CONFLICT(name) DO NOTHING",
    )?;
    // Names Ignite does not accept, such as browser checks titled with spaces, are reported as
    // one problem with an example, since a log can hold thousands of them.
    let mut invalid: Option<(u64, String)> = None;
    for check in &log.checks {
        report.kind("checks").read += 1;
        if let Err(error) = CheckName::new(check.name.as_str()) {
            report.kind("checks").skipped += 1;
            invalid.get_or_insert_with(|| (0, error.to_string())).0 += 1;
            continue;
        }
        let written = insert.execute(params![check.name, family(&check.name), check.lane])?;
        let count = report.kind("checks");
        if written > 0 {
            count.written += 1;
        } else {
            count.skipped += 1;
        }
    }
    if let Some((count, example)) = invalid {
        report.problem(format!(
            "{count} checks left out because Ignite does not accept their names, such as {example}"
        ));
    }
    Ok(())
}

fn write_stats(
    hub: &Connection,
    log: &Log,
    stats: &HashMap<(u32, u32), (u32, u32)>,
    report: &mut ImportReport,
) -> Result<()> {
    let mut upsert = hub.prepare(
        "INSERT INTO check_stats (check_name, area, runs, failures) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(check_name, area) DO UPDATE SET
           runs = excluded.runs, failures = excluded.failures
         WHERE runs IS NOT excluded.runs OR failures IS NOT excluded.failures",
    )?;
    let mut keys: Vec<&(u32, u32)> = stats.keys().collect();
    keys.sort();
    for key in keys {
        let (runs, failures) = stats[key];
        let name = &log.checks[key.0 as usize].name;
        let count = report.kind("check stats");
        count.read += 1;
        if CheckName::new(name.as_str()).is_err() {
            count.skipped += 1;
            continue;
        }
        // Two verdicts on one failing run would count it twice; a failure is always a run.
        let failures = failures.min(runs);
        let area = &log.areas[key.1 as usize];
        let written = upsert.execute(params![name, area, runs, failures])?;
        let count = report.kind("check stats");
        if written > 0 {
            count.written += 1;
        } else {
            count.skipped += 1;
        }
    }
    Ok(())
}

fn write_costs(hub: &Connection, log: &mut Log, report: &mut ImportReport) -> Result<()> {
    let mut upsert = hub.prepare(
        "INSERT INTO check_costs (check_name, costs, median, unit) VALUES (?1, ?2, ?3, 'cpu_s')
         ON CONFLICT(check_name) DO UPDATE SET
           costs = excluded.costs, median = excluded.median, unit = excluded.unit
         WHERE costs IS NOT excluded.costs OR unit IS NOT excluded.unit",
    )?;
    for check in &mut log.checks {
        if check.costs.is_empty() {
            continue;
        }
        let count = report.kind("check costs");
        count.read += 1;
        if CheckName::new(check.name.as_str()).is_err() {
            count.skipped += 1;
            continue;
        }
        keep_newest(&mut check.costs);
        let costs: Vec<f64> = check.costs.iter().map(|(_, cost)| *cost).collect();
        let json = serde_json::to_string(&costs).expect("numbers serialize");
        let written = upsert.execute(params![check.name, json, median(&costs)])?;
        let count = report.kind("check costs");
        if written > 0 {
            count.written += 1;
        } else {
            count.skipped += 1;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "runlog_tests.rs"]
mod tests;
