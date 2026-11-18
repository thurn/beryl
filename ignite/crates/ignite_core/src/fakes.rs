//! In-memory fakes of every trait in [`crate::traits`], for tests.
//!
//! Each fake records the calls it receives and answers from what the test scripted, with a
//! plain default when nothing was. They hold their state behind mutexes, because the traits take
//! `&self` and must be `Send + Sync`.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::ids::{Commit, Oid, Seq, TicketId, TreeId};
use crate::protocol::Usage;
use crate::protocol::hub::{
    AuditReport, CachedResult, Claim, ExplorationFind, FlakyReport, Landed, LandedCommit,
    MachineFree, MetricsImport, NewResult, Pause, Record, RegressionFiled, RegressionReport,
    ReportFiled, Reservation, ResultKey, TicketBrief, TipClaim, TipClaimReply, TipVerified,
    TipWaiting, TipWaitingFiled,
};
use crate::protocol::tickets::{
    BatchReply, BatchRequest, Comment, FeedBatch, FeedChange, Lease, LeaseBusy, LeaseOp,
    LeaseReply, Operation, OperationResult, Precondition, Ticket, TicketDetail, TicketQuery,
};
use crate::time::Timestamp;
use crate::traits::{
    BuildResult, Builds, Clock, CommandSpec, Error, Git, Hub, Launcher, Push, Rebase, Result,
    RunResult,
};

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A clock that moves only when the test moves it.
#[derive(Debug)]
pub struct FakeClock {
    now: Mutex<Timestamp>,
}

impl FakeClock {
    pub fn new(now: Timestamp) -> Self {
        Self {
            now: Mutex::new(now),
        }
    }

    pub fn set(&self, now: Timestamp) {
        *lock(&self.now) = now;
    }

    pub fn advance(&self, seconds: f64) {
        let mut now = lock(&self.now);
        *now = now.plus_seconds(seconds);
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Timestamp {
        *lock(&self.now)
    }
}

/// A launcher that runs nothing. A command whose arguments start with a scripted prefix gets
/// that script's result; any other exits 0 at no cost.
#[derive(Debug, Default)]
pub struct FakeLauncher {
    scripts: Mutex<Vec<(Vec<String>, RunResult)>>,
    runs: Mutex<Vec<CommandSpec>>,
}

impl FakeLauncher {
    pub fn script(&self, prefix: &[&str], result: RunResult) {
        let prefix = prefix.iter().map(|s| (*s).to_owned()).collect();
        lock(&self.scripts).push((prefix, result));
    }

    /// Every command run so far, in order.
    pub fn runs(&self) -> Vec<CommandSpec> {
        lock(&self.runs).clone()
    }
}

impl Launcher for FakeLauncher {
    fn run(&self, spec: &CommandSpec) -> RunResult {
        lock(&self.runs).push(spec.clone());
        let scripts = lock(&self.scripts);
        scripts
            .iter()
            .find(|(prefix, _)| spec.args.starts_with(prefix))
            .map_or_else(
                || RunResult {
                    exit: Some(0),
                    output: PathBuf::new(),
                    usage: Usage::default(),
                },
                |(_, result)| result.clone(),
            )
    }
}

/// Builds that succeed the first time a (tree, product) is asked for and are current after.
#[derive(Debug, Default)]
pub struct FakeBuilds {
    scripted: Mutex<HashMap<(TreeId, String), BuildResult>>,
    built: Mutex<Vec<(TreeId, String, Vec<String>)>>,
}

impl FakeBuilds {
    /// Answers every later request for `product` in `tree` with `result`.
    pub fn script(&self, tree: &TreeId, product: &str, result: BuildResult) {
        lock(&self.scripted).insert((tree.clone(), product.to_owned()), result);
    }

