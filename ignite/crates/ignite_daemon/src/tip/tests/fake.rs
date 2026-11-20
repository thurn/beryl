//! The scripted [`TipWork`] the tip tests verify with.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use ignite_core::ids::{CheckName, Commit, MachineId, RecordId};
use ignite_core::protocol::hub::{IncompleteReason, NewResult};
use ignite_core::protocol::{Class, Outcome};
use ignite_core::traits::{CommandSpec, Result};
use ignite_git::changed::CommitPaths;

use super::{commit, ticket};
use crate::server::Context;
use crate::tip::{TipCheck, TipLanding, TipRun, TipSet, TipWork, Unlisted, ticketed_landings};

/// Checks, run outcomes and landings scripted by commit. A run stores its results at the hub
/// under the keys the commit's checks have there, as the real runner does.
/// One run: the machine, the commit, the checks named and whether the cache was ignored.
pub(super) type Run = (MachineId, Commit, Vec<CheckName>, bool);

#[derive(Default)]
pub(super) struct FakeWork {
    pub(super) checks: Mutex<HashMap<Commit, Vec<TipCheck>>>,
    pub(super) outcomes: Mutex<HashMap<(Commit, CheckName), Outcome>>,
    pub(super) landings: Mutex<Vec<TipLanding>>,
    /// The output a failing run returns for a check, in place of the default message.
    pub(super) outputs: Mutex<HashMap<CheckName, String>>,
    /// Commits between the commits a test passes, grouped into landings as
    /// [`CheckTipWork`] groups them, when no landing is scripted.
    pub(super) commits: Mutex<Vec<CommitPaths>>,
    pub(super) runs: Mutex<Vec<Run>>,
    /// What each commit's listing could not list.
    pub(super) unlisted: Mutex<HashMap<Commit, Vec<Unlisted>>>,
    /// Checks whose incomplete runs carry no reason, as when no landing tree was free.
    pub(super) unreasoned: Mutex<HashSet<CheckName>>,
    /// Makes `landings` fail, as when git cannot walk the range.
    pub(super) landings_fail: Mutex<bool>,
    /// Checks that a failed build of the named product kept from running.
    pub(super) unbuilt: Mutex<HashMap<CheckName, String>>,
    /// How many families a round may run at once; 0 means the default of one.
    pub(super) parallel: Mutex<usize>,
    /// The runs under way, and the most that were under way at once. With `parallel` set, a
    /// run waits up to two seconds for the others to start.
    pub(super) active: Mutex<(usize, usize)>,
    pub(super) wake: Condvar,
    /// Each family's `covered_by`.
    pub(super) covered_by: Mutex<BTreeMap<String, String>>,
    /// The class each run saw from the thread's current activity.
    pub(super) classes: Mutex<Vec<Option<ignite_core::protocol::Class>>>,
    /// What the round scores its checks by, for the tip budget; `None` runs every check before
    /// `master` moves.
    pub(super) scoring: Mutex<Option<crate::checks::Scoring>>,
    /// Where `master` was when each run started, with the checks it ran.
    pub(super) masters: Mutex<Vec<(Vec<CheckName>, Commit)>>,
    /// The checks each commit's listing finds switched off.
    pub(super) off: Mutex<HashMap<Commit, Vec<TipCheck>>>,
    /// The families declared `builds = true`.
    pub(super) builds: Mutex<std::collections::BTreeSet<String>>,
    /// Called when a family's batches start, before the first claim, where the real work
    /// builds and lists the family.
    pub(super) before_batches: Mutex<HashMap<String, BeforeBatches>>,
}

/// What [`FakeWork::before_batches`] calls for one family.
pub(super) type BeforeBatches = Arc<dyn Fn() + Send + Sync>;

impl FakeWork {
    pub(super) fn listed(&self, at: &Commit) -> Vec<TipCheck> {
        self.checks
            .lock()
            .unwrap()
            .get(at)
            .cloned()
            .unwrap_or_default()
    }

    pub(super) fn set_checks(&self, at: &Commit, checks: Vec<TipCheck>) {
        self.checks.lock().unwrap().insert(at.clone(), checks);
    }

    /// Switches `check` off in the listing at `at`, as the hub's flaky switch does.
    pub(super) fn switch_off(&self, at: &Commit, check: &CheckName) {
        let mut checks = self.checks.lock().unwrap();
        let listed = checks.entry(at.clone()).or_default();
        let index = listed.iter().position(|c| c.check == *check).unwrap();
        let off = listed.remove(index);
        self.off
            .lock()
            .unwrap()
            .entry(at.clone())
            .or_default()
            .push(off);
    }

    pub(super) fn set_outcome(&self, at: &Commit, check: &CheckName, outcome: Outcome) {
        self.outcomes
            .lock()
            .unwrap()
            .insert((at.clone(), check.clone()), outcome);
    }

    pub(super) fn add_landing(&self, ticket_id: &str, commits: &[&str], paths: &[&str]) {
        self.landings.lock().unwrap().push(TipLanding {
            ticket: Some(ticket(ticket_id)),
            commits: commits.iter().map(|c| commit(c)).collect(),
            paths: paths.iter().map(|p| (*p).to_owned()).collect(),
        });
    }

    pub(super) fn add_commit(&self, sha: &str, paths: &[&str]) {
        self.commits.lock().unwrap().push(CommitPaths {
            commit: commit(sha),
            paths: paths.iter().map(|p| (*p).to_owned()).collect(),
        });
    }

    pub(super) fn set_output(&self, check: &CheckName, output: &str) {
        self.outputs
            .lock()
            .unwrap()
            .insert(check.clone(), output.to_owned());
    }

