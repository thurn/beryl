//! Hub upkeep: what the hub does by itself, on one thread, with time from the `Clock` trait.
//!
//! Each pass of [`Upkeep::pass`]:
//!
//! - lapses expired reservations;
//! - releases the claims of a machine that has not sent a heartbeat for 2 hours ([`offline`]);
//! - marks a regression fixed once its ticket closed and its check has passed since
//!   ([`regressions`]);
//! - switches off each check with a second flaky record and files its P3 ticket, or a P1 one
//!   for a check in the always set or a command in which many checks flaked ([`flaky`]);
//! - files a P1 ticket for a tip that has waited an hour on a machine that has not reported,
//!   and one for an epic in which nothing can start ([`stuck`]);
//! - takes an hourly snapshot of the database and a daily export ([`backup`]).
//!
//! The routes call the rest as reports arrive: a regression ([`regressions::file`]), a check
//! still incomplete after its attempts ([`stuck::incomplete`]), a cache audit that disagreed
//! ([`stuck::audit`]), an exploration find ([`explore::find`]) and a pass that replaced another
//! machine's failure ([`machines::disagreement`]). The dashboard asks [`machines::unassignable`]
//! for tickets that no machine can take.
//!
//! Every ticket the hub files by itself is matched against earlier ones by a signature
//! ([`signature`], [`filing`]), so a repeated fault adds a comment and never a second ticket.

pub mod backup;
pub mod explore;
pub mod filing;
pub mod flaky;
pub mod justiciar;
pub mod machines;
pub mod offline;
pub mod regressions;
pub mod signature;
pub mod stuck;

#[cfg(test)]
mod guard_tests;
#[cfg(test)]
mod justiciar_tests;
#[cfg(test)]
mod stuck_tests;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::Connection;

use ignite_core::time::Timestamp;
use ignite_core::traits::Clock;
use ignite_store::hub::fleet;
use ignite_store::{Result, StoreError};

/// How often the hub takes a snapshot of its database.
pub const BACKUP_SECONDS: f64 = 3_600.0;

/// How long the thread waits between passes.
pub const PASS_SECONDS: u64 = 60;

/// What one pass did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UpkeepReport {
    pub lapsed_reservations: u32,
    /// Tickets whose claim was released because their machine went offline.
    pub released_claims: Vec<String>,
    /// Tickets created or commented on.
    pub tickets_filed: Vec<String>,
    pub regressions_fixed: Vec<String>,
    pub checks_switched_off: Vec<String>,
    pub snapshot: Option<PathBuf>,
    pub export: Option<PathBuf>,
}

/// The upkeep thread's state between passes.
#[derive(Debug)]
pub struct Upkeep {
    /// Where snapshots and exports go; `None` takes none.
    backups: Option<PathBuf>,
    last_snapshot: Option<Timestamp>,
    last_export_day: Option<String>,
}

impl Upkeep {
    /// Upkeep that writes backups into `backups`, continuing from the newest ones there.
    pub fn new(backups: Option<PathBuf>) -> Result<Self> {
        let (last_snapshot, last_export_day) = match &backups {
            Some(dir) => (
                backup::snapshots(dir)?.first().map(|(at, _)| *at),
                backup::last_export_day(dir),
            ),
            None => (None, None),
        };
        Ok(Upkeep {
            backups,
            last_snapshot,
            last_export_day,
        })
    }

    /// One pass. A step that fails does not stop the steps after it; the first error is
    /// returned after all have run.
    pub fn pass(&mut self, conn: &mut Connection, now: Timestamp) -> Result<UpkeepReport> {
        let mut report = UpkeepReport::default();
        let mut first_error: Option<StoreError> = None;
        let mut keep = |result: Result<()>| {
            if let Err(error) = result
                && first_error.is_none()
            {
                first_error = Some(error);
            }
        };
        keep(fleet::lapse(conn, now).map(|lapsed| {
            report.lapsed_reservations = u32::try_from(lapsed.len()).unwrap_or(u32::MAX);
        }));
        keep(offline::release_claims(conn, now).map(|released| report.released_claims = released));
        keep(regressions::mark_fixed(conn).map(|fixed| report.regressions_fixed = fixed));
        keep(flaky::switch_off_repeated(conn, now).map(|flaky| {
            for (check, filed) in flaky.switched_off {
                report.checks_switched_off.push(check);
                report.tickets_filed.push(filed.ticket);
            }
            note(&mut report, flaky.filed);
        }));
        keep(stuck::tips_waiting(conn, now).map(|filed| note(&mut report, filed)));
        keep(stuck::stuck_epics(conn, now).map(|filed| note(&mut report, filed)));
        keep(justiciar::master_lag(conn, now).map(|filed| note(&mut report, filed)));
        if let Some(dir) = self.backups.clone() {
            keep(self.backups_due(conn, &dir, now, &mut report));
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(report),
        }
    }

    fn backups_due(
        &mut self,
        conn: &Connection,
        dir: &Path,
        now: Timestamp,
        report: &mut UpkeepReport,
    ) -> Result<()> {
        if self
            .last_snapshot
            .is_none_or(|last| now.seconds_since(last) >= BACKUP_SECONDS)
        {
            report.snapshot = Some(backup::snapshot(conn, dir, now)?);
            self.last_snapshot = Some(now);
        }
        if self.last_export_day.as_deref() != Some(now.day().as_str()) {
            report.export = Some(backup::export(conn, dir, now)?);
            self.last_export_day = Some(now.day());
        }
        Ok(())
    }

    /// Runs a pass every minute until the process ends. A failed pass is reported on standard
    /// error and the next one runs as usual.
    pub fn run(mut self, mut conn: Connection, clock: &dyn Clock) -> ! {
        loop {
            if let Err(error) = self.pass(&mut conn, clock.now()) {
                eprintln!("ignite hub upkeep: {error}");
            }
            std::thread::sleep(Duration::from_secs(PASS_SECONDS));
        }
    }
}

/// Records the tickets that a filing created or commented on.
fn note(report: &mut UpkeepReport, filed: Vec<filing::Filed>) {
    for filed in filed {
        if filed.change != filing::Change::Unchanged {
            report.tickets_filed.push(filed.ticket);
        }
    }
}

/// Parses a time stored in the hub's database.
fn parse_time(text: &str) -> Result<Timestamp> {
    Timestamp::parse(text)
        .map_err(|error| StoreError::Invalid(format!("stored time {text:?}: {error}")))
}

/// Bytes as lowercase hexadecimal.
fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
}

/// The last `lines` lines of `text`.
fn tail(text: &str, lines: usize) -> String {
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}
