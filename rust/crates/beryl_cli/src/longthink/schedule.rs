//! Which game the arena plays next: a pure function of the run's configuration and the games
//! it has finished, so a resumed run picks up the very game a stop interrupted.
//!
//! - An entrant is a fixed agent (`expert-frozen`), or a search family at a budget of
//!   engine steps a decision (`ismcts18/hidden/rollout/tuned@3M`). Each family opens at the
//!   first of the configured budgets; the next opens once the last has played
//!   [`OPEN_GAMES`] games, and past the configured ones (or the first two, with one
//!   configured), ×3 each, only while the last step up still gained rating with 80%
//!   confidence. So a family that stops climbing stops
//!   growing, and one that resumes climbing grows again.
//! - Every game is one of a seed pair: the same deal twice, each side taking each seat in
//!   turn. Pair `p` has the same seed in every matchup, so entrants compare pair by pair,
//!   and a matchup finishes a pair before anything else is played.
//! - Otherwise the next pair is the one that buys the most information per engine step:
//!   for each pair of open entrants, the drop in the variance of each side's rating that two
//!   games would bring (a Kalman update on their difference, through the rating pool's
//!   covariance), each weighted by how much that rating matters, over what the two cost a
//!   game. A search's rating matters by its chance of being the best of all the entrants,
//!   plus that of being the best fair one (seeing only what a player sees: the cheats would
//!   otherwise take every game spent on the top), plus that of being the best of its family,
//!   plus [`FLOOR`], so every curve keeps filling in slowly while the contenders for the top
//!   are resolved. A fixed agent's own rating
//!   matters not at all: it is a reference the searches are measured against, and its games
//!   cost a thousandth of a search's, so precision on it alone would buy every game.

use serde::{Deserialize, Serialize};

use super::ratings::{K, Ratings};
use super::rung_name;

/// A run's settings, fixed when it starts but for families added since (`--add`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    /// The search families, as `beryl_search::Family` prints them.
    pub families: Vec<String>,
    /// The agents of the registry that play without searching.
    pub fixed: Vec<String>,
    /// The fixed agent rated 0.
    pub anchor: String,
    /// The first budgets, in engine steps a decision; each after them triples the last.
    pub budgets: Vec<u64>,
    /// The threads a search grows its trees on; the moves do not depend on it.
    pub threads: u32,
    /// The seed the pairs' seeds derive from.
    pub seed: u64,
}

/// The families a new run plays: the WU Control mirror's agent as Milestone 1 left it
/// (`pimc18/hidden/rollout/tuned`), and from information-set search with full rollouts, one
/// axis changed at a time: how many worlds, what it sees, how a leaf is scored, and where card
/// values come from. Seeing only the hands is left out, a split of what hiding costs that the
/// omniscient twins already bound, and so are leaves scored by the evaluation alone, the
/// weakest at every budget to 10M in the first runs: `cut2` keeps evaluated leaves here.
pub const FAMILIES: [&str; 8] = [
    "pimc18/hidden/rollout/tuned",
    "pimc72/hidden/rollout/tuned",
    "pimc288/hidden/rollout/tuned",
    "pimc18/omniscient/rollout/tuned",
    "ismcts18/hidden/rollout/tuned",
    "ismcts18/omniscient/rollout/tuned",
    "ismcts18/hidden/cut2/tuned",
    "ismcts18/hidden/rollout/estimate",
];

impl Default for Config {
    /// The long-think arena: [`FAMILIES`] at 1M to 30M engine steps a decision (about
    /// 2,800 to 80,000 mid-game rollouts) and up, the frozen expert and greedy, 18 threads.
    fn default() -> Self {
        Config {
            families: FAMILIES.map(str::to_owned).to_vec(),
            fixed: vec!["expert-frozen".to_owned(), "greedy".to_owned()],
            anchor: "expert-frozen".to_owned(),
            budgets: vec![1_000_000, 3_000_000, 10_000_000, 30_000_000],
            threads: 18,
            seed: 0,
        }
    }
}

