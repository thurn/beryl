//! Check scores: the chance a check fails after a change, per billion instructions it costs.
//!
//! ```text
//! score(check, change) = P(fail | areas the change touches) / cost(check)
//! P     = (failures after changes to these areas + 20 x family rate)
//!         / (runs after changes to these areas + 20)
//! cost  = median instructions of its last 20 runs, in billions
//!         + the builds it needs that nothing chosen so far has paid for
//! ```
//!
//! The family rate stands in for 20 runs, so a check with few runs of its own starts from its
//! family's record. A change that touches several areas takes the largest P among them. See
//! "Scores" in `docs/ignite.md`; [`crate::gate`] chooses checks by these scores.

/// How many runs the family rate stands in for.
pub const PRIOR_RUNS: f64 = 20.0;

/// Billions of instructions per CPU second, measured on the reference Mac. A cost known only in
/// CPU seconds (from Windows, or from history imported at the switch) converts at this rate
/// until the check has run on a machine that counts instructions.
pub const BILLIONS_PER_CPU_SECOND: f64 = 17.0;

/// The smallest cost a score divides by, so that a check recorded as free still has a finite
/// score.
const MIN_COST: f64 = 1e-6;

/// What the hub knows of one check, for the areas one change touches.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CheckRecord {
    pub runs_in_areas: u32,
    pub failures_in_areas: u32,
    pub family_rate: f64,
    /// The median cost of its last 20 runs, in billions of instructions.
    pub median_cost: f64,
}

/// The smoothed chance that the check fails after the change.
pub fn probability(record: &CheckRecord) -> f64 {
    (f64::from(record.failures_in_areas) + PRIOR_RUNS * record.family_rate)
        / (f64::from(record.runs_in_areas) + PRIOR_RUNS)
}

/// The score, counting `unpaid_builds` billions for builds nothing chosen so far has paid for.
pub fn score(record: &CheckRecord, unpaid_builds: f64) -> f64 {
    per_cost(probability(record), record.median_cost + unpaid_builds)
}

/// A chance of failure divided by a cost in billions of instructions.
pub fn per_cost(probability: f64, cost: f64) -> f64 {
    probability / cost.max(MIN_COST)
}

/// Runs and failures of one check after changes to one area.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AreaRecord {
    pub runs: u32,
    pub failures: u32,
}

/// The largest smoothed chance of failure over the areas a change touches. An area the check
/// has no runs after contributes the family rate; so does a change that touches no area.
pub fn probability_over_areas(areas: &[AreaRecord], family_rate: f64) -> f64 {
    let in_area = |area: &AreaRecord| {
        probability(&CheckRecord {
            runs_in_areas: area.runs,
            failures_in_areas: area.failures,
            family_rate,
            median_cost: 0.0,
        })
    };
    areas
        .iter()
        .map(in_area)
        .fold(None, |largest: Option<f64>, p| {
            Some(largest.map_or(p, |largest| largest.max(p)))
        })
        .unwrap_or(family_rate)
}

/// A recorded cost, in the unit the machine that ran the check could measure.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cost {
    /// Billions of instructions retired.
    Billions(f64),
    /// CPU seconds, where the operating system counts no instructions.
    CpuSeconds(f64),
}

impl Cost {
    /// The cost in billions of instructions, converting CPU seconds at `billions_per_cpu_second`.
    pub fn billions(self, billions_per_cpu_second: f64) -> f64 {
        match self {
            Cost::Billions(billions) => billions,
            Cost::CpuSeconds(seconds) => seconds * billions_per_cpu_second,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The worked example under "Scores" in `docs/ignite.md`.
    #[test]
    fn worked_example() {
        let aura = CheckRecord {
            runs_in_areas: 74,
            failures_in_areas: 9,
            family_rate: 0.02,
            median_cost: 0.8,
        };
        assert!((probability(&aura) - 0.10).abs() < 1e-12);
        assert!((score(&aura, 0.0) - 0.125).abs() < 1e-12);

        // The replay test costs 95 billion, mostly for a test binary the tree has not built.
        let replay = CheckRecord {
            runs_in_areas: 310,
            failures_in_areas: 1,
            family_rate: 0.01,
            median_cost: 5.0,
        };
        assert!(((probability(&replay) * 1000.0).round() - 4.0).abs() < 1e-9);
        assert!(((score(&replay, 90.0) * 100_000.0).round() - 4.0).abs() < 1e-9);
    }

    #[test]
    fn largest_area_wins_and_unseen_areas_use_the_family_rate() {
        let areas = [
            AreaRecord {
                runs: 80,
                failures: 0,
            },
            AreaRecord {
                runs: 0,
                failures: 0,
            },
            AreaRecord {
                runs: 10,
                failures: 5,
            },
        ];
        let p = probability_over_areas(&areas, 0.1);
        assert!((p - (5.0 + 2.0) / 30.0).abs() < 1e-12);
        assert!((probability_over_areas(&areas[1..2], 0.1) - 0.1).abs() < 1e-12);
        assert!((probability_over_areas(&[], 0.25) - 0.25).abs() < 1e-12);
    }

    #[test]
    fn cpu_seconds_convert_at_the_given_rate() {
        assert!((Cost::CpuSeconds(2.0).billions(BILLIONS_PER_CPU_SECOND) - 34.0).abs() < 1e-12);
        assert!((Cost::Billions(3.0).billions(BILLIONS_PER_CPU_SECOND) - 3.0).abs() < 1e-12);
    }

    #[test]
    fn a_free_check_has_a_finite_score() {
        let free = CheckRecord {
            runs_in_areas: 0,
            failures_in_areas: 0,
            family_rate: 0.5,
            median_cost: 0.0,
        };
        assert!(score(&free, 0.0).is_finite());
    }
}
