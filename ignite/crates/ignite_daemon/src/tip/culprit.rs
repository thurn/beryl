//! Finding the landing that made a check fail, and reporting the regression (see "Finding the
//! culprit" and "Regression tickets" in `docs/ignite.md`).
//!
//! - The candidates are the landings since the last verified commit that changed any of the
//!   check's inputs. A commit no landing recorded is a landing of its own.
//! - One candidate is the culprit, with nothing run. Several are narrowed by running the check
//!   at commits between them, by bisection, for at most [`MAX_RUNS`] runs.
//! - The report says how the search ended ([`CulpritSearch`]): one candidate, narrowed to one,
//!   ended early with a range and why, or not run and why.

use ignite_core::ids::{CheckName, Commit};
use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::{FoundBy, RegressionReport};
use ignite_core::tipround::CulpritSearch;
use ignite_core::traits::Result;

use super::revert::Handled;
use super::{Culprit, MAX_RUNS, TipCheck, TipLanding, TipRun, TipVerifier, tail, touches};
use crate::server::Context;

/// The first bad commit a previous check in the same round narrowed to. Checks that fail
/// together often share one culprit, so probing that boundary first avoids repeating a full
/// search on commits the tip tree just built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CulpritHint {
    pub first_bad: Commit,
}

/// Outcome of probing `master` before searching across an older platform gap.
enum MasterProbe {
    /// The check passed at `master`, so the culprit is in `master..tip` and may revert.
    Passed,
    /// The check already failed at `master`, so the culprit is in `gap_good..master`.
    Failed,
    /// The probe was incomplete or missing, so keep searching `gap_good..tip`.
    Unknown,
}

/// Narrows the landings between a pass at `good` and a known failure at the end of
/// `landings` by running one check at candidate commits.
pub(crate) fn bisect(
    landings: &[TipLanding],
    inputs: &[String],
    good: &Commit,
    probe: impl FnMut(&Commit) -> Option<Outcome>,
) -> Option<Culprit> {
    bisect_with_hint(landings, inputs, good, None, probe)
}

struct SearchBounds<'a> {
    landings: &'a [TipLanding],
    candidates: &'a [usize],
    low: usize,
    high: usize,
    runs: u32,
    stopped: Option<String>,
}

impl<'a> SearchBounds<'a> {
    fn new(landings: &'a [TipLanding], candidates: &'a [usize]) -> Self {
        Self {
            landings,
            candidates,
            low: 0,
            high: candidates.len(),
            runs: 0,
            stopped: None,
        }
    }

    fn after(&self, pos: usize) -> Option<Commit> {
        self.landings[self.candidates[pos - 1]].last().cloned()
    }

    fn probe_at(
        &mut self,
        pos: usize,
        probe: &mut impl FnMut(&Commit) -> Option<Outcome>,
    ) -> Option<Outcome> {
        let Some(at) = self.after(pos) else {
            self.stopped = Some("a landing in the range has no commits".to_owned());
            return None;
        };
        let outcome = probe(&at);
        self.runs += 1;
        match outcome {
            Some(Outcome::Pass | Outcome::Flaky) => self.low = pos,
            Some(Outcome::Fail) => self.high = pos,
            // A run that says nothing cannot narrow the range.
            Some(Outcome::Incomplete) => {
                self.stopped = Some(format!("the run at {at} was incomplete"));
            }
            None => {
                self.stopped = Some(format!("the run at {at} did not report the check"));
            }
        }
        outcome
    }

    fn try_hint(&mut self, hint: &CulpritHint, probe: &mut impl FnMut(&Commit) -> Option<Outcome>) {
        let Some(idx) = self
            .candidates
            .iter()
            .position(|&c| self.landings[c].last() == Some(&hint.first_bad))
        else {
            return;
        };
        let (good_pos, bad_pos) = (idx, idx + 1);
        if good_pos > self.low && good_pos < self.high && self.runs < MAX_RUNS {
            self.probe_at(good_pos, probe);
        }
        if self.stopped.is_none()
            && self.high - self.low > 1
            && self.low == good_pos
            && bad_pos > self.low
            && bad_pos < self.high
            && self.runs < MAX_RUNS
        {
            self.probe_at(bad_pos, probe);
        }
    }
}

