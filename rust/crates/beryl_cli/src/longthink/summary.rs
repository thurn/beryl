//! summary.txt: what the run has learned so far, derived from its logs alone, so it can be
//! rebuilt whenever it is missing.

use std::collections::HashMap;
use std::fmt::Write;

use super::schedule::{Against, Config};
use super::standings::{Standing, Standings, climb_text};
use super::{GameRecord, Verdict};
use crate::tournament::wilson;

/// An Elo gain from budget `from` to budget `to`, per tripling of the budget.
pub fn per_tripling(gain: f64, from: u32, to: u32) -> f64 {
    gain / (f64::from(to) / f64::from(from)).log(3.0)
}

/// A matchup's line of the summary, under `label`.
fn line(s: &Standing, label: &str) -> String {
    let n = s.score.scored();
    let [lo, hi] = wilson(s.score.points(), n);
    let per_game = |total: f64| total / f64::from(s.games.max(1));
    format!(
        "{:<30} {:>5} {:>13}  {:>5.1}% [{:>4.1}, {:>5.1}] {:>6} {:>8.0} {:>8} {:>6} {:>7.1}",
        label,
        s.games,
        format!("{}-{}-{}", s.score.wins, s.score.draws, s.score.losses),
        100.0 * s.score.points() / f64::from(n.max(1)),
        100.0 * lo,
        100.0 * hi,
        s.score.elo().map_or("-".to_owned(), |e| format!("{e:+.0}")),
        per_game(s.searched as f64),
        rollouts(s.rollouts as f64 / s.searched.max(1) as f64),
        (s.budget > 0)
            .then_some(1.0 - s.rollouts as f64 / s.budget as f64)
            .map_or("-".to_owned(), |x| format!("{:.0}%", 100.0 * x)),
        per_game(s.seconds) / 60.0,
    )
}

/// The summary of run `name`'s games, with each game's wall seconds: per matchup its score,
/// searched decisions a game, fresh rollouts a searched decision, the share of their budgets
/// the exact stop and subtree reuse saved, and minutes a game; then Elo per tripling of the
/// budget against the opponent, as the live panel shows it, and each chained rung's Elo
/// against the rung below.
pub fn text(
    name: &str,
    c: &Config,
    games: &[GameRecord],
    seconds: &HashMap<String, f64>,
) -> String {
    let standings = Standings::new(c, games, seconds);
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
    for (m, s) in standings.iter() {
        let _ = writeln!(out, "{}", line(s, &m.label(c)));
    }
    if let Some(climb) = standings.climb(c) {
        let _ = writeln!(
            out,
            "\nelo per tripling, v {}: {}",
            c.opponent,
            climb_text(climb)
        );
    }
    let chain: Vec<String> = standings
        .iter()
        .filter(|(m, _)| m.against == Against::Below)
        .filter_map(|(m, s)| {
            let e = s.score.elo()?;
            let (rung, below) = m.sides(c);
            let gain = per_tripling(e, c.iterations(m.rung - 1), c.iterations(m.rung));
            Some(format!("{below}→{rung} {e:+.0} ({gain:+.0} per tripling)"))
        })
        .collect();
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
    use crate::longthink::schedule::Matchup;
    use crate::longthink::standings::elo;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-3
    }

    /// Hand-computed: 45 of 60 is 400 log10 3 Elo; from 3k to 10k is log3(10/3) = 1.0959
    /// triplings.
    #[test]
    fn elo_per_tripling_between_two_rungs() {
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
            matchup: Matchup {
                rung: 0,
                against: Against::Opponent,
            },
            iterations: 3_000,
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
