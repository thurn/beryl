//! A tip round's check set, verdicts and advancement of master.

use std::collections::{BTreeMap, HashMap, HashSet};

use ignite_core::config::Os;
use ignite_core::ids::{CheckName, Commit};
use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::{ResultKey, TipFailure, TipState, TipVerified};
use ignite_core::traits::{Push, Result};

use super::coverage::{Required, Scope};
use super::round::require_running;
use super::{
    Listed, TipCheck, TipOutcome, TipRun, TipSet, TipVerifier, candidate, covered, green, meter,
    names_of, settle_runs, split, verdict,
};
use crate::server::Context;

impl TipVerifier {
    /// Verifies one tip: runs what its set still needs, then moves `master`, reports failures
    /// or waits. A round that ran sends the hub a record of where its time went.
    pub fn verify(&mut self, ctx: &Context, tip: &Commit) -> Result<TipOutcome> {
        self.require_machine_on()?;
        crate::steps::step(format!("verifying tip {tip}"));
        let last = ctx.git.tip(&ctx.config.repository.master)?;
        let coverage = ctx.hub.tip_coverage()?;
        let scope = Scope::new(ctx, self, &last, tip, &coverage)?;
        if last == *tip && scope.catching_up.is_none() {
            // `master` already holds the tip: another machine verified it, or someone pushed it
            // by hand. The hub records it, so that it and older tips stop showing as waiting.
            ctx.hub.tip_verified(&TipVerified {
                commit: tip.clone(),
                machine: ctx.machine.clone(),
                state: TipState::Verified,
                regression_passes: Some(Vec::new()),
                failure: None,
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
        self.send_reverts(ctx);
        let TipSet {
            checks,
            unlisted,
            off,
        } = self.work.checks(ctx, tip)?;
        require_running()?;
        // Batches are cut over the listing alone, before checks switched off as flaky or
        // covered by another family are left out, so that every machine cuts them alike.
        self.listed = checks
            .iter()
            .chain(&off)
            .filter(|c| c.runnable)
            .map(|c| Listed {
                family: c.family.clone(),
                check: c.check.clone(),
                portable: c.key.platform.is_none(),
            })
            .collect();
        let regressions: HashSet<CheckName> = ctx
            .hub
            .open_regressions()
            .unwrap_or_default()
            .into_iter()
            .map(|regression| regression.check)
            .collect();
        let (checks, unlisted) = scope.select(
            covered::drop_covered(checks, &self.work.covered_by(), &regressions),
            unlisted,
        );
        let now = ctx.clock.now();
        let keys: Vec<ResultKey> = checks.iter().map(|c| c.key.clone()).collect();
        let cached = ctx.hub.lookup(&keys)?;
        let Required {
            checks,
            cached,
            unlisted,
            deferred,
        } = scope.defer(checks, cached, unlisted);
        let mut verdicts: HashMap<ResultKey, TipRun> = HashMap::new();
        let mut to_run: Vec<&TipCheck> = Vec::new();
        let mut recheck: Vec<&TipCheck> = Vec::new();
        for (check, hit) in checks.iter().zip(cached) {
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
                    recheck.push(check);
                }
                Some(hit) => {
                    verdicts.insert(check.key.clone(), verdict(check, hit.outcome, hit.output));
                }
                None if held => {}
                None if check.runnable && check.reuse => to_run.push(check),
                None if check.runnable => recheck.push(check),
                None => {}
            }
        }
        self.record_cached(ctx, tip, &checks, &to_run, &recheck);
        tally.cached = u32::try_from(verdicts.len()).unwrap_or(u32::MAX);

        // Required platform checks and the best-scored portable checks run before `master` moves.
        let split = self.split(
            ctx,
            last,
            tip,
            &to_run,
            &recheck,
            scope.catching_up.is_some(),
        );
        tally.budget.clone_from(&split.budget);
        // The build checks first; a broken build runs nothing more (see `green`).
        let step = self.run_build(ctx, tip, &checks, &unlisted, &split, &mut verdicts, tally)?;
        let mut superseded = match &step {
            green::BuildStep::Superseded(by) => Some((by.clone(), Vec::new())),
            _ => None,
        };
        if step == green::BuildStep::Continue {
            let (newer, runs) = self.run_families(ctx, tip, &split.within, false)?;
            if let Some(by) = newer {
                superseded = Some((by, runs));
            } else {
                tally.ran = tally
                    .ran
                    .saturating_add(u32::try_from(runs.len()).unwrap_or(u32::MAX));
                settle_runs(&checks, runs, &mut verdicts, &mut self.own_outputs);
            }
        }
        if let Some((by, runs)) = superseded {
            let left = split::unrun(split.within.iter().chain(&split.after), &runs);
            split::note_unrun(tally, left, Some(by.clone()));
            return Ok(TipOutcome::Superseded { by });
        }
        if step == green::BuildStep::Continue && !split.recheck.is_empty() {
            let names: Vec<CheckName> = split.recheck.iter().map(|c| c.check.clone()).collect();
            let runs = self.work.run(ctx, tip, &names, true);
            tally.ran = tally
                .ran
                .saturating_add(u32::try_from(runs.len()).unwrap_or(u32::MAX));
            settle_runs(&checks, runs, &mut verdicts, &mut self.own_outputs);
        }
        Self::fill_from_cache(ctx, &checks, &mut verdicts)?;
        require_running()?;
        self.note_incomplete(ctx, tip, &checks, &verdicts, true);
        self.report_unlisted(ctx, tip, &unlisted);
        self.save(ctx);
        self.require_machine_on()?;
        scope.record_pass(ctx, tip, &checks, &verdicts, &unlisted)?;

        let mut failed: Vec<&TipCheck> = Vec::new();
        let mut waiting: Vec<CheckName> = Vec::new();
        let mut blocked_here = unlisted.iter().any(|missing| missing.reportable);
        let now = ctx.clock.now();
        // The checks only another platform's machines can run, with their families.
        let mut elsewhere: BTreeMap<Os, Vec<(CheckName, String)>> = BTreeMap::new();
        for check in checks.iter().filter(|c| !split.is_after(c)) {
            match verdicts.get(&check.key).map(|v| v.outcome) {
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
            for check in &failed {
                let output = verdicts.get(&check.key).and_then(|v| v.output.clone());
                // A failure to find the culprit or to reach the hub must not keep the tip from
                // being recorded as failed; a later tip that still fails reports it again.
                let prior = scope.last_good_for(check);
                if let Err(error) = self.blame(
                    ctx,
                    prior.unwrap_or(last),
                    tip,
                    check,
                    output.as_deref(),
                    prior.is_none(),
                ) {
                    eprintln!(
                        "ignite: regression of {} not reported: {error}",
                        check.check
                    );
                }
                require_running()?;
                self.save(ctx);
            }
            let checks = names_of(&failed);
            if last != tip {
                let unverified_commits =
                    ignite_git::refs::commits_between(&ctx.repo, last, tip).unwrap_or_default();
                ctx.hub.tip_verified(&TipVerified {
                    commit: tip.clone(),
                    machine: ctx.machine.clone(),
                    state: TipState::Failed,
                    regression_passes: None,
                    failure: Some(TipFailure {
                        checks: failed.iter().map(|check| check.key.clone()).collect(),
                        unverified_commits,
                    }),
                })?;
                candidate::allow_newer(ctx, tip)?;
            }
            return Ok(TipOutcome::Failed { checks });
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
            self.record_verified(ctx, tip, regression_passes)?
        } else {
            self.move_master(ctx, tip, regression_passes)?
        };
        if outcome != TipOutcome::Verified {
            return Ok(outcome);
        }
        self.run_after(ctx, last, tip, &checks, &split, &mut verdicts, tally)?;
        Ok(if deferred.is_empty() {
            outcome
        } else {
            TipOutcome::VerifiedWithDeferrals { checks: deferred }
        })
    }

    /// Moves `master` to a tip whose every check passed and records it as verified.
    fn move_master(
        &mut self,
        ctx: &Context,
        tip: &Commit,
        regression_passes: Vec<ResultKey>,
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
        self.record_verified(ctx, tip, regression_passes)
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
    ) -> Result<TipOutcome> {
        ctx.hub.tip_verified(&TipVerified {
            commit: tip.clone(),
            machine: ctx.machine.clone(),
            state: TipState::Verified,
            regression_passes: Some(regression_passes),
            failure: None,
        })?;
        self.waiting_since.clear();
        self.flagged.clear();
        self.deferred.clear();
        Ok(TipOutcome::Verified)
    }
}
