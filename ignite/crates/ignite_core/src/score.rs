//! Check scores: the chance a check catches a product bug after a change, per billion instructions it costs.
//!
//! ```text
//! score(check, change) = P(product catch | areas the change touches) / cost(check)
//! P     = (product catches after changes to these areas + 20 x family rate)
//!         / (runs after changes to these areas + 20)
//! cost  = median instructions of its last 20 runs, in billions
//! ```
//!
//! A run's cost is its equal share of the command it ran in, after the family's start-up
//! ([`startup`]) is taken out of the command's cost. The gate charges a family's start-up once,
//! when it first runs one of the family's checks.
//!
//! The cost is what running the check takes. Builds are not part of it: the gate runs checks on
//! what its tree has already compiled and leaves a check that needs a cold build to tip
//! verification ([`crate::gate`]).
//!
//! The family rate stands in for 20 runs, so a check with few runs of its own starts from its
//! family's record. A change that touches several areas takes the largest P among them. See
//! "Scores" in `docs/ignite.md`; [`crate::gate`] chooses checks by these scores.

use std::collections::{BTreeMap, BTreeSet};

use crate::ids::{CheckName, Commit};
use crate::inputs::{self, CrateGraph};
use crate::protocol::hub::{AreaStats, CheckFix, CheckStatsReply, FixOutcome};
use crate::time::Timestamp;
use crate::traits::Git;

/// How long a recorded product catch makes a check mandatory in its culprit's areas.
pub const RECENT_PRODUCT_CATCH_SECONDS: f64 = 14.0 * 86_400.0;

/// One check whose failure led to a product fix, with the areas of its first bad commit.
#[derive(Debug, PartialEq, Eq)]
pub struct ProductCatch {
    pub check: CheckName,
    pub at: Timestamp,
    /// An empty string represents an unknown area and contributes only to the family rate.
    pub areas: Vec<String>,
}

/// Resolves each culprit once. When a ticket reopens and fixes the same check and culprit
/// again, use its latest product outcome and count the catch once.
pub fn product_catches(
    fixes: Vec<CheckFix>,
    graph: &CrateGraph,
    git: &dyn Git,
    now: Timestamp,
) -> Vec<ProductCatch> {
    let mut fixes = fixes;
    fixes.sort_unstable_by_key(|fix| std::cmp::Reverse(fix.at));
    let mut seen = BTreeSet::new();
    let mut by_commit = BTreeMap::<Commit, Vec<String>>::new();
    fixes
        .into_iter()
        .filter(|fix| fix.outcome == FixOutcome::Product && fix.at <= now)
        .filter_map(|fix| {
            let areas = if let Some(commit) = fix.first_bad {
                if !seen.insert((fix.check.clone(), commit.clone())) {
                    return None;
                }
                by_commit
                    .entry(commit.clone())
                    .or_insert_with(|| {
                        let paths = git.commit_paths(&commit).unwrap_or_default();
                        let areas: BTreeSet<_> = paths
                            .iter()
                            .map(|path| inputs::area(path, graph).to_string())
                            .collect();
                        areas.into_iter().collect()
                    })
                    .clone()
            } else {
                Vec::new()
            };
            Some(ProductCatch {
                check: fix.check,
                at: fix.at,
                areas: if areas.is_empty() {
                    vec![String::new()]
                } else {
                    areas
                },
            })
        })
        .collect()
}

impl ProductCatch {
    /// Whether this catch is recent and its culprit shares an area with the change.
    pub fn applies(&self, areas: &[String], now: Timestamp) -> bool {
        self.at >= now.plus_seconds(-RECENT_PRODUCT_CATCH_SECONDS)
            && self.areas.iter().any(|area| areas.contains(area))
    }
}

/// Uses classified catches as the failure counts and keeps the recorded run counts.
/// Tip failures can have no area counts, so each catch establishes at least one run there.
/// An older hub's reply lacks the total needed to recompute its family rate.
pub fn use_product_catches(
    reply: &mut CheckStatsReply,
    catches: &[ProductCatch],
    areas: &[String],
) {
    let Some(runs) = reply.runs else {
        return;
    };
    let mut by_check: BTreeMap<&CheckName, BTreeMap<&str, u32>> = BTreeMap::new();
    let mut family_catches = 0_u64;
    for catch in catches.iter().filter(|c| c.check.family() == reply.family) {
        let counts = by_check.entry(&catch.check).or_default();
        for area in &catch.areas {
            family_catches += 1;
            if areas.is_empty() || areas.contains(area) {
                *counts.entry(area).or_default() += 1;
            }
        }
    }
    let runs = runs.max(family_catches);
    reply.family_rate = if runs == 0 {
        0.0
    } else {
        family_catches as f64 / runs as f64
    };
    for check in &mut reply.checks {
        let mut catches = by_check.remove(&check.check).unwrap_or_default();
        for area in &mut check.areas {
            area.failures = catches.remove(area.area.as_str()).unwrap_or_default();
            area.runs = area.runs.max(area.failures);
        }
        check
            .areas
            .extend(catches.into_iter().map(|(area, count)| AreaStats {
                area: area.to_owned(),
                runs: count,
                failures: count,
            }));
    }
}

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

