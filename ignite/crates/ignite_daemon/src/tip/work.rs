//! The real [`TipWork`]: the check runner in a leased landing tree, and the landings from git.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use ignite_core::ids::{CheckName, Commit, TicketId, TreeId};
use ignite_core::protocol::hub::IncompleteReason;
use ignite_core::protocol::{Class, Outcome};
use ignite_core::traits::{Clock, Error, Reserve, Result};

use ignite_git::changed::CommitPaths;

use super::{CachedTipSet, ListingPhase, TipCheck, TipLanding, TipRun, TipSet, TipWork};
use crate::checks::{BUILD_FAILED, CheckRun, Checks, RunOptions, Scoring};
use crate::land::env::LandTrees;
use crate::server::Context;

/// How long a tip round waits for a landing tree before it leaves a family's checks for a later
/// round. Landings and a round's other families free their trees within minutes; a tree that
/// stays busy longer is held by a run that died, which the daemon frees when it notices.
const TREE_WAIT_S: f64 = 900.0;

/// How often a tip round that waits for a landing tree asks for one again.
const TREE_POLL: Duration = Duration::from_secs(5);

/// Lists and runs checks with [`Checks`] in a landing tree leased at the commit. In the daemon
/// the trees are the machine's own; in a tip round's verb process they are leased from the
/// daemon (see `crate::verbproc`), owned by the round's handle, so a restarted daemon keeps them.
pub struct CheckTipWork {
    pub checks: Arc<Checks>,
    pub trees: Arc<dyn LandTrees>,
    /// How many landing trees the machine has.
    pub landing_trees: usize,
}

impl CheckTipWork {
    /// Leases a landing tree at `commit` for `holder`, calls `f` with it and frees it again.
    /// Leases that may be held at once need holders of their own: a lease owned by a handle is
    /// given back to the same handle and holder (see `crate::trees`). Among free trees it
    /// prefers the one that built the most of `products` nearest `commit`, so that a family's
    /// build reuses what an earlier round built. While every landing tree is busy it waits for
    /// one (see [`wait_for_tree`]).
    fn at_for<T>(
        &self,
        ctx: &Context,
        holder: &str,
        commit: &Commit,
        products: &[String],
        f: impl FnOnce(&TreeId) -> T,
    ) -> Result<T> {
        let lease = wait_for_tree(
            ctx.clock.as_ref(),
            || {
                if products.is_empty() {
                    self.trees.lease_landing(holder, commit)
                } else {
                    self.trees
                        .lease_landing_for_products(holder, commit, products)
                }
            },
            || std::thread::sleep(TREE_POLL),
        )?;
        let value = f(&lease.tree);
        self.trees.release_landing(&lease.tree, Some(commit))?;
        Ok(value)
    }

    /// The products each family needs, by family name, to tell which build a check waits on.
    fn family_needs(&self) -> HashMap<String, Vec<String>> {
        self.checks
            .families()
            .map(|(name, family)| (name.to_owned(), family.needs.clone()))
            .collect()
    }

    /// [`CheckTipWork::at_for`] for one lease at a time, as listing and single runs take.
    fn at<T>(&self, ctx: &Context, commit: &Commit, f: impl FnOnce(&TreeId) -> T) -> Result<T> {
        self.at_for(ctx, &format!("tip {commit}"), commit, &[], f)
    }
}

impl TipWork for CheckTipWork {
    fn checks(&self, ctx: &Context, commit: &Commit, phase: ListingPhase) -> Result<TipSet> {
        self.at(ctx, commit, |tree| {
            self.checks.regression_checks_for(ctx, tree, phase)
        })?
    }

    fn check_states(
        &self,
        _ctx: &Context,
        family: &str,
    ) -> Result<HashMap<CheckName, ignite_core::protocol::hub::CheckState>> {
        self.checks.check_states(family)
    }

    fn cached_checks(&self, ctx: &Context, commit: &Commit) -> Result<Option<CachedTipSet>> {
        let Some(lease) = self
            .trees
            .lease_landing(&format!("tip ancestor {commit}"), commit)?
        else {
            return Ok(None);
        };
        let listing = self.checks.cached_regression_checks(ctx, &lease.tree);
        self.trees.release_landing(&lease.tree, Some(commit))?;
        listing.map(Some)
    }

    fn run(
        &self,
        ctx: &Context,
        commit: &Commit,
        checks: &[CheckName],
        rerun: bool,
        pre_move: bool,
    ) -> Vec<TipRun> {
        let options = options(rerun, pre_move);
        match self.at(ctx, commit, |tree| {
            self.checks.run(ctx, tree, checks, &options)
        }) {
            Ok(run) => tip_runs(&run, &self.family_needs()),
            Err(error) => not_run(checks.iter(), &error),
        }
    }

