//! Finding the landing that made a check fail, and reporting the regression (see "Finding the
//! culprit" and "Regression tickets" in `docs/ignite.md`).
//!
//! - The candidates are the landings since the last verified commit that changed any of the
//!   check's inputs. A commit no landing recorded is a landing of its own.
//! - One candidate is the culprit, with nothing run. Several are narrowed by running the check
//!   at commits between them, by bisection, for at most [`MAX_RUNS`] runs.
//! - The report says how the search ended ([`CulpritSearch`]): one candidate, narrowed to one,
//!   ended early with a range and why, or not run and why.

use ignite_core::ids::{Commit, TicketId};
use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::{FoundBy, RegressionReport};
use ignite_core::tipround::CulpritSearch;
use ignite_core::traits::Result;

use super::{Culprit, MAX_RUNS, TipCheck, TipVerifier, tail, touches};
use crate::server::Context;

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
        if landings.is_empty() {
            return Ok(None);
        }
        let touching: Vec<usize> = (0..landings.len())
            .filter(|&i| touches(&landings[i].paths, &check.inputs))
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
            let outcome = self
                .work
                .run(ctx, &at, std::slice::from_ref(&check.check), false)
                .into_iter()
                .find(|r| r.check == check.check)
                .map(|r| r.outcome);
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
            return Ok(None);
        };
        let suspects = candidates[low..high]
            .iter()
            .map(|&i| landings[i].clone())
            .collect();
        Ok(Some(Culprit {
            suspects,
            first_bad,
            last_good,
            runs,
            search,
        }))
    }

    /// Finds the culprit of a check that failed at `tip` and reports the regression, or reverts
    /// the culprit of a broken build (see `revert`), unless a newer tip changed the check's
    /// inputs (its next round runs the check there first) or this failure was reported already.
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
    ) -> Result<()> {
        if !check.runnable || self.reported.contains(&check.key) {
            return Ok(());
        }
        // Where git cannot say, the failure is reported, which at worst repeats a report.
        let newest = ctx
            .git
            .tip(&ctx.config.repository.staging)
            .unwrap_or_else(|_| tip.clone());
        if newest != *tip
            && ctx
                .git
                .changed(tip, &newest)
                .is_ok_and(|changed| touches(&changed, &check.inputs))
        {
            return Ok(());
        }
        // A failing check at a tip is always reported. With no landing to blame (the hub
        // recorded none between the commits), the report names the whole range since the last
        // verified commit and no ticket.
        let culprit = self.find_culprit(ctx, check, last, tip).map_err(|error| {
            eprintln!("ignite: no culprit found for {}: {error}", check.check);
            format!("the landings could not be read: {error}")
        });
        // A build check broken by one landing is reverted instead, when `revert` allows it.
        if let Ok(Some(found)) = &culprit
            && self.try_revert(ctx, tip, check, found, output)
        {
            self.reported.insert(check.key.clone());
            return Ok(());
        }
        let search = match &culprit {
            Ok(Some(culprit)) => culprit.search.clone(),
            Ok(None) => CulpritSearch::NotRun {
                why: "no landing with commits lies between the commits".to_owned(),
            },
            Err(why) => CulpritSearch::NotRun { why: why.clone() },
        };
        let (first_bad, last_good, caused_by, suspects) = match culprit.ok().flatten() {
            Some(culprit) => {
                let tickets: Vec<TicketId> = culprit
                    .suspects
                    .iter()
                    .filter_map(|l| l.ticket.clone())
                    .collect();
                if culprit.suspects.len() == 1 {
                    (
                        culprit.first_bad,
                        culprit.last_good,
                        tickets.into_iter().next(),
                        Vec::new(),
                    )
                } else {
                    (culprit.first_bad, culprit.last_good, None, tickets)
                }
            }
            None => (tip.clone(), last.clone(), None, Vec::new()),
        };
        ctx.hub.regression(&RegressionReport {
            check: check.check.clone(),
            first_bad,
            last_good,
            caused_by,
            output: output
                .map(|o| tail(o, &self.own_outputs))
                .unwrap_or_default(),
            suspects,
            platform: check.key.platform,
            search: Some(search),
            found_by: FoundBy::TipVerification,
        })?;
        self.reported.insert(check.key.clone());
        Ok(())
    }
}
