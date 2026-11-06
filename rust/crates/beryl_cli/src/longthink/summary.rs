//! summary.txt: what the run has learned so far, derived from its logs alone, so it can be
//! rebuilt whenever it is missing.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write;

use super::schedule::{Against, Config};
use super::{GameRecord, Verdict, matchup_label};
use crate::tournament::wilson;

/// The Elo difference a score of `score` in `n` games implies, the score kept half a game
/// from either end so a clean sweep stays finite.
pub fn elo(score: f64, n: u32) -> f64 {
    let n = f64::from(n.max(1));
    let p = (score / n).clamp(0.5 / n, 1.0 - 0.5 / n);
    400.0 * (p / (1.0 - p)).log10()
}

/// An Elo gain from budget `from` to budget `to`, per tripling of the budget.
pub fn per_tripling(gain: f64, from: u32, to: u32) -> f64 {
    gain / (f64::from(to) / f64::from(from)).log(3.0)
}

/// A matchup's games, from its rung's side.
#[derive(Clone, Copy, Debug, Default)]
struct Row {
    games: u32,
    wins: u32,
    draws: u32,
    losses: u32,
    crashed: u32,
    rollouts: u64,
    searched: u64,
    seconds: f64,
}

impl Row {
    fn scored(&self) -> u32 {
        self.wins + self.draws + self.losses
    }

    fn score(&self) -> f64 {
        f64::from(self.wins) + 0.5 * f64::from(self.draws)
    }

    fn elo(&self) -> Option<f64> {
        (self.scored() > 0).then(|| elo(self.score(), self.scored()))
    }
}

/// The summary of run `name`'s games, with each game's wall seconds.
pub fn text(
    name: &str,
    c: &Config,
    games: &[GameRecord],
    seconds: &HashMap<String, f64>,
) -> String {
    let mut rows: BTreeMap<(usize, Against), Row> = BTreeMap::new();
    for g in games {
        let row = rows.entry((g.rung, g.against)).or_default();
        row.games += 1;
        match g.outcome {
            Verdict::Win => row.wins += 1,
            Verdict::Loss => row.losses += 1,
            Verdict::Draw | Verdict::Stalled => row.draws += 1,
            Verdict::Crashed => row.crashed += 1,
        }
        row.rollouts += g.rollouts;
        row.searched += u64::from(g.searched);
        row.seconds += seconds.get(&g.game).copied().unwrap_or(0.0);
    }
    let mut out = format!(
        "{name}: {} games, {} trees, seed {}\n\n",
        games.len(),
        c.trees,
        c.seed
    );
    let _ = writeln!(
        out,
        "{:<30} {:>5} {:>13}  {:<20} {:>6} {:>9} {:>8} {:>7}",
        "matchup", "games", "w-d-l", "score (95%)", "elo", "rollouts", "searched", "min"
    );
    for (&(rung, against), row) in &rows {
        let n = row.scored();
        let [lo, hi] = wilson(row.score(), n);
        let per_game = |total: f64| total / f64::from(row.games.max(1));
        let _ = writeln!(
            out,
            "{:<30} {:>5} {:>13}  {:>5.1}% [{:>4.1}, {:>5.1}] {:>6} {:>9} {:>8.0} {:>7.1}",
            matchup_label(c, rung, against),
            row.games,
            format!("{}-{}-{}", row.wins, row.draws, row.losses),
            100.0 * row.score() / f64::from(n.max(1)),
            100.0 * lo,
            100.0 * hi,
            row.elo().map_or("-".to_owned(), |e| format!("{e:+.0}")),
            rollouts(per_game(row.rollouts as f64)),
            per_game(row.searched as f64),
            per_game(row.seconds) / 60.0,
        );
    }
    let against = |rung, a| rows.get(&(rung, a)).and_then(Row::elo);
    let mut climb = Vec::new();
    let mut chain = Vec::new();
    for (&(rung, a), row) in &rows {
        if rung == 0 {
            continue;
        }
        let (from, to) = (c.iterations(rung - 1), c.iterations(rung));
        let step = format!("{}→{}", super::rung_name(from), super::rung_name(to));
        match a {
            Against::Opponent => {
                if let (Some(lo), Some(hi)) = (
                    against(rung - 1, Against::Opponent),
                    against(rung, Against::Opponent),
                ) {
                    climb.push(format!("{step} {:+.0}", per_tripling(hi - lo, from, to)));
                }
            }
            Against::Below => {
                if let Some(e) = row.elo() {
                    chain.push(format!(
                        "{step} {e:+.0} ({:+.0} per tripling)",
                        per_tripling(e, from, to)
                    ));
                }
            }
            Against::Omniscient => {}
        }
    }
    if !climb.is_empty() {
        let _ = writeln!(
            out,
            "\nelo per tripling, v {}: {}",
            c.opponent,
            climb.join(", ")
        );
    }
    if !chain.is_empty() {
        let _ = writeln!(out, "rung v rung below: {}", chain.join(", "));
    }
    let crashed: Vec<String> = games
        .iter()
        .filter(|g| g.outcome == Verdict::Crashed)
        .map(|g| {
            format!(
                "{} (seed {}, decision {})",
                g.game,
                g.seed,
                g.crashed_at.unwrap_or_default()
            )
        })
        .collect();
    if !crashed.is_empty() {
        let _ = writeln!(out, "crashed: {}", crashed.join(", "));
    }
    out
}

/// A count of rollouts, short.
fn rollouts(n: f64) -> String {
    if n >= 1e6 {
        format!("{:.1}M", n / 1e6)
    } else if n >= 1e3 {
        format!("{:.0}k", n / 1e3)
    } else {
        format!("{n:.0}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-3
    }

    /// Hand-computed: 400 log10(0.75 / 0.25) = 400 log10 3; a sweep of 60 counts as
    /// 59.5 of 60, 400 log10 119; from 3k to 10k is log3(10/3) = 1.0959 triplings.
    #[test]
    fn elo_and_its_rate_per_tripling() {
        assert!(close(elo(45.0, 60), 190.848_5));
        assert!(close(elo(30.0, 60), 0.0));
        assert!(close(elo(60.0, 60), 830.218_8));
        assert!(close(elo(0.0, 60), -830.218_8));
        assert!(close(per_tripling(elo(45.0, 60), 3_000, 10_000), 174.147_2));
        assert!(close(per_tripling(100.0, 1_000, 3_000), 100.0));
    }

    /// Hand-computed with z = 1.96: 45 of 60 is [0.6277, 0.8422], and a draw counts half.
    #[test]
    fn wilson_intervals_of_ladder_scores() {
        let [lo, hi] = wilson(45.0, 60);
        assert!(close(lo, 0.627_7) && close(hi, 0.842_2), "{lo} {hi}");
        let [lo, hi] = wilson(52.5, 60);
        assert!(close(lo, 0.768_2) && close(hi, 0.936_6), "{lo} {hi}");
    }
}
