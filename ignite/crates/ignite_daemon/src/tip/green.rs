//! The green branch: a round lists and runs its build checks before listing other families, and moves
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

use std::collections::HashMap;

use ignite_core::ids::Commit;
use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::ResultKey;
use ignite_core::traits::{Push, Result};

use super::families::RunPhase;
use super::meter::Tally;
use super::verify::RunPlan;
use super::{TipCheck, TipRun, TipVerifier, Unlisted};
use crate::server::Context;

impl TipVerifier {
    /// Runs the first phase without waiting for any other family's listing or budget.
    pub(super) fn run_build(
        &mut self,
        ctx: &Context,
        tip: &Commit,
        checks: &[TipCheck],
        plan: &RunPlan<'_>,
        verdicts: &mut HashMap<ResultKey, TipRun>,
        tally: &mut Tally,
    ) -> Result<Option<Commit>> {
        if !plan.queued.is_empty() {
            let (superseded, runs) = self.run_families(ctx, tip, &plan.queued, RunPhase::Build)?;
            self.record_runs(checks, runs, verdicts, tally);
            if superseded.is_some() {
                return Ok(superseded);
            }
        }
        self.run_rechecks(ctx, tip, checks, &plan.recheck, verdicts, tally);
        Ok(None)
    }
}

/// Attempts to publish green once the build phase has accounted for its required listings
/// and the checks of each applicable platform have passed for their listed inputs.
pub(super) fn advance_if_ready(
    ctx: &Context,
    tip: &Commit,
    checks: &[TipCheck],
    unlisted: &[Unlisted],
    verdicts: &HashMap<ResultKey, TipRun>,
) -> bool {
    let passed = !checks.is_empty()
        && unlisted.is_empty()
        && checks.iter().all(|check| {
            verdicts
                .get(&check.key)
                .is_some_and(|run| matches!(run.outcome, Outcome::Pass | Outcome::Flaky))
        });
    if passed {
        move_green(ctx, tip);
    }
    passed
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
