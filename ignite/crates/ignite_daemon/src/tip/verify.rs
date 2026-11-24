//! A tip round's check set, verdicts and advancement of master.

use std::collections::{BTreeMap, HashMap, HashSet};

use ignite_core::config::Os;
use ignite_core::ids::{CheckName, Commit};
use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::{CachedResult, ResultKey, TipFailure, TipState, TipVerified};
use ignite_core::traits::{Push, Result};

use super::coverage::{Required, Scope};
use super::families::{RunPhase, observe_failures};
use super::round::require_running;
use super::state::FailedTip;
use super::{
    Listed, ListingPhase, TipCheck, TipOutcome, TipRun, TipSet, TipVerifier, candidate, covered,
    green, meter, names_of, settle_runs, split, verdict,
};
use crate::server::Context;

/// The checks of one phase that need a run, separating those that must ignore the cache.
#[derive(Default)]
pub(super) struct RunPlan<'a> {
    pub queued: Vec<&'a TipCheck>,
    pub recheck: Vec<&'a TipCheck>,
}

fn count(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}

fn observe_failed_verdicts<'a>(
    ctx: &Context,
    tip: &Commit,
    required: impl Iterator<Item = &'a TipCheck>,
    verdicts: &HashMap<ResultKey, TipRun>,
) {
    let keys = required
        .filter_map(|check| {
            let key = verdicts.get(&check.key)?.failure_key.as_ref()?;
            (key == &check.key).then(|| key.clone())
        })
        .collect();
    observe_failures(ctx, tip, keys);
}

impl TipVerifier {
    /// Verifies one tip: runs what its set still needs, then moves `master`, reports failures
    /// or waits. A round that ran sends the hub a record of where its time went.
    pub fn verify(&mut self, ctx: &Context, tip: &Commit) -> Result<TipOutcome> {
        self.require_machine_on()?;
        self.failed = None;
        self.retry_for_stall = None;
        crate::steps::step(format!("verifying tip {tip}"));
        let last = ctx.git.tip(&ctx.config.repository.master)?;
        self.master_at_start = Some(last.clone());
        let coverage = ctx.hub.tip_coverage()?;
        let scope = Scope::new(ctx, self, &last, tip, &coverage)?;
        if last == *tip && !scope.catch_up_only() {
            // `master` already holds the tip: another machine verified it, or someone pushed it
            // by hand. The hub records it, so that it and older tips stop showing as waiting.
            ctx.hub.tip_verified(&TipVerified {
                commit: tip.clone(),
                machine: ctx.machine.clone(),
                state: TipState::Verified,
                regression_passes: Some(Vec::new()),
                failure: None,
                failed_descendant: None,
            })?;
            return Ok(TipOutcome::Verified);
        }
        let (meter, metered) = meter::Meter::start(ctx);
        let mut tally = meter::Tally::default();
        let outcome = self.verify_round(&metered, &last, tip, &scope, &mut tally);
        self.save(ctx);
        meter::send(ctx, &meter.finish(ctx, tip, &tally));
        outcome
    }

