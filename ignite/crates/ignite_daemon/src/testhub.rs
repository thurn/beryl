//! A hub for tests that sees every call. It delegates to a [`FakeHub`], tells a spy the name of
//! each method before the call is made, and answers chosen methods with an error instead.

use std::sync::Arc;

use ignite_core::fakes::FakeHub;
use ignite_core::ids::{Commit, RecordId, Seq, TicketId};
use ignite_core::protocol::hub::{
    AuditReport, CachedResult, Claim, ExplorationFind, FlakyReport, Landed, LandedCommit,
    LandedReply, MachineFree, MetricsImport, NewResult, OpenRegression, Pause, Record,
    RegressionFiled, RegressionReport, ReportFiled, Reservation, ResultKey, RevertFiled, Reverted,
    TicketBrief, TipClaim, TipClaimReply, TipUnclaimed, TipUnclaimedReply, TipVerified, TipWaiting,
    TipWaitingFiled,
};
use ignite_core::protocol::tickets::{
    BatchReply, BatchRequest, FeedBatch, LeaseOp, LeaseReply, Ticket, TicketDetail, TicketQuery,
};
use ignite_core::tipround::{TipCandidate, TipCandidateUpdate};
use ignite_core::traits::{Error, Hub, Result};

type Spy = Box<dyn Fn(&str) + Send + Sync>;
type Refuse = Box<dyn Fn(&str) -> Option<Error> + Send + Sync>;

pub struct SpyHub {
    inner: Arc<FakeHub>,
    spy: Spy,
    refuse: Refuse,
}

impl SpyHub {
    pub fn new(inner: Arc<FakeHub>) -> Self {
        Self {
            inner,
            spy: Box::new(|_| {}),
            refuse: Box::new(|_| None),
        }
    }

    /// Calls `spy` with the method's name at the start of every call.
    #[must_use]
    pub fn spying(mut self, spy: impl Fn(&str) + Send + Sync + 'static) -> Self {
        self.spy = Box::new(spy);
        self
    }

    /// Answers every method for which `refuse` returns an error with that error.
    #[must_use]
    pub fn refusing(
        mut self,
        refuse: impl Fn(&str) -> Option<Error> + Send + Sync + 'static,
    ) -> Self {
        self.refuse = Box::new(refuse);
        self
    }

    fn gate(&self, name: &str) -> Result<()> {
        (self.spy)(name);
        (self.refuse)(name).map_or(Ok(()), Err)
    }
}

impl Hub for SpyHub {
    fn lookup(&self, keys: &[ResultKey]) -> Result<Vec<Option<CachedResult>>> {
        self.gate("lookup")?;
        self.inner.lookup(keys)
    }
    fn store(&self, results: &[NewResult]) -> Result<()> {
        self.gate("store")?;
        self.inner.store(results)
    }
    fn next(&self, free: &MachineFree) -> Result<Option<Reservation>> {
        self.gate("next")?;
        self.inner.next(free)
    }
    fn claim(&self, claim: &Claim) -> Result<TicketBrief> {
        self.gate("claim")?;
        self.inner.claim(claim)
    }
    fn landed(&self, landed: &Landed) -> Result<LandedReply> {
        self.gate("landed")?;
        self.inner.landed(landed)
    }
    fn lease(&self, name: &str, op: LeaseOp) -> Result<LeaseReply> {
        self.gate("lease")?;
        self.inner.lease(name, op)
    }
    fn feed(&self, since: Seq, wait_s: u32) -> Result<FeedBatch> {
        self.gate("feed")?;
        self.inner.feed(since, wait_s)
    }
    fn records(&self, batch: &[Record]) -> Result<()> {
        self.gate("records")?;
        self.inner.records(batch)
    }
    fn numbered_records(&self, batch: &[Record]) -> Result<Vec<RecordId>> {
        self.gate("records")?;
        self.inner.numbered_records(batch)
    }
    fn metrics(&self, import: &MetricsImport) -> Result<()> {
        self.gate("metrics")?;
        self.inner.metrics(import)
    }
    fn flaky(&self, report: &FlakyReport) -> Result<()> {
        self.gate("flaky")?;
        self.inner.flaky(report)
    }
    fn tip_claim(&self, claim: &TipClaim) -> Result<TipClaimReply> {
        self.gate("tip_claim")?;
        self.inner.tip_claim(claim)
    }
    fn tip_unclaimed(&self, ask: &TipUnclaimed) -> Result<TipUnclaimedReply> {
        self.gate("tip_unclaimed")?;
        self.inner.tip_unclaimed(ask)
    }
    fn tip_candidate(&self) -> Result<Option<TipCandidate>> {
        self.gate("tip_candidate")?;
        self.inner.tip_candidate()
    }
    fn update_tip_candidate(&self, update: &TipCandidateUpdate) -> Result<bool> {
        self.gate("update_tip_candidate")?;
        self.inner.update_tip_candidate(update)
    }
    fn tip_verified(&self, verified: &TipVerified) -> Result<()> {
        self.gate("tip_verified")?;
        self.inner.tip_verified(verified)
    }
    fn regression(&self, report: &RegressionReport) -> Result<RegressionFiled> {
        self.gate("regression")?;
        self.inner.regression(report)
    }
    fn reverted(&self, reverted: &Reverted) -> Result<RevertFiled> {
        self.gate("reverted")?;
        self.inner.reverted(reverted)
    }
    fn open_regressions(&self) -> Result<Vec<OpenRegression>> {
        self.gate("open_regressions")?;
        self.inner.open_regressions()
    }
    fn audit(&self, report: &AuditReport) -> Result<ReportFiled> {
        self.gate("audit")?;
        self.inner.audit(report)
    }
    fn find(&self, find: &ExplorationFind) -> Result<ReportFiled> {
        self.gate("find")?;
        self.inner.find(find)
    }
    fn landed_commits(&self, commits: &[Commit]) -> Result<Vec<LandedCommit>> {
        self.gate("landed_commits")?;
        self.inner.landed_commits(commits)
    }
    fn tip_waiting(&self, waiting: &TipWaiting) -> Result<TipWaitingFiled> {
        self.gate("tip_waiting")?;
        self.inner.tip_waiting(waiting)
    }
    fn ticket(&self, id: &TicketId) -> Result<TicketDetail> {
        self.gate("ticket")?;
        self.inner.ticket(id)
    }
    fn tickets(&self, query: &TicketQuery) -> Result<Vec<Ticket>> {
        self.gate("tickets")?;
        self.inner.tickets(query)
    }
    fn batch(&self, request: &BatchRequest) -> Result<BatchReply> {
        self.gate("batch")?;
        self.inner.batch(request)
    }
    fn pause(&self, pause: &Pause) -> Result<()> {
        self.gate("pause")?;
        self.inner.pause(pause)
    }
    fn emergency(
        &self,
        change: &ignite_core::protocol::emergency::EmergencyChange,
    ) -> Result<Option<ignite_core::protocol::emergency::Emergency>> {
        self.gate("emergency")?;
        self.inner.emergency(change)
    }
}
