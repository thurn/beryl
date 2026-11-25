//! Every agent plays the keystones of the casting and combat exceptions family: cascade
//! (Bloodbraid Elf), casting while searching a library (Panglacial Wurm), hand-only cast
//! triggers with casts during resolution (Knowledge Pool, Possibility Storm), multi-blocking
//! (Palace Guard), double strike (Fencing Ace), banding (Benalish Hero, Benalish Infantry)
//! and "bands with other" (Master of the Hunt).
//!
//! Each game deals two fuzz decks from the `casting` and `combat` keystone families together
//! (`decks::KEYSTONE_FAMILIES`), so every deck holds keystones from both, and plays it to its
//! end with the same agent in both seats: the greedy agent (which is also the searching
//! agents' rollout policy), the expert system, and MCTS. No agent may choose outside a
//! decision's candidates, and no game may stall except on the turn limit.
//!
//! This plays only as many games as take a few seconds; the family tests of `families.rs` play
//! many more with random agents under `just fuzz`.

use std::collections::{BTreeMap, HashMap};

use crate::common::widening::play_widening;
use beryl_cards::CARDS;
use beryl_cards::decks::{fuzz_deck_from, keystone_family};
use beryl_cards::names;
use beryl_engine::{
    DecisionKind, Format, GameTable, Observation, Opt, Outcome, Profile, StallReason, State,
};
use beryl_search::{Budget, GameAgent, RolloutPolicy, agent};

/// The cards the games deal from: the `casting` and `combat` families and the basic lands.
fn pool() -> Vec<&'static beryl_engine::card::CardDef> {
    let mut cards = keystone_family("casting").expect("the casting family");
    for card in keystone_family("combat").expect("the combat family") {
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
        match played.outcome {
            Outcome::Stalled(StallReason::Turns) => {
                *stalls.entry("Turns".to_owned()).or_default() += 1;
            }
            Outcome::Stalled(reason) => panic!("{name}, seed {seed}: stalled on {reason:?}"),
            Outcome::Win(_) | Outcome::Draw => {}
        }
    }
    println!("{name}: {games} games, {decisions} decisions; stalls by reason {stalls:?}");
}

#[test]
fn the_greedy_agent_plays_the_casting_and_combat_keystones() {
    plays("greedy", 4);
}

#[test]
fn the_expert_plays_the_casting_and_combat_keystones() {
    plays("expert", 4);
}

#[test]
fn mcts_plays_the_casting_and_combat_keystones() {
    plays("mcts-hidden", 2);
}

/// The rollout policy casts Panglacial Wurm when offered during a library search before taking
/// a found card, because finding the first card ends the casting window.
#[test]
fn rollout_policy_casts_panglacial_wurm_during_a_search() {
    use beryl_scenario::prelude::{ME, Resolve};
    use beryl_scenario::{P0, Scenario};

    const SCENARIO: Scenario = Scenario::new("Policy casts Wurm during Trinket Mage search")
        .battlefield(
            P0,
            &[
                names::ISLAND,
                names::ISLAND,
                names::ISLAND,
                names::FOREST,
                names::FOREST,
                names::FOREST,
                names::FOREST,
                names::FOREST,
                names::FOREST,
                names::FOREST,
            ],
        )
        .hand(P0, &[names::TRINKET_MAGE])
        .library(
            P0,
            &[names::PANGLACIAL_WURM, names::MANA_CRYPT, names::FOREST],
        )
        .actions(&[
            ME.casts(names::TRINKET_MAGE),
            Resolve,
            Resolve,
            ME.yes(),
            ME.picks_up_to(names::MANA_CRYPT),
        ]);

    let mut checked = false;
    beryl_scenario::visit(&CARDS, &SCENARIO, |t, s, d| {
        if d.kind != DecisionKind::ChooseUpTo {
            return;
        }
        let policy = RolloutPolicy::new(t);
        let choice = policy.choose(t, Observation::new(s, d.decider), d);
        let opt = d.candidates.option(choice);
        assert!(
            matches!(opt, Some(Opt::Cast { .. })),
            "expected the policy to cast Panglacial Wurm, got {opt:?}"
        );
        checked = true;
    })
    .unwrap_or_else(|e| panic!("{e}"));
    assert!(checked, "never reached the search decision");
}

/// When a defending player assigns an attacker's combat damage because a blocker has banding,
/// the rollout policy assigns non-lethal damage to its own blocker so both blockers survive.
#[test]
fn rollout_policy_defending_player_divides_damage_to_save_blockers() {
    use beryl_engine::TurnStep;
    use beryl_scenario::prelude::{ME, Pick, THEM, advance_to};
    use beryl_scenario::{P0, P1, Scenario};

    const SCENARIO: Scenario = Scenario::new("Defending player divides damage with banding")
        .battlefield(P0, &[names::BLOODBRAID_ELF])
        .battlefield(P1, &[names::BENALISH_HERO, names::BENALISH_INFANTRY])
        .actions(&[
            advance_to(1, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.attacks_with(names::BLOODBRAID_ELF),
            THEM.chooses(DecisionKind::Blocker, names::BENALISH_HERO),
            THEM.chooses(DecisionKind::Blocker, names::BENALISH_INFANTRY),
            THEM.answers(DecisionKind::DamageDivision, Pick::Number { n: 0 }),
        ]);

    let mut checked = false;
    beryl_scenario::visit(&CARDS, &SCENARIO, |t, s, d| {
        if d.kind != DecisionKind::DamageDivision {
            return;
        }
        let policy = RolloutPolicy::new(t);
        let choice = policy.choose(t, Observation::new(s, d.decider), d);
        let opt = d.candidates.option(choice);
        // Benalish Hero is a 1/1 (first recipient), Benalish Infantry is a 1/3, and
        // Bloodbraid Elf has power 3. Assigning 0 to the Hero lets the Hero survive.
        assert_eq!(
            opt,
            Some(Opt::Number(0)),
            "defending player should spare Benalish Hero, got {opt:?}"
        );
        checked = true;
    })
    .unwrap_or_else(|e| panic!("{e}"));
    assert!(checked, "never reached the damage division decision");
}
