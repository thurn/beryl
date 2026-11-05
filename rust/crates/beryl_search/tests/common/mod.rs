//! Random games over the cards implemented so far, shared by the test binaries.

// Each test binary uses its own part of this module.
#![allow(dead_code)]

use std::hash::{DefaultHasher, Hash, Hasher};

use beryl_cards::CARDS;
use beryl_cards::decks::{WU_CONTROL, fuzz_deck};
use beryl_engine::{
    Decision, GameTable, Outcome, Profile, Replay, Scratch, State, Step, apply, next, observe,
};
use beryl_search::{Agent, RandomAgent};

/// A game between two fuzz decks drawn from `seed`, under the exact profile on even seeds and
/// smart stops on odd ones.
pub fn game(seed: u64) -> (GameTable, State) {
    let decks = [fuzz_deck(seed), fuzz_deck(seed.wrapping_add(1 << 32))];
    let t = GameTable::new(&CARDS, [decks[0].as_slice(), decks[1].as_slice()]).unwrap();
    let profile = if seed.is_multiple_of(2) {
        Profile::EXACT
    } else {
        Profile::SMART
    };
    let s = State::new(&t, seed, profile);
    (t, s)
}

/// The WU Control mirror's table.
pub fn mirror() -> GameTable {
    GameTable::new(&CARDS, [WU_CONTROL, WU_CONTROL]).unwrap()
}

/// A random agent per seat, seeded from the game's seed.
pub fn agents(seed: u64) -> [RandomAgent; 2] {
    [RandomAgent::new(seed * 2), RandomAgent::new(seed * 2 + 1)]
}

/// Plays a game to the end with `agents`, calling `watch` at every decision before it is
/// answered.
pub fn play(
    t: &GameTable,
    s: &mut State,
    x: &mut Scratch,
    agents: &mut [impl Agent; 2],
    mut watch: impl FnMut(&State, &Decision),
) -> Outcome {
    loop {
        match next(s, t, x, &mut Replay) {
            Step::Over(outcome) => return outcome,
            Step::Decide(d) => {
                watch(s, &d);
                // A paused resolution is seen where its body stopped (§7.3).
                let c = agents[d.player.index()].choose(observe(s, t, x, d.player), &d);
                apply(s, t, x, &mut Replay, c);
            }
        }
    }
}

pub fn fingerprint(s: &State) -> u64 {
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

/// How many games to play: `BERYL_FUZZ_GAMES` when set, for a long run on demand, else
/// `default`.
pub fn games(default: u64) -> u64 {
    std::env::var("BERYL_FUZZ_GAMES").map_or(default, |n| n.parse().expect("a game count"))
}