    /// [`TipVerifier::verify`] when the tip still has checks to run, counting runs and cache hits
    /// in `tally`.
    #[allow(clippy::too_many_lines)]
    fn verify_round(
        &mut self,
        ctx: &Context,
        last: &Commit,
        tip: &Commit,
        scope: &Scope,
        tally: &mut meter::Tally,
    ) -> Result<TipOutcome> {
        self.own_outputs.clear();
        self.reverted.clear();
        self.listed.clear();
        self.send_reverts(ctx);
        let mut verdicts = HashMap::new();
        let mut pending_green = None;
        let mut listing = if ctx.config.repository.green.is_some() {
            let mut builds = self.work.checks(ctx, tip, ListingPhase::Build)?;
            require_running()?;
            self.record_listing(&builds);
            let local = builds
                .checks
                .iter()
                .filter(|check| check.runnable)
                .cloned()
                .collect();
            let Required { checks, cached, .. } =
                scope.required(ctx, local, Vec::new(), &builds.defer_platforms)?;
            let build = self.plan_runs(ctx, tip, checks.iter().zip(cached), &mut verdicts);
            tally.cached = count(verdicts.len());
            if let Some(by) = self.run_build(ctx, tip, &checks, &build, &mut verdicts, tally)? {
                return Ok(TipOutcome::Superseded { by });
            }

            // The last verifier to finish its local builds can see the other platforms'
            // listings and results, even if those verifiers have moved on to ordinary listings.
            require_running()?;
            builds.append(self.work.checks(ctx, tip, ListingPhase::PeerBuild)?);
            let Required {
                checks, unlisted, ..
            } = scope.required(
                ctx,
                builds.checks.clone(),
                builds.unlisted.clone(),
                &builds.defer_platforms,
            )?;
            let before = verdicts.len();
            Self::fill_from_cache(ctx, &checks, &mut verdicts)?;
            tally.cached = tally.cached.saturating_add(count(verdicts.len() - before));
            require_running()?;
            if let Some(by) = candidate::replacement(ctx, tip) {
                return Ok(TipOutcome::Superseded { by });
            }
            if last != tip && !green::advance_if_ready(ctx, tip, &checks, &unlisted, &verdicts) {
                pending_green = Some((checks, unlisted));
            }
            builds
        } else {
            TipSet::default()
        };
        let phase = if ctx.config.repository.green.is_some() {
            ListingPhase::Other
        } else {
            ListingPhase::All
        };
        let other = self.work.checks(ctx, tip, phase)?;
        self.record_listing(&other);
        listing.append(other);
        require_running()?;
        let regressions: HashSet<CheckName> = ctx
            .hub
            .open_regressions()
            .unwrap_or_default()
            .into_iter()
            .map(|regression| regression.check)
            .collect();
        let Required {
            checks,
            cached,
            unlisted,
            mut deferred,
        } = scope.required(
            ctx,
            covered::drop_covered(
                listing.checks,
                &self.work.covered_by(),
                &regressions,
                (!scope.catch_up_only()).then_some(&listing.defer_platforms),
            ),
            listing.unlisted,
            &listing.defer_platforms,
        )?;
        let is_build = |check: &TipCheck| ctx.config.repository.green.is_some() && check.is_build();
        let before = verdicts.len();
        let plan = self.plan_runs(
            ctx,
            tip,
            checks
                .iter()
                .zip(cached)
                .filter(|(check, _)| !is_build(check)),
            &mut verdicts,
        );
        tally.cached = tally.cached.saturating_add(count(verdicts.len() - before));
        if let Some((builds, unlisted_builds)) = &pending_green {
            // A native verifier may have finished its checks while we listed the other families.
            Self::fill_from_cache(ctx, builds, &mut verdicts)?;
            require_running()?;
            if let Some(by) = candidate::replacement(ctx, tip) {
                return Ok(TipOutcome::Superseded { by });
            }
            if green::advance_if_ready(ctx, tip, builds, unlisted_builds, &verdicts) {
                pending_green = None;
            }
        }

        // Coverage and deferrals still decide which failures hold master. The green phase
        // considered build checks on their own, before any covering family could be listed.
        let broken = checks.iter().filter(|check| is_build(check)).any(|check| {
            verdicts
                .get(&check.key)
                .is_some_and(|run| run.outcome == Outcome::Fail)
        });
        if last != tip {
            observe_failed_verdicts(ctx, tip, checks.iter(), &verdicts);
        }
        let split = self.split(
            ctx,
            last,
            tip,
            &plan.queued,
            &plan.recheck,
            scope.catch_up_only(),
        );
        tally.budget.clone_from(&split.budget);
        if !broken {
            // Confirm cached failures before unrelated batches so a newer tip can be
            // selected as soon as the failure repeats.
            let (confirm, cache_disabled): (Vec<_>, Vec<_>) =
                split.recheck.iter().copied().partition(|check| check.reuse);
            self.run_rechecks(ctx, tip, &checks, &confirm, &mut verdicts, tally);
            require_running()?;
            if last != tip {
                observe_failed_verdicts(ctx, tip, confirm.into_iter(), &verdicts);
            }
            let phase = if scope.catch_up_only() {
                RunPhase::AfterMaster
            } else {
                RunPhase::BeforeMaster
            };
            let (newer, runs) = self.run_families(ctx, tip, &split.within, phase)?;
            if let Some(by) = newer {
                tally.ran = tally.ran.saturating_add(count(runs.len()));
                let left = split::unrun(
                    split
                        .within
                        .iter()
                        .chain(&cache_disabled)
                        .chain(&split.after),
                    &runs,
                );
                split::note_unrun(tally, left, Some(by.clone()));
                return Ok(TipOutcome::Superseded { by });
            }
            self.record_runs(&checks, runs, &mut verdicts, tally);
            self.run_rechecks(ctx, tip, &checks, &cache_disabled, &mut verdicts, tally);
        }
        Self::fill_from_cache(ctx, &checks, &mut verdicts)?;
        require_running()?;
        if checks
            .iter()
            .filter(|check| check.runnable && !split.is_after(check))
            .all(|check| {
                verdicts
                    .get(&check.key)
                    .is_some_and(|run| matches!(run.outcome, Outcome::Pass | Outcome::Flaky))
            })
        {
            scope.publish_budget(ctx, tip, &split.after)?;
        }
        let peer_after = scope.peer_after(ctx, tip, &checks, &verdicts)?;
        if !peer_after.is_empty() {
            // The peer stored its pre-promotion results before sharing its plan.
            Self::fill_from_cache(
                ctx,
                checks
                    .iter()
                    .filter(|check| !peer_after.contains(&check.key)),
                &mut verdicts,
            )?;
        }
        require_running()?;
        if let Some(by) = candidate::replacement(ctx, tip) {
            return Ok(TipOutcome::Superseded { by });
        }
        if let Some((builds, unlisted_builds)) = pending_green {
            Self::fill_from_cache(ctx, &builds, &mut verdicts)?;
            require_running()?;
            green::advance_if_ready(ctx, tip, &builds, &unlisted_builds, &verdicts);
        }
        self.note_incomplete(ctx, tip, &checks, &verdicts, true);
        self.report_unlisted(ctx, tip, &unlisted);
        self.save(ctx);
        self.require_machine_on()?;
        let explicitly_deferred_here = scope.local.is_some_and(|os| deferred.contains_key(&os));

        let mut failed: Vec<&TipCheck> = Vec::new();
        let mut waiting: Vec<CheckName> = Vec::new();
        let mut blocked_here = unlisted.iter().any(|missing| missing.reportable);
        let now = ctx.clock.now();
        // The checks only another platform's machines can run, with their families.
        let mut elsewhere: BTreeMap<Os, Vec<(CheckName, String)>> = BTreeMap::new();
        for check in &checks {
            let verdict = verdicts.get(&check.key).map(|v| v.outcome);
            let after = split.is_after(check)
                || (!check.runnable
                    && check.reuse
                    && !check.is_build()
                    && !regressions.contains(&check.check)
                    && peer_after.contains(&check.key));
            if after && matches!(verdict, None | Some(Outcome::Incomplete)) {
                if let Some(os) = check.key.platform {
                    deferred.entry(os).or_default().push(check.check.clone());
                }
                continue;
            }
            match verdict {
                Some(Outcome::Pass | Outcome::Flaky) => {}
                Some(Outcome::Fail) => failed.push(check),
                Some(Outcome::Incomplete) | None => {
                    waiting.push(check.check.clone());
                    blocked_here |= check.runnable
                        && (self
                            .held_off
                            .get(&check.key)
                            .is_some_and(|until| now < *until)
                            || verdicts
                                .get(&check.key)
                                .is_some_and(|run| run.reason.is_some() || run.build.is_some()));
                    if let (false, Some(os)) = (check.runnable, check.key.platform) {
                        elsewhere
                            .entry(os)
                            .or_default()
                            .push((check.check.clone(), check.family.clone()));
                    }
                }
            }
        }
        for missing in &unlisted {
            waiting.push(missing.name.clone());
            for os in missing.platforms.iter().filter(|_| !missing.reportable) {
                elsewhere
                    .entry(*os)
                    .or_default()
                    .push((missing.name.clone(), missing.family.clone()));
            }
        }
        if !failed.is_empty() {
            let keys: Vec<_> = failed.iter().map(|check| check.key.clone()).collect();
            let recorded = if last == tip {
                Ok(())
            } else {
                // A culprit probe may wait for a build slot. The tip already has a verdict,
                // so record the exact keys and let a newer candidate run before the search.
                let unverified_commits =
                    ignite_git::refs::commits_between(&ctx.repo, last, tip).unwrap_or_default();
                ctx.hub
                    .tip_verified(&TipVerified {
                        commit: tip.clone(),
                        machine: ctx.machine.clone(),
                        state: TipState::Failed,
                        regression_passes: None,
                        failed_descendant: None,
                        failure: Some(TipFailure {
                            checks: keys.clone(),
                            unverified_commits,
                        }),
                    })
                    .and_then(|()| candidate::allow_newer(ctx, tip))
            };
            // Checks whose cache is disabled must run again to establish a new verdict.
            // Their older cached passes cannot end a search or requeue a settled tip.
            if recorded.is_ok() && failed.iter().all(|check| check.reuse) {
                self.failed = Some(FailedTip {
                    commit: tip.clone(),
                    checks: keys,
                    round: crate::steps::current().and_then(|activity| activity.handle()),
                });
                self.save(ctx);
                if self.failure_recovered(ctx, tip, None) {
                    return Ok(TipOutcome::Recovered);
                }
            }
            let can_revert = |check: &TipCheck| {
                last != tip
                    && check.runnable
                    && scope.last_good_for(check).is_none()
                    && !split.is_after(check)
            };
            let stall_at = failed
                .iter()
                .any(|check| !check.is_build() && can_revert(check))
                .then(|| self.stall_revert_at(ctx, last, tip))
                .flatten();
            let now = ctx.clock.now();
            self.retry_for_stall = stall_at.filter(|at| now < *at).map(|at| (tip.clone(), at));
            let stalled = stall_at.is_some_and(|at| now >= at);

            let mut culprits = HashSet::new();
            let mut blame_complete = true;
            for check in &failed {
                let output = verdicts.get(&check.key).and_then(|v| v.output.clone());
                // If this search is interrupted and the check still fails, the next round reports it.
                let prior = scope.last_good_for(check);
                match self.blame(
                    ctx,
                    prior.unwrap_or(last),
                    tip,
                    check,
                    output.as_deref(),
                    can_revert(check)
                        && if check.is_build() {
                            ctx.config.revert.broken_build && ctx.config.repository.green.is_some()
                        } else {
                            stalled
                        },
                ) {
                    Ok(culprit) => culprits.extend(culprit),
                    Err(error) => {
                        blame_complete = false;
                        eprintln!(
                            "ignite: regression of {} not reported: {error}",
                            check.check
                        );
                    }
                }
                require_running()?;
                self.save(ctx);
            }
            recorded?;
            let promoted = if blame_complete {
                self.promote_ancestor(ctx, last, tip, &culprits, &regressions)
                    .inspect_err(|error| {
                        eprintln!("ignite: tip {tip}: ancestor promotion: {error}");
                    })
                    .unwrap_or(None)
            } else {
                None
            };
            return Ok(TipOutcome::Failed {
                checks: names_of(&failed),
                promoted,
            });
        }
        if !waiting.is_empty() {
            let flagged = self.flag_missing_machines(ctx, tip, elsewhere);
            self.save(ctx);
            let unrunnable = flagged?;
            if last != tip && (blocked_here || !unrunnable.is_empty()) {
                candidate::allow_newer(ctx, tip)?;
            }
            waiting.sort();
            waiting.dedup();
            return Ok(TipOutcome::Waiting {
                checks: waiting,
                unrunnable,
            });
        }
        for names in deferred.values_mut() {
            names.sort();
            names.dedup();
        }
        scope.record_deferrals(ctx, last, tip, &deferred)?;
        let regression_passes = checks
            .iter()
            .filter(|check| regressions.contains(&check.check))
            .filter(|check| {
                verdicts
                    .get(&check.key)
                    .is_some_and(|run| matches!(run.outcome, Outcome::Pass | Outcome::Flaky))
            })
            .map(|check| check.key.clone())
            .collect();
        let outcome = if last == tip {
            self.record_verified(ctx, tip, regression_passes, None)?
        } else {
            self.move_master(ctx, tip, regression_passes, None)?
        };
        if outcome != TipOutcome::Verified {
            return Ok(outcome);
        }
        self.run_after(ctx, last, tip, scope, &checks, &split, &mut verdicts, tally)?;
        if !explicitly_deferred_here {
            let deferred_here = scope.local.is_some_and(|os| deferred.contains_key(&os));
            if scope.record_pass(ctx, tip, &checks, &verdicts, &unlisted, deferred_here)?
                && let Some(os) = scope.local
            {
                deferred.remove(&os);
            }
        }
        Ok(if deferred.is_empty() {
            outcome
        } else {
            TipOutcome::VerifiedWithDeferrals { checks: deferred }
        })
    }

