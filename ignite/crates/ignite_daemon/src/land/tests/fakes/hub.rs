//! The landing tests' hub. Integration branch leases use the fake clock. The attribution
//! test sends regression requests through the real routes and an in-memory hub database.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use ignite_core::fakes::{FakeClock, FakeHub};
use ignite_core::ids::{Commit, Seq, TicketId};
use ignite_core::protocol::hub::{
    CachedResult, Claim, FlakyReport, Landed, LandedCommit, LandedReply, LandingAttribution,
    LandingRegression, MachineFree, MetricsImport, NewResult, OpenRegression, OpenRegressions,
    Record, RegressionAttribution, RegressionFiled, RegressionReport, Reservation, ResultKey,
    TicketBrief, TipClaim, TipClaimReply, TipUnclaimed, TipUnclaimedReply, TipVerified, Verdict,
    VerdictKey,
};
use ignite_core::protocol::tickets::{
    FeedBatch, Lease, LeaseBusy, LeaseOp, LeaseReply, TicketDetail,
};
use ignite_core::time::Timestamp;
use ignite_core::tipround::{TipCandidate, TipCandidateUpdate};
use ignite_core::traits::{Clock, Error, Hub, Result};

/// The fake hub, with leases that expire by the clock. `landed_down` makes only `POST /landed`
/// fail as an outage would.
pub struct LeaseHub {
    pub hub: FakeHub,
    pub clock: Arc<FakeClock>,
    /// Tests of the baseline attribution workflow can use the hub's real regression routes.
    pub regression_db: Mutex<Option<rusqlite::Connection>>,
    pub leases: Mutex<HashMap<String, (String, Timestamp)>>,
    pub landed_down: AtomicBool,
    /// Makes the next acquisition succeed and then, at once, hands the lease to another holder,
    /// as if it had expired and been taken over before the landing used it.
    pub steal_after_acquire: AtomicBool,
}

impl LeaseHub {
    fn regression_route<R: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        path: &str,
        data: &impl serde::Serialize,
    ) -> Result<R> {
        let mut db = self.regression_db.lock().unwrap();
        let conn = db
            .as_mut()
            .ok_or_else(|| Error::NotFound("no regression routes in this test".to_owned()))?;
        let request = ignite_hub::routes::Request {
            method: method.to_owned(),
            path: path.to_owned(),
            query: Vec::new(),
            writer: Some(ignite_hub::auth::Writer {
                machine: "m-1".parse().unwrap(),
                actor: "ignite".to_owned(),
            }),
            body: serde_json::to_vec(data).unwrap(),
            headers: Vec::new(),
        };
        let response =
            ignite_hub::routes::checks::handle(conn, &request, self.clock.now()).unwrap();
        assert_eq!(
            response.status,
            200,
            "{}",
            String::from_utf8_lossy(&response.body)
        );
        Ok(serde_json::from_slice(&response.body).unwrap())
    }

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
    fn audit(
        &self,
        report: &ignite_core::protocol::hub::AuditReport,
    ) -> Result<ignite_core::protocol::hub::ReportFiled> {
        self.hub.audit(report)
    }
    fn find(
        &self,
        find: &ignite_core::protocol::hub::ExplorationFind,
    ) -> Result<ignite_core::protocol::hub::ReportFiled> {
        self.hub.find(find)
    }
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
    fn ticket(&self, id: &TicketId) -> Result<TicketDetail> {
        self.hub.ticket(id)
    }
    fn landed(&self, landed: &Landed) -> Result<LandedReply> {
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
            ref op @ (LeaseOp::Acquire {
                ref holder, ttl_s, ..
            }
            | LeaseOp::Renew {
                ref holder, ttl_s, ..
            }) => {
                let holder = holder.clone();
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
                if matches!(op, LeaseOp::Acquire { .. })
                    && self.steal_after_acquire.swap(false, Ordering::SeqCst)
                {
                    leases.insert("staging".to_owned(), ("thief".to_owned(), expires_at));
                }
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
    fn tip_unclaimed(&self, ask: &TipUnclaimed) -> Result<TipUnclaimedReply> {
        self.hub.tip_unclaimed(ask)
    }
    fn tip_candidate(&self) -> Result<Option<TipCandidate>> {
        self.hub.tip_candidate()
    }
    fn update_tip_candidate(&self, update: &TipCandidateUpdate) -> Result<bool> {
        self.hub.update_tip_candidate(update)
    }
    fn tip_verified(&self, verified: &TipVerified) -> Result<()> {
        self.hub.tip_verified(verified)
    }
    fn regression(&self, report: &RegressionReport) -> Result<RegressionFiled> {
        self.hub.regression(report)
    }
    fn landing_regression(&self, failure: &LandingRegression) -> Result<RegressionFiled> {
        match self.regression_route("POST", "/regressions/landing", failure) {
            Err(Error::NotFound(_)) => self.regression(&failure.report),
            result => result,
        }
    }
    fn next_landing_attribution(&self) -> Result<Option<LandingAttribution>> {
        self.regression_route("GET", "/regressions/landing", &())
    }
    fn attribute_regression(&self, attribution: &RegressionAttribution) -> Result<()> {
        let _: serde_json::Value =
            self.regression_route("POST", "/regressions/attribution", attribution)?;
        Ok(())
    }
    fn open_regressions(&self) -> Result<Vec<OpenRegression>> {
        let reply: OpenRegressions = self.regression_route("GET", "/regressions/open", &())?;
        Ok(reply.regressions)
    }
    fn landed_commits(&self, commits: &[Commit]) -> Result<Vec<LandedCommit>> {
        self.hub.landed_commits(commits)
    }
    fn verdict(&self, key: &VerdictKey) -> Result<Option<Verdict>> {
        self.hub.verdict(key)
    }
    fn store_verdict(&self, verdict: &Verdict) -> Result<()> {
        self.hub.store_verdict(verdict)
    }
}
