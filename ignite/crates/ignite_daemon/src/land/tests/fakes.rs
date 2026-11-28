//! The fakes the landing tests use: a repository held in memory ([`TestGit`]), a hub whose
//! leases expire by the fake clock ([`LeaseHub`]), build trees in maps ([`TestTrees`]) and a
//! gate that keys each check by its inputs' object IDs and keeps results in the hub's cache
//! ([`CacheGate`]), as the real gate does.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use ignite_core::fakes::FakeClock;
use ignite_core::ids::{CheckName, Commit, InputHash, Oid, RecordId, TicketId, TreeId};
use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::{NewResult, ResultKey};
use ignite_core::time::Timestamp;
use ignite_core::traits::{Error, Git, Push, Rebase, Result};

use crate::checks::{Carried, CheckRun, Excused, Failure, GateRun, RunOptions};
use crate::land::env::{CheckVerdict, GateTree, LandGate, LandTrees, TicketTree};
use crate::server::Context;

mod hub;

pub use super::repo::{Repo, edits, hex};
pub use hub::LeaseHub;

#[derive(Default)]
pub struct TestGit {
    pub repo: Mutex<Repo>,
    pub push_unreachable: AtomicBool,
    /// Makes the next push reach the remote and then fail as if the connection dropped
    /// before the remote's answer arrived.
    pub push_lands_then_fails: AtomicBool,
    /// Makes every push answer that the tip moved, whatever the tip is.
    pub push_always_rejected: AtomicBool,
    /// Makes every push fail as a hook or a protected branch would refuse it.
    pub push_refused: AtomicBool,
    /// Makes a push to any branch but the integration branch answer that the tip moved.
    pub other_branch_rejected: AtomicBool,
    /// Makes every commit hold the changes of every other, as if each were a rebased copy.
    pub copies_hold: AtomicBool,
    /// Changes another machine lands just before each of the next pushes to the integration
    /// branch, first queued first, so that push is rejected because the tip moved.
    pub landed_before_push: Mutex<VecDeque<Vec<(&'static str, &'static str)>>>,
    /// Run at the next push to a branch other than the integration branch, before it lands.
    pub on_branch_push: Mutex<Option<Hook>>,
}

impl TestGit {
    /// A repository whose root commit holds `files`. Its commits record the time on `clock`,
    /// for `Git::committed_at`, or the epoch without one.
    pub fn with_clock(files: &[(&str, &str)], clock: Option<Arc<FakeClock>>) -> Self {
        let git = TestGit::default();
        let mut repo = git.repo.lock().unwrap();
        repo.clock = clock;
        let root = repo.commit(None, &edits(files));
        repo.history.push(root);
        drop(repo);
        git
    }

    /// Starts a ticket branch in `path` at the tip, with `changes` committed on it.
    pub fn start(&self, path: &Path, changes: &[(&str, &str)]) {
        let mut repo = self.repo.lock().unwrap();
        let tip = repo.tip();
        let changes = edits(changes);
        let head = repo.commit(Some(&tip), &changes);
        repo.base.insert(path.to_path_buf(), tip);
        repo.edits.insert(path.to_path_buf(), changes);
        repo.checked_out.insert(path.to_path_buf(), head);
    }

    /// Another machine lands `changes` on the integration branch.
    pub fn land_other(&self, changes: &[(&str, &str)]) {
        let mut repo = self.repo.lock().unwrap();
        let tip = repo.tip();
        let commit = repo.commit(Some(&tip), &edits(changes));
        repo.history.push(commit);
    }

    pub fn check_out(&self, path: &Path, at: &Commit) {
        let mut repo = self.repo.lock().unwrap();
        repo.checked_out.insert(path.to_path_buf(), at.clone());
    }

    /// Commits `changes` on the branch checked out in `path`, as a formatter at landing does.
    /// A branch here is one commit on its base, so the new commit holds the branch's earlier
    /// edits as well and replaces that commit as the branch's head.
    pub fn commit_on(&self, path: &Path, changes: &[(&str, &str)]) -> Commit {
        let mut repo = self.repo.lock().unwrap();
        let base = repo.base[path].clone();
        let mut all = repo.edits[path].clone();
        all.extend(edits(changes));
        let head = repo.commit(Some(&base), &all);
        repo.edits.insert(path.to_path_buf(), all);
        repo.checked_out.insert(path.to_path_buf(), head.clone());
        head
    }

