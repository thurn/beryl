//! What running one check cost, as a machine reports it with each result (see
//! [`super::NewResult`]).

use serde::{Deserialize, Serialize};

/// What running one check cost, measured by the machine that ran it. A run of several checks
/// at once gives each an equal share of the run's usage.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResultCost {
    /// Instructions retired; `None` where the machine does not count them.
    #[serde(default)]
    pub instructions: Option<u64>,
    /// CPU time in milliseconds.
    #[serde(default)]
    pub cpu_ms: u64,
    /// How many checks shared the command's start-up: the checks of the command for one run,
    /// so the hub takes `start-up / shared_by` off the share (see `score::startup`). For the
    /// sum of several runs, the number whose reciprocal is the sum of theirs. 0 when unknown,
    /// from a daemon built before it was sent, and nothing is taken off.
    #[serde(default)]
    pub shared_by: u32,
}

impl ResultCost {
    /// An equal share of `usage` among `shares` checks (at least one).
    #[must_use]
    pub fn share_of(usage: &crate::protocol::Usage, shares: usize) -> Self {
        let shares = shares.max(1);
        let divisor = u64::try_from(shares).unwrap_or(u64::MAX);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let cpu_ms = (usage.cpu_s.max(0.0) * 1000.0 / divisor as f64).round() as u64;
        Self {
            instructions: usage.instructions.map(|n| n / divisor),
            cpu_ms,
            shared_by: u32::try_from(shares).unwrap_or(u32::MAX),
        }
    }

    /// The sum of two costs; instructions are known only when both know them (or one is empty).
    /// Each run paid one start-up, so the sum's share of start-ups is the sum of the two shares.
    #[must_use]
    pub fn plus(self, other: Self) -> Self {
        let shared_by = if self == Self::default() {
            other.shared_by
        } else if other == Self::default() {
            self.shared_by
        } else if self.shared_by == 0 || other.shared_by == 0 {
            0
        } else {
            let (a, b) = (u64::from(self.shared_by), u64::from(other.shared_by));
            u32::try_from((a * b / (a + b)).max(1)).unwrap_or(u32::MAX)
        };
        Self {
            instructions: match (self.instructions, other.instructions) {
                (Some(a), Some(b)) => Some(a.saturating_add(b)),
                (a, b) => a.or(b),
            },
            cpu_ms: self.cpu_ms.saturating_add(other.cpu_ms),
            shared_by,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A check run twice, in a batch of 50 and then alone, paid 1/50 and then a whole start-up.
    #[test]
    fn a_cost_counts_the_checks_that_shared_each_start_up() {
        let usage = |cpu_s: f64| crate::protocol::Usage {
            cpu_s,
            ..crate::protocol::Usage::default()
        };
        let batch = ResultCost::share_of(&usage(50.0), 50);
        assert_eq!((batch.cpu_ms, batch.shared_by), (1000, 50));
        let first = ResultCost::default().plus(batch);
        assert_eq!(first, batch, "the empty sum keeps the run's count");
        let both = first.plus(ResultCost::share_of(&usage(3.0), 1));
        assert_eq!((both.cpu_ms, both.shared_by), (4000, 1));
        let old = ResultCost {
            cpu_ms: 5,
            ..ResultCost::default()
        };
        assert_eq!(
            batch.plus(old).shared_by,
            0,
            "unknown once one run is unknown"
        );
        let legacy: ResultCost = serde_json::from_str(r#"{"cpu_ms": 7}"#).unwrap();
        assert_eq!(legacy.shared_by, 0);
    }
}
