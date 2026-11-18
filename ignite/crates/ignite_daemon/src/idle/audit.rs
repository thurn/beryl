//! Cache audits: a random 0.5% of each day's cache hits is run again in a landing tree at the
//! commit the hit answered a check at. A rerun that disagrees with the cached result is reported
//! to the hub, which switches caching off for the check and files a P1 naming both runs.
//!
//! The sample is drawn as hits happen: a hit is sampled when a hash of its key and the day falls
//! under the rate, so the same hit on the same day is always sampled or always not, and a
//! restart draws the same sample.

use std::fmt::Write as _;
use std::path::Path;

use ignite_core::ids::{Commit, RecordId};
use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::{AuditReport, CachedResult, ResultKey};
use ignite_core::traits::Result;

use super::Idle;
use super::explore::tail;
use super::state::{Hit, Report};
use crate::checks::HitSink;
use crate::server::Context;

/// The share of each day's cache hits that is audited.
const SAMPLE_RATE: f64 = 0.005;

/// Days a sampled hit is kept before it is dropped, audited or not.
const KEEP_DAYS: f64 = 7.0;

/// What one audit did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AuditSummary {
    /// Hits run again.
    pub rerun: u32,
    /// The reruns that disagreed with the cache, as reported to the hub.
    pub disagreed: Vec<AuditReport>,
    /// True if the audit stopped because no spare landing tree was free: idle work always
    /// leaves one landing tree free for landings and tip verification.
    pub busy: bool,
}

impl AuditSummary {
    pub fn render(&self) -> String {
        let mut text = match (self.rerun, self.disagreed.len()) {
            (0, _) => "no cache hit is waiting to be audited\n".to_owned(),
            (n, 0) => format!("audited {n} cache hits; all agreed\n"),
            (n, d) => format!("audited {n} cache hits; {d} disagreed and were reported\n"),
        };
        for report in &self.disagreed {
            let _ = writeln!(
                text,
                "  {}  cached {}, rerun {}, at {}",
                report.key.check,
                outcome_word(report.cached),
                outcome_word(report.rerun),
                report.commit
            );
        }
        if self.busy {
            text.push_str("stopped: no spare landing tree was free\n");
        }
        text
    }
}

fn outcome_word(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::Pass => "pass",
        Outcome::Fail => "fail",
        Outcome::Flaky => "flaky",
        Outcome::Incomplete => "incomplete",
    }
}

impl HitSink for Idle {
    fn hit(&self, cached: &CachedResult, commit: &Commit) {
        let now = self.clock.now();
        let day = now.day();
        let cutoff = now.plus_seconds(-KEEP_DAYS * 86_400.0).day();
        let value = sample_value(&cached.key, &day);
        let hit = Hit {
            cached: cached.clone(),
            commit: commit.clone(),
            audited: false,
        };
        // A hit that cannot be saved is only a smaller sample.
        let _ = self.update_if(|state| {
            let today = state.hits.entry(day.clone()).or_default();
            let known = today.sampled.iter().any(|h| h.cached.key == cached.key)
                || today
                    .lowest
                    .as_ref()
                    .is_some_and(|(_, h)| h.cached.key == cached.key);
            if known {
                return None;
            }
            if value < threshold() {
                today.sampled.push(hit);
            } else if today
                .lowest
                .as_ref()
                .is_none_or(|(lowest, _)| value < *lowest)
            {
                today.lowest = Some((value, hit));
            } else {
                return None;
            }
            state.hits.retain(|day, _| *day >= cutoff);
            Some(())
        });
    }
}

impl Idle {
    /// True when a sampled hit has not been audited.
    pub(super) fn audits_due(&self) -> bool {
        self.state()
            .hits
            .values()
            .any(|day| day.sampled.iter().any(|h| !h.audited))
    }

    /// Reruns the next sampled hit, or with `now` every sampled hit, adding today's lowest when
    /// nothing is sampled yet so that `ignite audit --now` always has a hit to rerun.
    pub(super) fn audit(&self, ctx: &Context, now: bool) -> Result<AuditSummary> {
        if now && !self.audits_due() {
            let today = self.clock.now().day();
            self.update(|state| {
                if let Some(day) = state.hits.get_mut(&today)
                    && let Some((_, hit)) = day.lowest.take()
                {
                    day.sampled.push(hit);
                }
            })?;
        }
        let mut summary = AuditSummary::default();
        while let Some((day, hit)) = self.next_hit() {
            let Some((tree, _)) = self.host.lease(&hit.commit)? else {
                summary.busy = true;
                break;
            };
            let (rerun, said, rerun_record) = self.rerun.rerun(ctx, &tree, &hit.cached.key.check);
            let released = self.host.release(&tree, Some(&hit.commit));
            let report = disagrees(hit.cached.outcome, rerun).then(|| AuditReport {
                key: hit.cached.key.clone(),
                cached: hit.cached.outcome,
                cached_record: hit.cached.record,
                cached_machine: hit.cached.machine.clone(),
                rerun,
                // 0 when the hub has not numbered the rerun's record yet: it was queued.
                rerun_record: rerun_record.unwrap_or(RecordId(0)),
                rerun_machine: ctx.machine.clone(),
                commit: hit.commit.clone(),
                output: said.map(|said| output_text(&said)).unwrap_or_default(),
            });
            self.update(|state| {
                if let Some(day) = state.hits.get_mut(&day)
                    && let Some(saved) = day
                        .sampled
                        .iter_mut()
                        .find(|h| h.cached.key == hit.cached.key)
                {
                    saved.audited = true;
                }
                state.unsent.extend(report.clone().map(Report::Audit));
            })?;
            summary.rerun += 1;
            summary.disagreed.extend(report);
            released?;
            if !now {
                break;
            }
        }
        self.send_reports(ctx);
        Ok(summary)
    }

    /// The oldest sampled hit not yet audited, with its day.
    fn next_hit(&self) -> Option<(String, Hit)> {
        self.state().hits.iter().find_map(|(day, hits)| {
            hits.sampled
                .iter()
                .find(|h| !h.audited)
                .map(|h| (day.clone(), h.clone()))
        })
    }
}

/// True when a rerun's verdict contradicts the cached one. A rerun that ended without a verdict
/// says nothing about the cache.
fn disagrees(cached: Outcome, rerun: Outcome) -> bool {
    let verdict = |o: Outcome| matches!(o, Outcome::Pass | Outcome::Fail);
    verdict(cached) && verdict(rerun) && cached != rerun
}

/// A rerun's saved output, or what it said when it saved none.
fn output_text(said: &str) -> String {
    let path = Path::new(said);
    if path.is_file() {
        tail(path)
    } else {
        said.to_owned()
    }
}

/// Where a key falls in the day's sample: FNV-1a over the key and the day, which stays the same
/// across builds and restarts.
fn sample_value(key: &ResultKey, day: &str) -> u64 {
    let platform = key.platform.map_or("", |os| os.as_str());
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for part in [key.check.as_str(), key.hash.as_str(), platform, day] {
        for byte in part.bytes().chain([0]) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    }
    hash
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn threshold() -> u64 {
    (SAMPLE_RATE * u64::MAX as f64) as u64
}