impl Config {
    /// Budget `k`'s engine steps a decision.
    pub fn budget(&self, k: usize) -> u64 {
        let last = self.budgets.len() - 1;
        if k <= last {
            self.budgets[k]
        } else {
            self.budgets[last].saturating_mul(3u64.saturating_pow((k - last) as u32))
        }
    }

    /// Family `f` at budget `k`, as an entrant's name.
    pub fn entrant(&self, f: &str, k: usize) -> String {
        format!("{f}@{}", rung_name(self.budget(k)))
    }
}

/// An entrant's family: its name before the budget, or a fixed agent's name.
pub fn family_of(e: &str) -> &str {
    e.rsplit_once('@').map_or(e, |(f, _)| f)
}

/// Two entrants who play, `a` first by name.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Matchup {
    pub a: String,
    pub b: String,
}

impl Matchup {
    pub fn new(x: &str, y: &str) -> Matchup {
        let (a, b) = if x <= y { (x, y) } else { (y, x) };
        Matchup {
            a: a.to_owned(),
            b: b.to_owned(),
        }
    }

    /// `a v b`.
    pub fn label(&self) -> String {
        format!("{} v {}", self.a, self.b)
    }

    /// The prefix of its games' keys, fit for a file name: `a~b`, slashes as dots.
    pub fn key(&self) -> String {
        format!("{}~{}", self.a, self.b).replace('/', ".")
    }
}

/// Games the last open budget of a family plays before the next opens.
pub const OPEN_GAMES: u32 = 16;

/// The confidence a step up the budget must have gained rating with for a family to grow
/// past its configured budgets.
pub const CLIMBING: f64 = 0.8;

/// What every rating's precision is worth, whatever its chance of being best.
pub const FLOOR: f64 = 0.05;

/// What a fixed agent's game is taken to cost, in engine steps: its games take milliseconds.
const FIXED_COST: f64 = 50_000.0;

/// Searched decisions a seat makes in a game, for the cost of an entrant not yet played.
const SEARCHED: f64 = 66.0;

/// The entrants open after these ratings: every fixed agent, then each family's budgets up
/// to its last open one.
pub fn open(c: &Config, r: &Ratings) -> Vec<String> {
    let mut out = c.fixed.clone();
    for f in &c.families {
        let mut k = 0;
        loop {
            let e = c.entrant(f, k);
            out.push(e.clone());
            if r.tally(&e).games < OPEN_GAMES {
                break;
            }
            // With one configured budget, the second opens as the configured ones do: a
            // step up is measured only once there is one.
            let more = k + 1 < c.budgets.len().max(2) || {
                let (gain, sd) = r.diff(&e, &c.entrant(f, k - 1));
                sd > 0.0 && phi(gain / sd) >= CLIMBING
            };
            if !more {
                break;
            }
            k += 1;
        }
    }
    out
}

/// The standard normal distribution function.
pub fn phi(z: f64) -> f64 {
    f64::midpoint(1.0, erf(z / std::f64::consts::SQRT_2))
}

/// The error function, to 1.5e-7 (Abramowitz and Stegun 7.1.26).
fn erf(x: f64) -> f64 {
    let t = 1.0 / (1.0 + 0.327_591_1 * x.abs());
    let y = 1.0
        - (((((1.061_405_429 * t - 1.453_152_027) * t) + 1.421_413_741) * t - 0.284_496_736) * t
            + 0.254_829_592)
            * t
            * (-x * x).exp();
    if x >= 0.0 { y } else { -y }
}

/// What an entrant's game costs, in engine steps: its mean, or for one not yet played, its
/// budget for each searched decision of a seat.
pub fn cost(c: &Config, r: &Ratings, e: &str) -> f64 {
    if c.fixed.iter().any(|f| f == e) {
        return FIXED_COST;
    }
    r.tally(e).work_per_game().map_or_else(
        || budget_of(e).map_or(FIXED_COST, |b| b as f64 * SEARCHED),
        |w| w.max(FIXED_COST),
    )
}