    /// Every request so far: tree, product and packages.
    pub fn requests(&self) -> Vec<(TreeId, String, Vec<String>)> {
        lock(&self.built).clone()
    }
}

impl Builds for FakeBuilds {
    fn ensure(&self, tree: &TreeId, product: &str, packages: &[String]) -> BuildResult {
        let mut built = lock(&self.built);
        let key = (tree.clone(), product.to_owned());
        if let Some(result) = lock(&self.scripted).get(&key) {
            built.push((key.0, key.1, packages.to_vec()));
            return result.clone();
        }
        let seen = built.iter().any(|(t, p, _)| *t == key.0 && *p == key.1);
        built.push((key.0, key.1, packages.to_vec()));
        if seen {
            BuildResult::Current
        } else {
            BuildResult::Built {
                usage: Usage::default(),
                units: Vec::new(),
            }
        }
    }
}

/// A repository held in maps: object IDs by path, changed paths by commit pair, branch tips.
#[derive(Debug, Default)]
pub struct FakeGit {
    objects: Mutex<BTreeMap<String, Oid>>,
    changes: Mutex<HashMap<(Commit, Commit), Vec<String>>>,
    tips: Mutex<HashMap<String, Commit>>,
    rebase: Mutex<Option<Rebase>>,
    pushes: Mutex<Vec<(PathBuf, Commit, String)>>,
}

impl FakeGit {
    pub fn set_object(&self, path: &str, oid: Oid) {
        lock(&self.objects).insert(path.to_owned(), oid);
    }

    pub fn set_changed(&self, from: &Commit, to: &Commit, paths: &[&str]) {
        let paths = paths.iter().map(|p| (*p).to_owned()).collect();
        lock(&self.changes).insert((from.clone(), to.clone()), paths);
    }

    pub fn set_tip(&self, branch: &str, commit: Commit) {
        lock(&self.tips).insert(branch.to_owned(), commit);
    }

    /// The next rebase's result; by default a rebase is clean onto the tip given.
    pub fn script_rebase(&self, result: Rebase) {
        *lock(&self.rebase) = Some(result);
    }

    /// Every push so far: tree, commit and branch.
    pub fn pushes(&self) -> Vec<(PathBuf, Commit, String)> {
        lock(&self.pushes).clone()
    }
}

impl Git for FakeGit {
    fn object_ids(&self, _tree: &Path, paths: &[String]) -> Result<Vec<(String, Oid)>> {
        let objects = lock(&self.objects);
        paths
            .iter()
            .map(|path| match objects.get(path) {
                Some(oid) => Ok((path.clone(), oid.clone())),
                None => Err(Error::NotFound(path.clone())),
            })
            .collect()
    }

    fn changed(&self, from: &Commit, to: &Commit) -> Result<Vec<String>> {
        Ok(lock(&self.changes)
            .get(&(from.clone(), to.clone()))
            .cloned()
            .unwrap_or_default())
    }

    fn rebase_onto(&self, _tree: &Path, tip: &Commit) -> Result<Rebase> {
        Ok(lock(&self.rebase).take().unwrap_or_else(|| Rebase::Clean {
            onto: tip.clone(),
            head: tip.clone(),
            landed_since: 0,
        }))
    }

    fn push(&self, tree: &Path, commit: &Commit, branch: &str) -> Result<Push> {
        lock(&self.pushes).push((tree.to_path_buf(), commit.clone(), branch.to_owned()));
        lock(&self.tips).insert(branch.to_owned(), commit.clone());
        Ok(Push::Pushed {
            commit: commit.clone(),
        })
    }

    fn tip(&self, branch: &str) -> Result<Commit> {
        lock(&self.tips)
            .get(branch)
            .cloned()
            .ok_or_else(|| Error::NotFound(branch.to_owned()))
    }
}

/// A hub held in memory. [`FakeHub::set_unreachable`] makes every call fail as a network outage
/// would.
#[derive(Debug, Default)]
pub struct FakeHub {
    unreachable: AtomicBool,
    results: Mutex<HashMap<ResultKey, CachedResult>>,
    reservations: Mutex<VecDeque<Reservation>>,
    briefs: Mutex<HashMap<TicketId, TicketBrief>>,
    claims: Mutex<Vec<Claim>>,
    landings: Mutex<Vec<Landed>>,
    leases: Mutex<HashMap<String, Lease>>,
    feed: Mutex<Vec<FeedChange>>,
    records: Mutex<Vec<Record>>,
    metrics: Mutex<Vec<MetricsImport>>,
    flaky: Mutex<Vec<FlakyReport>>,
    tip_claims: Mutex<Vec<TipClaim>>,
    tip_states: Mutex<Vec<TipVerified>>,
    regressions: Mutex<Vec<RegressionReport>>,
    tips_waiting: Mutex<Vec<TipWaiting>>,
    tickets: Mutex<BTreeMap<TicketId, TicketDetail>>,
    batches: Mutex<Vec<BatchRequest>>,
    pauses: Mutex<Vec<Pause>>,
    asked: Mutex<Vec<MachineFree>>,
    audits: Mutex<Vec<AuditReport>>,
    finds: Mutex<Vec<ExplorationFind>>,
}

impl FakeHub {
    /// Stores a ticket for `ticket`, `tickets` and `batch` to read and change.
    pub fn insert_ticket(&self, detail: TicketDetail) {
        lock(&self.tickets).insert(detail.ticket.id.clone(), detail);
    }

