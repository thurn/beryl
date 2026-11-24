//! Finding the landing that made a check fail, and reporting the regression (see "Finding the
//! culprit" and "Regression tickets" in `docs/ignite.md`).
//!
//! - The candidates are the landings since the last verified commit that changed any of the
//!   check's inputs. A commit no landing recorded is a landing of its own.
//! - One candidate is the culprit, with nothing run. Several are narrowed by running the check
//!   at commits between them, by bisection, for at most [`MAX_RUNS`] runs.
//! - The report says how the search ended ([`CulpritSearch`]): one candidate, narrowed to one,
//!   ended early with a range and why, or not run and why.

use ignite_core::ids::Commit;
use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::{FoundBy, RegressionReport};
use ignite_core::tipround::CulpritSearch;
use ignite_core::traits::Result;

use super::revert::Handled;
use super::{Culprit, MAX_RUNS, TipCheck, TipLanding, TipVerifier, tail, touches};
use crate::server::Context;

/// Narrows the landings between a pass at `good` and a known failure at the end of
/// `landings` by running one check at candidate commits.
pub(crate) fn bisect(
    landings: &[TipLanding],
    inputs: &[String],
    good: &Commit,
    mut probe: impl FnMut(&Commit) -> Option<Outcome>,
) -> Option<Culprit> {
    if landings.is_empty() {
        return None;
    }
    let touching: Vec<usize> = (0..landings.len())
        .filter(|&i| touches(&landings[i].paths, inputs))
        .collect();
    // A failure that no landing explains (a changed toolchain, say) blames every landing.
    let candidates: Vec<usize> = if touching.is_empty() {
        (0..landings.len()).collect()
    } else {
        touching
    };
    // Only candidates change what the check reads, so its result can change only at a
    // candidate's last commit. Position `i` is "after the i-th candidate": 0 passes, as
    // `good` did, and `candidates.len()` fails, as `bad` did.
    let after = |i: usize| -> Option<Commit> { landings[candidates[i - 1]].last().cloned() };
    let (mut low, mut high, mut runs) = (0, candidates.len(), 0);
    // What ended the search before it reached one landing, for the ticket to say.
    let mut stopped = None;
    while high - low > 1 {
        if runs >= MAX_RUNS {
            stopped = Some(format!("it reached its limit of {MAX_RUNS} runs"));
            break;
        }
        let middle = usize::midpoint(low, high);
        let Some(at) = after(middle) else {
            stopped = Some("a landing in the range has no commits".to_owned());
            break;
        };
        let outcome = probe(&at);
        runs += 1;
        match outcome {
            Some(Outcome::Pass | Outcome::Flaky) => low = middle,
            Some(Outcome::Fail) => high = middle,
            // A run that says nothing cannot narrow the range.
            Some(Outcome::Incomplete) => {
                stopped = Some(format!("the run at {at} was incomplete"));
                break;
            }
            None => {
                stopped = Some(format!("the run at {at} did not report the check"));
                break;
            }
        }
    }
    let search = match stopped {
        Some(why) => CulpritSearch::EndedEarly {
            runs,
            candidates: u32::try_from(high - low).unwrap_or(u32::MAX),
            why,
        },
        None if candidates.len() == 1 => CulpritSearch::OneCandidate,
        None => CulpritSearch::Bisected { runs },
    };
    let first_suspect = candidates[low];
    let last_good = match first_suspect.checked_sub(1) {
        Some(previous) => landings[previous].last().cloned(),
        None => Some(good.clone()),
    };
    let (Some(first_bad), Some(last_good)) = (after(high), last_good) else {
        return None;
    };
    let suspects = candidates[low..high]
        .iter()
        .map(|&i| landings[i].clone())
        .collect();
    Some(Culprit {
        suspects,
        first_bad,
        last_good,
        runs,
        search,
    })
}

/// Records the narrowed range and its ticket when the search found a single landing.
pub(crate) fn with_culprit(
    mut report: RegressionReport,
    culprit: Result<Option<Culprit>>,
) -> RegressionReport {
    let culprit = match culprit {
        Ok(Some(culprit)) => culprit,
        Ok(None) => {
            report.search = Some(CulpritSearch::NotRun {
                why: "no landing with commits lies between the commits".to_owned(),
            });
            return report;
        }
        Err(error) => {
            eprintln!("ignite: no culprit found for {}: {error}", report.check);
            report.search = Some(CulpritSearch::NotRun {
                why: format!("the landings could not be read: {error}"),
            });
            return report;
        }
    };
    report.first_bad = culprit.first_bad;
    report.last_good = culprit.last_good;
    report.search = Some(culprit.search);
    if let [landing] = culprit.suspects.as_slice() {
        report.caused_by.clone_from(&landing.ticket);
    } else {
        report.suspects = culprit
            .suspects
            .into_iter()
            .filter_map(|l| l.ticket)
            .collect();
    }
    report
}

