//! Finding the landing that made a check fail, and reporting the regression (see "Finding the
//! culprit" and "Regression tickets" in `docs/ignite.md`).
//!
//! - The candidates are the landings since the last verified commit that changed any of the
//!   check's inputs. A commit no landing recorded is a landing of its own.
//! - One candidate is the culprit, with nothing run. Several are narrowed by running the check
//!   at commits between them, by bisection, for at most [`MAX_RUNS`] runs.
//! - The report says how the search ended ([`CulpritSearch`]): one candidate, narrowed to one,
//!   ended early with a range and why, or not run and why.

use std::collections::{HashMap, HashSet};

use ignite_core::ids::{CheckName, Commit};
use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::{FoundBy, IncompleteReason, RegressionReport};
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

/// Outcome of a culprit probe at an earlier commit. When a check's family listed cleanly there
/// without the check, the check did not exist yet (it was added by a later landing) and was not
/// failing, so treat it as passing rather than stopping bisection early.
fn probe_outcome(run: &TipRun) -> Outcome {
    if run.outcome == Outcome::Incomplete && run.reason == Some(IncompleteReason::NotListed) {
        Outcome::Pass
    } else {
        run.outcome
    }
}

/// Outcome of probing `master` before searching across an older platform gap.
#[derive(Clone, Copy)]
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

/// One failing check handed to [`TipVerifier::blame_failed`].
pub(super) struct BlameEntry<'a> {
    pub check: &'a TipCheck,
    pub prior: Option<&'a Commit>,
    pub output: Option<&'a str>,
    pub allow_revert: bool,
}

/// Outcome of blaming a round's failing checks.
pub(super) struct BlameSummary {
    pub culprits: HashSet<Commit>,
    pub complete: bool,
}

