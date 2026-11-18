//! The real [`TipWork`]: the check runner in a leased landing tree, and the landings from git.

use std::sync::Arc;

use ignite_core::ids::{CheckName, Commit, TreeId};
use ignite_core::protocol::{Class, Outcome};
use ignite_core::traits::{Error, Result};

use super::{TipCheck, TipLanding, TipRun, TipWork};
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
    fn checks(&self, ctx: &Context, commit: &Commit) -> Result<Vec<TipCheck>> {
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
                    let output = run
                        .failures
                        .iter()
                        .find(|f| f.check == check)
                        .and_then(|f| f.output.clone());
                    TipRun {
                        check,
                        outcome,
                        output,
                    }
                })
                .collect(),
            Err(error) => checks
                .iter()
                .map(|check| TipRun {
                    check: check.clone(),
                    outcome: Outcome::Incomplete,
                    output: Some(error.to_string()),
                })
                .collect(),
        }
    }

    /// Each commit counts as its own landing with no ticket: the hub has no route yet that
    /// says which commits each ticket landed, so a search may name a single commit of a
    /// landing and cannot name its ticket.
    fn landings(&self, ctx: &Context, from: &Commit, to: &Commit) -> Result<Vec<TipLanding>> {
        Ok(ignite_git::changed::commits(&ctx.repo, from, to)?
            .into_iter()
            .map(|entry| TipLanding {
                ticket: None,
                commits: vec![entry.commit],
                paths: entry.paths,
            })
            .collect())
    }
}
