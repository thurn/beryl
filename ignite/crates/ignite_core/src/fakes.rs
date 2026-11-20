//! In-memory fakes of every trait in [`crate::traits`], for tests.
//!
//! Each fake records the calls it receives and answers from what the test scripted, with a
//! plain default when nothing was. They hold their state behind mutexes, because the traits take
//! `&self` and must be `Send + Sync`.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::ids::{Commit, RecordId, Seq, TicketId};
use crate::protocol::Usage;
use crate::protocol::hub::{
    AuditReport, CachedResult, Claim, ExplorationFind, FlakyReport, IncompleteReport,
    JusticiarReport, Landed, LandedCommit, ListingKey, MachineFree, MetricsImport, NewResult,
    OpenRegression, Pause, Record, RegressionFiled, RegressionReport, ReportFiled, Reservation,
    ResultKey, RevertFiled, Reverted, SessionTitle, StoredListing, TicketBrief, TipClaim,
    TipClaimReply, TipVerified, TipWaiting, TipWaitingFiled, Verdict,
};
use crate::protocol::tickets::{
    BatchReply, BatchRequest, Comment, FeedBatch, FeedChange, ImportReply, ImportRequest, Lease,
    LeaseBusy, LeaseOp, LeaseReply, Operation, OperationResult, Precondition, Ticket, TicketDetail,
    TicketQuery, WholeTicket,
};
use crate::time::Timestamp;
use crate::tipround::{TipCosts, TipCostsRequest, TipRound, TipRoundFiled};
use crate::traits::{Clock, CommandSpec, Error, Hub, Launcher, Result, RunResult};

mod builds;
mod fleet;
mod git;
mod host;
mod tips;
mod verdicts;
mod whole;
pub use builds::FakeBuilds;
pub use git::FakeGit;
pub use host::FakeHost;

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
    /// A clock each run moves on by the time its result says it waited and ran.
    clock: Mutex<Option<Arc<FakeClock>>>,
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

    /// Makes each run move `clock` on by its result's `queued_s` and `wall_s` before it
    /// returns, as a real command takes time while it runs.
    pub fn advance_clock(&self, clock: Arc<FakeClock>) {
        *lock(&self.clock) = Some(clock);
    }
}

impl Launcher for FakeLauncher {
    fn run(&self, spec: &CommandSpec) -> RunResult {
        lock(&self.runs).push(spec.clone());
        let result = lock(&self.scripts)
            .iter()
            .find(|(prefix, _)| spec.args.starts_with(prefix))
            .map_or_else(
                || RunResult {
                    exit: Some(0),
                    output: PathBuf::new(),
                    usage: Usage::default(),
                    hung: false,
                },
                |(_, result)| result.clone(),
            );
        if let Some(clock) = lock(&self.clock).as_ref() {
            clock.advance(result.usage.queued_s + result.usage.wall_s);
        }
        result
    }
}

/// A hub held in memory. [`FakeHub::set_unreachable`] makes every call fail as a network outage
/// would.
#[derive(Debug, Default)]
pub struct FakeHub {
    unreachable: AtomicBool,
    records_down: AtomicBool,
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
    session_titles: Mutex<Vec<SessionTitle>>,
    tip_claims: Mutex<Vec<TipClaim>>,
    /// Every tip claim asked, renewals included.
    tip_claim_asks: Mutex<Vec<TipClaim>>,
    tip_states: Mutex<Vec<TipVerified>>,
    regressions: Mutex<Vec<RegressionReport>>,
    /// The open regressions `open_regressions` answers; none set means the hub does not know the
    /// request (see `set_open_regressions`).
    open_regressions: Mutex<Option<Vec<OpenRegression>>>,
    tips_waiting: Mutex<Vec<TipWaiting>>,
    tickets: Mutex<BTreeMap<TicketId, TicketDetail>>,
    /// Whole tickets, for `export` and `import` (see `whole`).
    whole: Mutex<BTreeMap<TicketId, WholeTicket>>,
    /// The most tickets one export reply holds, as the real hub cuts a large page short; 0 for
    /// no cap beyond the page size.
    export_cap: Mutex<usize>,
    imports: Mutex<Vec<ImportRequest>>,
    batches: Mutex<Vec<BatchRequest>>,
    pauses: Mutex<Vec<Pause>>,
    /// The emergency, changed at [`Timestamp::EPOCH`] and ended by resuming every machine.
    emergency: Mutex<Option<crate::protocol::emergency::Emergency>>,
    asked: Mutex<Vec<MachineFree>>,
    audits: Mutex<Vec<AuditReport>>,
    finds: Mutex<Vec<ExplorationFind>>,
    justiciars: Mutex<Vec<JusticiarReport>>,
    incompletes: Mutex<Vec<IncompleteReport>>,
    listings: Mutex<HashMap<ListingKey, String>>,
    tips: tips::FakeTips,
    fleet: fleet::FakeFleet,
    verdicts: verdicts::FakeVerdicts,
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