    /// Cuts batches over the listing before omitting covered and switched-off checks, so that
    /// every machine cuts them alike.
    fn record_listing(&mut self, listing: &TipSet) {
        self.listed.extend(
            listing
                .checks
                .iter()
                .chain(&listing.off)
                .filter(|check| check.runnable)
                .map(|check| Listed {
                    family: check.family.clone(),
                    check: check.check.clone(),
                    portable: check.key.platform.is_none(),
                }),
        );
    }

    fn plan_runs<'a>(
        &mut self,
        ctx: &Context,
        tip: &Commit,
        checks: impl Iterator<Item = (&'a TipCheck, Option<CachedResult>)>,
        verdicts: &mut HashMap<ResultKey, TipRun>,
    ) -> RunPlan<'a> {
        let now = ctx.clock.now();
        let mut plan = RunPlan::default();
        let mut planned = Vec::new();
        for (check, hit) in checks {
            planned.push(check);
            let held = self
                .held_off
                .get(&check.key)
                .is_some_and(|until| now < *until);
            match hit.filter(|_| check.reuse) {
                // A failure is run again once here before it holds `master`, whoever stored it:
                // another machine's may be a fault of that machine, this one's a fault of one
                // run. The rerun's result is stored over it, and stands afterwards.
                Some(hit)
                    if hit.outcome == Outcome::Fail
                        && check.runnable
                        && !held
                        && (hit.machine != ctx.machine
                            || self.rechecked.insert(check.key.clone())) =>
                {
                    plan.recheck.push(check);
                }
                Some(hit) => {
                    verdicts.insert(check.key.clone(), verdict(check, hit.outcome, hit.output));
                }
                None if held => {}
                None if check.runnable && check.reuse => plan.queued.push(check),
                None if check.runnable => plan.recheck.push(check),
                None => {}
            }
        }
        self.record_cached(ctx, tip, planned, &plan.queued, &plan.recheck);
        plan
    }

    pub(super) fn run_rechecks(
        &mut self,
        ctx: &Context,
        tip: &Commit,
        checks: &[TipCheck],
        recheck: &[&TipCheck],
        verdicts: &mut HashMap<ResultKey, TipRun>,
        tally: &mut meter::Tally,
    ) {
        if !recheck.is_empty() {
            let names: Vec<CheckName> = recheck.iter().map(|check| check.check.clone()).collect();
            let runs = self.work.run(ctx, tip, &names, true);
            self.record_runs(checks, runs, verdicts, tally);
        }
    }

    pub(super) fn record_runs(
        &mut self,
        checks: &[TipCheck],
        runs: Vec<TipRun>,
        verdicts: &mut HashMap<ResultKey, TipRun>,
        tally: &mut meter::Tally,
    ) {
        tally.ran = tally.ran.saturating_add(count(runs.len()));
        settle_runs(checks, runs, verdicts, &mut self.own_outputs);
    }

    /// Moves `master` to a tip whose every check passed and records it as verified.
    pub(super) fn move_master(
        &mut self,
        ctx: &Context,
        tip: &Commit,
        regression_passes: Vec<ResultKey>,
        failed_descendant: Option<&Commit>,
    ) -> Result<TipOutcome> {
        let repository = &ctx.config.repository;
        match ctx.git.push(&ctx.repo, tip, &repository.master)? {
            Push::Pushed { .. } => {}
            Push::Rejected { reason } => {
                // Another machine may have verified a later tip and moved `master` past this
                // one first. `master` then holds the tip already, so it is verified; reporting
                // it as stuck would hold up every landing that waits for its result.
                let moved_past = ctx
                    .git
                    .tip(&repository.master)
                    .and_then(|master| ctx.git.is_ancestor(tip, &master))
                    .unwrap_or(false);
                if !moved_past {
                    return Ok(self.master_stuck(ctx, tip, &reason));
                }
            }
        }
        self.record_verified(ctx, tip, regression_passes, failed_descendant)
    }

    /// Reports once that `master` refused the tip, and waits on it.
    fn master_stuck(&mut self, ctx: &Context, tip: &Commit, reason: &str) -> TipOutcome {
        // Typically `master` holds a commit `staging` lacks, such as a push by hand.
        // Trying again gives the same answer, so the hub is told once.
        let name = CheckName::new("tip:move-master")
            .unwrap_or_else(|_| unreachable!("a family and a word make a check name"));
        let output = format!(
            "{} did not move to {tip}: {reason}",
            ctx.config.repository.master
        );
        self.report_stuck(ctx, tip, &name, None, &output);
        TipOutcome::Waiting {
            checks: vec![name],
            unrunnable: Vec::new(),
        }
    }

    /// Records `tip` as verified with the hub and clears the waiting state.
    fn record_verified(
        &mut self,
        ctx: &Context,
        tip: &Commit,
        regression_passes: Vec<ResultKey>,
        failed_descendant: Option<&Commit>,
    ) -> Result<TipOutcome> {
        ctx.hub.tip_verified(&TipVerified {
            commit: tip.clone(),
            machine: ctx.machine.clone(),
            state: TipState::Verified,
            regression_passes: Some(regression_passes),
            failure: None,
            failed_descendant: failed_descendant.cloned(),
        })?;
        if failed_descendant.is_none() {
            self.waiting_since.clear();
            self.flagged.clear();
            self.deferred.clear();
        }
        Ok(TipOutcome::Verified)
    }
}