    pub fn tip_now(&self) -> Commit {
        self.repo.lock().unwrap().tip()
    }

    pub fn parent_of(&self, commit: &Commit) -> Option<Commit> {
        self.repo.lock().unwrap().parent.get(commit).cloned()
    }

    pub fn pushes(&self) -> Vec<(PathBuf, Commit)> {
        self.repo.lock().unwrap().pushes.clone()
    }
}

impl Git for TestGit {
    fn object_ids(&self, tree: &Path, paths: &[String]) -> Result<Vec<(String, Oid)>> {
        let repo = self.repo.lock().unwrap();
        let files = &repo.files[&repo.checked_out[tree]];
        Ok(paths
            .iter()
            .filter_map(|p| files.get(p).map(|oid| (p.clone(), oid.clone())))
            .collect())
    }

    fn changed(&self, from: &Commit, to: &Commit) -> Result<Vec<String>> {
        let repo = self.repo.lock().unwrap();
        let (a, b) = (&repo.files[from], &repo.files[to]);
        let mut paths: Vec<String> = a.keys().chain(b.keys()).cloned().collect();
        paths.sort();
        paths.dedup();
        Ok(paths.into_iter().filter(|p| a.get(p) != b.get(p)).collect())
    }

    fn rebase_onto(&self, tree: &Path, tip: &Commit) -> Result<Rebase> {
        let mut repo = self.repo.lock().unwrap();
        if let Some(files) = repo.conflicts.get(tree) {
            return Ok(Rebase::Conflict {
                files: files.clone(),
            });
        }
        let old = repo.base[tree].clone();
        let position = |c: &Commit| repo.history.iter().position(|h| h == c).unwrap();
        let landed_since = u32::try_from(position(tip) - position(&old)).unwrap();
        let head = if old == *tip {
            repo.checked_out[tree].clone()
        } else if repo.history.contains(&repo.checked_out[tree]) {
            // The branch's commits are on the integration branch already, so the rebase
            // fast-forwards it to the tip and leaves nothing to land, as git does.
            tip.clone()
        } else {
            let changes = repo.edits[tree].clone();
            repo.commit(Some(tip), &changes)
        };
        repo.base.insert(tree.to_path_buf(), tip.clone());
        repo.checked_out.insert(tree.to_path_buf(), head.clone());
        Ok(Rebase::Clean {
            onto: tip.clone(),
            head,
            landed_since,
        })
    }

    fn push(&self, tree: &Path, commit: &Commit, branch: &str) -> Result<Push> {
        if branch != "staging" {
            let hook = self.on_branch_push.lock().unwrap().take();
            if let Some(hook) = hook {
                hook();
            }
            if self.other_branch_rejected.load(Ordering::SeqCst) {
                return Ok(Push::Rejected {
                    reason: "[rejected] (non-fast-forward)".to_owned(),
                });
            }
            let mut repo = self.repo.lock().unwrap();
            repo.branch_pushes.push((branch.to_owned(), commit.clone()));
            return Ok(Push::Pushed {
                commit: commit.clone(),
            });
        }
        if self.push_unreachable.load(Ordering::SeqCst) {
            return Err(Error::Unreachable("remote down".to_owned()));
        }
        if self.push_refused.load(Ordering::SeqCst) {
            return Err(Error::Conflict(
                "the remote refused the push to staging: [remote rejected] (hook declined)"
                    .to_owned(),
            ));
        }
        let before = self.landed_before_push.lock().unwrap().pop_front();
        if let Some(changes) = before {
            self.land_other(&changes);
        }
        let mut repo = self.repo.lock().unwrap();
        if self.push_always_rejected.load(Ordering::SeqCst)
            || repo.parent.get(commit) != Some(&repo.tip())
        {
            return Ok(Push::Rejected {
                reason: "[rejected] (fetch first)".to_owned(),
            });
        }
        repo.history.push(commit.clone());
        repo.pushes.push((tree.to_path_buf(), commit.clone()));
        if self.push_lands_then_fails.swap(false, Ordering::SeqCst) {
            return Err(Error::Unreachable("connection reset".to_owned()));
        }
        Ok(Push::Pushed {
            commit: commit.clone(),
        })
    }

    fn tip(&self, branch: &str) -> Result<Commit> {
        let held = self.repo.lock().unwrap().other_tips.get(branch).cloned();
        Ok(held.unwrap_or_else(|| self.tip_now()))
    }