    fn run_batches(
        &self,
        ctx: &Context,
        commit: &Commit,
        family: &str,
        batches: &[(u32, Vec<CheckName>)],
        callbacks: &crate::checks::BatchCallbacks<'_>,
        pre_move: bool,
    ) -> (Vec<TipRun>, usize) {
        let options = options(false, pre_move);
        // Families run at once, each in a tree of its own, preferably one that built the
        // products the family needs.
        let needs = self.family_needs();
        let products = needs.get(family).map(Vec::as_slice).unwrap_or_default();
        let ran = self.at_for(
            ctx,
            &format!("tip {commit} {family}"),
            commit,
            products,
            |tree| {
                self.checks
                    .run_batches(ctx, tree, family, batches, &options, callbacks)
            },
        );
        match ran {
            Ok((run, ran)) => (tip_runs(&run, &needs), ran),
            Err(error) => (
                not_run(batches.iter().flat_map(|(_, names)| names), &error),
                0,
            ),
        }
    }

    fn covered_by(&self) -> std::collections::BTreeMap<String, String> {
        self.checks.covered_by().clone()
    }

    fn single_batch(&self) -> std::collections::BTreeSet<String> {
        self.checks.single_batch()
    }

    /// One fewer than the landing trees, so that a landing always finds a tree free.
    fn parallel(&self) -> usize {
        self.landing_trees.saturating_sub(1).max(1)
    }

    /// The paths the commits after `from` changed, read with git, and the hub's record of each
    /// check, in a landing tree at `to`. A failure is logged and leaves the round unbudgeted.
    fn scoring(
        &self,
        ctx: &Context,
        from: &Commit,
        to: &Commit,
        checks: &[&TipCheck],
    ) -> Option<Scoring> {
        let scored = ignite_git::changed::commits(&ctx.repo, from, to).and_then(|commits| {
            let mut changed: Vec<String> = commits.into_iter().flat_map(|c| c.paths).collect();
            changed.sort();
            changed.dedup();
            let named: Vec<(CheckName, String)> = checks
                .iter()
                .map(|c| (c.check.clone(), c.family.clone()))
                .collect();
            self.at(ctx, to, |tree| {
                self.checks.scoring(ctx, tree, &changed, &named)
            })?
        });
        scored
            .inspect_err(|error| {
                eprintln!("ignite: tip {to}: checks not scored, all run before master: {error}");
            })
            .ok()
    }

    /// The first-parent commits between the two, grouped by the ticket whose landing the hub
    /// recorded for each.
    fn landings(&self, ctx: &Context, from: &Commit, to: &Commit) -> Result<Vec<TipLanding>> {
        ticketed_landings(ctx, ignite_git::changed::commits(&ctx.repo, from, to)?)
    }

    fn proven_checks(
        &self,
        rules: &ignite_core::config::Rules,
    ) -> std::collections::BTreeSet<CheckName> {
        self.checks.proven_checks(rules)
    }
}

/// Groups `commits`, oldest first, into landings by the ticket the hub says landed each one.
/// Each commit no landing recorded, such as a push by hand, is a landing of its own with no
/// ticket.
///
/// Grouping a run of unrecorded commits into one landing made a culprit search see one
/// candidate where there were dozens: the commits pushed before the hub recorded landings were
/// all blamed at once, with nothing run, and the regression ticket named the tip as the first
/// bad commit.
pub fn ticketed_landings(ctx: &Context, commits: Vec<CommitPaths>) -> Result<Vec<TipLanding>> {
    let shas: Vec<Commit> = commits.iter().map(|c| c.commit.clone()).collect();
    let owners: HashMap<Commit, (TicketId, Option<Commit>)> = ctx
        .hub
        .landed_commits(&shas)?
        .into_iter()
        .map(|landed| (landed.commit, (landed.ticket, landed.gated_base)))
        .collect();
    let owner = |i: usize| owners.get(&shas[i]).map(|(ticket, _)| ticket);
    let ends: Vec<Commit> = (0..shas.len())
        .filter(|&i| i + 1 == shas.len() || owner(i).is_none() || owner(i) != owner(i + 1))
        .map(|i| shas[i].clone())
        .collect();
    Ok(ignite_git::changed::group(commits, &ends)
        .into_iter()
        .map(|landing| {
            let owner = landing.commits.last().and_then(|c| owners.get(c));
            TipLanding {
                ticket: owner.map(|(ticket, _)| ticket.clone()),
                gated_base: owner.and_then(|(_, base)| base.clone()),
                commits: landing.commits,
                paths: landing.paths,
            }
        })
        .collect())
}

/// How a tip round runs checks. Before `master` moves, the round's builds and commands may draw
/// on the cores the machine keeps for tip work (`Reserve::TipRound`), so that agents holding
/// every other core cannot hold `master` back. So do culprit searches and reverts
/// (`TipVerifier::probe`), which the tip loop waits for. Checks past the budget, after `master`
/// moves, take free cores like any other `tip` work.
fn options(rerun: bool, pre_move: bool) -> RunOptions {
    RunOptions {
        verb: "tip".to_owned(),
        class: Class::Tip,
        rerun,
        ticket: None,
        session: None,
        reserve: pre_move.then_some(Reserve::TipRound),
    }
}