    /// Every batch applied so far, in order.
    pub fn batches(&self) -> Vec<BatchRequest> {
        lock(&self.batches).clone()
    }

    /// Every pause or resume sent so far, in order.
    pub fn pauses(&self) -> Vec<Pause> {
        lock(&self.pauses).clone()
    }

    /// What each call to `next` said the machine had free, in order.
    pub fn asked(&self) -> Vec<MachineFree> {
        lock(&self.asked).clone()
    }

    pub fn set_unreachable(&self, unreachable: bool) {
        self.unreachable.store(unreachable, Ordering::SeqCst);
    }

    /// Queues a reservation for the next call to `next`.
    pub fn queue_reservation(&self, reservation: Reservation) {
        lock(&self.reservations).push_back(reservation);
    }

    /// The brief that a claim of this ticket returns.
    pub fn set_brief(&self, brief: TicketBrief) {
        lock(&self.briefs).insert(brief.ticket.id.clone(), brief);
    }

    /// Appends a change to the feed, numbered after the last one.
    pub fn push_change(&self, kind: &str, ticket: Option<TicketId>) -> Seq {
        let mut feed = lock(&self.feed);
        let seq = Seq(feed.last().map_or(0, |c| c.seq.0) + 1);
        feed.push(FeedChange {
            seq,
            kind: kind.to_owned(),
            ticket,
            name: None,
        });
        seq
    }

    pub fn claims(&self) -> Vec<Claim> {
        lock(&self.claims).clone()
    }

    pub fn landings(&self) -> Vec<Landed> {
        lock(&self.landings).clone()
    }

    pub fn stored_records(&self) -> Vec<Record> {
        lock(&self.records).clone()
    }

    /// Every efficiency import sent so far, in order.
    pub fn metrics_imports(&self) -> Vec<MetricsImport> {
        lock(&self.metrics).clone()
    }

    pub fn flaky_reports(&self) -> Vec<FlakyReport> {
        lock(&self.flaky).clone()
    }

    /// Every tip claim granted so far, in order. A batch is granted to the first machine that
    /// claims it, and again to that machine; claims never expire here.
    pub fn tip_claims(&self) -> Vec<TipClaim> {
        lock(&self.tip_claims).clone()
    }

    /// Every tip state reported so far, in order.
    pub fn tip_states(&self) -> Vec<TipVerified> {
        lock(&self.tip_states).clone()
    }

    /// Every regression reported so far, in order.
    pub fn regressions(&self) -> Vec<RegressionReport> {
        lock(&self.regressions).clone()
    }

    /// Every waiting tip reported so far, in order.
    pub fn tips_waiting(&self) -> Vec<TipWaiting> {
        lock(&self.tips_waiting).clone()
    }

    pub fn audit_reports(&self) -> Vec<AuditReport> {
        lock(&self.audits).clone()
    }

    pub fn exploration_finds(&self) -> Vec<ExplorationFind> {
        lock(&self.finds).clone()
    }

    /// The ticket a report filed: one per report, numbered in the order they arrived.
    fn filed(number: usize) -> ReportFiled {
        ReportFiled {
            ticket: TicketId::new(format!("T.{number}")).unwrap_or_else(|_| unreachable!()),
        }
    }

    fn reachable(&self) -> Result<()> {
        if self.unreachable.load(Ordering::SeqCst) {
            Err(Error::Unreachable("fake hub is down".to_owned()))
        } else {
            Ok(())
        }
    }
}

impl Hub for FakeHub {
    fn lookup(&self, keys: &[ResultKey]) -> Result<Vec<Option<CachedResult>>> {
        self.reachable()?;
        let results = lock(&self.results);
        Ok(keys.iter().map(|key| results.get(key).cloned()).collect())
    }

