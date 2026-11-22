//! The tip verification rounds of [`FakeHub`]: frozen claim costs and round records.

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use crate::ids::Commit;
use crate::tipround::{
    TipCandidate, TipCandidateUpdate, TipCosts, TipCostsRequest, TipCoverage, TipCoverageUpdate,
    TipRound, TipRoundFiled,
};
use crate::traits::Result;

use super::{FakeHub, lock};

/// What [`FakeHub`] keeps of tip rounds.
#[derive(Debug, Default)]
pub(super) struct FakeTips {
    /// Each check's recorded cost in CPU seconds.
    check_costs: Mutex<BTreeMap<String, f64>>,
    /// The costs given for each tip and family, frozen at the first request.
    frozen: Mutex<HashMap<(Commit, String), TipCosts>>,
    rounds: Mutex<Vec<TipRound>>,
    coverage: Mutex<TipCoverage>,
    candidate: Mutex<Option<TipCandidate>>,
    /// The open improvement ticket a slow round filed, if any.
    open_ticket: Mutex<Option<String>>,
    filed: Mutex<u32>,
}

impl FakeHub {
    pub(super) fn fake_tip_candidate(&self) -> Result<Option<TipCandidate>> {
        self.reachable()?;
        Ok(lock(&self.tips.candidate).clone())
    }

    pub(super) fn fake_update_tip_candidate(&self, update: &TipCandidateUpdate) -> Result<bool> {
        self.reachable()?;
        let mut candidate = lock(&self.tips.candidate);
        if *candidate != update.expected {
            return Ok(false);
        }
        *candidate = Some(update.replacement.clone());
        Ok(true)
    }

    pub fn set_tip_coverage(&self, coverage: TipCoverage) {
        *lock(&self.tips.coverage) = coverage;
    }

    pub(super) fn fake_tip_coverage(&self) -> Result<TipCoverage> {
        self.reachable()?;
        Ok(lock(&self.tips.coverage).clone())
    }

    pub(super) fn fake_update_tip_coverage(&self, update: &TipCoverageUpdate) -> Result<bool> {
        self.reachable()?;
        let mut coverage = lock(&self.tips.coverage);
        if coverage.platforms.get(&update.platform) != update.expected.as_ref() {
            return Ok(false);
        }
        coverage
            .platforms
            .insert(update.platform, update.replacement.clone());
        Ok(true)
    }

    /// Records a check's cost in CPU seconds, as stored results do at the real hub.
    pub fn set_check_cost(&self, check: &str, cost: f64) {
        lock(&self.tips.check_costs).insert(check.to_owned(), cost);
    }

    /// Every tip round recorded so far, in order.
    pub fn tip_rounds(&self) -> Vec<TipRound> {
        lock(&self.tips.rounds).clone()
    }

    /// How many improvement tickets slow rounds have filed.
    pub fn round_tickets_filed(&self) -> u32 {
        *lock(&self.tips.filed)
    }

    /// Closes the open improvement ticket a slow round filed, so the next one files again.
    pub fn close_round_ticket(&self) {
        *lock(&self.tips.open_ticket) = None;
    }

    pub(super) fn fake_tip_costs(&self, request: &TipCostsRequest) -> Result<TipCosts> {
        self.reachable()?;
        let key = (request.commit.clone(), request.family.clone());
        let mut frozen = lock(&self.tips.frozen);
        let costs = frozen.entry(key).or_insert_with(|| {
            let prefix = format!("{}:", request.family);
            TipCosts {
                claim_seconds: request.claim_seconds,
                costs: lock(&self.tips.check_costs)
                    .iter()
                    .filter(|(check, _)| check.starts_with(&prefix))
                    .map(|(check, cost)| (check.clone(), *cost))
                    .collect(),
            }
        });
        Ok(costs.clone())
    }

    /// Records the round and, as the hub does, files an improvement ticket for a round with
    /// too much overhead unless one is open.
    pub(super) fn fake_tip_round(&self, round: &TipRound) -> Result<TipRoundFiled> {
        self.reachable()?;
        lock(&self.tips.rounds).push(round.clone());
        if round.overhead().is_none() {
            return Ok(TipRoundFiled::default());
        }
        let mut open = lock(&self.tips.open_ticket);
        if let Some(ticket) = open.as_ref() {
            return Ok(TipRoundFiled {
                ticket: Some(ticket.clone()),
                created: false,
            });
        }
        let mut filed = lock(&self.tips.filed);
        *filed += 1;
        let ticket = format!("round-{filed}");
        *open = Some(ticket.clone());
        Ok(TipRoundFiled {
            ticket: Some(ticket),
            created: true,
        })
    }

    pub(super) fn fake_last_tip_round(&self) -> Result<Option<TipRound>> {
        self.reachable()?;
        Ok(lock(&self.tips.rounds).last().cloned())
    }
}