/// Like [`bisect`], but checks `hint`'s boundary first when present and gallops backwards from
/// the newest landing on wide ranges (more than four candidates) so recent culprits are found
/// in one to three probes instead of jumping dozens of landings into the past.
pub(super) fn bisect_with_hint(
    landings: &[TipLanding],
    inputs: &[String],
    good: &Commit,
    hint: Option<&CulpritHint>,
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
    let mut bounds = SearchBounds::new(landings, &candidates);
    if bounds.high - bounds.low > 1
        && let Some(hint) = hint
    {
        bounds.try_hint(hint, &mut probe);
    }
    // On wide ranges (such as a platform gap), gallop backwards from the newest candidate by
    // powers of two until a passing commit brackets the culprit, then binary search inside it.
    let mut gallop = (bounds.low == 0 && bounds.high > 4).then_some(1usize);
    while bounds.stopped.is_none() && bounds.high - bounds.low > 1 {
        if bounds.runs >= MAX_RUNS {
            bounds.stopped = Some(format!("it reached its limit of {MAX_RUNS} runs"));
            break;
        }
        let pos = if let Some(step) = gallop {
            let candidate = bounds.high.saturating_sub(step);
            if candidate > bounds.low {
                candidate
            } else {
                gallop = None;
                usize::midpoint(bounds.low, bounds.high)
            }
        } else {
            usize::midpoint(bounds.low, bounds.high)
        };
        match bounds.probe_at(pos, &mut probe) {
            Some(Outcome::Pass | Outcome::Flaky) => gallop = None,
            Some(Outcome::Fail) => {
                if let Some(step) = &mut gallop {
                    *step = step.saturating_mul(2);
                }
            }
            _ => break,
        }
    }
    let (low, high, runs) = (bounds.low, bounds.high, bounds.runs);
    let search = match bounds.stopped.take() {
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
    let (Some(first_bad), Some(last_good)) = (bounds.after(high), last_good) else {
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
    /// Runs `checks` at `at` for a culprit search, a revert or a revert's build checks, on the
    /// tip reserve (`Reserve::TipRound`). These runs happen inside the tip loop, so the
    /// machine starts no newer round until they finish, and a revert they lead to is what moves
    /// `master` past a failure. Without the reserve they waited in the `tip` class behind every
    /// landing's and agent's request for cores and shared resources. On 2026-11-26 jarvis, the
    /// only Mac running `client-frame-counts`, searched for the culprit of a failure at tip
    /// `fe6c757`, and one probe waited 1,534 s for the `bench-vm` resource while five landing
    /// checks took turns with it. The newer tip's macOS checks waited for the search, and
    /// `master` stood still for over an hour.
    pub(super) fn probe(
        &self,
        ctx: &Context,
        at: &Commit,
        checks: &[CheckName],
        rerun: bool,
    ) -> Vec<TipRun> {
        self.work.run(ctx, at, checks, rerun, true)
    }

    /// The landing between `good` and `bad` that made `check` fail, or the range of landings a
    /// search that ran out of runs ended with. `None` when no commit lies between them.
    pub fn find_culprit(
        &self,
        ctx: &Context,
        check: &TipCheck,
        good: &Commit,
        bad: &Commit,
    ) -> Result<Option<Culprit>> {
        self.find_culprit_with_hint(ctx, check, good, bad, None)
    }

    fn find_culprit_with_hint(
        &self,
        ctx: &Context,
        check: &TipCheck,
        good: &Commit,
        bad: &Commit,
        hint: Option<&CulpritHint>,
    ) -> Result<Option<Culprit>> {
        let landings = self.work.landings(ctx, good, bad)?;
        Ok(bisect_with_hint(
            &landings,
            &check.inputs,
            good,
            hint,
            |at| {
                self.probe(ctx, at, std::slice::from_ref(&check.check), false)
                    .into_iter()
                    .find(|run| run.check == check.check)
                    .map(|run| run.outcome)
            },
        ))
    }

    fn probe_master(&self, ctx: &Context, master: &Commit, check: &TipCheck) -> MasterProbe {
        match self
            .probe(ctx, master, std::slice::from_ref(&check.check), false)
            .into_iter()
            .find(|run| run.check == check.check)
            .map(|run| run.outcome)
        {
            Some(Outcome::Pass | Outcome::Flaky) => MasterProbe::Passed,
            Some(Outcome::Fail) => MasterProbe::Failed,
            Some(Outcome::Incomplete) | None => MasterProbe::Unknown,
        }
    }

    /// Finds the culprit of a check that failed at `tip` and reports the regression, or reverts
    /// a landing when the check qualifies for automatic revert (see `revert`). A failure
    /// already reported can qualify for a revert once the stall threshold is reached. A newer
    /// tip that changed the check's inputs gets one chance to run the check first.
    ///
    /// When `prior` is before `master` (a platform gap) and the check had no cached pass at
    /// `master`, probes `master` first: a pass narrows the search to `master..tip` and allows
    /// automatic reverts, while a failure shrinks the search's upper bound to `master`.
    ///
    /// A check only another platform can run is left to that platform's machines: a search here
    /// would run this platform's version of the check, which passes at every commit, and blame
    /// the newest candidate whatever the real culprit.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn blame(
        &mut self,
        ctx: &Context,
        master: &Commit,
        prior: Option<&Commit>,
        tip: &Commit,
        check: &TipCheck,
        output: Option<&str>,
        allow_revert: bool,
        hint: Option<&CulpritHint>,
    ) -> Result<(Option<Commit>, Option<CulpritHint>)> {
        super::round::require_running()?;
        let reported = self.reported.contains(&check.key);
        if !check.runnable || (reported && (check.is_build() || !allow_revert)) {
            return Ok((None, None));
        }
        // Where git cannot say, the failure is reported, which at worst repeats a report.
        let newest = ctx
            .git
            .tip(&ctx.config.repository.staging)
            .unwrap_or_else(|_| tip.clone());
        // A newer tip that changed what the check reads may already hold a fix, so the check runs
        // there before reporting (using a cached pass if one exists). Deferring blindly without
        // checking the newer tip left a failing check unreported for hours whenever an unrelated
        // landing moved `staging` and later tips stopped on other failing checks. Only a pass at
        // the newer tip suppresses the report; if the check still fails there (or gives no pass),
        // it is blamed and reported at once.
        let repaired = newest != *tip
            && ctx
                .git
                .changed(tip, &newest)
                .is_ok_and(|changed| touches(&changed, &check.inputs))
            && self.passes_at(ctx, &newest, check);
        super::round::require_running()?;
        if repaired {
            super::candidate::advance_to_best_effort(ctx, &newest);
            eprintln!(
                "ignite: {} fails at {tip}, but passes at {newest}; not reporting",
                check.check
            );
            return Ok((None, None));
        }
        let (good, search_bad, allow_revert) = if let Some(gap_good) =
            prior.filter(|good| *good != master)
            && master != tip
        {
            match self.probe_master(ctx, master, check) {
                MasterProbe::Passed => (master, tip, allow_revert),
                MasterProbe::Failed => (gap_good, master, false),
                MasterProbe::Unknown => (gap_good, tip, false),
            }
        } else {
            let at_or_no_gap = prior.is_none_or(|good| good == master);
            (prior.unwrap_or(master), tip, at_or_no_gap && allow_revert)
        };
        super::round::require_running()?;
        // A failing check at a tip is always reported. With no landing to blame (the hub
        // recorded none between the commits), the report names the whole range since the last
        // verified commit and no ticket.
        let culprit = self.find_culprit_with_hint(ctx, check, good, search_bad, hint);
        super::round::require_running()?;
        let found_hint = culprit.as_ref().ok().and_then(|found| {
            let found = found.as_ref()?;
            (found.suspects.len() == 1).then(|| CulpritHint {
                first_bad: found.first_bad.clone(),
            })
        });
        // The last suspect in the narrowed range is known to fail by its end. Promotion
        // must stop before that landing, including when the search ended with one suspect.
        let cutoff = culprit
            .as_ref()
            .ok()
            .and_then(|culprit| culprit.as_ref()?.suspects.last()?.commits.first().cloned());
        // An eligible failure can revert a single confirmed landing.
        let mut note = None;
        if allow_revert && let Ok(Some(found)) = &culprit {
            match self.try_revert(ctx, tip, check, found, output) {
                Handled::Reverted => {
                    self.reported.insert(check.key.clone());
                    return Ok((cutoff, found_hint));
                }
                // The revert gave up because the tip moved with a change to what the check
                // reads; as above, only once per check.
                Handled::NextRound if self.deferred.insert(check.check.clone()) => {
                    return Ok((cutoff, found_hint));
                }
                // The revert would not build. The failure may have been reported before, but
                // without saying why no revert came, so it is reported again with the note.
                Handled::Unbuildable(text) => note = Some(text),
                Handled::NextRound | Handled::Report => {}
            }
        }
        super::round::require_running()?;
        if reported && note.is_none() {
            return Ok((cutoff, found_hint));
        }
        let output = output
            .map(|o| tail(o, &self.own_outputs))
            .unwrap_or_default();
        let report = RegressionReport {
            check: check.check.clone(),
            first_bad: search_bad.clone(),
            last_good: good.clone(),
            caused_by: None,
            output: match note {
                Some(note) => format!("{note}\n\n{output}"),
                None => output,
            },
            suspects: Vec::new(),
            platform: check.key.platform,
            search: None,
            found_by: FoundBy::TipVerification,
            detected_at: None,
        };
        ctx.hub.regression(&with_culprit(report, culprit))?;
        self.reported.insert(check.key.clone());
        Ok((cutoff, found_hint))
    }
}
