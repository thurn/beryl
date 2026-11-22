//! The platform work named by a tip wait, shared by filing and clearing its ticket.

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use ignite_core::config::Os;
use ignite_core::ids::{CheckName, Commit};
use ignite_core::protocol::hub::TipWaiting;
use ignite_core::time::Timestamp;
use ignite_store::Result;
use ignite_store::hub::rows;

use super::stuck::TIP_WAIT_SECONDS;

/// The most recent platform wait reported on a ticket. Heartbeats and reports of silence
/// do not change this scope.
pub(super) const WAIT_SCOPE_KEY: &str = "ignite_platform_wait";

#[derive(Serialize, Deserialize)]
pub(super) struct PlatformWait {
    pub commit: Commit,
    pub platform: Os,
    checks: Vec<CheckName>,
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
        }
    }
}

impl PlatformWait {
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
}
