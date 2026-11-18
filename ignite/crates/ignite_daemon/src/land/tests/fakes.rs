//! The fakes the landing tests use: a repository held in memory ([`TestGit`]), a hub whose
//! leases expire by the fake clock ([`LeaseHub`]), build trees in maps ([`TestTrees`]) and a
//! gate that keys each check by its inputs' object IDs and keeps results in the hub's cache
//! ([`CacheGate`]), as the real gate does.

use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use ignite_core::fakes::{FakeClock, FakeHub};
use ignite_core::ids::{CheckName, Commit, InputHash, Oid, RecordId, Seq, TicketId, TreeId};
use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::{
    CachedResult, Claim, FlakyReport, Landed, MachineFree, MetricsImport, NewResult, Record,
    RegressionFiled, RegressionReport, Reservation, ResultKey, TicketBrief, TipClaim,
    TipClaimReply, TipVerified,
};
use ignite_core::protocol::tickets::{FeedBatch, Lease, LeaseBusy, LeaseOp, LeaseReply};
use ignite_core::time::Timestamp;
use ignite_core::traits::{Clock, Error, Git, Hub, Push, Rebase, Result};

use crate::checks::{CheckRun, Failure, GateRun, RunOptions};
use crate::land::env::{GateTree, LandGate, LandTrees, TicketTree};
use crate::server::Context;

pub fn hex(text: &str) -> String {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    format!("{:016x}", hasher.finish()).repeat(3)[..40].to_owned()
}

/// A repository in maps: each commit's files, the integration branch's history, and what each
/// tree has checked out. A ticket tree's branch is its base commit plus its own edits.
#[derive(Default)]
pub struct Repo {
    pub next: u64,
    pub files: HashMap<Commit, BTreeMap<String, Oid>>,
    pub parent: HashMap<Commit, Commit>,
    pub history: Vec<Commit>,
    pub checked_out: HashMap<PathBuf, Commit>,
    pub base: HashMap<PathBuf, Commit>,
    pub edits: HashMap<PathBuf, BTreeMap<String, Oid>>,
    pub conflicts: HashMap<PathBuf, Vec<String>>,
    pub pushes: Vec<(PathBuf, Commit)>,
}

#[derive(Default)]
pub struct TestGit {
    pub repo: Mutex<Repo>,
    pub push_unreachable: AtomicBool,
}

impl Repo {
    pub fn tip(&self) -> Commit {
        self.history.last().cloned().expect("a root commit")
    }

    pub fn commit(&mut self, parent: Option<&Commit>, edits: &BTreeMap<String, Oid>) -> Commit {
        self.next += 1;
        let commit = Commit::new(format!("{:040x}", self.next)).unwrap();
        let mut files = parent.map(|p| self.files[p].clone()).unwrap_or_default();
        files.extend(edits.clone());
        self.files.insert(commit.clone(), files);
        if let Some(parent) = parent {
            self.parent.insert(commit.clone(), parent.clone());
        }
        commit
    }
}

pub fn edits(pairs: &[(&str, &str)]) -> BTreeMap<String, Oid> {
    pairs
        .iter()
        .map(|(path, text)| ((*path).to_owned(), Oid::new(hex(text)).unwrap()))
        .collect()
}