    fn is_ancestor(&self, ancestor: &Commit, descendant: &Commit) -> Result<bool> {
        // The integration branch is linear, so order in its history is ancestry.
        let repo = self.repo.lock().unwrap();
        let position = |c: &Commit| repo.history.iter().position(|h| h == c);
        Ok(matches!(
            (position(ancestor), position(descendant)),
            (Some(a), Some(d)) if a <= d
        ))
    }

    fn holds_changes(&self, history: &Commit, version: &Commit, _path: &str) -> Result<bool> {
        Ok(self.copies_hold.load(Ordering::SeqCst) || self.is_ancestor(version, history)?)
    }

    fn replace(
        &self,
        _tree: &Path,
        commit: &Commit,
        branch: &str,
        expected: &Commit,
    ) -> Result<Push> {
        let mut repo = self.repo.lock().unwrap();
        repo.replaces
            .push((branch.to_owned(), commit.clone(), expected.clone()));
        Ok(Push::Pushed {
            commit: commit.clone(),
        })
    }

    fn replay(&self, _repo: &Path, head: &Commit, tip: &Commit) -> Result<Rebase> {
        Ok(self.repo.lock().unwrap().replay(head, tip))
    }

    fn merge_base(&self, _repo: &Path, a: &Commit, b: &Commit) -> Result<Commit> {
        Ok(self.repo.lock().unwrap().merge_base(a, b))
    }

    fn committed_at(&self, _repo: &Path, commit: &Commit) -> Result<Timestamp> {
        let repo = self.repo.lock().unwrap();
        let at = repo.committed.get(commit).copied();
        at.ok_or_else(|| Error::NotFound(commit.to_string()))
    }

    fn tree_id(&self, tree: &Path) -> Result<String> {
        Ok(self.repo.lock().unwrap().tree_id(tree))
    }
}

/// Ticket trees in a map, and free landing trees in a list.
#[derive(Default)]
pub struct TestTrees {
    pub git: Arc<TestGit>,
    pub tickets: Mutex<HashMap<TreeId, TicketTree>>,
    pub branches: Mutex<HashMap<PathBuf, String>>,
    pub uncommitted: Mutex<HashMap<PathBuf, Vec<String>>>,
    pub landing: Mutex<Vec<GateTree>>,
    /// The landing trees leased, by holder: a holder asking again gets its tree back, as a
    /// landing's handle does from the machine's trees.
    pub held: Mutex<Vec<(String, GateTree)>>,
    /// Every landing tree taken from the free list, in order.
    pub leased: Mutex<Vec<TreeId>>,
    pub released: Mutex<Vec<TreeId>>,
    pub deleted: Mutex<Vec<TicketId>>,
    /// What [`LandTrees::ticket_builds_less`] answers.
    pub ticket_builds_less: std::sync::atomic::AtomicBool,
}

