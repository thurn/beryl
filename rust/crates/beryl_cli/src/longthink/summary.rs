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
    budget: u64,
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

    /// The row's line of the summary, under `label`.
    fn line(&self, label: &str) -> String {
        let n = self.scored();
        let [lo, hi] = wilson(self.score(), n);
        let per_game = |total: f64| total / f64::from(self.games.max(1));
        format!(
            "{:<30} {:>5} {:>13}  {:>5.1}% [{:>4.1}, {:>5.1}] {:>6} {:>8.0} {:>8} {:>6} {:>7.1}",
            label,
            self.games,
            format!("{}-{}-{}", self.wins, self.draws, self.losses),
            100.0 * self.score() / f64::from(n.max(1)),
            100.0 * lo,
            100.0 * hi,
            self.elo().map_or("-".to_owned(), |e| format!("{e:+.0}")),
            per_game(self.searched as f64),
            rollouts(self.rollouts as f64 / self.searched.max(1) as f64),
            (self.budget > 0)
                .then_some(1.0 - self.rollouts as f64 / self.budget as f64)
                .map_or("-".to_owned(), |s| format!("{:.0}%", 100.0 * s)),
            per_game(self.seconds) / 60.0,
        )
    }
}

/// The summary of run `name`'s games, with each game's wall seconds: per matchup its score,
/// searched decisions a game, fresh rollouts a searched decision, the share of their budgets
/// the exact stop and subtree reuse saved, and minutes a game.
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
        row.budget += g.budget;
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
        "{:<30} {:>5} {:>13}  {:<20} {:>6} {:>8} {:>8} {:>6} {:>7}",
        "matchup", "games", "w-d-l", "score (95%)", "elo", "searched", "rollouts", "saved", "min"
    );
    for (&(rung, against), row) in &rows {
        let _ = writeln!(out, "{}", row.line(&matchup_label(c, rung, against)));
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
    } else if n >= 1e4 {
        format!("{:.0}k", n / 1e3)
    } else if n >= 1e3 {
        format!("{:.1}k", n / 1e3)
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

    /// Two 3k games of 100 searched decisions each, 300k of budget a game, that spent 180k
    /// and 120k fresh rollouts: 1.5k a decision, and half the budget saved.
    #[test]
    fn rows_show_rollouts_a_decision_and_the_saving() {
        let c = Config::default();
        let game = |seat: u8, outcome, rollouts| GameRecord {
            game: format!("g{seat}"),
            rung: 0,
            iterations: 3_000,
            against: Against::Opponent,
            pair: 0,
            seed: 0,
            seat,
            outcome,
            turns: 20,
            decisions: 300,
            searched: 100,
            rollouts,
            budget: 300_000,
            crashed_at: None,
        };
        let games = [
            game(0, Verdict::Win, 180_000),
            game(1, Verdict::Loss, 120_000),
        ];
        let text = text("run-001", &c, &games, &HashMap::new());
        let row = text
            .lines()
            .find(|l| l.starts_with("3k v expert-frozen"))
            .expect(&text);
        let cells: Vec<&str> = row.split_whitespace().collect();
        assert_eq!(cells[3..6], ["2", "1-0-1", "50.0%"], "{text}");
        assert_eq!(
            cells[cells.len() - 4..],
            ["100", "1.5k", "50%", "0.0"],
            "{text}"
        );
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
