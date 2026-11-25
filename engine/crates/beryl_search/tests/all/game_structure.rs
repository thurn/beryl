//! Every agent plays the keystones that change the shape of a turn or a game: an additional
//! upkeep (Paradox Haze), a skipped untap step (Stasis), an additional combat (Relentless
//! Assault), a skipped or extra turn (Time Vault, Final Fortune), a turn ended during a
//! resolution (Time Stop, Sundial of the Infinite), a restarted game (Karn Liberated) and a
//! subgame (Shahrazad). Each game deals two fuzz decks from the `turn` and `subgames` keystone
//! families together (`decks::KEYSTONE_FAMILIES`), so every deck holds every one of those
//! cards, and plays it to its end with the same agent in both seats: the greedy agent, which
//! is also the searching agents' rollout policy, the expert system, and MCTS. No agent may
//! choose outside a decision's candidates, and no game may stall except on the turn limit.
//!
//! This plays only as many games as take a few seconds; the `turn_structure` and `subgames`
//! tests of `families.rs` play many more with random agents under `just fuzz`.

use std::collections::{BTreeMap, HashMap};

use crate::common::widening::play_widening;
use beryl_cards::CARDS;
use beryl_cards::decks::{fuzz_deck_from, keystone_family};
use beryl_engine::{Format, GameTable, Outcome, Profile, StallReason, State};
use beryl_search::{Budget, GameAgent, agent};

/// The cards the games deal from: the `turn` and `subgames` families and the basic lands.
fn pool() -> Vec<&'static beryl_engine::card::CardDef> {
    let mut cards = keystone_family("turn").expect("the turn family");
    for card in keystone_family("subgames").expect("the subgames family") {
        if !cards.iter().any(|c| c.name == card.name) {
            cards.push(card);
        }
    }
    cards
}

/// The searching agents' budget here: enough iterations to search, few enough that a debug
/// build plays a game in about a second.
const SMALL: Budget = Budget {
    iterations: 8,
    trees: 1,
    parallel: false,
};

/// Plays `games` seeded games with the registered agent `name` in both seats. A game that
/// stalls for any reason but the game's turn limit fails, as does a capacity running out in
/// `Large`.
fn plays(name: &str, games: u64) {
    let info = agent(name).expect("a registered agent");
    let cards = pool();
    let mut stalls: BTreeMap<String, u64> = BTreeMap::new();
    let mut entered_subgame = 0;
    let mut decisions = 0u64;
    for seed in 0..games {
        let decks = [
            fuzz_deck_from(seed, &cards),
            fuzz_deck_from(seed.wrapping_add(1 << 32), &cards),
        ];
        let t = GameTable::new(
            Format::CONSTRUCTED,
            &CARDS,
            decks.each_ref().map(Vec::as_slice),
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
        entered_subgame += u32::from(played.entered_subgame);
        match played.outcome {
            Outcome::Stalled(StallReason::Turns) => {
                *stalls.entry("Turns".to_owned()).or_default() += 1;
            }
            // A turn that runs out of decisions is an agent repeating itself, as two players
            // skipping each turn for their Time Vaults did.
            Outcome::Stalled(reason) => panic!("{name}, seed {seed}: stalled on {reason:?}"),
            Outcome::Win(_) | Outcome::Draw => {}
        }
    }
    println!(
        "{name}: {games} games, {decisions} decisions; {entered_subgame} entered a subgame; \
         stalls by reason {stalls:?}"
    );
}

#[test]
fn the_greedy_agent_plays_the_turn_and_game_keystones() {
    plays("greedy", 4);
}

#[test]
fn the_expert_plays_the_turn_and_game_keystones() {
    plays("expert", 4);
}

#[test]
fn mcts_plays_the_turn_and_game_keystones() {
    plays("mcts-hidden", 2);
}