impl LandTrees for TestTrees {
    fn ticket_tree(&self, tree: &TreeId) -> Result<Option<TicketTree>> {
        Ok(self.tickets.lock().unwrap().get(tree).cloned())
    }
    fn branch(&self, path: &Path) -> Result<Option<String>> {
        if let Some(branch) = self.branches.lock().unwrap().get(path) {
            return Ok(Some(branch.clone()));
        }
        let tickets = self.tickets.lock().unwrap();
        Ok(tickets
            .values()
            .find(|t| t.path == path)
            .and_then(|t| t.branch.clone()))
    }
    fn head(&self, path: &Path) -> Result<Commit> {
        Ok(self.git.repo.lock().unwrap().checked_out[path].clone())
    }
    fn uncommitted(&self, path: &Path) -> Result<Vec<String>> {
        let uncommitted = self.uncommitted.lock().unwrap();
        Ok(uncommitted.get(path).cloned().unwrap_or_default())
    }
    fn commits(&self, _path: &Path, _from: &Commit, to: &Commit) -> Result<Vec<Commit>> {
        Ok(vec![to.clone()])
    }
    fn commit_paths(
        &self,
        _path: &Path,
        from: &Commit,
        to: &Commit,
    ) -> Result<Vec<ignite_git::changed::CommitPaths>> {
        self.git.commit_paths(from, to)
    }
    fn subjects(&self, _path: &Path, _from: &Commit, to: &Commit) -> Result<Vec<String>> {
        Ok(vec![format!("subject of {to}")])
    }
    fn is_ancestor(&self, _path: &Path, ancestor: &Commit, _descendant: &Commit) -> Result<bool> {
        // The integration branch is linear, so its history is the commits that reached it.
        Ok(self.git.repo.lock().unwrap().history.contains(ancestor))
    }
    fn lease_landing_keeping(
        &self,
        holder: &str,
        at: &Commit,
        keep_free: usize,
    ) -> Result<Option<GateTree>> {
        let mut held = self.held.lock().unwrap();
        let kept = held
            .iter()
            .find(|(h, _)| h == holder)
            .map(|(_, t)| t.clone());
        let tree = kept.or_else(|| {
            let mut free = self.landing.lock().unwrap();
            if free.len() <= keep_free {
                return None;
            }
            let tree = free.pop();
            if let Some(tree) = &tree {
                held.push((holder.to_owned(), tree.clone()));
                self.leased.lock().unwrap().push(tree.tree.clone());
            }
            tree
        });
        if let Some(tree) = &tree {
            self.git.check_out(&tree.path, at);
        }
        Ok(tree)
    }
    fn release_landing(&self, tree: &TreeId, _built_at: Option<&Commit>) -> Result<()> {
        self.held.lock().unwrap().retain(|(_, t)| t.tree != *tree);
        self.landing.lock().unwrap().push(GateTree {
            tree: tree.clone(),
            // No machine has this root. On Windows a path with no drive is read on the current
            // drive, so `/trees/...` once found a real tree's `ignite.toml` there.
            path: PathBuf::from(format!("/ignite-test-trees/{tree}")),
        });
        Ok(())
    }
    fn ticket_builds_less(
        &self,
        _ticket: &TreeId,
        _built: Option<&Commit>,
        _head: &Commit,
        _change: &[String],
    ) -> Result<bool> {
        Ok(self
            .ticket_builds_less
            .load(std::sync::atomic::Ordering::SeqCst))
    }
    fn release_ticket(&self, tree: &TreeId, _park_at: &Commit, _caller: Option<u32>) -> Result<()> {
        self.tickets.lock().unwrap().remove(tree);
        self.released.lock().unwrap().push(tree.clone());
        Ok(())
    }
    fn delete_saved(&self, ticket: &TicketId) -> Result<()> {
        self.deleted.lock().unwrap().push(ticket.clone());
        Ok(())
    }
}

pub type Hook = Box<dyn FnOnce() + Send>;

/// A gate over fixed checks, each reading the files named. A check whose inputs' object IDs
/// have a cached pass is a hit; any other runs, passing unless it is in `failing`.
#[derive(Default)]
pub struct CacheGate {
    pub checks: Vec<(&'static str, Vec<&'static str>)>,
    pub failing: Mutex<HashSet<String>>,
    /// The checks each pass ran, in order.
    pub passes: Mutex<Vec<Vec<String>>>,
    /// Run at the start of each pass, first queued first: what other landings do meanwhile.
    pub hooks: Mutex<VecDeque<Hook>>,
    /// Checks that only `run_family` knows: the gate's own run does not include them.
    pub family_only: Vec<&'static str>,
    /// The families `run_family` was asked to run.
    pub family_runs: Mutex<Vec<String>>,
    /// The families each gate judged, in order, and the tree it ran in.
    pub judged: Mutex<Vec<(PathBuf, Vec<String>)>>,
    /// Checks the gate leaves to tip verification instead of running them.
    pub left: Vec<&'static str>,
    /// Families the gate does not list, as if listing them needed a product the tree has
    /// never built.
    pub cold: Vec<&'static str>,
    /// Run at the next `run_family`, before it runs.
    pub on_family: Mutex<Option<Hook>>,
    /// Checks that fail in a tree where a file holds the given text: check, path, text.
    pub broken_by: Mutex<Vec<(&'static str, &'static str, &'static str)>>,
    /// The checks each `run_checks` ran, not answered from the cache, with the tree it ran in.
    pub checks_runs: Mutex<Vec<(PathBuf, Vec<String>)>>,
    pub on_historical_check: Mutex<Option<Hook>>,
}

impl LandGate for CacheGate {
    fn run_family(
        &self,
        _ctx: &Context,
        _tree: &GateTree,
        family: &str,
        _options: &RunOptions,
    ) -> CheckRun {
        let hook = self.on_family.lock().unwrap().take();
        if let Some(hook) = hook {
            hook();
        }
        self.family_run(family)
    }