impl TestGit {
    pub fn new(files: &[(&str, &str)]) -> Self {
        let git = TestGit::default();
        let mut repo = git.repo.lock().unwrap();
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

    fn push(&self, tree: &Path, commit: &Commit, _branch: &str) -> Result<Push> {
        if self.push_unreachable.load(Ordering::SeqCst) {
            return Err(Error::Unreachable("remote down".to_owned()));
        }
        let mut repo = self.repo.lock().unwrap();
        if repo.parent.get(commit) != Some(&repo.tip()) {
            return Ok(Push::Rejected {
                reason: "[rejected] (fetch first)".to_owned(),
            });
        }
        repo.history.push(commit.clone());
        repo.pushes.push((tree.to_path_buf(), commit.clone()));
        Ok(Push::Pushed {
            commit: commit.clone(),
        })
    }

    fn tip(&self, _branch: &str) -> Result<Commit> {
        Ok(self.tip_now())
    }
}

/// The fake hub, with leases that expire by the clock. `landed_down` makes only `POST /landed`
/// fail as an outage would.
pub struct LeaseHub {
    pub hub: FakeHub,
    pub clock: Arc<FakeClock>,
    pub leases: Mutex<HashMap<String, (String, Timestamp)>>,
    pub landed_down: AtomicBool,
}

impl LeaseHub {
    pub fn holder(&self, name: &str) -> Option<String> {
        let leases = self.leases.lock().unwrap();
        let now = self.clock.now();
        leases
            .get(name)
            .filter(|(_, expires)| *expires > now)
            .map(|(holder, _)| holder.clone())
    }
}

impl Hub for LeaseHub {
    fn lookup(&self, keys: &[ResultKey]) -> Result<Vec<Option<CachedResult>>> {
        self.hub.lookup(keys)
    }
    fn store(&self, results: &[NewResult]) -> Result<()> {
        self.hub.store(results)
    }
    fn next(&self, free: &MachineFree) -> Result<Option<Reservation>> {
        self.hub.next(free)
    }
    fn claim(&self, claim: &Claim) -> Result<TicketBrief> {
        self.hub.claim(claim)
    }
    fn landed(&self, landed: &Landed) -> Result<()> {
        if self.landed_down.load(Ordering::SeqCst) {
            return Err(Error::Unreachable("hub down".to_owned()));
        }
        self.hub.landed(landed)
    }
    fn lease(&self, name: &str, op: LeaseOp) -> Result<LeaseReply> {
        let now = self.clock.now();
        let mut leases = self.leases.lock().unwrap();
        let live = leases
            .get(name)
            .filter(|(_, expires)| *expires > now)
            .cloned();
        match op {
            LeaseOp::Acquire { holder, ttl_s, .. } | LeaseOp::Renew { holder, ttl_s, .. } => {
                if let Some((other, expires)) = live.filter(|(h, _)| *h != holder) {
                    return Ok(LeaseReply::Busy(LeaseBusy {
                        holder: Some(other),
                        expires_at: Some(expires),
                        position: None,
                        seq: None,
                    }));
                }
                #[allow(clippy::cast_precision_loss)]
                let expires_at = now.plus_seconds(ttl_s as f64);
                leases.insert(name.to_owned(), (holder.clone(), expires_at));
                Ok(LeaseReply::Held(Lease {
                    name: name.to_owned(),
                    holder,
                    machine: String::new(),
                    expires_at,
                    payload: serde_json::Value::Null,
                    waiters: Vec::new(),
                }))
            }
            LeaseOp::Release { holder, .. } => {
                if live.is_some_and(|(h, _)| h == holder) {
                    leases.remove(name);
                }
                Ok(LeaseReply::Busy(LeaseBusy {
                    holder: None,
                    expires_at: None,
                    position: None,
                    seq: None,
                }))
            }
        }
    }
    fn feed(&self, since: Seq, wait_s: u32) -> Result<FeedBatch> {
        self.hub.feed(since, wait_s)
    }
    fn records(&self, batch: &[Record]) -> Result<()> {
        self.hub.records(batch)
    }
    fn metrics(&self, import: &MetricsImport) -> Result<()> {
        self.hub.metrics(import)
    }
    fn flaky(&self, report: &FlakyReport) -> Result<()> {
        self.hub.flaky(report)
    }
    fn tip_claim(&self, claim: &TipClaim) -> Result<TipClaimReply> {
        self.hub.tip_claim(claim)
    }
    fn tip_verified(&self, verified: &TipVerified) -> Result<()> {
        self.hub.tip_verified(verified)
    }
    fn regression(&self, report: &RegressionReport) -> Result<RegressionFiled> {
        self.hub.regression(report)
    }
}

/// Ticket trees in a map, and free landing trees in a list.
pub struct TestTrees {
    pub git: Arc<TestGit>,
    pub tickets: Mutex<HashMap<TreeId, TicketTree>>,
    pub branches: Mutex<HashMap<PathBuf, String>>,
    pub uncommitted: Mutex<HashMap<PathBuf, Vec<String>>>,
    pub landing: Mutex<Vec<GateTree>>,
    pub released: Mutex<Vec<TreeId>>,
    pub deleted: Mutex<Vec<TicketId>>,
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
    fn uncommitted(&self, path: &Path) -> Result<Vec<String>> {
        let uncommitted = self.uncommitted.lock().unwrap();
        Ok(uncommitted.get(path).cloned().unwrap_or_default())
    }
    fn commits(&self, _path: &Path, _from: &Commit, to: &Commit) -> Result<Vec<Commit>> {
        Ok(vec![to.clone()])
    }
    fn lease_landing(&self, _holder: &str, at: &Commit) -> Result<Option<GateTree>> {
        let tree = self.landing.lock().unwrap().pop();
        if let Some(tree) = &tree {
            self.git.check_out(&tree.path, at);
        }
        Ok(tree)
    }
    fn release_landing(&self, tree: &TreeId, _built_at: Option<&Commit>) -> Result<()> {
        self.landing.lock().unwrap().push(GateTree {
            tree: tree.clone(),
            path: PathBuf::from(format!("/trees/{tree}")),
        });
        Ok(())
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
}

impl LandGate for CacheGate {
    fn run(&self, ctx: &Context, tree: &GateTree, _options: &RunOptions) -> GateRun {
        let hook = self.hooks.lock().unwrap().pop_front();
        if let Some(hook) = hook {
            hook();
        }
        let keys: Vec<ResultKey> = self
            .checks
            .iter()
            .map(|(name, paths)| {
                let paths: Vec<String> = paths.iter().map(|p| (*p).to_owned()).collect();
                let ids = ctx.git.object_ids(&tree.path, &paths).unwrap();
                ResultKey {
                    check: CheckName::new(*name).unwrap(),
                    hash: InputHash::new(format!("{}{}", hex(&format!("{ids:?}")), "0".repeat(24)))
                        .unwrap(),
                    platform: None,
                }
            })
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
            if self.failing.lock().unwrap().contains(key.check.as_str()) {
                run.failures.push(Failure {
                    check: key.check.clone(),
                    outcome: Outcome::Fail,
                    at: Some(format!("{}/src/a.rs:3", tree.path.display())),
                    message: Some("left: 1 right: 2".to_owned()),
                    output: None,
                });
                run.outcomes.push((key.check, Outcome::Fail));
            } else {
                let new = NewResult {
                    key: key.clone(),
                    outcome: Outcome::Pass,
                    machine: ctx.machine.clone(),
                    record: RecordId(0),
                    output: None,
                };
                ctx.hub.store(&[new]).unwrap();
                run.outcomes.push((key.check, Outcome::Pass));
            }
        }
        self.passes.lock().unwrap().push(ran);
        GateRun { run, left: 0 }
    }
}
