//! Which game the ladder plays next: a pure function of the run's configuration and the games
//! it has finished, so a resumed run picks up the very game a stop interrupted.
//!
//! - Rungs are budgets, in iterations per decision: the configured ones, then x3 each.
//! - Every game is one of a seed pair: the same deal played twice, the rung's agent taking
//!   each seat in turn. Pair `p` has the same seed on every rung, so rungs compare pair by
//!   pair, and a matchup finishes a pair before anything else is played.
//! - The cheap rungs play first, one after another, each finishing its pairs.
//! - Then each rung above them opens when the one below it has its pairs against the
//!   opponent. The newest rung and the one below it alternate a seed pair at a time, so the
//!   one below keeps adding pairs while the newest catches up.
//! - A rung that beats the opponent 85%+ over its first pairs also plays the rung below it
//!   (an Elo chain): the opponent no longer resolves its strength. The highest rung that has
//!   its pairs also plays the omniscient search at its budget against the opponent, as a
//!   diagnostic of whether hidden information is what holds it back.

use serde::{Deserialize, Serialize};

use super::rung_name;
use super::standings::Standings;

/// A run's settings, fixed when it starts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    /// The first rungs, in iterations per decision; each one after them triples the last.
    pub rungs: Vec<u32>,
    /// How many of the first rungs are cheap, played to their pairs before any other.
    pub cheap: usize,
    /// Seed pairs a rung plays against an opponent before the next rung opens.
    pub pairs: u32,
    /// The search's independent trees.
    pub trees: u32,
    /// The agent every rung plays.
    pub opponent: String,
    /// The seed the pairs' seeds derive from.
    pub seed: u64,
}

impl Default for Config {
    /// The long-think ladder: 3k to 1M rollouts per decision and on, 30 pairs a rung, 18
    /// trees, against the frozen expert.
    fn default() -> Self {
        Config {
            rungs: vec![3_000, 10_000, 30_000, 100_000, 300_000, 1_000_000],
            cheap: 3,
            pairs: 30,
            trees: 18,
            opponent: "expert-frozen".to_owned(),
            seed: 0,
        }
    }
}

impl Config {
    /// Rung `i`'s iterations per decision.
    pub fn iterations(&self, i: usize) -> u32 {
        let last = self.rungs.len() - 1;
        if i <= last {
            self.rungs[i]
        } else {
            self.rungs[last].saturating_mul(3u32.saturating_pow((i - last) as u32))
        }
    }
}

/// Who a rung's agent plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Against {
    /// The hidden search against the configured opponent.
    Opponent,
    /// The omniscient search at the rung's budget against the configured opponent.
    Omniscient,
    /// The hidden search against the hidden search one rung down.
    Below,
}

/// A rung and who it plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Matchup {
    pub rung: usize,
    pub against: Against,
}

impl Matchup {
    /// The matchup's sides, as every view names them: the rung's agent (`3k`, `omniscient
    /// 3k`) and who it plays (`expert-frozen`, `1k`).
    pub fn sides(&self, c: &Config) -> (String, String) {
        let r = rung_name(c.iterations(self.rung));
        match self.against {
            Against::Opponent => (r, c.opponent.clone()),
            Against::Omniscient => (format!("omniscient {r}"), c.opponent.clone()),
            Against::Below => (r, rung_name(c.iterations(self.rung - 1))),
        }
    }

    /// The matchup's name: `3k v expert-frozen`.
    pub fn label(&self, c: &Config) -> String {
        let (a, b) = self.sides(c);
        format!("{a} v {b}")
    }

    /// The prefix of its games' keys: `3k-v-expert-frozen`.
    pub fn key(&self, c: &Config) -> String {
        self.label(c).replace(' ', "-")
    }
}

/// The share of the first pairs' games a rung must score against the opponent to play the
/// rung below it as well.
pub const CHAIN_AT: f64 = 0.85;

/// The matchup whose next game the ladder plays.
pub fn next(c: &Config, s: &Standings) -> Matchup {
    let pair = 2 * c.pairs;
    let games = |rung, against| s.get(Matchup { rung, against }).games;
    // A matchup finishes the pair it began.
    for rung in 0..=top(c, s) {
        for against in [Against::Opponent, Against::Omniscient, Against::Below] {
            if games(rung, against) % 2 == 1 {
                return Matchup { rung, against };
            }
        }
    }
    let chains = |rung: usize| {
        let t = s.get(Matchup {
            rung,
            against: Against::Opponent,
        });
        rung > 0
            && t.games >= pair
            && t.first.scored() > 0
            && t.first.points() >= CHAIN_AT * f64::from(t.first.scored())
    };
    // What a rung plays on its turn: whichever of its matchups has played least, the
    // opponent first among equals.
    let turn = |rung: usize, omniscient: bool| {
        let mut options = vec![Against::Opponent];
        if omniscient {
            options.push(Against::Omniscient);
        }
        if chains(rung) {
            options.push(Against::Below);
        }
        let against = options
            .into_iter()
            .min_by_key(|&a| games(rung, a))
            .expect("the opponent");
        Matchup { rung, against }
    };
    for rung in 0..c.cheap {
        let m = turn(rung, false);
        if games(rung, m.against) < pair {
            return m;
        }
    }
    let top = top(c, s);
    if top == c.cheap {
        return Matchup {
            rung: top,
            against: Against::Opponent,
        };
    }
    // The rung below the newest had its pairs, all against the opponent, when the newest
    // opened; since then the two have taken turns.
    let pairs_of = |rung: usize| {
        [Against::Opponent, Against::Omniscient, Against::Below]
            .iter()
            .map(|&a| games(rung, a) / 2)
            .sum::<u32>()
    };
    if pairs_of(top - 1) - c.pairs <= pairs_of(top) {
        turn(top - 1, true)
    } else {
        Matchup {
            rung: top,
            against: Against::Opponent,
        }
    }
}