    /// Each check's family is the part of its name before the colon; every family is in scope.
    /// A family `carried` names, whose checks read none of its moved files, is carried over.
    /// A check with an open regression is left out as known to fail unless the landing's own
    /// ticket holds it, switched off or not, as the real gate does.
    fn run(
        &self,
        ctx: &Context,
        tree: &GateTree,
        options: &RunOptions,
        carried: Option<&Carried>,
        excused: &Excused,
    ) -> GateRun {
        let mut scoped: Vec<String> = Vec::new();
        for (name, _) in &self.checks {
            let family = CheckName::new(*name).unwrap().family().to_owned();
            if !scoped.contains(&family) {
                scoped.push(family);
            }
        }
        let carry = |family: &str| {
            carried.is_some_and(|c| {
                c.families.iter().any(|f| f == family)
                    && !self.checks.iter().any(|(name, paths)| {
                        CheckName::new(*name).unwrap().family() == family
                            && paths.iter().any(|p| c.moved.iter().any(|m| m == p))
                    })
            })
        };
        let (carried, judged): (Vec<String>, Vec<String>) =
            scoped.iter().cloned().partition(|f| carry(f));
        if judged.is_empty() && !carried.is_empty() {
            return GateRun {
                scoped,
                carried,
                ..GateRun::default()
            };
        }
        self.judged
            .lock()
            .unwrap()
            .push((tree.path.clone(), judged.clone()));
        let hook = self.hooks.lock().unwrap().pop_front();
        if let Some(hook) = hook {
            hook();
        }
        let mut known = excused.clone();
        known.extend(
            ctx.hub
                .open_regressions()
                .unwrap_or_default()
                .into_iter()
                .filter(|regression| options.ticket.as_ref() != Some(&regression.ticket))
                .map(|regression| (regression.check, regression.ticket)),
        );
        let keys: Vec<ResultKey> = self
            .checks
            .iter()
            .filter(|(name, _)| {
                let family = CheckName::new(*name).unwrap().family().to_owned();
                judged.contains(&family)
                    && !known.contains_key(&CheckName::new(*name).unwrap())
                    && !self.left.contains(name)
                    && !self.cold.contains(&family.as_str())
            })
            .map(|(name, paths)| key(ctx, &tree.path, name, paths))
            .collect();
        let hits = ctx.hub.lookup(&keys).unwrap();
        let mut run = CheckRun::default();
        let mut ran = Vec::new();
        for (key, hit) in keys.into_iter().zip(hits) {
            if hit.is_some_and(|h| h.outcome == Outcome::Pass) {
                run.from_cache += 1;
                run.outcomes.push((key.check, Outcome::Pass));
                continue;
            }
            run.ran += 1;
            ran.push(key.check.to_string());
            if self.fails(ctx, &tree.path, key.check.as_str()) {
                run.failures.push(Failure {
                    check: key.check.clone(),
                    outcome: Outcome::Fail,
                    at: Some(format!("{}/src/a.rs:3", tree.path.display())),
                    message: Some("left: 1 right: 2".to_owned()),
                    output: None,
                    reason: None,
                });
                run.outcomes.push((key.check, Outcome::Fail));
            } else {
                store(ctx, &key, Outcome::Pass);
                run.outcomes.push((key.check, Outcome::Pass));
            }
        }
        self.passes.lock().unwrap().push(ran);
        let mut gated = GateRun {
            run,
            scoped,
            carried,
            known_failing: known.into_iter().collect(),
            ..GateRun::default()
        };
        self.leave(&judged, &mut gated);
        gated
    }

