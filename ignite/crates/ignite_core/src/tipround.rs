//! What one tip verification round claims and what it reports (see "How long a round takes" in
//! `docs/ignite.md`).
//!
//! - **Claims sized by cost.** A round claims one family's checks from the hub in batches, by
//!   family and batch number, so two machines never run the same checks. Every machine must
//!   therefore cut the same batches. The hub answers `POST /tips/costs` with the costs and the
//!   claim length a round at that tip uses, frozen the first time any machine asks, and
//!   [`cut`] divides the family's checks from those alone. A batch holds about
//!   [`TipCosts::claim_seconds`] of recorded cost. A check with no recorded cost counts as the
//!   family's median; a family with none starts at [`UNMEASURED_BATCH`] checks a claim and
//!   doubles with each claim.
//! - **Where a round's time went.** Each round sends one [`TipRound`] when it ends. A round in
//!   which more than [`OVERHEAD_SHARE`] of the time went to anything but checks and builds, and
//!   that took at least [`SMALLEST_ROUND_S`], asks the hub for an improvement ticket naming the
//!   largest part ([`TipRound::overhead`]).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ids::{CheckName, Commit, MachineId};
use crate::time::Timestamp;

/// How many checks the first claim of a family with no recorded cost holds. Each later claim
/// of that family holds twice as many as the one before.
pub const UNMEASURED_BATCH: usize = 50;

/// The share of a round's time that may go to anything but checks and builds before the
/// round asks for an improvement ticket.
pub const OVERHEAD_SHARE: f64 = 0.10;

/// A round shorter than this files nothing, whatever its shares.
pub const SMALLEST_ROUND_S: f64 = 120.0;

/// `POST /tips/costs`: the costs to cut one family's batches by at one tip.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TipCostsRequest {
    pub commit: Commit,
    pub family: String,
    /// The asking machine's `tip.claim_seconds`. The first request for a tip and family fixes
    /// it, with the costs, for every machine.
    pub claim_seconds: f64,
}

/// The reply to `POST /tips/costs`, the same for every request about one tip and family.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TipCosts {
    pub claim_seconds: f64,
    /// Each check of the family the hub has a recorded cost for, in CPU seconds: the lowest of
    /// its recent costs. Each recorded cost is an equal share of one command, which also paid
    /// the program's start-up, so the lowest share comes from the largest batch and is the
    /// closest to what one more check adds.
    pub costs: BTreeMap<String, f64>,
}

impl TipCosts {
    /// Costs that cut every family as one nobody has measured. A round that cannot reach the
    /// hub uses these, which every machine in the same position cuts alike.
    pub fn unmeasured(claim_seconds: f64) -> Self {
        Self {
            claim_seconds,
            costs: BTreeMap::new(),
        }
    }

    /// The median of the recorded costs among `names`, or `None` when none is recorded.
    fn median(&self, names: &[CheckName]) -> Option<f64> {
        let mut known: Vec<f64> = names
            .iter()
            .filter_map(|name| self.costs.get(name.as_str()).copied())
            .filter(|cost| cost.is_finite() && *cost >= 0.0)
            .collect();
        if known.is_empty() {
            return None;
        }
        known.sort_by(f64::total_cmp);
        Some(known[known.len() / 2])
    }
}

/// Cuts one family's checks into batches, in the order given, which callers sort by name. The
/// cut depends only on `names` and `costs`, so every machine that lists the same checks and is
/// given the same costs cuts the same batches. Each batch holds at least one check.
pub fn cut(names: &[CheckName], costs: &TipCosts) -> Vec<Vec<CheckName>> {
    let Some(median) = costs.median(names) else {
        let mut batches = Vec::new();
        let mut size = UNMEASURED_BATCH;
        let mut rest = names;
        while !rest.is_empty() {
            let (batch, after) = rest.split_at(size.min(rest.len()));
            batches.push(batch.to_vec());
            rest = after;
            size = size.saturating_mul(2);
        }
        return batches;
    };
    let mut batches = Vec::new();
    let mut batch = Vec::new();
    let mut total = 0.0;
    for name in names {
        let cost = costs
            .costs
            .get(name.as_str())
            .copied()
            .filter(|cost| cost.is_finite() && *cost >= 0.0)
            .unwrap_or(median);
        batch.push(name.clone());
        total += cost;
        if total >= costs.claim_seconds {
            batches.push(std::mem::take(&mut batch));
            total = 0.0;
        }
    }
    if !batch.is_empty() {
        batches.push(batch);
    }
    batches
}

/// One finished tip verification round, as the daemon that ran it measured it with the `Clock`
/// trait (`POST /tips/rounds`). Times are seconds summed over every family the round ran, so
/// with families running at once they can add up to more than the round's wall time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TipRound {
    pub commit: Commit,
    pub machine: MachineId,
    /// When the round ended.
    pub at: Timestamp,
    /// The round's wall time, from listing the tip to its verdict.
    pub wall_s: f64,
    /// Checks the round ran, counting each rerun once.
    pub ran: u32,
    /// Checks the result cache answered.
    pub cached: u32,
    /// Time the check commands ran, apart from waiting for cores.
    pub checks_s: f64,
    /// Time the families' `list` commands ran.
    pub listing_s: f64,
    /// Time spent making build products current, waiting for them included.
    pub building_s: f64,
    /// Time commands waited for cores before they started.
    pub waiting_s: f64,
}

/// A part of a round's time that is neither running checks nor building.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overhead {
    Listing,
    Waiting,
    /// Everything else: keying checks, asking the hub, claiming batches.
    Other,
}