    fn store(&self, new: &[NewResult]) -> Result<()> {
        self.reachable()?;
        let mut results = lock(&self.results);
        for result in new {
            let cached = CachedResult {
                key: result.key.clone(),
                outcome: result.outcome,
                machine: result.machine.clone(),
                record: result.record,
                at: Timestamp::EPOCH,
                output: result.output.clone(),
            };
            results.insert(result.key.clone(), cached);
        }
        Ok(())
    }

    fn next(&self, free: &MachineFree) -> Result<Option<Reservation>> {
        self.reachable()?;
        lock(&self.asked).push(free.clone());
        Ok(lock(&self.reservations).pop_front())
    }

    fn claim(&self, claim: &Claim) -> Result<TicketBrief> {
        self.reachable()?;
        let brief = lock(&self.briefs)
            .get(&claim.ticket)
            .cloned()
            .ok_or_else(|| Error::NotFound(claim.ticket.to_string()))?;
        lock(&self.claims).push(claim.clone());
        Ok(brief)
    }

    fn landed(&self, landed: &Landed) -> Result<()> {
        self.reachable()?;
        lock(&self.landings).push(landed.clone());
        Ok(())
    }

    /// Leases never expire here: an acquire succeeds when the lease is free or already the
    /// holder's, and a release by the holder frees it.
    fn lease(&self, name: &str, op: LeaseOp) -> Result<LeaseReply> {
        self.reachable()?;
        let mut leases = lock(&self.leases);
        let busy = |lease: &Lease| {
            LeaseReply::Busy(LeaseBusy {
                holder: Some(lease.holder.clone()),
                expires_at: Some(lease.expires_at),
                position: None,
                seq: None,
            })
        };
        match op {
            LeaseOp::Acquire {
                holder, payload, ..
            }
            | LeaseOp::Renew {
                holder, payload, ..
            } => {
                if let Some(lease) = leases.get(name)
                    && lease.holder != holder
                {
                    return Ok(busy(lease));
                }
                let lease = Lease {
                    name: name.to_owned(),
                    holder,
                    machine: String::new(),
                    expires_at: Timestamp::EPOCH,
                    payload: payload.unwrap_or_default(),
                    waiters: Vec::new(),
                };
                leases.insert(name.to_owned(), lease.clone());
                Ok(LeaseReply::Held(lease))
            }
            LeaseOp::Release { holder, .. } => match leases.get(name) {
                Some(lease) if lease.holder == holder => Ok(LeaseReply::Held(
                    leases.remove(name).unwrap_or_else(|| unreachable!()),
                )),
                Some(lease) => Ok(busy(lease)),
                None => Err(Error::NotFound(name.to_owned())),
            },
        }
    }

    fn feed(&self, since: Seq, _wait_s: u32) -> Result<FeedBatch> {
        self.reachable()?;
        let feed = lock(&self.feed);
        Ok(FeedBatch {
            seq: feed.last().map_or(Seq(0), |c| c.seq),
            changes: feed.iter().filter(|c| c.seq > since).cloned().collect(),
        })
    }

    fn records(&self, batch: &[Record]) -> Result<()> {
        self.reachable()?;
        lock(&self.records).extend_from_slice(batch);
        Ok(())
    }

    fn metrics(&self, import: &MetricsImport) -> Result<()> {
        self.reachable()?;
        lock(&self.metrics).push(import.clone());
        Ok(())
    }

    fn flaky(&self, report: &FlakyReport) -> Result<()> {
        self.reachable()?;
        lock(&self.flaky).push(report.clone());
        Ok(())
    }

    fn tip_claim(&self, claim: &TipClaim) -> Result<TipClaimReply> {
        self.reachable()?;
        let mut claims = lock(&self.tip_claims);
        let holder = claims.iter().find(|c| {
            c.commit == claim.commit && c.family == claim.family && c.batch == claim.batch
        });
        let granted = holder.is_none_or(|c| c.machine == claim.machine);
        if granted && holder.is_none() {
            claims.push(claim.clone());
        }
        Ok(TipClaimReply {
            granted,
            expires_at: None,
        })
    }

    fn tip_verified(&self, verified: &TipVerified) -> Result<()> {
        self.reachable()?;
        lock(&self.tip_states).push(verified.clone());
        Ok(())
    }

    fn regression(&self, report: &RegressionReport) -> Result<RegressionFiled> {
        self.reachable()?;
        let mut regressions = lock(&self.regressions);
        regressions.push(report.clone());
        let ticket = TicketId::new(format!("tk.{}", regressions.len()))
            .map_err(|e| Error::Invalid(e.to_string()))?;
        Ok(RegressionFiled { ticket })
    }

