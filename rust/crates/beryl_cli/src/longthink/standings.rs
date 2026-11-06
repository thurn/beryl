//! Where each matchup stands: its finished games tallied once, read by the schedule, the
//! summary and the live panel alike, so every view of a run counts and names it one way.

use std::collections::{BTreeMap, HashMap};

use super::GameRecord;
use super::Verdict;
use super::schedule::{Against, Config, Matchup};

/// A rung's results in some games: a draw or a stall counts half, a crash not at all.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Score {
    pub wins: u32,
    pub draws: u32,
    pub losses: u32,
}

impl Score {
    fn add(&mut self, v: Verdict) {
        match v {
            Verdict::Win => self.wins += 1,
            Verdict::Draw | Verdict::Stalled => self.draws += 1,
            Verdict::Loss => self.losses += 1,
            Verdict::Crashed => {}
        }
    }

    /// The games scored: those that did not crash.
    pub fn scored(&self) -> u32 {
        self.wins + self.draws + self.losses
    }

    /// The rung's points.
    pub fn points(&self) -> f64 {
        f64::from(self.wins) + 0.5 * f64::from(self.draws)
    }

    /// The Elo difference the score implies; none before a scored game.
    pub fn elo(&self) -> Option<f64> {
        (self.scored() > 0).then(|| elo(self.points(), self.scored()))
    }
}

/// A matchup's finished games, from its rung's side.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Standing {
    /// Games finished, crashed ones included: a crashed game is not replayed.
    pub games: u32,
    pub score: Score,
    /// The score in the first pairs alone, the games the chain rule reads.
    pub first: Score,
    /// The searched decisions, the fresh rollouts they spent and what their budgets allowed.
    pub searched: u64,
    pub rollouts: u64,
    pub budget: u64,
    /// The games with a wall time, and their seconds.
    pub clocked: u32,
    pub seconds: f64,
}

impl Standing {
    /// Mean wall seconds a game; none before a timed one.
    pub fn mean_seconds(&self) -> Option<f64> {
        (self.clocked > 0).then(|| self.seconds / f64::from(self.clocked))
    }
}

/// Every matchup begun, and where it stands.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Standings(BTreeMap<Matchup, Standing>);

impl Standings {
    /// The standings after `games`, each with its wall seconds if `clock` has them.
    pub fn new(c: &Config, games: &[GameRecord], clock: &HashMap<String, f64>) -> Standings {
        let mut s = Standings::default();
        for g in games {
            s.record(c, g, clock.get(&g.game).copied());
        }
        s
    }

    /// Counts finished game `g`, which took `seconds`.
    pub fn record(&mut self, c: &Config, g: &GameRecord, seconds: Option<f64>) {
        let s = self.0.entry(g.matchup).or_default();
        s.games += 1;
        s.score.add(g.outcome);
        if g.pair < c.pairs {
            s.first.add(g.outcome);
        }
        s.searched += u64::from(g.searched);
        s.rollouts += g.rollouts;
        s.budget += g.budget;
        if let Some(t) = seconds {
            s.clocked += 1;
            s.seconds += t;
        }
    }

    /// Matchup `m`'s standing, empty if it has not begun.
    pub fn get(&self, m: Matchup) -> Standing {
        self.0.get(&m).copied().unwrap_or_default()
    }

    /// The matchups begun, in order of rung.
    pub fn iter(&self) -> impl Iterator<Item = (Matchup, &Standing)> {
        self.0.iter().map(|(&m, s)| (m, s))
    }

