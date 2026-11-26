//! `value-test`'s report: each predictor's log loss, Brier score and share of winners called,
//! overall and by the deciding entrant's budget, and the two differences against `search`
//! with intervals from resampling the groups.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use beryl_engine::rng::SplitMix64;
use serde::{Deserialize, Serialize};

use super::model::log_loss;
use super::{PREDICTORS, Sample};
use crate::match_log::rung_name;

/// Bootstrap resamples of the groups.
const RESAMPLES: usize = 2_000;

/// The most budgets the text lists, so that it stays within 20 lines.
const BUDGET_LINES: usize = 10;

/// How well one predictor did on some samples.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Score {
    pub samples: usize,
    pub log_loss: f64,
    pub brier: f64,
    /// The share of samples whose winner it called: a chance above one half for a winner,
    /// below for a loser, and half credit for exactly one half.
    pub called: f64,
}

impl Score {
    fn of(p: &[f64], won: &[bool]) -> Score {
        let n = p.len().max(1) as f64;
        let mut s = Score {
            samples: p.len(),
            log_loss: 0.0,
            brier: 0.0,
            called: 0.0,
        };
        for (&p, &y) in p.iter().zip(won) {
            s.log_loss += log_loss(p, y) / n;
            s.brier += (p - f64::from(u8::from(y))).powi(2) / n;
            let credit = match (p.total_cmp(&0.5), y) {
                (std::cmp::Ordering::Equal, _) => 0.5,
                (std::cmp::Ordering::Greater, true) | (std::cmp::Ordering::Less, false) => 1.0,
                _ => 0.0,
            };
            s.called += credit / n;
        }
        s
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Predictor {
    pub name: String,
    pub overall: Score,
    /// By the deciding entrant's budget, short (`3k`).
    pub by_budget: BTreeMap<String, Score>,
}

/// One predictor's log loss less `search`'s, with its 95% interval.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Difference {
    pub name: String,
    pub log_loss: f64,
    pub low: f64,
    pub high: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    pub seed: u64,
    pub samples: usize,
    pub games: usize,
    pub groups: usize,
    /// Searched decisions left out because the search had no estimate for the move played.
    pub unestimated: usize,
    pub predictors: Vec<Predictor>,
    pub differences: Vec<Difference>,
}

impl Report {
    pub fn new(
        samples: &[Sample],
        predictions: &[Vec<f64>; 4],
        unestimated: usize,
        seed: u64,
    ) -> Report {
        let won: Vec<bool> = samples.iter().map(|s| s.won).collect();
        let budgets: BTreeSet<u64> = samples.iter().map(|s| s.budget).collect();
        let predictors = PREDICTORS
            .iter()
            .zip(predictions)
            .map(|(&name, p)| {
                let by_budget = budgets
                    .iter()
                    .map(|&b| {
                        let at: Vec<usize> = (0..samples.len())
                            .filter(|&i| samples[i].budget == b)
                            .collect();
                        let p: Vec<f64> = at.iter().map(|&i| p[i]).collect();
                        let won: Vec<bool> = at.iter().map(|&i| won[i]).collect();
                        (rung_name(b), Score::of(&p, &won))
                    })
                    .collect();
                Predictor {
                    name: name.to_owned(),
                    overall: Score::of(p, &won),
                    by_budget,
                }
            })
            .collect();
        let groups: BTreeSet<&str> = samples.iter().map(|s| s.group.as_str()).collect();
        let games: BTreeSet<&str> = samples.iter().map(|s| s.game.as_str()).collect();
        let differences = [2, 3]
            .into_iter()
            .map(|j| {
                let diffs: Vec<f64> = (0..samples.len())
                    .map(|i| {
                        log_loss(predictions[j][i], won[i]) - log_loss(predictions[0][i], won[i])
                    })
                    .collect();
                let (low, high) = bootstrap(samples, &diffs, seed ^ j as u64);
                Difference {
                    name: format!("{} - search", PREDICTORS[j]),
                    log_loss: diffs.iter().sum::<f64>() / diffs.len() as f64,
                    low,
                    high,
                }
            })
            .collect();
        Report {
            seed,
            samples: samples.len(),
            games: games.len(),
            groups: groups.len(),
            unestimated,
            predictors,
            differences,
        }
    }

    pub fn json(&self) -> String {
        serde_json::to_string_pretty(self).expect("a report serialises") + "\n"
    }

    /// The report as at most 20 lines of text.
    pub fn text(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(
            out,
            "{} samples from {} games in {} groups; {} searched decisions had no estimate",
            self.samples, self.games, self.groups, self.unestimated
        );
        let _ = writeln!(out, "predictor    log loss  Brier   called");
        for p in &self.predictors {
            let s = p.overall;
            let _ = writeln!(
                out,
                "{:<11}  {:.4}    {:.4}  {:.1}%",
                p.name,
                s.log_loss,
                s.brier,
                100.0 * s.called
            );
        }
        let budgets: Vec<&String> = self.predictors[0].by_budget.keys().collect();
        if budgets.len() > 1 {
            let _ = writeln!(out, "log loss by budget: {}", PREDICTORS.join(", "));
            for b in budgets.iter().take(BUDGET_LINES) {
                let losses: Vec<String> = self
                    .predictors
                    .iter()
                    .map(|p| format!("{:.4}", p.by_budget[*b].log_loss))
                    .collect();
                let n = self.predictors[0].by_budget[*b].samples;
                let _ = writeln!(out, "  {b:<6} ({n}): {}", losses.join("  "));
            }
            if budgets.len() > BUDGET_LINES {
                let _ = writeln!(
                    out,
                    "  and {} more budgets in value.json",
                    budgets.len() - BUDGET_LINES
                );
            }
        }
        for d in &self.differences {
            let _ = writeln!(
                out,
                "{}: log loss {:+.4} (95% interval {:+.4} to {:+.4})",
                d.name, d.log_loss, d.low, d.high
            );
        }
        out
    }
}

/// The 95% interval of the mean of `diffs` from resampling the samples' groups with
/// replacement.
fn bootstrap(samples: &[Sample], diffs: &[f64], seed: u64) -> (f64, f64) {
    let mut by_group: BTreeMap<&str, (f64, f64)> = BTreeMap::new();
    for (s, d) in samples.iter().zip(diffs) {
        let e = by_group.entry(s.group.as_str()).or_default();
        e.0 += d;
        e.1 += 1.0;
    }
    let groups: Vec<(f64, f64)> = by_group.into_values().collect();
    let mut rng = SplitMix64::new(seed);
    let mut means: Vec<f64> = (0..RESAMPLES)
        .map(|_| {
            let (mut sum, mut n) = (0.0, 0.0);
            for _ in 0..groups.len() {
                let (s, c) = groups[rng.below(groups.len() as u64) as usize];
                sum += s;
                n += c;
            }
            sum / n
        })
        .collect();
    means.sort_by(f64::total_cmp);
    let at = |q: f64| means[((q * RESAMPLES as f64) as usize).min(RESAMPLES - 1)];
    (at(0.025), at(0.975))
}