    fn ticket(&self, id: &TicketId) -> Result<TicketDetail> {
        self.reachable()?;
        lock(&self.tickets)
            .get(id)
            .cloned()
            .ok_or_else(|| Error::NotFound(id.to_string()))
    }

    /// Filters by status and assignee only.
    fn tickets(&self, query: &TicketQuery) -> Result<Vec<Ticket>> {
        self.reachable()?;
        Ok(lock(&self.tickets)
            .values()
            .map(|detail| &detail.ticket)
            .filter(|t| query.status.is_none_or(|status| t.status == status))
            .filter(|t| query.assignee.is_none() || t.assignee == query.assignee)
            .cloned()
            .collect())
    }

    /// Checks `version` preconditions and applies comments and metadata updates; any other
    /// operation or precondition is refused as invalid. Each changed ticket gets a new version
    /// and a feed change.
    fn batch(&self, request: &BatchRequest) -> Result<BatchReply> {
        self.reachable()?;
        let mut tickets = lock(&self.tickets);
        let mut changed = tickets.clone();
        for precondition in &request.preconditions {
            let Precondition::Version { id, version } = precondition else {
                return Err(Error::Invalid("fake hub checks only versions".to_owned()));
            };
            let current = changed
                .get(id)
                .ok_or_else(|| Error::NotFound(id.to_string()))?;
            if current.ticket.version != *version {
                return Err(Error::Conflict(format!("{id} has changed")));
            }
        }
        let mut results = Vec::new();
        for operation in &request.ops {
            let (Operation::Comment { id, .. } | Operation::Update { id, .. }) = operation else {
                return Err(Error::Invalid(
                    "fake hub applies only comments and updates".to_owned(),
                ));
            };
            let id = TicketId::new(id.clone()).map_err(|e| Error::Invalid(e.to_string()))?;
            let detail = changed
                .get_mut(&id)
                .ok_or_else(|| Error::NotFound(id.to_string()))?;
            match operation {
                Operation::Comment { text, .. } => {
                    let comments = detail.comments.get_or_insert_with(Vec::new);
                    let n = comments.iter().map(|c| c.n).max().unwrap_or(0) + 1;
                    comments.push(Comment {
                        ticket: id.clone(),
                        n,
                        author: "fake".to_owned(),
                        at: Timestamp::EPOCH,
                        text: text.clone(),
                        legacy_id: None,
                    });
                }
                Operation::Update { set_meta, .. } => {
                    detail.ticket.metadata.extend(set_meta.clone());
                }
                _ => unreachable!("refused above"),
            }
            results.push(OperationResult {
                id,
                closed: Vec::new(),
                ready: None,
            });
        }
        let mut seq = Seq(0);
        for result in &results {
            seq = self.push_change("ticket", Some(result.id.clone()));
            if let Some(detail) = changed.get_mut(&result.id) {
                detail.ticket.version = seq;
            }
        }
        *tickets = changed;
        lock(&self.batches).push(request.clone());
        Ok(BatchReply { seq, results })
    }

    fn pause(&self, pause: &Pause) -> Result<()> {
        self.reachable()?;
        lock(&self.pauses).push(pause.clone());
        Ok(())
    }

    fn audit(&self, report: &AuditReport) -> Result<ReportFiled> {
        self.reachable()?;
        let mut audits = lock(&self.audits);
        audits.push(report.clone());
        Ok(Self::filed(audits.len()))
    }

    fn find(&self, find: &ExplorationFind) -> Result<ReportFiled> {
        self.reachable()?;
        let mut finds = lock(&self.finds);
        finds.push(find.clone());
        Ok(Self::filed(finds.len()))
    }

    /// Answered from the landings recorded with [`Hub::landed`]; a later landing of the same
    /// commit wins.
    fn landed_commits(&self, commits: &[Commit]) -> Result<Vec<LandedCommit>> {
        self.reachable()?;
        let landings = lock(&self.landings);
        Ok(commits
            .iter()
            .filter_map(|commit| {
                landings
                    .iter()
                    .rev()
                    .find(|landed| landed.work.commits.contains(commit))
                    .map(|landed| LandedCommit {
                        commit: commit.clone(),
                        ticket: landed.ticket.clone(),
                    })
            })
            .collect())
    }