impl TipVerifier {
    /// The landing between `good` and `bad` that made `check` fail, or the range of landings a
    /// search that ran out of runs ended with. `None` when no commit lies between them.
    pub fn find_culprit(
        &self,
        ctx: &Context,
        check: &TipCheck,
        good: &Commit,
        bad: &Commit,
    ) -> Result<Option<Culprit>> {
        let landings = self.work.landings(ctx, good, bad)?;
        Ok(bisect(&landings, &check.inputs, good, |at| {
            self.work
                .run(ctx, at, std::slice::from_ref(&check.check), false)
                .into_iter()
                .find(|run| run.check == check.check)
                .map(|run| run.outcome)
        }))
    }

    /// Finds the culprit of a check that failed at `tip` and reports the regression, or reverts
    /// the culprit of a broken build (see `revert`), unless this failure was reported already,
    /// or a newer tip changed the check's inputs and the check has not been left to a later
    /// round before (its next round runs the check there first).
    ///
    /// A check only another platform can run is left to that platform's machines: a search here
    /// would run this platform's version of the check, which passes at every commit, and blame
    /// the newest candidate whatever the real culprit.
    pub(super) fn blame(
        &mut self,
        ctx: &Context,
        last: &Commit,
        tip: &Commit,
        check: &TipCheck,
        output: Option<&str>,
        allow_revert: bool,
    ) -> Result<Option<Commit>> {
        super::round::require_running()?;
        if !check.runnable || self.reported.contains(&check.key) {
            return Ok(None);
        }
        // Where git cannot say, the failure is reported, which at worst repeats a report.
        let newest = ctx
            .git
            .tip(&ctx.config.repository.staging)
            .unwrap_or_else(|_| tip.clone());
        // A newer tip that changed what the check reads may already hold a fix, so the failure
        // is left to the next round, once. Rounds take longer than the gaps between landings
        // on a busy day, so the next round's tip is also overtaken by the time it fails, and
        // leaving every such failure unreported kept `master` from moving for hours with no
        // ticket to fix it. A check that fails again is reported whatever the newer tip changed.
        let overtaken = newest != *tip
            && ctx
                .git
                .changed(tip, &newest)
                .is_ok_and(|changed| touches(&changed, &check.inputs));
        if overtaken && self.deferred.insert(check.check.clone()) {
            eprintln!(
                "ignite: {} fails at {tip}, but {newest} changes what it reads; the next round \
                 reports it if it still fails",
                check.check
            );
            return Ok(None);
        }
        // A failing check at a tip is always reported. With no landing to blame (the hub
        // recorded none between the commits), the report names the whole range since the last
        // verified commit and no ticket.
        let culprit = self.find_culprit(ctx, check, last, tip);
        super::round::require_running()?;
        // The last suspect in the narrowed range is known to fail by its end. Promotion
        // must stop before that landing, including when the search ended with one suspect.
        let cutoff = culprit
            .as_ref()
            .ok()
            .and_then(|culprit| culprit.as_ref()?.suspects.last()?.commits.first().cloned());
        // A build check broken by one landing is reverted instead, when `revert` allows it.
        if allow_revert && let Ok(Some(found)) = &culprit {
            match self.try_revert(ctx, tip, check, found, output) {
                Handled::Reverted => {
                    self.reported.insert(check.key.clone());
                    return Ok(cutoff);
                }
                // The revert gave up because the tip moved with a change to what the check
                // reads; as above, only once per check.
                Handled::NextRound if self.deferred.insert(check.check.clone()) => {
                    return Ok(cutoff);
                }
                Handled::NextRound | Handled::Report => {}
            }
        }
        super::round::require_running()?;
        let report = RegressionReport {
            check: check.check.clone(),
            first_bad: tip.clone(),
            last_good: last.clone(),
            caused_by: None,
            output: output
                .map(|o| tail(o, &self.own_outputs))
                .unwrap_or_default(),
            suspects: Vec::new(),
            platform: check.key.platform,
            search: None,
            found_by: FoundBy::TipVerification,
            detected_at: None,
        };
        ctx.hub.regression(&with_culprit(report, culprit))?;
        self.reported.insert(check.key.clone());
        Ok(cutoff)
    }
}
