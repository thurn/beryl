//! Backoff for idle jobs that fail. A job that fails waits before it runs again, half an hour
//! after its first failure and twice as long after each later one, up to a day. The wait is
//! saved in the state file, so a restart does not start the job again at once. After
//! [`REPORT_AFTER`] failures in a row the daemon reports the job to the hub once, as a justiciar
//! report, the path it uses for its other loops that keep failing. A success clears the job's
//! failures and lets a later run of failures be reported again.
//!
//! A job that fails because the hub cannot be reached is not counted: the job is not at fault,
//! and the report could not reach the hub either.

use serde::{Deserialize, Serialize};

use ignite_core::protocol::hub::JusticiarReport;
use ignite_core::time::Timestamp;
use ignite_core::traits::{Error, Result};

use super::{Idle, IdleJob};
use crate::server::Context;

/// How long a job waits after its first failure: half an hour.
const RETRY_AFTER_S: f64 = 1_800.0;

/// The longest a job waits after failures: a day.
const RETRY_MAX_S: f64 = 86_400.0;

/// The failures in a row after which the job is reported to the hub.
pub(super) const REPORT_AFTER: u32 = 3;

/// An idle job's failures in a row.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct Failing {
    pub failures: u32,
    /// When the first of these failures happened, which names the report to the hub.
    pub since: Timestamp,
    /// When the job may run again.
    pub retry_at: Timestamp,
    /// The last failure's error.
    pub error: String,
    /// Whether the hub has taken the report of these failures.
    pub reported: bool,
}

impl IdleJob {
    /// The job's name in the state file and in reports, such as `rebuild kernel-body`.
    pub(super) fn key(&self) -> String {
        match self {
            IdleJob::Explore { family } => format!("explore {family}"),
            IdleJob::Audit => "audit".to_owned(),
            IdleJob::Rebuild { name } => format!("rebuild {name}"),
        }
    }
}

/// The wait after the `failures`th failure in a row.
fn retry_wait(failures: u32) -> f64 {
    let doublings = failures.saturating_sub(1).min(10);
    (RETRY_AFTER_S * f64::from(1_u32 << doublings)).min(RETRY_MAX_S)
}

impl Idle {
    /// Whether `job` may run now: it has not failed, or its wait after failing is over.
    pub(super) fn ready(&self, job: &IdleJob) -> bool {
        let now = self.clock.now();
        self.state()
            .failing
            .get(&job.key())
            .is_none_or(|failing| now >= failing.retry_at)
    }

    /// Whether `job` failed the last time it ran.
    pub(super) fn has_failed(&self, job: &IdleJob) -> bool {
        self.state().failing.contains_key(&job.key())
    }

    /// Forgets `job`'s failures after it ran and succeeded.
    pub(super) fn note_success(&self, job: &IdleJob) -> Result<()> {
        if !self.has_failed(job) {
            return Ok(());
        }
        self.update(|state| {
            state.failing.remove(&job.key());
        })
    }

    /// Counts a failure of `job`, sets its wait, and reports it to the hub once it has failed
    /// [`REPORT_AFTER`] times in a row.
    pub(super) fn note_failure(&self, ctx: &Context, job: &IdleJob, error: &Error) -> Result<()> {
        if matches!(error, Error::Unreachable(_)) {
            return Ok(());
        }
        let now = self.clock.now();
        let key = job.key();
        let failing = self.update(|state| {
            let failing = state.failing.entry(key.clone()).or_insert(Failing {
                failures: 0,
                since: now,
                retry_at: now,
                error: String::new(),
                reported: false,
            });
            failing.failures += 1;
            failing.retry_at = now.plus_seconds(retry_wait(failing.failures));
            failing.error = error.to_string();
            failing.clone()
        })?;
        if failing.failures < REPORT_AFTER || failing.reported {
            return Ok(());
        }
        match ctx.hub.justiciar(&self.failure_report(ctx, &key, &failing)) {
            Ok(_) => self.update(|state| {
                if let Some(failing) = state.failing.get_mut(&key) {
                    failing.reported = true;
                }
            }),
            Err(error) => {
                // The next failure tries again.
                eprintln!("ignite: failing idle work {key} not reported: {error}");
                Ok(())
            }
        }
    }

    fn failure_report(&self, ctx: &Context, key: &str, failing: &Failing) -> JusticiarReport {
        let host = &self.settings.hostname;
        JusticiarReport {
            subject: format!("idle:{}", key.replace(' ', ":")),
            title: format!("Ignite's idle work `{key}` keeps failing on {host}"),
            details: format!(
                "The idle job `{key}` has failed {} times in a row on {host} since {}. The last \
                 failure:\n\n{}\n\nIgnite {}. The daemon runs the job again after a wait that \
                 doubles with each failure, up to a day, and reports nothing more until it \
                 succeeds. The daemon log has each failure.",
                failing.failures, failing.since, failing.error, self.settings.ignite
            ),
            evidence: format!("{key}@{}", failing.since),
            machine: ctx.machine.clone(),
            host: host.clone(),
            // The error names files, such as a build's output, on this machine.
            on_machine: true,
        }
    }
}