/// A check's resolved search bounds after checking `newest` and `master`.
struct BlamePlan<'a> {
    entry: &'a BlameEntry<'a>,
    good: &'a Commit,
    search_bad: &'a Commit,
    allow_revert: bool,
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
        let landings = self.work.landings(ctx, good, bad)?;
        Ok(bisect_with_hint(
            &landings,
            &check.inputs,
            good,
            None,
            |at| {
                self.probe(ctx, at, std::slice::from_ref(&check.check), false)
                    .into_iter()
                    .find(|run| run.check == check.check)
                    .map(|run| probe_outcome(&run))
            },
        ))
    }

    /// Probes `newest` once for all checks whose inputs changed in `tip..newest`, returning the
    /// subset that passed there. Batching avoids checking out and rebuilding the tree once per
    /// failing check when a regression breaks many checks in the same family.
    fn repaired_at_newest(
        &self,
        ctx: &Context,
        tip: &Commit,
        entries: &[&BlameEntry<'_>],
    ) -> Result<HashSet<CheckName>> {
        let newest = ctx
            .git
            .tip(&ctx.config.repository.staging)
            .unwrap_or_else(|_| tip.clone());
        if newest == *tip {
            return Ok(HashSet::new());
        }
        let Ok(changed) = ctx.git.changed(tip, &newest) else {
            return Ok(HashSet::new());
        };
        let names: Vec<CheckName> = entries
            .iter()
            .filter(|entry| touches(&changed, &entry.check.inputs))
            .map(|entry| entry.check.check.clone())
            .collect();
        if names.is_empty() {
            return Ok(HashSet::new());
        }
        let runs = self.probe(ctx, &newest, &names, false);
        super::round::require_running()?;
        let repaired: HashSet<CheckName> = runs
            .into_iter()
            .filter(|run| matches!(run.outcome, Outcome::Pass | Outcome::Flaky))
            .map(|run| run.check)
            .collect();
        if !repaired.is_empty() {
            super::candidate::advance_to_best_effort(ctx, &newest);
            for entry in entries {
                if repaired.contains(&entry.check.check) {
                    eprintln!(
                        "ignite: {} fails at {tip}, but passes at {newest}; not reporting",
                        entry.check.check
                    );
                }
            }
        }
        Ok(repaired)
    }

    /// Probes `master` once for all checks whose platform-gap `prior` predates `master`.
    fn probe_master_batch(
        &self,
        ctx: &Context,
        master: &Commit,
        tip: &Commit,
        entries: &[&BlameEntry<'_>],
    ) -> Result<HashMap<CheckName, MasterProbe>> {
        if master == tip {
            return Ok(HashMap::new());
        }
        let names: Vec<CheckName> = entries
            .iter()
            .filter(|entry| entry.prior.is_some_and(|good| good != master))
            .map(|entry| entry.check.check.clone())
            .collect();
        if names.is_empty() {
            return Ok(HashMap::new());
        }
        let runs = self.probe(ctx, master, &names, false);
        super::round::require_running()?;
        let mut outcomes = HashMap::with_capacity(names.len());
        for run in runs {
            let probe = match probe_outcome(&run) {
                Outcome::Pass | Outcome::Flaky => MasterProbe::Passed,
                Outcome::Fail => MasterProbe::Failed,
                Outcome::Incomplete => MasterProbe::Unknown,
            };
            outcomes.insert(run.check, probe);
        }
        Ok(outcomes)
    }

    fn plan_blame<'a>(
        &self,
        ctx: &Context,
        master: &'a Commit,
        tip: &'a Commit,
        open: &HashSet<CheckName>,
        entries: &'a [BlameEntry<'a>],
    ) -> Result<Vec<BlamePlan<'a>>> {
        let already_reported =
            |check: &TipCheck| self.reported.contains(&check.key) || open.contains(&check.check);
        let active: Vec<&BlameEntry<'_>> = entries
            .iter()
            .filter(|entry| {
                entry.check.runnable
                    && !(already_reported(entry.check)
                        && (entry.check.is_build() || !entry.allow_revert))
            })
            .collect();
        if active.is_empty() {
            return Ok(Vec::new());
        }
        let repaired = self.repaired_at_newest(ctx, tip, &active)?;
        let active: Vec<&BlameEntry<'_>> = active
            .into_iter()
            .filter(|entry| !repaired.contains(&entry.check.check))
            .collect();
        if active.is_empty() {
            return Ok(Vec::new());
        }
        let master_probes = self.probe_master_batch(ctx, master, tip, &active)?;
        Ok(active
            .into_iter()
            .filter_map(|entry| {
                let (good, search_bad, allow_revert) = if let Some(gap_good) =
                    entry.prior.filter(|good| *good != master)
                    && master != tip
                {
                    match master_probes
                        .get(&entry.check.check)
                        .copied()
                        .unwrap_or(MasterProbe::Unknown)
                    {
                        MasterProbe::Passed => (master, tip, entry.allow_revert),
                        MasterProbe::Failed => (gap_good, master, false),
                        MasterProbe::Unknown => (gap_good, tip, false),
                    }
                } else {
                    let at_or_no_gap = entry.prior.is_none_or(|good| good == master);
                    (
                        entry.prior.unwrap_or(master),
                        tip,
                        at_or_no_gap && entry.allow_revert,
                    )
                };
                if already_reported(entry.check) && (entry.check.is_build() || !allow_revert) {
                    return None;
                }
                Some(BlamePlan {
                    entry,
                    good,
                    search_bad,
                    allow_revert,
                })
            })
            .collect())
    }

    fn bisect_plan(
        &self,
        ctx: &Context,
        plans: &[BlamePlan<'_>],
        idx: usize,
        hint: Option<&CulpritHint>,
        probe_cache: &mut HashMap<(Commit, CheckName), Option<Outcome>>,
        landings_cache: &mut HashMap<(Commit, Commit), Vec<TipLanding>>,
    ) -> Result<Option<Culprit>> {
        let plan = &plans[idx];
        let check = plan.entry.check;
        let range = (plan.good.clone(), plan.search_bad.clone());
        if !landings_cache.contains_key(&range) {
            let found = self.work.landings(ctx, plan.good, plan.search_bad)?;
            landings_cache.insert(range.clone(), found);
        }
        let landings = &landings_cache[&range];
        Ok(bisect_with_hint(
            landings,
            &check.inputs,
            plan.good,
            hint,
            |at| {
                let key = (at.clone(), check.check.clone());
                if let Some(&cached) = probe_cache.get(&key) {
                    return cached;
                }
                let batch: Vec<CheckName> = plans[idx..]
                    .iter()
                    .filter(|other| {
                        other.entry.check.family == check.family
                            && other.good == plan.good
                            && other.search_bad == plan.search_bad
                            && !probe_cache
                                .contains_key(&(at.clone(), other.entry.check.check.clone()))
                    })
                    .map(|other| other.entry.check.check.clone())
                    .collect();
                let runs = self.probe(ctx, at, &batch, false);
                let mut by_name: HashMap<CheckName, Outcome> = runs
                    .into_iter()
                    .map(|run| {
                        let outcome = probe_outcome(&run);
                        (run.check, outcome)
                    })
                    .collect();
                for name in batch {
                    let outcome = by_name.remove(&name);
                    probe_cache.insert((at.clone(), name), outcome);
                }
                probe_cache.get(&key).copied().flatten()
            },
        ))
    }

    fn report_blamed(
        &mut self,
        ctx: &Context,
        tip: &Commit,
        plan: &BlamePlan<'_>,
        culprit: Result<Option<Culprit>>,
        reported: bool,
        open: &mut HashSet<CheckName>,
    ) -> Result<bool> {
        let check = plan.entry.check;
        let mut note = None;
        if plan.allow_revert
            && let Ok(Some(found)) = &culprit
        {
            match self.try_revert(ctx, tip, check, found, plan.entry.output) {
                Handled::Reverted => {
                    self.reported.insert(check.key.clone());
                    return Ok(true);
                }
                Handled::NextRound if self.deferred.insert(check.check.clone()) => {
                    return Ok(true);
                }
                Handled::Unbuildable(text) => note = Some(text),
                Handled::NextRound | Handled::Report => {}
            }
        }
        super::round::require_running()?;
        if reported && note.is_none() {
            return Ok(true);
        }
        let output = plan
            .entry
            .output
            .map(|o| tail(o, &self.own_outputs))
            .unwrap_or_default();
        let report = RegressionReport {
            check: check.check.clone(),
            first_bad: plan.search_bad.clone(),
            last_good: plan.good.clone(),
            caused_by: None,
            output: match note {
                Some(note) => format!("{note}\n\n{output}"),
                None => output,
            },
            suspects: Vec::new(),
            platform: check.key.platform,
            search: None,
            found_by: FoundBy::TipVerification,
            detected_at: Some(tip.clone()),
        };
        match ctx.hub.regression(&with_culprit(report, culprit)) {
            Ok(_) => {
                self.reported.insert(check.key.clone());
                open.insert(check.check.clone());
                Ok(true)
            }
            Err(error) => {
                eprintln!(
                    "ignite: regression of {} not reported: {error}",
                    check.check
                );
                Ok(false)
            }
        }
    }

    /// Finds culprits for a round's failing checks, reports regressions, and reverts eligible
    /// landings.
    ///
    /// - Checks already open as regressions on the hub (or reported locally) are skipped when
    ///   they cannot be reverted, so a second machine or a newer tip with a changed input hash
    ///   does not re-bisect an already-filed regression.
    /// - Probes at `newest`, `master`, and each bisection commit run all co-failing checks of
    ///   the same family and search range together in one checkout and build, reusing the cached
    ///   outcomes for the remaining checks in the round.
    pub(super) fn blame_failed(
        &mut self,
        ctx: &Context,
        master: &Commit,
        tip: &Commit,
        open_regressions: &HashSet<CheckName>,
        entries: &[BlameEntry<'_>],
        save_after_each: bool,
    ) -> Result<BlameSummary> {
        super::round::require_running()?;
        let mut open = open_regressions.clone();
        if let Ok(current) = ctx.hub.open_regressions() {
            open.extend(current.into_iter().map(|regression| regression.check));
        }
        let plans = self.plan_blame(ctx, master, tip, &open, entries)?;
        let mut culprits = HashSet::new();
        let mut complete = true;
        let mut hint = None;
        let mut probe_cache = HashMap::new();
        let mut landings_cache = HashMap::new();
        for (idx, plan) in plans.iter().enumerate() {
            let check = plan.entry.check;
            let reported = self.reported.contains(&check.key) || open.contains(&check.check);
            if reported && (check.is_build() || !plan.allow_revert) {
                continue;
            }
            let culprit = self.bisect_plan(
                ctx,
                &plans,
                idx,
                hint.as_ref(),
                &mut probe_cache,
                &mut landings_cache,
            );
            super::round::require_running()?;
            if let Ok(Some(found)) = &culprit {
                if found.suspects.len() == 1 {
                    hint = Some(CulpritHint {
                        first_bad: found.first_bad.clone(),
                    });
                }
                // The last suspect in the narrowed range is known to fail by its end. Promotion
                // must stop before that landing, including when the search ended with one suspect.
                if let Some(cutoff) = found.suspects.last().and_then(|s| s.commits.first()) {
                    culprits.insert(cutoff.clone());
                }
            }
            complete &= self.report_blamed(ctx, tip, plan, culprit, reported, &mut open)?;
            super::round::require_running()?;
            if save_after_each {
                self.save(ctx);
            }
        }
        Ok(BlameSummary { culprits, complete })
    }
}
