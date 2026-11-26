//! Random games over the cards implemented so far, shared by the test binaries.

// Each test binary uses its own part of this module.
#![allow(dead_code)]

use std::hash::{DefaultHasher, Hash, Hasher};

use beryl_cards::CARDS;
use beryl_cards::decks::{VINTAGE_WU_CONTROL, WU_CONTROL, fuzz_deck};
use beryl_engine::rng::SplitMix64;
use beryl_engine::{
    Decision, Format, GameTable, Layout, Observation, Outcome, Profile, Replay, Scratch, Seat,
    State, Step, apply, next, observe,
};
use beryl_search::{
    Agent, AgentDecision, Budget, DEEP_SEARCHER, DEEP_SEER, DEEP_SEER_UNANCHORED, GameAgent,
    RandomAgent, deep_family,
};

pub mod widening;

/// A game between two fuzz decks drawn from `seed`, under the exact profile on even seeds and
/// smart stops on odd ones.
pub fn game(seed: u64) -> (GameTable, State) {
    let decks = [fuzz_deck(seed), fuzz_deck(seed.wrapping_add(1 << 32))];
    let t = GameTable::new(
        Format::CONSTRUCTED,
        &CARDS,
        [decks[0].as_slice(), decks[1].as_slice()],
    )
    .unwrap();
    dealt(t, seed)
}

/// A game between the two fixed test decks, WU Control against the Vintage Cube WU Control,
/// shuffled by `seed`, under the profile [`game`] picks. Assertions about how a game goes (how
/// long it lasts, what it reaches) play these decks, which do not change as cards are written.
pub fn fixed_game(seed: u64) -> (GameTable, State) {
    dealt(
        GameTable::new(
            Format::CONSTRUCTED,
            &CARDS,
            [WU_CONTROL, VINTAGE_WU_CONTROL],
        )
        .unwrap(),
        seed,
    )
}

/// The game `seed` deals from `t`, under the exact profile on even seeds and smart stops on
/// odd ones.
fn dealt(t: GameTable, seed: u64) -> (GameTable, State) {
    let profile = if seed.is_multiple_of(2) {
        Profile::EXACT
    } else {
        Profile::SMART
    };
    let s = State::new(&t, seed, profile).unwrap();
    (t, s)
}

/// The WU Control mirror's table.
pub fn mirror() -> GameTable {
    GameTable::new(Format::CONSTRUCTED, &CARDS, [WU_CONTROL, WU_CONTROL]).unwrap()
}

/// A random agent per seat, seeded from the game's seed.
pub fn agents(seed: u64) -> [RandomAgent; 2] {
    [RandomAgent::new(seed * 2), RandomAgent::new(seed * 2 + 1)]
}

/// Plays a game to the end with `agents`, calling `watch` at every decision before it is
/// answered.
pub fn play<L: Layout>(
    t: &GameTable,
    s: &mut State<L>,
    x: &mut Scratch<L>,
    agents: &mut [impl Agent<L>; 2],
    mut watch: impl FnMut(&State<L>, &Decision<L>),
) -> Outcome {
    let mut asked = [AgentDecision(0); 2];
    loop {
        match next(s, t, x, &mut Replay) {
            Step::Over(outcome) => return outcome,
            Step::Decide(d) => {
                watch(s, &d);
                // A paused resolution is seen where its body stopped (§7.3).
                let i = d.decider.index();
                let c = agents[i].choose(observe(s, t, x, d.decider), &d, asked[i].take());
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

/// The first seed of a fuzz run: `BERYL_FUZZ_FIRST_SEED` when set, else 0. A run of `n` games
/// plays the seeds from it up, so runs started from different seeds play different games.
pub fn first_seed() -> u64 {
    std::env::var("BERYL_FUZZ_FIRST_SEED").map_or(0, |n| n.parse().expect("a seed"))
}

/// The seeds of a fuzz run of `n` games, from [`first_seed`].
pub fn seeds(n: u64) -> std::ops::Range<u64> {
    let first = first_seed();
    first..first + n
}

/// The state redealt from what `viewer` knows of `s` ([`Observation::determinize`]), under the
/// budgets search plays under ([`State::search_limits`]). `next` counts each decision it returns
/// toward the turn's budget, and `s` has counted its open decision already, as has the redealt
/// copy, which carries the same count. Asking the copy with `next` counts that decision a
/// second time, so at the turn's last budgeted decision the copy would stall where `s` offers
/// the decision. Search gives its copies the same room before it asks them anything.
pub fn redeal<L: Layout>(
    s: &State<L>,
    viewer: Seat,
    t: &GameTable,
    rng: &mut SplitMix64,
) -> State<L> {
    let mut det = Observation::new(s, viewer).determinize(t, rng);
    det.set_profile(Profile {
        limits: det.search_limits(),
        ..det.profile()
    });
    det
}

/// The engine steps a long-think agent spends on a decision in the tests, in place of its usual
/// 30M, so that a debug build plays a whole game in seconds.
pub const SMALL_WORK: u64 = 2_000;

/// The agent `name` with a small budget: the registry's own constructor for the agents that
/// take a budget, and the long-think agents' families at [`SMALL_WORK`], since those ignore
/// the budget they are given.
pub fn cheap(
    t: &GameTable,
    name: &str,
    new: fn(&GameTable, u64, Budget) -> Box<dyn GameAgent + Send>,
    seed: u64,
) -> Box<dyn GameAgent + Send> {
    let family = match name {
        "pimc18-omniscient-30m-anchor" => Some(DEEP_SEER),
        "pimc72-hidden-30m-anchor" => Some(DEEP_SEARCHER),
        "pimc18-omniscient-30m" => Some(DEEP_SEER_UNANCHORED),
        _ => None,
    };
    match family {
        Some(family) => deep_family(t, seed, family, SMALL_WORK),
        None => new(
            t,
            seed,
            Budget {
                iterations: 16,
                trees: 1,
                parallel: false,
            },
        ),
    }
}
