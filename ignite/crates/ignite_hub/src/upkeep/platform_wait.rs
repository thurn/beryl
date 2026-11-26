//! The platform work named by a tip wait, shared by filing and clearing its ticket.

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use ignite_core::config::Os;
use ignite_core::ids::{CheckName, Commit};
use ignite_core::pins::machine_label;
use ignite_core::protocol::hub::TipWaiting;
use ignite_core::protocol::tickets::{TicketFields, TicketType};
use ignite_core::time::Timestamp;
use ignite_store::Result;
use ignite_store::hub::rows;

use super::filing::Filing;
use super::signature::Signature;
use super::stuck::{TIP_SLOW_WAIT_SECONDS, TIP_WAIT_SECONDS};

/// The most recent platform wait reported on a ticket. Heartbeats and reports of silence
/// do not change this scope.
pub(super) const WAIT_SCOPE_KEY: &str = "ignite_platform_wait";

#[derive(Serialize, Deserialize)]
pub(super) struct PlatformWait {
    pub commit: Commit,
    pub platform: Os,
    checks: Vec<CheckName>,
    /// True when the ticket was filed because total wait time exceeded the slow-progress bound
    /// even while a machine was still storing results.
    #[serde(default)]
    pub slow: bool,
}

impl From<&TipWaiting> for PlatformWait {
    fn from(report: &TipWaiting) -> Self {
        let mut checks = report.checks.clone();
        checks.sort();
        checks.dedup();
        Self {
            commit: report.commit.clone(),
            platform: report.platform,
            checks,
            slow: false,
        }
    }
}

impl PlatformWait {
    pub fn with_slow(mut self, slow: bool) -> Self {
        self.slow = slow;
        self
    }

    pub fn remaining(&self) -> usize {
        self.checks.len()
    }

    /// Plain-English count of the awaited checks still missing for this wait.
    pub fn remaining_summary(&self) -> String {
        match self.remaining() {
            1 => "1 awaited check remains".to_owned(),
            n => format!("{n} awaited checks remain"),
        }
    }

    /// Plain-English summary of how many awaited checks remain and how fast `machine` has been
    /// storing results for this platform over the last [`TIP_WAIT_SECONDS`].
    pub fn progress_summary(
        &self,
        conn: &Connection,
        machine: &rows::Machine,
        now: Timestamp,
    ) -> Result<String> {
        let completed = self.completed_recently(conn, machine, now)?;
        let rate = completed as f64 / (TIP_WAIT_SECONDS / 3_600.0);
        let window_min = TIP_WAIT_SECONDS / 60.0;
        let noun = if completed == 1 { "result" } else { "results" };
        Ok(format!(
            "{}; this machine stored {completed} {} {noun} in the last {window_min:.0} minutes \
             ({rate:.1}/hour).",
            self.remaining_summary(),
            self.platform.as_str(),
        ))
    }

