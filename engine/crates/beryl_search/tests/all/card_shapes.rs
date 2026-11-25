//! Every agent plays the keystones of card shapes, face-down cards and cards outside the game:
//! split cards and fuse (Fire // Ice, Wear // Tear), Adventures and prepared creatures
//! (Bonecrusher Giant, Adventurous Eater), a modal double-faced card (Valakut Awakening), a
//! prototype (Arcane Proxy), transforming and flipping permanents (Delver of Secrets, Erayo),
//! meld (Gisela and Bruna), a Saga (The Eldest Reborn), a Class, a Case and a leveler (the
//! `designations` family), morph and face-down spells (Exalted Angel, Gathan Raiders,
//! Illusionary Mask), a wish and a companion (Burning Wish, Lurrus of the Dream-Den).
//!
//! Each game deals two fuzz decks from one of those keystone families
//! (`decks::KEYSTONE_FAMILIES`), so every deck holds every card of the family, and plays it to
//! its end with the same agent in both seats: the greedy agent, which is also the searching
//! agents' rollout policy, the expert system, and MCTS. Both players have a sideboard holding
//! Lurrus and a card Burning Wish can fetch. No agent may choose outside a decision's
//! candidates, and no game may stall except on the turn limit.
//!
//! This plays one game a family for each agent, as many as take a few seconds; the family
//! tests of `families.rs` play many more with random agents under `just fuzz`.

use std::collections::{BTreeMap, HashMap};

use crate::common::widening::play_widening;
use beryl_cards::CARDS;
use beryl_cards::decks::{fuzz_deck_from, keystone_family};
use beryl_cards::names;
use beryl_engine::{Format, GameTable, Outcome, Profile, StallReason, State};
use beryl_search::{Budget, GameAgent, agent};

/// The keystone families of card shapes, face-down cards and cards outside the game.
pub const FAMILIES: &[&str] = &[
    "split",
    "casting",
    "turn-in-place",
    "meld",
    "sagas",
    "designations",
    "face-down",
    "outside-the-game",
];

/// Each player's sideboard: a companion and a sorcery for Burning Wish.
pub const SIDEBOARD: &[(u8, &str)] = &[
    (1, names::LURRUS_OF_THE_DREAM_DEN),
    (1, names::VALAKUT_AWAKENING),
];

/// The searching agents' budget here: enough iterations to search, few enough that a debug
/// build plays a game in about a second.
const SMALL: Budget = Budget {
    iterations: 8,
    trees: 1,
    parallel: false,
};

/// Plays one seeded game of each family with the registered agent `name` in both seats, from
/// `first_seed`. A game that stalls for any reason but the game's turn limit fails, as does a
/// capacity running out in `Large`.
fn plays(name: &str, first_seed: u64) {
    let info = agent(name).expect("a registered agent");
    let mut stalls: BTreeMap<String, u64> = BTreeMap::new();
    let mut decisions = 0u64;
    for (i, family) in FAMILIES.iter().enumerate() {
        let cards = keystone_family(family).expect("a keystone family");
        let seed = first_seed + i as u64;
        let decks = [
            fuzz_deck_from(seed, &cards),
            fuzz_deck_from(seed.wrapping_add(1 << 32), &cards),
        ];
        let t = GameTable::with_sideboards(
            Format::CONSTRUCTED,
            &CARDS,
            decks.each_ref().map(Vec::as_slice),
            [SIDEBOARD; 2],
        )
        .unwrap();
        let mut agents: [Box<dyn GameAgent + Send>; 2] = [
            (info.new)(&t, seed * 2, SMALL),
            (info.new)(&t, seed * 2 + 1, SMALL),
        ];
        let played = play_widening(
            &t,
            State::new(&t, seed, Profile::SMART).unwrap(),
            seed,
            &mut HashMap::new(),
            false,
            &mut agents,
        );
        decisions += u64::from(played.decisions);
        match played.outcome {
            Outcome::Stalled(StallReason::Turns) => {
                *stalls.entry((*family).to_owned()).or_default() += 1;
            }
            Outcome::Stalled(reason) => {
                panic!("{name}, {family}, seed {seed}: stalled on {reason:?}")
            }
            Outcome::Win(_) | Outcome::Draw => {}
        }
    }
    println!(
        "{name}: {} games, {decisions} decisions; turn-limit stalls by family {stalls:?}",
        FAMILIES.len()
    );
}

/// From seed 100, the `face-down` family's game is seed 106, where the greedy agent once
/// activated Illusionary Mask for X = 0 with its lands tapped, again and again, until the turn
/// ran out of decisions.
#[test]
fn the_greedy_agent_plays_the_card_shape_keystones() {
    plays("greedy", 100);
}

#[test]
fn the_expert_plays_the_card_shape_keystones() {
    plays("expert", 0);
}

#[test]
fn mcts_plays_the_card_shape_keystones() {
    plays("mcts-hidden", 0);
}