/// The newest open rung: the lowest above the cheap ones short of its pairs against the
/// opponent, or the cheap ones' last while they play.
fn top(c: &Config, s: &Standings) -> usize {
    let done = |rung| {
        s.get(Matchup {
            rung,
            against: Against::Opponent,
        })
        .games
            >= 2 * c.pairs
    };
    if (0..c.cheap).any(|rung| !done(rung)) {
        return c.cheap.saturating_sub(1);
    }
    let mut rung = c.cheap;
    while done(rung) {
        rung += 1;
    }
    rung
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::longthink::{GameRecord, Verdict};

    /// Plays `games` games of the schedule, game `g` of matchup `m` ending
    /// `outcome(m, g)` for the rung's agent, and returns the matchups in the order played.
    fn play(c: &Config, games: usize, outcome: impl Fn(Matchup, u32) -> Verdict) -> Vec<Matchup> {
        let mut s = Standings::default();
        let mut played = Vec::new();
        for _ in 0..games {
            let m = next(c, &s);
            let g = s.get(m).games;
            s.record(c, &GameRecord::example(m, g, outcome(m, g)), None);
            played.push(m);
        }
        played
    }

    fn config() -> Config {
        Config {
            rungs: vec![1, 3, 10],
            cheap: 2,
            pairs: 2,
            trees: 1,
            opponent: "expert".to_owned(),
            seed: 0,
        }
    }

    /// Each game's matchup, a pair (two games) to a letter: `o` the opponent, `s` the
    /// omniscient search, `b` the rung below, after the rung's index.
    fn pairs(played: &[Matchup]) -> String {
        played
            .chunks(2)
            .map(|p| {
                assert_eq!(p[0], p[1], "a pair is played through");
                let a = match p[0].against {
                    Against::Opponent => 'o',
                    Against::Omniscient => 's',
                    Against::Below => 'b',
                };
                format!("{}{a}", p[0].rung)
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn rungs_triple_after_the_configured_ones() {
        let c = config();
        let rungs: Vec<u32> = (0..5).map(|i| c.iterations(i)).collect();
        assert_eq!(rungs, [1, 3, 10, 30, 90]);
    }

    /// Cheap rungs first, each to its pairs; then each rung above them opens when the one
    /// below has its pairs, and the two alternate a pair at a time, the one below cycling
    /// through the opponent and the omniscient diagnostic.
    #[test]
    fn cheap_rungs_first_then_new_rungs_alternate_with_the_one_below() {
        let played = play(&config(), 32, |_, _| Verdict::Draw);
        assert_eq!(
            pairs(&played),
            "0o 0o 1o 1o 2o 2o 2s 3o 2s 3o 3s 4o 3s 4o 4s 5o"
        );
    }

    /// A rung that scores 85% over its first pairs against the opponent also plays the
    /// rung below it; one that scores less, here 75%, never does.
    #[test]
    fn a_rung_that_beats_the_opponent_plays_the_rung_below() {
        let played = play(&config(), 24, |m, g| {
            if m.rung == 1 || g % 4 != 3 {
                Verdict::Win
            } else {
                Verdict::Loss
            }
        });
        assert_eq!(pairs(&played), "0o 0o 1o 1o 1b 1b 2o 2o 2s 3o 2s 3o");
        let strong = play(&config(), 24, |_, _| Verdict::Win);
        assert_eq!(pairs(&strong), "0o 0o 1o 1o 1b 1b 2o 2o 2s 3o 2b 3o");
    }

    /// The schedule is a function of the finished games alone, so a run stopped mid-pair
    /// resumes the very game it stopped in.
    #[test]
    fn a_pair_begun_is_finished_first() {
        let c = config();
        let m = Matchup {
            rung: 1,
            against: Against::Opponent,
        };
        let mut half = Standings::default();
        half.record(&c, &GameRecord::example(m, 0, Verdict::Win), None);
        assert_eq!(next(&c, &half), m);
    }
}
