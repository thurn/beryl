//! The Dandân rollout benchmark: [`rollout`](crate::rollout)'s benchmark over `decks::FORGETFUL_FISH`
//! in `Format::SHARED_DECK`, where both players draw from one library and share one graveyard.
//! It tracks what a rollout costs with that deck and has no bar in `count-gate.toml`, so a
//! Dandân rollout getting slower shows in the trend without gating a change.
//!
//! The games the positions are cut from are played by `greedy` agents on both seats: the
//! frozen expert's evaluation names WU Control cards. Everything else is shared with the
//! WU Control corpus (its constants, `starts`, [`Workload`] and the subjects), so the two
//! counts differ only by the deck.

use beryl_cards::CARDS;
use beryl_cards::decks::FORGETFUL_FISH;
use beryl_engine::{Format, GameTable};
use beryl_search::{Agent, GreedyAgent};

use crate::rollout::{Corpus, GAMES, SEED, Workload, cut};

/// The table both seats play at: one shared library of the Dandân deck, so the second seat's
/// own list is empty.
pub fn table() -> GameTable {
    GameTable::new(Format::SHARED_DECK, &CARDS, [FORGETFUL_FISH, &[]]).expect("the Dandân deck")
}

/// Plays the seeded games and cuts their positions. Deterministic: the test
/// `corpus_never_changes` pins it.
pub fn corpus() -> Corpus {
    let table = table();
    let mut positions = Vec::new();
    for game in 0..GAMES {
        let mut agents: [Box<dyn Agent>; 2] = [
            Box::new(GreedyAgent::new(&table)),
            Box::new(GreedyAgent::new(&table)),
        ];
        cut(&table, SEED ^ (game << 1), &mut agents, &mut positions);
    }
    Corpus { table, positions }
}

/// Everything the Dandân benchmarks measure, built before measuring.
pub fn workload() -> Workload {
    Workload::of(corpus())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rollout::{CUTS, record, roll, stable_hash};
    use beryl_engine::{Choice, Scratch};
    use beryl_search::RolloutPolicy;

    /// The corpus is the benchmark's input: if it changed, every saved count would silently
    /// measure something else. A change to the engine, the cards or the agents that moves
    /// it updates this pin on purpose, and the trend's earlier counts are void.
    #[test]
    fn corpus_never_changes() {
        let c = corpus();
        let turns: Vec<u16> = c.positions.iter().map(|s| s.turn().number()).collect();
        assert_eq!(turns, CUTS.repeat(GAMES as usize));
        assert_eq!(stable_hash(&c.positions), 15_561_151_361_807_732_256);
    }

    /// Every Dandân rollout answers every question as it always has and ends the same way.
    #[test]
    fn rollouts_never_change() {
        let c = corpus();
        let starts = c.starts();
        let policy = RolloutPolicy::new(&c.table);
        let (scripts, _, summary) = record(&c.table, &starts, &policy);
        let mut x = Scratch::unchecked();
        let mut s = starts[0];
        let ends: Vec<u64> = starts
            .iter()
            .map(|start| {
                roll(
                    &c.table,
                    std::slice::from_ref(start),
                    &policy,
                    &mut s,
                    &mut x,
                )
            })
            .collect();
        let answers: Vec<&[Choice]> = scripts.iter().map(|s| s.answers.as_slice()).collect();
        assert_eq!(
            (summary.decisions, summary.inline, summary.turns),
            (10_180, 1_172, 1_618)
        );
        assert_eq!(stable_hash(&(answers, ends)), 14_790_571_293_840_855_178);
    }
}