/// Each check's outcome from a run. A check that a failed build kept from running names the
/// product its own family needs that failed, so the round reports each build once instead of
/// once for each of its checks. `needs` lists the products of each family.
pub(super) fn tip_runs(run: &CheckRun, needs: &HashMap<String, Vec<String>>) -> Vec<TipRun> {
    let failed: HashMap<_, _> = run
        .failed_keys
        .iter()
        .map(|key| (&key.check, key))
        .collect();
    let build_of = |check: &CheckName| -> Option<(String, String)> {
        let products = needs
            .get(check.family())
            .map(Vec::as_slice)
            .unwrap_or_default();
        let found = products
            .iter()
            .find_map(|p| run.build_failures.iter().find(|(failed, _)| failed == p));
        // A family the manifest no longer names still waits on a failed build.
        let found = found.or_else(|| run.build_failures.first().filter(|_| products.is_empty()));
        found.map(|(product, output)| (product.clone(), output.to_string_lossy().into_owned()))
    };
    run.outcomes
        .iter()
        .map(|&(ref check, outcome)| {
            let check = check.clone();
            let failure = run.failures.iter().find(|f| f.check == check);
            let mut output = failure.and_then(|f| f.output.clone());
            let unbuilt = outcome == Outcome::Incomplete
                && failure.is_some_and(|f| f.message.as_deref() == Some(BUILD_FAILED));
            if unbuilt && let Some((product, build_output)) = build_of(&check) {
                return TipRun {
                    check,
                    input_hash: None,
                    failure_key: None,
                    outcome,
                    output: Some(build_output),
                    reason: None,
                    build: Some(product),
                };
            }
            // An incomplete check always has a reason for the hub's ticket; one that never got
            // to run, for a failed build say, was not reported by a command.
            // A failure whose run saved no output file still has the message the check printed,
            // which the regression ticket quotes instead of an empty section.
            let failure_key = if outcome == Outcome::Fail {
                output = output.take().or(failure.and_then(|f| f.message.clone()));
                failed.get(&check).copied().cloned()
            } else {
                None
            };
            let reason = (outcome == Outcome::Incomplete).then(|| {
                output = output.take().or(failure.and_then(|f| f.message.clone()));
                failure
                    .and_then(|f| f.reason)
                    .unwrap_or(IncompleteReason::Unreported)
            });
            TipRun {
                input_hash: run.input_hashes.get(&check).cloned(),
                failure_key,
                check,
                outcome,
                output,
                reason,
                build: None,
            }
        })
        .collect()
}

/// The checks of a run that could not start, such as when no landing tree was free.
fn not_run<'a>(checks: impl Iterator<Item = &'a CheckName>, error: &Error) -> Vec<TipRun> {
    checks
        .map(|check| TipRun {
            check: check.clone(),
            input_hash: None,
            failure_key: None,
            outcome: Outcome::Incomplete,
            output: Some(error.to_string()),
            reason: None,
            build: None,
        })
        .collect()
}

/// Calls `lease` until it gives a landing tree, calling `nap` between asks, for up to
/// [`TREE_WAIT_S`] by `clock`.
///
/// A family that found every landing tree busy used to give up at once, and its checks came
/// back incomplete with no reason, so the round waited on them and rested ten minutes before
/// it tried the tip again. After a killed tip round left two landing trees leased, the next
/// round ran one family of six and `master` stayed put.
fn wait_for_tree<T>(
    clock: &dyn Clock,
    mut lease: impl FnMut() -> Result<Option<T>>,
    mut nap: impl FnMut(),
) -> Result<T> {
    let start = clock.now();
    loop {
        super::round::require_running()?;
        if let Some(tree) = lease()? {
            return Ok(tree);
        }
        if clock.now().seconds_since(start) >= TREE_WAIT_S {
            return Err(Error::Conflict("every landing tree is busy".to_owned()));
        }
        crate::steps::step("waiting for a free landing tree");
        nap();
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use ignite_core::fakes::FakeClock;
    use ignite_core::time::Timestamp;

    use super::*;

    #[test]
    fn a_round_waits_for_a_busy_landing_tree() {
        let clock = FakeClock::new(Timestamp::from_millis(0));
        let asks = Cell::new(0);
        let tree = wait_for_tree(
            &clock,
            || {
                asks.set(asks.get() + 1);
                Ok((asks.get() == 3).then_some("landing-2"))
            },
            || clock.advance(5.0),
        );
        assert_eq!(tree.unwrap(), "landing-2");
        assert_eq!(asks.get(), 3);
    }

    #[test]
    fn a_round_gives_up_on_a_tree_that_stays_busy() {
        let clock = FakeClock::new(Timestamp::from_millis(0));
        let naps = Cell::new(0);
        let tree: Result<&str> = wait_for_tree(
            &clock,
            || Ok(None),
            || {
                naps.set(naps.get() + 1);
                clock.advance(5.0);
            },
        );
        assert!(matches!(tree, Err(Error::Conflict(_))));
        assert_eq!(naps.get(), 180);
    }
}