    /// Elo per tripling of the budget against the opponent, and its 95% margin: the
    /// weighted least-squares slope of each rung's Elo against log3 of its budget. None
    /// before two budgets have scored games.
    pub fn climb(&self, c: &Config) -> Option<(f64, f64)> {
        let points: Vec<(f64, f64, f64)> = self
            .iter()
            .filter(|(m, s)| m.against == Against::Opponent && s.score.scored() > 0)
            .map(|(m, s)| {
                let n = f64::from(s.score.scored());
                let p = (s.score.points() / n).clamp(0.5 / n, 1.0 - 0.5 / n);
                // The delta method: Elo is 400 log10(p / (1 - p)), p's variance p(1 - p) / n.
                let se = 400.0 / (std::f64::consts::LN_10 * (n * p * (1.0 - p)).sqrt());
                (
                    f64::from(c.iterations(m.rung)).log(3.0),
                    elo(s.score.points(), s.score.scored()),
                    se,
                )
            })
            .collect();
        slope(&points)
    }
}

/// The Elo difference a score of `score` in `n` games implies, the score kept half a game
/// from either end so a clean sweep stays finite.
pub fn elo(score: f64, n: u32) -> f64 {
    let n = f64::from(n.max(1));
    let p = (score / n).clamp(0.5 / n, 1.0 - 0.5 / n);
    400.0 * (p / (1.0 - p)).log10()
}

/// Elo per tripling as the summary and the panel print it: `+103 ±47`.
pub fn climb_text((slope, margin): (f64, f64)) -> String {
    // Rounded first, so that a slope a hair below zero reads +0, not -0; adding zero turns
    // the -0 rounding leaves into 0.
    let slope = slope.round() + 0.0;
    format!("{slope:+.0} ±{margin:.0}")
}

/// The weighted least-squares slope of y against x, and its 95% margin, from (x, y,
/// standard error) points; none from fewer than two distinct x.
fn slope(points: &[(f64, f64, f64)]) -> Option<(f64, f64)> {
    let w: Vec<f64> = points.iter().map(|&(_, _, se)| 1.0 / (se * se)).collect();
    let total: f64 = w.iter().sum();
    let mean = |f: fn(&(f64, f64, f64)) -> f64| {
        points.iter().zip(&w).map(|(p, w)| w * f(p)).sum::<f64>() / total
    };
    let (x, y) = (mean(|p| p.0), mean(|p| p.1));
    let sxx: f64 = points
        .iter()
        .zip(&w)
        .map(|(p, w)| w * (p.0 - x).powi(2))
        .sum();
    if points.len() < 2 || sxx <= 0.0 {
        return None;
    }
    let sxy: f64 = points
        .iter()
        .zip(&w)
        .map(|(p, w)| w * (p.0 - x) * (p.1 - y))
        .sum();
    Some((sxy / sxx, 1.96 / sxx.sqrt()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-3
    }

    /// Hand-computed: 400 log10(0.75 / 0.25) = 400 log10 3; a sweep of 60 counts as
    /// 59.5 of 60, 400 log10 119.
    #[test]
    fn elo_of_a_score() {
        assert!(close(elo(45.0, 60), 190.848_5));
        assert!(close(elo(30.0, 60), 0.0));
        assert!(close(elo(60.0, 60), 830.218_8));
        assert!(close(elo(0.0, 60), -830.218_8));
    }

    /// Hand-computed: two budgets a tripling apart, 100 Elo between them, each known to a
    /// standard error of 10, climb 100 per tripling, ±1.96 √200; a third point on the line
    /// changes nothing but the margin.
    #[test]
    fn elo_per_tripling_is_a_weighted_slope() {
        let (s, margin) = slope(&[(0.0, 0.0, 10.0), (1.0, 100.0, 10.0)]).unwrap();
        assert!((s - 100.0).abs() < 1e-9 && (margin - 27.718).abs() < 1e-3);
        let (s, margin) =
            slope(&[(0.0, 0.0, 10.0), (1.0, 100.0, 10.0), (2.0, 200.0, 10.0)]).unwrap();
        assert!((s - 100.0).abs() < 1e-9 && margin < 27.0);
        assert_eq!(slope(&[(1.0, 50.0, 10.0)]), None);
        assert_eq!(slope(&[(1.0, 0.0, 10.0), (1.0, 50.0, 5.0)]), None);
    }
}