/// A search entrant's budget, from its name.
pub fn budget_of(e: &str) -> Option<u64> {
    let (_, b) = e.rsplit_once('@')?;
    parse_work(b)
}

/// A budget as [`rung_name`] prints it, read back: `300k`, `3M`, `1G`.
pub fn parse_work(s: &str) -> Option<u64> {
    let (digits, scale) = match s.as_bytes().last()? {
        b'k' => (&s[..s.len() - 1], 1_000),
        b'M' => (&s[..s.len() - 1], 1_000_000),
        b'G' => (&s[..s.len() - 1], 1_000_000_000),
        _ => (s, 1),
    };
    digits.parse::<u64>().ok()?.checked_mul(scale)
}

/// The matchup whose next game the arena plays, after `games` (whose ratings `r` are) and
/// while the games of `busy` play.
pub fn next(c: &Config, r: &Ratings, games: usize, busy: &[Matchup]) -> Matchup {
    // A matchup finishes the pair it began, counting the games being played.
    let odd = |a: &str, b: &str| {
        let playing = busy.iter().filter(|m| m.a == a && m.b == b).count() as u32;
        (r.played(a, b) + playing) % 2 == 1
    };
    if let Some(m) = busy.iter().find(|m| odd(&m.a, &m.b)) {
        return m.clone();
    }
    let names = r.names();
    for a in names {
        for b in names {
            if a < b && odd(a, b) {
                return Matchup::new(a, b);
            }
        }
    }
    let open = open(c, r);
    let best = r.best(&open, games as u64);
    let weight: Vec<f64> = open
        .iter()
        .zip(&best)
        .map(|(e, chance)| {
            if c.fixed.contains(e) {
                0.0
            } else {
                FLOOR + chance.overall + chance.fair + chance.family
            }
        })
        .collect();
    let costs: Vec<f64> = open.iter().map(|e| cost(c, r, e)).collect();
    let mut choice = (0, 1, f64::NEG_INFINITY);
    for i in 0..open.len() {
        for j in i + 1..open.len() {
            let (x, y) = (&open[i], &open[j]);
            let (d, _) = r.diff(x, y);
            let p = 1.0 / (1.0 + (-K * d).exp());
            let info = 2.0 * K * K * p * (1.0 - p);
            let (vx, cxy) = r.var_cov(x, y);
            let (vy, _) = r.var_cov(y, x);
            let var_d = (vx + vy - 2.0 * cxy).max(0.0);
            let shrink = info / (1.0 + info * var_d);
            let gain =
                weight[i] * (vx - cxy).powi(2) * shrink + weight[j] * (vy - cxy).powi(2) * shrink;
            let value = gain / (costs[i] + costs[j]);
            if value > choice.2 {
                choice = (i, j, value);
            }
        }
    }
    Matchup::new(&open[choice.0], &open[choice.1])
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::longthink::{GameRecord, Verdict};

    fn config() -> Config {
        Config {
            families: vec!["ismcts2/hidden/rollout/tuned".to_owned()],
            fixed: vec!["expert".to_owned(), "greedy".to_owned()],
            anchor: "expert".to_owned(),
            budgets: vec![1_000, 3_000],
            threads: 1,
            seed: 0,
        }
    }

    /// Plays `n` games of the schedule, game `g` of matchup `m` ending `outcome(m, g)` for
    /// its first side, and returns the matchups in the order played.
    fn play(
        c: &Config,
        n: usize,
        outcome: impl Fn(&Matchup, u32) -> Verdict,
    ) -> (Vec<Matchup>, Vec<GameRecord>) {
        let mut games: Vec<GameRecord> = Vec::new();
        let mut played = Vec::new();
        for _ in 0..n {
            let r = Ratings::new(c, &games, &HashMap::new(), &[]);
            let m = next(c, &r, games.len(), &[]);
            let g = r.played(&m.a, &m.b);
            games.push(GameRecord::example(&m, g, outcome(&m, g)));
            played.push(m);
        }
        (played, games)
    }

    #[test]
    fn budgets_triple_after_the_configured_ones() {
        let c = config();
        let b: Vec<u64> = (0..4).map(|k| c.budget(k)).collect();
        assert_eq!(b, [1_000, 3_000, 9_000, 27_000]);
        assert_eq!(c.entrant("f", 2), "f@9k");
        assert_eq!(budget_of("a/b/c/d@9k"), Some(9_000));
        assert_eq!(family_of("a/b/c/d@9k"), "a/b/c/d");
        assert_eq!(
            [parse_work("3M"), parse_work("12"), parse_work("x")],
            [Some(3_000_000), Some(12), None]
        );
    }

    /// Pairs are played through, and the first games go to the entrants nothing is known
    /// about yet, against something cheap.
    #[test]
    fn pairs_are_finished_and_the_unknown_go_first() {
        let c = config();
        let (played, _) = play(&c, 12, |_, _| Verdict::Draw);
        for p in played.chunks(2) {
            assert_eq!(p[0], p[1], "{played:?}");
        }
        assert!(
            played
                .iter()
                .all(|m| m.a.contains('@') || m.b.contains('@')),
            "fixed agents never play each other alone: {played:?}"
        );
    }

    /// A game being played counts toward its pair, so the next game finishes it.
    #[test]
    fn games_being_played_count_toward_their_pair() {
        let c = config();
        let r = Ratings::new(&c, &[], &HashMap::new(), &[]);
        let first = next(&c, &r, 0, &[]);
        let other = Matchup::new("expert", "ismcts2/hidden/rollout/tuned@3k");
        assert_ne!(first, other);
        let busy = [other.clone(), first.clone(), other.clone()];
        assert_eq!(next(&c, &r, 0, &busy), first);
    }

    /// A family's next budget opens once the last has its games; past the configured ones,
    /// only while the step up gains rating.
    #[test]
    fn a_family_grows_while_it_climbs() {
        let c = config();
        let f = &c.families[0];
        let strong = |m: &Matchup, _| {
            // The higher budget wins; a search beats a fixed agent.
            let rank = |e: &str| budget_of(e).unwrap_or(0);
            match rank(&m.a).cmp(&rank(&m.b)) {
                std::cmp::Ordering::Greater => Verdict::Win,
                std::cmp::Ordering::Less => Verdict::Loss,
                std::cmp::Ordering::Equal => Verdict::Draw,
            }
        };
        let (_, games) = play(&c, 400, strong);
        let r = Ratings::new(&c, &games, &HashMap::new(), &[]);
        let opened = open(&c, &r);
        assert!(opened.contains(&c.entrant(f, 2)), "{opened:?}");
        let (_, flat) = play(&c, 400, |_, _| Verdict::Draw);
        let r = Ratings::new(&c, &flat, &HashMap::new(), &[]);
        let opened = open(&c, &r);
        assert!(opened.contains(&c.entrant(f, 1)), "{opened:?}");
        assert!(!opened.contains(&c.entrant(f, 2)), "{opened:?}");
    }

    #[test]
    fn the_normal_distribution_function() {
        assert!((phi(0.0) - 0.5).abs() < 1e-7);
        assert!((phi(1.959_964) - 0.975).abs() < 1e-6);
        assert!((phi(-1.0) - 0.158_655).abs() < 1e-6);
    }

    #[test]
    fn matchups_name_themselves() {
        let m = Matchup::new("pimc18/hidden/rollout/tuned@1M", "expert-frozen");
        assert_eq!(m.a, "expert-frozen");
        assert_eq!(m.key(), "expert-frozen~pimc18.hidden.rollout.tuned@1M");
        assert_eq!(m.label(), "expert-frozen v pimc18/hidden/rollout/tuned@1M");
    }
}