    /// Makes `records` alone fail, as a hub that refuses telemetry does.
    pub fn set_records_down(&self, down: bool) {
        self.records_down.store(down, Ordering::SeqCst);
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

    /// Every session title reported so far, in order.
    pub fn session_titles(&self) -> Vec<SessionTitle> {
        lock(&self.session_titles).clone()
    }

    /// Every tip claim granted so far, in order. A batch is granted to the first machine that
    /// claims it, and again to that machine; claims never expire here.
    pub fn tip_claims(&self) -> Vec<TipClaim> {
        lock(&self.tip_claims).clone()
    }

    /// Every tip claim asked so far, in order, renewals of a claim already held included.
    pub fn tip_claim_asks(&self) -> Vec<TipClaim> {
        lock(&self.tip_claim_asks).clone()
    }

    /// Every tip state reported so far, in order.
    pub fn tip_states(&self) -> Vec<TipVerified> {
        lock(&self.tip_states).clone()
    }

    /// Sets what `open_regressions` answers.
    pub fn set_open_regressions(&self, open: Vec<OpenRegression>) {
        *lock(&self.open_regressions) = Some(open);
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

    /// Every incomplete-check report sent so far, in order.
    pub fn incomplete_reports(&self) -> Vec<IncompleteReport> {
        lock(&self.incompletes).clone()
    }

    pub fn exploration_finds(&self) -> Vec<ExplorationFind> {
        lock(&self.finds).clone()
    }

    pub fn justiciar_reports(&self) -> Vec<JusticiarReport> {
        lock(&self.justiciars).clone()
    }

    /// The keys of every listing stored so far.
    pub fn listing_keys(&self) -> Vec<ListingKey> {
        lock(&self.listings).keys().cloned().collect()
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

    fn listings(&self, keys: &[ListingKey]) -> Result<Vec<Option<String>>> {
        self.reachable()?;
        let listings = lock(&self.listings);
        Ok(keys.iter().map(|key| listings.get(key).cloned()).collect())
    }

    fn tip_costs(&self, request: &TipCostsRequest) -> Result<TipCosts> {
        self.fake_tip_costs(request)
    }

    fn tip_round(&self, round: &TipRound) -> Result<TipRoundFiled> {
        self.fake_tip_round(round)
    }

    fn last_tip_round(&self) -> Result<Option<TipRound>> {
        self.fake_last_tip_round()
    }

    fn fleet_versions(&self) -> Result<crate::protocol::fleet::FleetVersions> {
        self.fake_fleet_versions()
    }

    fn verdict(&self, key: &crate::protocol::hub::VerdictKey) -> Result<Option<Verdict>> {
        self.fake_verdict(key)
    }

    fn store_verdict(&self, verdict: &Verdict) -> Result<()> {
        self.fake_store_verdict(verdict)
    }

    fn reverted(&self, reverted: &Reverted) -> Result<RevertFiled> {
        self.fake_reverted(reverted)
    }

    fn store_listing(&self, listing: &StoredListing) -> Result<()> {
        self.reachable()?;
        let mut listings = lock(&self.listings);
        listings.insert(listing.key.clone(), listing.listing.clone());
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
        if self.records_down.load(Ordering::SeqCst) {
            return Err(Error::Unreachable("records are refused".to_owned()));
        }
        lock(&self.records).extend_from_slice(batch);
        Ok(())
    }

    /// Numbers each record by its place among every record sent, from 1.
    fn numbered_records(&self, batch: &[Record]) -> Result<Vec<RecordId>> {
        let first = lock(&self.records).len() as u64 + 1;
        self.records(batch)?;
        Ok((first..).take(batch.len()).map(RecordId).collect())
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

    fn session_title(&self, title: &SessionTitle) -> Result<()> {
        self.reachable()?;
        lock(&self.session_titles).push(title.clone());
        Ok(())
    }

    fn tip_claim(&self, claim: &TipClaim) -> Result<TipClaimReply> {
        self.reachable()?;
        lock(&self.tip_claim_asks).push(claim.clone());
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

    fn open_regressions(&self) -> Result<Vec<OpenRegression>> {
        self.reachable()?;
        lock(&self.open_regressions)
            .clone()
            .ok_or_else(|| Error::Invalid("open regressions are not supported".to_owned()))
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
            .filter(|t| {
                query.label_any.is_empty() || query.label_any.iter().any(|l| t.labels.contains(l))
            })
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
                filed: Vec::new(),
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

    fn export(&self, after: Option<&TicketId>, ids: &[TicketId]) -> Result<Vec<WholeTicket>> {
        self.export_whole(after, ids)
    }

    fn import(&self, request: &ImportRequest) -> Result<ImportReply> {
        self.import_whole(request)
    }

    fn pause(&self, pause: &Pause) -> Result<()> {
        self.reachable()?;
        lock(&self.pauses).push(pause.clone());
        if pause.machine.is_none() && !pause.paused {
            *lock(&self.emergency) = None;
        }
        Ok(())
    }

    fn emergency(
        &self,
        change: &crate::protocol::emergency::EmergencyChange,
    ) -> Result<Option<crate::protocol::emergency::Emergency>> {
        self.reachable()?;
        let mut current = lock(&self.emergency);
        let next = crate::protocol::emergency::apply(current.take(), change, Timestamp::EPOCH)
            .map_err(Error::Conflict)?;
        *current = Some(next);
        Ok(current.clone())
    }

    fn audit(&self, report: &AuditReport) -> Result<ReportFiled> {
        self.reachable()?;
        let mut audits = lock(&self.audits);
        audits.push(report.clone());
        Ok(Self::filed(audits.len()))
    }

    fn incomplete(&self, report: &IncompleteReport) -> Result<ReportFiled> {
        self.reachable()?;
        let mut reports = lock(&self.incompletes);
        reports.push(report.clone());
        Ok(Self::filed(reports.len()))
    }

    fn find(&self, find: &ExplorationFind) -> Result<ReportFiled> {
        self.reachable()?;
        let mut finds = lock(&self.finds);
        finds.push(find.clone());
        Ok(Self::filed(finds.len()))
    }

    fn justiciar(&self, report: &JusticiarReport) -> Result<ReportFiled> {
        self.reachable()?;
        let mut reports = lock(&self.justiciars);
        reports.push(report.clone());
        Ok(Self::filed(reports.len()))
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
mod tests;
