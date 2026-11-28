//! Hub upkeep: what the hub does by itself, on one thread, with time from the `Clock` trait.
//!
//! Each pass of [`Upkeep::pass`]:
//!
//! - lapses expired reservations;
//! - returns a machine to `active` when its `checks-only` or `off` state ends
//!   ([`machine_states`]);
//! - releases the claims of a machine that has not sent a heartbeat for 2 hours ([`offline`]);
//! - marks a regression fixed once its ticket closed and its check has passed since
//!   ([`regressions`]);
//! - files remedies for test-only regression fixes and flakes, and switches off ordinary
//!   checks that fail for those reasons ([`invalid_checks`], [`flaky`]);
//! - files a P1 ticket for a tip that has waited an hour on a machine that has not reported,
//!   and one for an epic in which nothing can start ([`stuck`]);
//! - files a P0 ticket for a person when a machine that is not off stops reporting for half
//!   an hour ([`silent`]);
//! - files justiciar tickets for sustained machine resource stalls ([`resource_stalls`]);
//! - freezes landings across the fleet when `master` falls six hours behind, files a
//!   postmortem ticket for the stall, and ends the freeze once `master` catches up
//!   ([`freeze`]);
//! - files the copy of each meta-ticket whose schedule says one is due ([`schedules`]);
//! - opens the tickets the hub filed pending once their automated source has a positive
//!   allocation, its own or the General pool's ([`source_release`]);
//! - closes tickets whose fault has stayed clear for an hour, unless a person touched them or
//!   a worker holds them, and settles regressions as soon as verification confirms the fix
//!   ([`clearing`]);
//! - deletes efficiency rows older than any reader needs, a few small batches at a time
//!   (`ignite_store::hub::metrics::delete_expired`);
//! - deletes transcript digest lines a week after they arrived
//!   (`ignite_store::hub::transcripts::delete_expired`);
//! - once an hour, removes the attachment files of tickets closed long ago or beyond the
//!   budget, and files a ticket when open tickets alone fill it ([`attachments`]);
//! - takes an hourly snapshot of the database and a daily export ([`backup`]).
//!
//! The routes call the rest as reports arrive: a landing a tip round reverted ([`reverted::file`]),
//! a regression ([`regressions::file`], which
//! records why tip verification missed a check a landing found failing, in [`missed`]), a check
//! still incomplete after its attempts ([`stuck::incomplete`]), a cache audit that disagreed
//! ([`stuck::audit`]), an exploration find ([`explore::find`]) and a pass that replaced another
//! machine's failure ([`machines::disagreement`]), and a heartbeat from a machine whose claims
//! were released for silence ([`returning::reconcile`]). The dashboard asks [`machines::unassignable`]
//! for tickets that no machine can take.
//!
//! Automatic tickets match earlier faults by signature ([`signature`], [`filing`]).
//! Exploration findings also share a limited set of tickets waiting for approval.

pub mod attachments;
pub mod backup;
pub mod clearing;
mod clearing_kinds;
pub(crate) mod culprit_affinity;
pub mod explore;
pub mod filing;
mod flaky;
pub mod freeze;
mod invalid_checks;
pub mod justiciar;
pub mod machine_states;
pub mod machines;
pub mod missed;
pub mod offline;
mod platform_wait;
mod regression_stalls;
pub mod regressions;
mod resource_stalls;
pub mod returning;
pub mod reverted;
pub mod schedules;
pub mod signature;
pub mod silent;
pub mod source_release;
pub mod stuck;
mod unproven_holds;

#[cfg(test)]
mod attachments_tests;
#[cfg(test)]
mod clearing_tests;
#[cfg(test)]
mod freeze_tests;
#[cfg(test)]
mod guard_tests;
#[cfg(test)]
mod justiciar_recovery_tests;
#[cfg(test)]
mod justiciar_tests;
#[cfg(test)]
mod missed_tests;
#[cfg(test)]
mod platform_wait_tests;
#[cfg(test)]
mod regression_stalls_replay;
#[cfg(test)]
mod resource_stalls_tests;
#[cfg(test)]
mod reverted_tests;
#[cfg(test)]
mod schedules_tests;
#[cfg(test)]
mod source_release_tests;
#[cfg(test)]
mod stuck_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod unproven_holds_tests;

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::Connection;

