//! The green branch: a round runs its build checks before any other check, and moves
//! `repository.green` to the tip once they have all passed (see "Tip verification" in
//! `docs/ignite.md`).
//!
//! - A build check is a check of a family declared `builds = true` in the check manifests: one
//!   that fails when the code does not compile or type-check. Agents start work from `green`, so
//!   it must only hold commits that compile, and it must not wait behind the rest of a round,
//!   which can take an hour.
//! - The round runs the build checks first, in the `tip` class with the cores set aside for
//!   them (`Reserve::TipBuild`, which the work puts on their commands). When every build check
//!   in the tip's set has a pass, run here or cached, and none waits, the round pushes the tip
//!   to `green` and goes on with the rest of its checks. Whatever they find, `green` stays.
//! - A failing build check ends the build step: the round runs none of its other checks, since
//!   code that does not compile tells them nothing, and the failure is blamed and holds
//!   `master` like any other.
//! - With `repository.green` unset there is no build step, and a round does what it did before
//!   the green branch existed.

use std::collections::{BTreeSet, HashMap};

use ignite_core::ids::{CheckName, Commit};
use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::ResultKey;
use ignite_core::traits::{Push, Result};

use super::meter::Tally;
use super::split::Split;
use super::{TipCheck, TipRun, TipVerifier, Unlisted, settle_runs};
use crate::server::Context;

/// How a round's build step ended.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum BuildStep {
    /// The round goes on with its other checks: the build checks passed, or some wait.
    Continue,
    /// A build check failed. The round runs nothing more and reports the failure.
    Broken,
    /// A newer `staging` tip ended the round before any build check ran.
    Superseded(Commit),
}

impl TipVerifier {
    /// The families whose checks are build checks; none when `repository.green` is unset, so
    /// that a round has no build step.
    pub(super) fn build_families(&self, ctx: &Context) -> BTreeSet<String> {
        if ctx.config.repository.green.is_none() {
            return BTreeSet::new();
        }
        self.work.build_families()
    }

    /// Runs the round's build checks, `split.build` and `split.build_recheck`, before any other
    /// check, files their outcomes in `verdicts`, and moves `green` to `tip` when every build
    /// check of the tip's set has passed and none waits.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn run_build(
        &mut self,
        ctx: &Context,
        tip: &Commit,
        checks: &[TipCheck],
        unlisted: &[Unlisted],
        split: &Split<'_>,
        verdicts: &mut HashMap<ResultKey, TipRun>,
        tally: &mut Tally,
    ) -> Result<BuildStep> {
        let builds = self.build_families(ctx);
        if builds.is_empty() {
            return Ok(BuildStep::Continue);
        }
        if !split.build.is_empty() {
            let (superseded, runs) = self.run_families(ctx, tip, &split.build, false)?;
            if let Some(by) = superseded {
                return Ok(BuildStep::Superseded(by));
            }
            tally.ran = tally.ran.saturating_add(count(&runs));
            settle_runs(checks, runs, verdicts, &mut self.own_outputs);
        }
        if !split.build_recheck.is_empty() {
            let names: Vec<CheckName> = split
                .build_recheck
                .iter()
                .map(|c| c.check.clone())
                .collect();
            let runs = self.work.run(ctx, tip, &names, true);
            tally.ran = tally.ran.saturating_add(count(&runs));
            settle_runs(checks, runs, verdicts, &mut self.own_outputs);
        }
        // Another machine may have run a batch of them meanwhile.
        Self::fill_from_cache(ctx, checks, verdicts)?;
        super::round::require_running()?;
        let outcomes: Vec<Option<Outcome>> = checks
            .iter()
            .filter(|c| builds.contains(&c.family))
            .map(|c| verdicts.get(&c.key).map(|run| run.outcome))
            .collect();
        if outcomes.contains(&Some(Outcome::Fail)) {
            return Ok(BuildStep::Broken);
        }
        let unlisted_build = unlisted.iter().any(|u| builds.contains(&u.family));
        let passed = !outcomes.is_empty()
            && !unlisted_build
            && outcomes
                .iter()
                .all(|o| matches!(o, Some(Outcome::Pass | Outcome::Flaky)));
        if passed {
            move_green(ctx, tip);
        }
        Ok(BuildStep::Continue)
    }
}

fn count(runs: &[TipRun]) -> u32 {
    u32::try_from(runs.len()).unwrap_or(u32::MAX)
}

/// Pushes `tip` to `repository.green`. A refused push is fine when `green` already holds the
/// tip, as when another machine's round moved it there or past it first; otherwise it is
/// logged and `green` stays where it is until a later round moves it. Nothing here holds
/// `master` or ends the round.
fn move_green(ctx: &Context, tip: &Commit) {
    let Some(green) = &ctx.config.repository.green else {
        return;
    };
    match ctx.git.push(&ctx.repo, tip, green) {
        Ok(Push::Pushed { .. }) => {
            eprintln!("ignite: the build checks passed; {green} is at {tip}");
        }
        Ok(Push::Rejected { reason }) => {
            let holds = ctx
                .git
                .tip(green)
                .and_then(|at| ctx.git.is_ancestor(tip, &at))
                .unwrap_or(false);
            if !holds {
                eprintln!("ignite: {green} did not move to {tip}: {reason}");
            }
        }
        Err(error) => eprintln!("ignite: {green} did not move to {tip}: {error}"),
    }
}
