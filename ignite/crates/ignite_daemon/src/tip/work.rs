//! The real [`TipWork`]: the check runner in a leased landing tree, and the landings from git.

use std::sync::Arc;

use ignite_core::ids::{CheckName, Commit, TreeId};
use ignite_core::protocol::hub::IncompleteReason;
use ignite_core::protocol::{Class, Outcome};
use ignite_core::traits::{Error, Result};

use super::{TipLanding, TipRun, TipSet, TipWork, ticketed_landings};
use crate::checks::{Checks, RunOptions};
use crate::server::Context;
use crate::trees::Trees;

/// Lists and runs checks with [`Checks`] in a landing tree that [`Trees`] leases at the commit.
pub struct CheckTipWork {
    pub checks: Arc<Checks>,
    pub trees: Arc<Trees>,
}

impl CheckTipWork {
    /// Leases a landing tree at `commit`, calls `f` with it and frees it again.
    fn at<T>(&self, commit: &Commit, f: impl FnOnce(&TreeId) -> T) -> Result<T> {
        let holder = format!("tip {commit}");
        let lease = self
            .trees
            .lease_landing(&holder, commit)?
            .ok_or_else(|| Error::Conflict("every landing tree is busy".to_owned()))?;
        let value = f(&lease.tree);
        self.trees.release_landing(&lease.tree, Some(commit))?;
        Ok(value)
    }
}

impl TipWork for CheckTipWork {
    fn checks(&self, ctx: &Context, commit: &Commit) -> Result<TipSet> {
        self.at(commit, |tree| self.checks.regression_checks(ctx, tree))?
    }

    fn run(
        &self,
        ctx: &Context,
        commit: &Commit,
        checks: &[CheckName],
        rerun: bool,
    ) -> Vec<TipRun> {
        let options = RunOptions {
            verb: "tip".to_owned(),
            class: Class::Tip,
            rerun,
            ticket: None,
            session: None,
        };
        match self.at(commit, |tree| self.checks.run(ctx, tree, checks, &options)) {
            Ok(run) => run
                .outcomes
                .into_iter()
                .map(|(check, outcome)| {
                    let failure = run.failures.iter().find(|f| f.check == check);
                    let mut output = failure.and_then(|f| f.output.clone());
                    // An incomplete check always has a reason for the hub's ticket; one that
                    // never got to run, for a failed build say, was not reported by a command.
                    let reason = (outcome == Outcome::Incomplete).then(|| {
                        output = output.take().or(failure.and_then(|f| f.message.clone()));
                        failure
                            .and_then(|f| f.reason)
                            .unwrap_or(IncompleteReason::Unreported)
                    });
                    TipRun {
                        check,
                        outcome,
                        output,
                        reason,
                    }
                })
                .collect(),
            Err(error) => checks
                .iter()
                .map(|check| TipRun {
                    check: check.clone(),
                    outcome: Outcome::Incomplete,
                    output: Some(error.to_string()),
                    reason: None,
                })
                .collect(),
        }
    }

    /// The first-parent commits between the two, grouped by the ticket whose landing the hub
    /// recorded for each.
    fn landings(&self, ctx: &Context, from: &Commit, to: &Commit) -> Result<Vec<TipLanding>> {
        ticketed_landings(ctx, ignite_git::changed::commits(&ctx.repo, from, to)?)
    }
}