use ignite_core::config::Rules;
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
    /// Tickets whose claim was released because their machine went offline, or because no
    /// Ignite session or machine held them.
    pub released_claims: Vec<String>,
    /// Tickets created or commented on.
    pub tickets_filed: Vec<String>,
    pub regressions_fixed: Vec<String>,
    /// Tickets closed because the fault they report cleared ([`clearing`]).
    pub tickets_closed: Vec<String>,
    pub checks_switched_off: Vec<String>,
    /// Checks restored after they were switched off by imported flake verdicts.
    pub checks_restored: Vec<String>,
    /// Efficiency rows deleted for being older than any reader needs.
    pub metrics_deleted: usize,
    /// `Some(true)` when this pass froze landings because `master` fell far behind, and
    /// `Some(false)` when it ended the freeze because `master` caught up ([`freeze`]).
    pub landings_frozen: Option<bool>,
    /// Tickets whose attachment files were removed.
    pub attachments_removed: Vec<String>,
    pub snapshot: Option<PathBuf>,
    pub export: Option<PathBuf>,
}

/// The upkeep thread's state between passes.
#[derive(Debug)]
pub struct Upkeep {
    /// Where snapshots and exports go; `None` takes none.
    backups: Option<PathBuf>,
    rules: Rules,
    last_snapshot: Option<Timestamp>,
    last_export_day: Option<String>,
    /// The attachment files to keep within their budget; `None` tidies none.
    attachments: Option<crate::attachments::AttachmentFiles>,
    last_tidy: Option<Timestamp>,
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
            rules: Rules::default(),
            last_snapshot,
            last_export_day,
            attachments: None,
            last_tidy: None,
        })
    }

    /// Uses the repository's protected check list for automatic switching.
    #[must_use]
    pub fn with_rules(mut self, rules: Rules) -> Self {
        self.rules = rules;
        self
    }

    /// The same upkeep, also keeping `files` within their budget once an hour.
    #[must_use]
    pub fn with_attachments(mut self, files: crate::attachments::AttachmentFiles) -> Self {
        self.attachments = Some(files);
        self
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
        keep(machine_states::lapse(conn, now).map(|_| ()));
        keep(ignite_store::hub::placement_deferrals::prune(conn));
        keep(offline::release_claims(conn, now).map(|released| report.released_claims = released));
        keep(offline::release_unheld(conn, now).map(|released| {
            report.released_claims.extend(released);
        }));
        keep(regressions::mark_fixed(conn).map(|fixed| report.regressions_fixed = fixed));
        keep(flaky::recover_imported(conn, now).map(|restored| report.checks_restored = restored));
        for result in [
            invalid_checks::regressions(conn, &self.rules, now),
            flaky::handle(conn, &self.rules, now),
        ] {
            keep(result.map(|remedy| {
                report.checks_switched_off.extend(remedy.switched_off);
                note(&mut report, remedy.filed);
            }));
        }
        keep(
            unproven_holds::switch_off(conn, &self.rules, now)
                .map(|off| report.checks_switched_off.extend(off)),
        );
        keep(stuck::tips_waiting(conn, now).map(|filed| note(&mut report, filed)));
        keep(silent::machines_silent(conn, now).map(|filed| note(&mut report, filed)));
        keep(stuck::stuck_epics(conn, now).map(|filed| note(&mut report, filed)));
        keep(justiciar::master_lag(conn, now).map(|filed| note(&mut report, filed)));
        keep(freeze::pass(conn, now).map(|freeze| {
            if freeze.lifted {
                report.landings_frozen = Some(false);
            } else if freeze.frozen {
                report.landings_frozen = Some(true);
            }
            note(&mut report, freeze.filed);
        }));
        keep(resource_stalls::file(conn, now).map(|filed| note(&mut report, filed)));
        keep(schedules::file_due(conn, now).map(|copies| report.tickets_filed.extend(copies)));
        if let Some(files) = self.attachments.clone()
            && self
                .last_tidy
                .is_none_or(|last| now.seconds_since(last) >= attachments::TIDY_SECONDS)
        {
            self.last_tidy = Some(now);
            keep(attachments::tidy(conn, &files, now).map(|tidied| {
                report.attachments_removed = tidied.removed;
                note(&mut report, tidied.filed.into_iter().collect());
            }));
        }
        // After every step that files, so a ticket filed on this pass opens on the same pass.
        keep(source_release::release(conn, now).map(|_| ()));
        // After every step that files, so a fault seen again on this pass keeps its ticket open.
        keep(clearing::close_cleared(conn, now).map(|closed| report.tickets_closed = closed));
        keep(
            ignite_store::hub::metrics::delete_expired(conn, now)
                .map(|deleted| report.metrics_deleted = deleted),
        );
        keep(ignite_store::hub::machine_snapshots::delete_expired(conn, now).map(|_| ()));
        keep(ignite_store::hub::transcripts::delete_expired(conn, now).map(|_| ()));
        if let Some(dir) = self.backups.clone() {
            keep(backup::compress_existing(&dir));
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

/// Records the tickets that a filing created or updated.
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