    /// Counts platform results and awaited family listings `machine` stored within
    /// [`TIP_WAIT_SECONDS`], so a ticket can distinguish a slow machine from a stalled one.
    pub fn completed_recently(
        &self,
        conn: &Connection,
        machine: &rows::Machine,
        now: Timestamp,
    ) -> Result<u64> {
        let platform = self.platform.as_str();
        let cutoff = now.plus_seconds(-TIP_WAIT_SECONDS).to_string();
        let results: i64 = conn.query_row(
            "SELECT COUNT(*) FROM results
             WHERE platform = ?1 AND machine = ?2 AND at >= ?3",
            params![platform, machine.id, cutoff],
            |row| row.get(0),
        )?;
        let mut listing = conn.prepare_cached(
            "SELECT COUNT(*) FROM listings WHERE family = ?1 AND machine = ?2 AND at >= ?3",
        )?;
        let mut listings = 0i64;
        for check in &self.checks {
            if let Some((family, "listing")) = check.as_str().split_once(':') {
                listings += listing.query_row(params![family, machine.id, cutoff], |row| {
                    row.get::<_, i64>(0)
                })?;
            }
        }
        Ok((results + listings).max(0) as u64)
    }
    /// Finds a machine that recently stored one of the required check results or published
    /// a listing for one of the families the tip cannot yet enumerate.
    pub fn working_machine<'a>(
        &self,
        conn: &Connection,
        machines: impl Iterator<Item = &'a rows::Machine>,
        now: Timestamp,
    ) -> Result<Option<&'a rows::Machine>> {
        let platform = self.platform.as_str();
        let cutoff = now.plus_seconds(-TIP_WAIT_SECONDS).to_string();
        let mut result = conn.prepare_cached(
            "SELECT EXISTS(SELECT 1 FROM results
             WHERE check_name = ?1 AND platform = ?2 AND machine = ?3 AND at >= ?4)",
        )?;
        let mut listing = conn.prepare_cached(
            "SELECT EXISTS(SELECT 1 FROM listings WHERE family = ?1 AND machine = ?2 AND at >= ?3)",
        )?;
        for machine in machines.filter(|machine| machine.os == platform) {
            for check in &self.checks {
                let progresses = match check.as_str().split_once(':') {
                    Some((family, "listing")) => {
                        listing.query_row(params![family, machine.id, cutoff], |row| row.get(0))?
                    }
                    _ => result.query_row(
                        params![check.as_str(), platform, machine.id, cutoff],
                        |row| row.get(0),
                    )?,
                };
                if progresses {
                    return Ok(Some(machine));
                }
            }
        }
        Ok(None)
    }

    /// A newer verified tip also settles the wait when it supersedes the reported tip.
    pub fn verified(&self, conn: &Connection) -> Result<bool> {
        Ok(conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM tips WHERE state = 'verified' AND
             (commit_sha = ?1 OR seen > (SELECT seen FROM tips WHERE commit_sha = ?1)))",
            [self.commit.as_str()],
            |row| row.get(0),
        )?)
    }

    /// Builds the ticket filing for one machine on the waited platform, either stalled or
    /// progressing too slowly past [`TIP_SLOW_WAIT_SECONDS`].
    pub fn machine_filing(
        &self,
        conn: &Connection,
        report: &TipWaiting,
        machine: &rows::Machine,
        waited: &str,
        listed: &str,
        now: Timestamp,
    ) -> Result<Filing> {
        let os = report.platform.as_str();
        let progress = self.progress_summary(conn, machine, now)?;
        let (title, status) = if self.slow {
            (
                format!(
                    "Tip verification is progressing too slowly on machine {}",
                    machine.name
                ),
                format!(
                    "Machine {} ({}) runs {os}, last reported at {}, and is still storing \
                     results, but the wait has passed {:.0} minutes. {progress}",
                    machine.name,
                    machine.id,
                    machine.last_seen,
                    TIP_SLOW_WAIT_SECONDS / 60.0
                ),
            )
        } else {
            (
                format!(
                    "Tip verification has waited an hour on machine {}",
                    machine.name
                ),
                format!(
                    "Machine {} ({}) runs {os} and last reported at {}. {progress}",
                    machine.name, machine.id, machine.last_seen
                ),
            )
        };
        Ok(Filing {
            signature: Signature::of_subject(&machine.id, "unreachable"),
            evidence: format!("{}@{}", report.commit, machine.last_seen),
            fields: TicketFields {
                title: Some(title),
                body: Some(format!("{waited}\n{status}\n\nchecks:\n- {listed}")),
                ticket_type: Some(TicketType::Bug),
                priority: Some(1),
                labels: Some(vec!["fleet".to_owned(), machine_label(&machine.name)]),
                needs_os: Some(Some(report.platform)),
                ..TicketFields::default()
            },
            deps: Vec::new(),
            released: false,
        })
    }
}