impl Overhead {
    /// The part in words, as the improvement ticket names it.
    pub fn words(self) -> &'static str {
        match self {
            Overhead::Listing => "listing families",
            Overhead::Waiting => "waiting for cores",
            Overhead::Other => "Ignite's own work between commands",
        }
    }
}

impl TipRound {
    /// The round's time not accounted for by commands and builds: its wall time beyond the
    /// sum of the measured parts, or zero when families running at once add up to more.
    pub fn other_s(&self) -> f64 {
        (self.wall_s - self.checks_s - self.listing_s - self.building_s - self.waiting_s).max(0.0)
    }

    /// The whole of the round's time: the measured parts and the rest.
    pub fn total_s(&self) -> f64 {
        self.checks_s + self.listing_s + self.building_s + self.waiting_s + self.other_s()
    }

    /// The largest part of the round's time that is neither checks nor builds, and its share
    /// of the whole, when the parts that are neither come to more than [`OVERHEAD_SHARE`] of a
    /// round of at least [`SMALLEST_ROUND_S`]. `None` otherwise.
    pub fn overhead(&self) -> Option<(Overhead, f64)> {
        let total = self.total_s();
        if self.wall_s < SMALLEST_ROUND_S || total <= 0.0 {
            return None;
        }
        let parts = [
            (Overhead::Listing, self.listing_s),
            (Overhead::Waiting, self.waiting_s),
            (Overhead::Other, self.other_s()),
        ];
        let overhead: f64 = parts.iter().map(|(_, s)| s).sum();
        if overhead / total <= OVERHEAD_SHARE {
            return None;
        }
        let (largest, seconds) = parts
            .into_iter()
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap_or((Overhead::Other, 0.0));
        Some((largest, seconds / total))
    }

    /// The round's figures in one line, in minutes, as `ignite status` and the dashboard show
    /// them.
    pub fn summary(&self) -> String {
        let minutes = |seconds: f64| format!("{:.1}", seconds / 60.0);
        format!(
            "{} checks run, {} cached; minutes: {} checks, {} listing, {} building, {} waiting \
             for cores, {} other, {} in all",
            self.ran,
            self.cached,
            minutes(self.checks_s),
            minutes(self.listing_s),
            minutes(self.building_s),
            minutes(self.waiting_s),
            minutes(self.other_s()),
            minutes(self.wall_s),
        )
    }
}

/// The reply to `POST /tips/rounds`: the improvement ticket the round filed or found open, if
/// it asked for one.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TipRoundFiled {
    pub ticket: Option<String>,
    /// True when the ticket was filed for this round, false when an open one was reused.
    #[serde(default)]
    pub created: bool,
}

/// The reply to `GET /tips/rounds/last`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LastTipRound {
    pub round: Option<TipRound>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(count: usize) -> Vec<CheckName> {
        (0..count)
            .map(|i| CheckName::new(format!("unit:t{i:05}")).unwrap())
            .collect()
    }

    fn costing(names: &[CheckName], each: f64) -> TipCosts {
        TipCosts {
            claim_seconds: 60.0,
            costs: names.iter().map(|n| (n.to_string(), each)).collect(),
        }
    }

    fn sizes(batches: &[Vec<CheckName>]) -> Vec<usize> {
        batches.iter().map(Vec::len).collect()
    }

    /// A kernel change made tip verification launch the program behind 9,153 interface checks
    /// 183 times, one launch per 50 checks, although one launch runs them all in seconds.
    #[test]
    fn nine_thousand_checks_of_ten_milliseconds_are_one_or_two_claims() {
        let names = names(9_000);
        let batches = cut(&names, &costing(&names, 0.01));
        assert!(batches.len() <= 2, "{:?}", sizes(&batches));
        assert_eq!(batches.concat(), names);
    }

    #[test]
    fn checks_of_thirty_seconds_are_claimed_two_at_a_time() {
        let names = names(7);
        assert_eq!(sizes(&cut(&names, &costing(&names, 30.0))), [2, 2, 2, 1]);
    }

    #[test]
    fn a_check_with_no_cost_counts_as_the_family_median() {
        let names = names(6);
        let mut costs = costing(&names[..3], 20.0);
        costs.costs.insert(names[1].to_string(), 10.0);
        // The median of 20, 10 and 20 is 20, so the three unmeasured checks cost 20 each.
        assert_eq!(sizes(&cut(&names, &costs)), [4, 2]);
    }

    #[test]
    fn a_family_nobody_measured_starts_at_fifty_and_doubles() {
        let names = names(400);
        assert_eq!(
            sizes(&cut(&names, &TipCosts::unmeasured(60.0))),
            [50, 100, 200, 50]
        );
        assert!(cut(&[], &TipCosts::unmeasured(60.0)).is_empty());
    }

    fn round(wall_s: f64, listing_s: f64) -> TipRound {
        TipRound {
            commit: Commit::new("abcd").unwrap(),
            machine: MachineId::new("m-1").unwrap(),
            at: Timestamp::EPOCH,
            wall_s,
            ran: 10,
            cached: 5,
            checks_s: wall_s - listing_s,
            listing_s,
            building_s: 0.0,
            waiting_s: 0.0,
        }
    }

    #[test]
    fn a_round_with_much_overhead_names_its_largest_part() {
        let (part, share) = round(600.0, 180.0).overhead().unwrap();
        assert_eq!(part, Overhead::Listing);
        assert!((share - 0.3).abs() < 1e-9, "{share}");
        // Ten percent or less files nothing, nor does a short round.
        assert_eq!(round(600.0, 60.0).overhead(), None);
        assert_eq!(round(100.0, 90.0).overhead(), None);
    }
}