    /// Launches a command as the real work does, `list` to list families and `run` with a
    /// results file to run checks.
    fn launch(ctx: &Context, command: &str) {
        let env = if command == "run" {
            vec![("IGNITE_RESULTS".to_owned(), "/dev/null".to_owned())]
        } else {
            Vec::new()
        };
        ctx.launcher.run(&CommandSpec {
            args: vec![command.to_owned()],
            cwd: PathBuf::from("/repo"),
            env,
            class: Class::Tip,
            cores: 1,
            resources: Vec::new(),
            grows: false,
            tree: None,
            ticket: None,
            hang_limit_s: None,
            reserve: None,
        });
    }

    pub(super) fn runs(&self) -> Vec<Run> {
        self.runs.lock().unwrap().clone()
    }
}

impl TipWork for FakeWork {
    fn checks(&self, ctx: &Context, at: &Commit) -> Result<TipSet> {
        Self::launch(ctx, "list");
        Ok(TipSet {
            checks: self.listed(at),
            unlisted: self
                .unlisted
                .lock()
                .unwrap()
                .get(at)
                .cloned()
                .unwrap_or_default(),
            off: self
                .off
                .lock()
                .unwrap()
                .get(at)
                .cloned()
                .unwrap_or_default(),
        })
    }

    fn parallel(&self) -> usize {
        (*self.parallel.lock().unwrap()).max(1)
    }

    fn scoring(
        &self,
        _ctx: &Context,
        _from: &Commit,
        _to: &Commit,
        _checks: &[&TipCheck],
    ) -> Option<crate::checks::Scoring> {
        self.scoring.lock().unwrap().clone()
    }

    fn covered_by(&self) -> BTreeMap<String, String> {
        self.covered_by.lock().unwrap().clone()
    }

    fn build_families(&self) -> std::collections::BTreeSet<String> {
        self.builds.lock().unwrap().clone()
    }

    fn run_batches(
        &self,
        ctx: &Context,
        at: &Commit,
        family: &str,
        batches: &[(u32, Vec<CheckName>)],
        claim: &dyn Fn(u32) -> bool,
    ) -> (Vec<TipRun>, usize) {
        let before = self.before_batches.lock().unwrap().get(family).cloned();
        if let Some(before) = before {
            before();
        }
        let mut runs = Vec::new();
        let mut ran = 0;
        for (batch, names) in batches {
            if claim(*batch) {
                ran += 1;
                runs.extend(self.run(ctx, at, names, false));
            }
        }
        (runs, ran)
    }

    fn run(&self, ctx: &Context, at: &Commit, checks: &[CheckName], rerun: bool) -> Vec<TipRun> {
        self.classes.lock().unwrap().push(crate::steps::class());
        let wanted = *self.parallel.lock().unwrap();
        if wanted > 1 {
            let mut active = self.active.lock().unwrap();
            active.0 += 1;
            active.1 = active.1.max(active.0);
            self.wake.notify_all();
            let (mut active, _) = self
                .wake
                .wait_timeout_while(active, Duration::from_secs(2), |a| a.1 < wanted)
                .unwrap();
            active.0 -= 1;
        }
        Self::launch(ctx, "run");
        if let Ok(master) = ctx.git.tip(&ctx.config.repository.master) {
            self.masters.lock().unwrap().push((checks.to_vec(), master));
        }
        self.runs
            .lock()
            .unwrap()
            .push((ctx.machine.clone(), at.clone(), checks.to_vec(), rerun));
        let listed = self.listed(at);
        let outcomes = self.outcomes.lock().unwrap();
        let mut runs = Vec::new();
        for check in checks {
            let outcome = outcomes
                .get(&(at.clone(), check.clone()))
                .copied()
                .unwrap_or(Outcome::Pass);
            if let Some(listed) = listed.iter().find(|c| c.check == *check)
                && matches!(outcome, Outcome::Pass | Outcome::Fail)
            {
                let output = (outcome == Outcome::Fail).then(|| format!("{check} failed"));
                ctx.hub
                    .store(&[NewResult {
                        key: listed.key.clone(),
                        outcome,
                        machine: ctx.machine.clone(),
                        record: RecordId(1),
                        output,
                        cost: ignite_core::protocol::hub::ResultCost::default(),
                        areas: Vec::new(),
                    }])
                    .unwrap();
            }
            runs.push(TipRun {
                check: check.clone(),
                outcome,
                output: (outcome == Outcome::Fail).then(|| {
                    let scripted = self.outputs.lock().unwrap().get(check).cloned();
                    scripted.unwrap_or_else(|| format!("{check} failed"))
                }),
                reason: (outcome == Outcome::Incomplete
                    && !self.unreasoned.lock().unwrap().contains(check)
                    && !self.unbuilt.lock().unwrap().contains_key(check))
                .then_some(IncompleteReason::Unreported),
                build: self.unbuilt.lock().unwrap().get(check).cloned(),
            });
        }
        runs
    }

    fn landings(&self, ctx: &Context, from: &Commit, to: &Commit) -> Result<Vec<TipLanding>> {
        // The scripted landings and commits all lie between the commits a test passes.
        assert_ne!(from, to);
        if *self.landings_fail.lock().unwrap() {
            return Err(ignite_core::traits::Error::Io("git failed".to_owned()));
        }
        let commits = self.commits.lock().unwrap().clone();
        if commits.is_empty() {
            Ok(self.landings.lock().unwrap().clone())
        } else {
            ticketed_landings(ctx, commits)
        }
    }
}
