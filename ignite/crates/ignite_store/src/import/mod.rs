//! Reading the old systems' data into the hub's database at the switch. Every importer only
//! reads its source, so a failed switch can be undone.
//!
//! [`run`] does every import asked for in one transaction on the hub's database, so that a dry
//! run can roll all of it back, and so that the run log's importer sees the verdict comments the
//! ticket importer has just written. Each importer writes a row only when the hub's copy differs,
//! so a second import of the same sources writes nothing.

pub mod compile;
pub mod runlog;
pub mod tickets;
mod verdicts;

use std::fmt;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};

use ignite_core::inputs::CrateGraph;
use ignite_core::time::Timestamp;
use ignite_core::traits::Git;

use crate::{Result, StoreError};

/// How many problems a report quotes; the rest are only counted.
const PROBLEMS_QUOTED: usize = 20;

/// What one importer did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportReport {
    /// What was read, such as `tickets` or `run log`.
    pub source: String,
    /// What it read, wrote and skipped, by kind of row, in the order first counted.
    pub kinds: Vec<KindCount>,
    /// Rows that could not be imported, each with the reason; the first few only.
    pub problems: Vec<String>,
    /// Problems beyond those quoted in `problems`.
    pub more_problems: u64,
}

/// The rows of one kind an importer read, wrote (new or changed in the hub) and skipped
/// (unchanged, or left out for a reason the importer documents). A kind that is not a hub row,
/// such as a run log line, is only read or skipped: what it adds up to is written as other
/// kinds.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KindCount {
    pub kind: String,
    pub read: u64,
    pub written: u64,
    pub skipped: u64,
}

impl ImportReport {
    pub fn new(source: &str) -> Self {
        Self {
            source: source.to_owned(),
            ..Self::default()
        }
    }

    /// The counts of `kind`, added at the end the first time.
    pub fn kind(&mut self, kind: &str) -> &mut KindCount {
        let index = if let Some(index) = self.kinds.iter().position(|k| k.kind == kind) {
            index
        } else {
            self.kinds.push(KindCount {
                kind: kind.to_owned(),
                ..KindCount::default()
            });
            self.kinds.len() - 1
        };
        &mut self.kinds[index]
    }

    /// The counts of `kind`, if any were taken.
    pub fn get(&self, kind: &str) -> Option<&KindCount> {
        self.kinds.iter().find(|k| k.kind == kind)
    }

    pub fn problem(&mut self, message: String) {
        if self.problems.len() < PROBLEMS_QUOTED {
            self.problems.push(message);
        } else {
            self.more_problems += 1;
        }
    }

    /// Whether anything was written.
    pub fn wrote(&self) -> bool {
        self.kinds.iter().any(|k| k.written > 0)
    }
}

impl fmt::Display for ImportReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}:", self.source)?;
        let width = self.kinds.iter().map(|k| k.kind.len()).max().unwrap_or(0);
        for k in &self.kinds {
            writeln!(
                f,
                "  {:<width$}  read {:>8}  written {:>8}  skipped {:>8}",
                k.kind, k.read, k.written, k.skipped
            )?;
        }
        for problem in &self.problems {
            writeln!(f, "  problem: {problem}")?;
        }
        if self.more_problems > 0 {
            writeln!(f, "  and {} more problems", self.more_problems)?;
        }
        Ok(())
    }
}

/// What the run log's importer needs besides the log itself.
pub struct RunLogSource<'a> {
    /// The folder of the run log's `*.jsonl` files.
    pub folder: PathBuf,
    /// The repository the logged commits are in, for the paths each change touched.
    pub git: &'a dyn Git,
    /// The repository's workspace packages, which name the areas of those paths.
    pub graph: CrateGraph,
    /// The name of the branch changes land on, whose runs mark where each change started.
    pub staging: String,
    /// The old lanes whose checks are exploration: their runs and failures are left out.
    pub exploration: Vec<String>,
}

/// Everything one `ignite import` reads; each source is optional.
#[derive(Default)]
pub struct Sources<'a> {
    /// The old ticket service's database file.
    pub tickets: Option<PathBuf>,
    pub run_log: Option<RunLogSource<'a>>,
    /// The efficiency store that holds the `compile_timings` table.
    pub compile_timings: Option<PathBuf>,
}

/// Imports every source given, tickets first, in one transaction that a dry run rolls back.
pub fn run(hub: &mut Connection, sources: &Sources, dry_run: bool) -> Result<Vec<ImportReport>> {
    let tx = hub.transaction()?;
    let mut reports = Vec::new();
    if let Some(path) = &sources.tickets {
        reports.push(tickets::import(&tx, path)?);
    }
    if let Some(source) = &sources.run_log {
        reports.push(runlog::import(&tx, source)?);
    }
    if let Some(path) = &sources.compile_timings {
        reports.push(compile::import(&tx, path)?);
    }
    if dry_run {
        tx.rollback()?;
    } else {
        tx.commit()?;
    }
    Ok(reports)
}

/// Opens an old system's database for reading only.
pub(crate) fn open_source(path: &Path) -> Result<Connection> {
    if !path.is_file() {
        return Err(StoreError::NotFound(format!(
            "{}: no such file",
            path.display()
        )));
    }
    Ok(Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?)
}

/// Parses the old systems' times: RFC 3339 with `Z` or a `+HH:MM`/`-HH:MM` offset, with or
/// without fractional seconds. The hub stores every time in UTC with a `Z`.
pub(crate) fn parse_time(text: &str) -> Option<Timestamp> {
    if text.ends_with('Z') {
        return Timestamp::parse(text).ok();
    }
    let split = text.len().checked_sub(6)?;
    let (local, offset) = text.split_at(split);
    let sign = match offset.as_bytes().first()? {
        b'+' => 1.0,
        b'-' => -1.0,
        _ => return None,
    };
    let (hours, minutes) = offset[1..].split_once(':')?;
    let hours: f64 = hours.parse().ok()?;
    let minutes: f64 = minutes.parse().ok()?;
    let local = Timestamp::parse(&format!("{local}Z")).ok()?;
    Some(local.plus_seconds(-sign * (hours * 3600.0 + minutes * 60.0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_with_offsets_become_utc() {
        let utc = parse_time("2026-11-06T08:41:48Z").unwrap();
        assert_eq!(parse_time("2026-11-06T00:41:48-08:00"), Some(utc));
        assert_eq!(parse_time("2026-11-06T08:41:48+00:00"), Some(utc));
        assert_eq!(
            parse_time("2026-11-06T10:41:48.500+02:00").unwrap(),
            utc.plus_seconds(0.5)
        );
        assert_eq!(parse_time("yesterday"), None);
    }

    #[test]
    fn a_report_quotes_the_first_problems_and_counts_the_rest() {
        let mut report = ImportReport::new("test");
        for n in 0..25 {
            report.problem(format!("problem {n}"));
        }
        assert_eq!(report.problems.len(), PROBLEMS_QUOTED);
        assert_eq!(report.more_problems, 5);
        report.kind("rows").read += 2;
        report.kind("rows").written += 1;
        let text = report.to_string();
        assert!(
            text.contains("rows  read        2  written        1"),
            "{text}"
        );
        assert!(text.contains("and 5 more problems"), "{text}");
    }
}