/// How many runs of a family [`startup`] needs before it estimates the family's start-up.
pub const STARTUP_MIN_RUNS: usize = 5;

/// The share of a family's runs that cost less than the start-up [`startup`] may estimate: a
/// start-up is never estimated above what the cheapest tenth of its commands cost in all.
const STARTUP_QUANTILE: f64 = 0.1;

/// The cost of starting one command of a family, estimated from its runs, each a number of
/// checks and what the whole command cost. A command costs about `start-up + checks x
/// marginal`, so the estimate is the intercept of the least-squares line through the runs,
/// at least zero and at most the [`STARTUP_QUANTILE`] quantile of the runs' costs, so that a
/// family whose checks differ in cost from batch to batch, which tilts the line, never has
/// more taken off than its cheap commands cost. `None` with fewer than [`STARTUP_MIN_RUNS`]
/// runs or with every run the same size, which say nothing about the intercept.
///
/// A check's recorded cost is its equal share of the command it ran in. Without taking the
/// start-up out first, every share carried a slice of it, so a family of cheap checks run 50
/// to a command looked many times as costly as it is.
pub fn startup(runs: &[(u32, f64)]) -> Option<f64> {
    let runs: Vec<(f64, f64)> = runs
        .iter()
        .filter(|(checks, cost)| *checks > 0 && cost.is_finite() && *cost >= 0.0)
        .map(|(checks, cost)| (f64::from(*checks), *cost))
        .collect();
    if runs.len() < STARTUP_MIN_RUNS {
        return None;
    }
    let count = runs.len() as f64;
    let mean_checks = runs.iter().map(|r| r.0).sum::<f64>() / count;
    let mean_cost = runs.iter().map(|r| r.1).sum::<f64>() / count;
    let spread: f64 = runs.iter().map(|r| (r.0 - mean_checks).powi(2)).sum();
    if spread <= 0.0 {
        return None;
    }
    let slope = runs
        .iter()
        .map(|r| (r.0 - mean_checks) * (r.1 - mean_cost))
        .sum::<f64>()
        / spread;
    let intercept = mean_cost - slope * mean_checks;
    let mut costs: Vec<f64> = runs.iter().map(|r| r.1).collect();
    costs.sort_by(f64::total_cmp);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let cheap = costs[(STARTUP_QUANTILE * (costs.len() - 1) as f64) as usize];
    Some(intercept.clamp(0.0, cheap))
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

    /// Runs of 10, 20 and 50 checks that each cost 2 to start and 0.5 a check.
    #[test]
    fn startup_is_the_intercept_of_cost_against_size() {
        let runs: Vec<(u32, f64)> = [10, 20, 50, 50, 20, 10]
            .iter()
            .map(|n| (*n, 2.0 + 0.5 * f64::from(*n)))
            .collect();
        assert!((startup(&runs).unwrap() - 2.0).abs() < 1e-9);
    }

    #[test]
    fn startup_needs_runs_of_several_sizes() {
        assert_eq!(startup(&[(50, 10.0); 4]), None, "too few runs");
        assert_eq!(startup(&[(50, 10.0); 8]), None, "one size says nothing");
    }

    /// The b3d-interface family's shape: batches of 50 whose cost varies with the recordings
    /// they play, and whole runs of thousands. The line through them starts high above what
    /// the cheapest batches cost in all, which bounds the start-up.
    #[test]
    fn startup_is_never_above_what_the_cheap_runs_cost() {
        let mut runs = vec![(50, 17.0), (50, 20.0), (50, 25.0), (50, 600.0), (50, 900.0)];
        runs.extend([(8884, 3725.0), (8884, 3773.0)]);
        let estimate = startup(&runs).unwrap();
        assert!((estimate - 17.0).abs() < 1e-9, "{estimate}");
        let falling = [(1, 9.0), (2, 8.0), (3, 7.0), (4, 6.0), (5, 5.0)];
        assert!((startup(&falling).unwrap() - 5.0).abs() < 1e-9);
        let rising_from_below = [(10, 1.0), (20, 3.0), (30, 5.0), (40, 7.0), (50, 9.0)];
        assert_eq!(startup(&rising_from_below), Some(0.0), "never below zero");
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