    fn tip_waiting(&self, waiting: &TipWaiting) -> Result<TipWaitingFiled> {
        self.reachable()?;
        let mut reports = lock(&self.tips_waiting);
        reports.push(waiting.clone());
        Ok(TipWaitingFiled {
            tickets: vec![Self::filed(reports.len()).ticket],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Os;
    use crate::ids::{CheckName, InputHash, MachineId, RecordId};
    use crate::protocol::Outcome;

    fn key() -> ResultKey {
        ResultKey {
            check: CheckName::new("rust:a::b").unwrap(),
            hash: InputHash::new("a".repeat(64)).unwrap(),
            platform: Some(Os::Macos),
        }
    }

    #[test]
    fn the_hub_caches_what_it_stores_and_fails_when_down() {
        let hub = FakeHub::default();
        assert_eq!(hub.lookup(&[key()]).unwrap(), vec![None]);
        let machine = MachineId::new("m-1").unwrap();
        let new = NewResult {
            key: key(),
            outcome: Outcome::Pass,
            machine,
            record: RecordId(1),
            output: None,
        };
        hub.store(&[new]).unwrap();
        assert_eq!(
            hub.lookup(&[key()]).unwrap()[0].as_ref().unwrap().outcome,
            Outcome::Pass
        );
        hub.set_unreachable(true);
        assert!(matches!(hub.lookup(&[key()]), Err(Error::Unreachable(_))));
    }

    #[test]
    fn leases_exclude_other_holders() {
        let hub = FakeHub::default();
        let acquire = |holder: &str| LeaseOp::Acquire {
            holder: holder.to_owned(),
            ttl_s: 60,
            payload: None,
            no_queue: true,
        };
        assert!(matches!(
            hub.lease("land", acquire("a")).unwrap(),
            LeaseReply::Held(_)
        ));
        assert!(matches!(
            hub.lease("land", acquire("b")).unwrap(),
            LeaseReply::Busy(_)
        ));
        let release = LeaseOp::Release {
            holder: "a".to_owned(),
            if_revision: None,
        };
        assert!(matches!(
            hub.lease("land", release).unwrap(),
            LeaseReply::Held(_)
        ));
        assert!(matches!(
            hub.lease("land", acquire("b")).unwrap(),
            LeaseReply::Held(_)
        ));
    }

    #[test]
    fn the_feed_returns_changes_after_since() {
        let hub = FakeHub::default();
        hub.push_change("ticket", None);
        let second = hub.push_change("ticket", None);
        let batch = hub.feed(Seq(1), 0).unwrap();
        assert_eq!(batch.seq, second);
        assert_eq!(batch.changes.len(), 1);
    }

    #[test]
    fn builds_are_built_once_then_current() {
        let builds = FakeBuilds::default();
        let tree = TreeId::new("tree-01").unwrap();
        assert!(matches!(
            builds.ensure(&tree, "tests", &[]),
            BuildResult::Built { .. }
        ));
        assert_eq!(builds.ensure(&tree, "tests", &[]), BuildResult::Current);
    }

    #[test]
    fn the_launcher_answers_scripted_prefixes() {
        let launcher = FakeLauncher::default();
        let failing = RunResult {
            exit: Some(2),
            output: PathBuf::from("out"),
            usage: Usage::default(),
        };
        launcher.script(&["cargo", "test"], failing);
        let spec = |args: &[&str]| CommandSpec {
            args: args.iter().map(|s| (*s).to_owned()).collect(),
            cwd: PathBuf::new(),
            env: Vec::new(),
            class: crate::protocol::Class::Agent,
            cores: 1,
            resources: Vec::new(),
            tree: None,
            ticket: None,
        };
        assert_eq!(
            launcher.run(&spec(&["cargo", "test", "-p", "x"])).exit,
            Some(2)
        );
        assert_eq!(launcher.run(&spec(&["cargo", "build"])).exit, Some(0));
        assert_eq!(launcher.runs().len(), 2);
    }

    #[test]
    fn git_pushes_move_the_tip() {
        let git = FakeGit::default();
        let commit = Commit::new("3a9e4c1").unwrap();
        git.push(Path::new("t"), &commit, "staging").unwrap();
        assert_eq!(git.tip("staging").unwrap(), commit);
        assert!(git.tip("master").is_err());
    }
}