    /// Answers each check from a cached result for its key in `tree`, or runs it and caches
    /// what it did.
    fn run_checks(
        &self,
        ctx: &Context,
        tree: &GateTree,
        checks: &[CheckName],
        options: &RunOptions,
    ) -> Vec<CheckVerdict> {
        if options.verb == crate::land::attribution::NAME {
            let hook = self.on_historical_check.lock().unwrap().take();
            if let Some(hook) = hook {
                hook();
            }
        }
        let mut verdicts = Vec::new();
        let mut ran = Vec::new();
        for (name, paths) in &self.checks {
            if !checks.iter().any(|c| c.as_str() == *name) {
                continue;
            }
            let key = key(ctx, &tree.path, name, paths);
            let hits = ctx.hub.lookup(std::slice::from_ref(&key)).unwrap();
            let outcome = if let Some(hit) = &hits[0] {
                hit.outcome
            } else {
                ran.push((*name).to_owned());
                let outcome = if self.broken(ctx, &tree.path, name) {
                    Outcome::Fail
                } else {
                    Outcome::Pass
                };
                store(ctx, &key, outcome);
                outcome
            };
            verdicts.push(CheckVerdict {
                check: key.check,
                outcome,
                platform: None,
                inputs: paths.iter().map(|path| (*path).to_owned()).collect(),
                output: format!("{name} at {}", tree.path.display()),
            });
        }
        self.checks_runs
            .lock()
            .unwrap()
            .push((tree.path.clone(), ran));
        verdicts
    }
}

/// The key of `name`, which reads `paths`, in the tree at `path`.
pub fn key(ctx: &Context, path: &Path, name: &str, paths: &[&str]) -> ResultKey {
    let paths: Vec<String> = paths.iter().map(|p| (*p).to_owned()).collect();
    let ids = ctx.git.object_ids(path, &paths).unwrap();
    ResultKey {
        check: CheckName::new(name).unwrap(),
        hash: InputHash::new(format!("{}{}", hex(&format!("{ids:?}")), "0".repeat(24))).unwrap(),
        platform: None,
    }
}

/// Stores `outcome` for `key` in the hub's result cache.
pub fn store(ctx: &Context, key: &ResultKey, outcome: Outcome) {
    let new = NewResult {
        key: key.clone(),
        outcome,
        machine: ctx.machine.clone(),
        record: RecordId(0),
        output: None,
        cost: ignite_core::protocol::hub::ResultCost::default(),
        areas: Vec::new(),
    };
    ctx.hub.store(&[new]).unwrap();
}

impl CacheGate {
    /// Whether `name` fails in the gate's run in the tree at `path`: it is in `failing`, or
    /// [`CacheGate::broken`].
    fn fails(&self, ctx: &Context, path: &Path, name: &str) -> bool {
        self.failing.lock().unwrap().contains(name) || self.broken(ctx, path, name)
    }

    /// Whether a file `broken_by` names for `name` holds the text given in the tree at `path`.
    /// Only this decides `run_checks`, which the landing runs at the tip: a check in `failing`
    /// is one the change breaks, so it passes there.
    fn broken(&self, ctx: &Context, path: &Path, name: &str) -> bool {
        self.broken_by
            .lock()
            .unwrap()
            .iter()
            .any(|(check, file, text)| {
                *check == name
                    && ctx
                        .git
                        .object_ids(path, &[(*file).to_owned()])
                        .unwrap()
                        .iter()
                        .any(|(_, oid)| oid.as_str() == hex(text))
            })
    }

    /// Records the checks in `left` and the families in `cold` among `judged` as left to tip
    /// verification.
    fn leave(&self, judged: &[String], gated: &mut GateRun) {
        for name in &self.left {
            let family = CheckName::new(*name).unwrap().family().to_owned();
            if judged.contains(&family) {
                *gated.left_by_family.entry(family).or_insert(0) += 1;
                gated.left += 1;
            }
        }
        gated.run.cold_families = self
            .cold
            .iter()
            .filter(|family| judged.iter().any(|f| f == *family))
            .map(|family| (*family).to_owned())
            .collect();
    }

    /// A pass of one family's checks, which fails the ones in `failing`.
    fn family_run(&self, family: &str) -> CheckRun {
        self.family_runs.lock().unwrap().push(family.to_owned());
        let mut run = CheckRun::default();
        let names = self
            .checks
            .iter()
            .map(|(name, _)| name)
            .chain(&self.family_only);
        for name in names {
            let check = CheckName::new(*name).unwrap();
            if check.family() != family {
                continue;
            }
            run.ran += 1;
            if self.failing.lock().unwrap().contains(*name) {
                run.failures.push(Failure {
                    check: check.clone(),
                    outcome: Outcome::Fail,
                    at: None,
                    message: None,
                    output: None,
                    reason: None,
                });
                run.outcomes.push((check, Outcome::Fail));
            } else {
                run.outcomes.push((check, Outcome::Pass));
            }
        }
        run
    }
}
