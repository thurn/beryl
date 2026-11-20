//! Random games over one keystone family's cards at a time (`decks::KEYSTONE_FAMILIES`): fuzz
//! decks dealt from the family's keystones, its partners and the basic lands, so the cards of a
//! family meet each other in nearly every game, where decks drawn from every implemented card
//! rarely hold two of them. Each game starts in `Small` and goes on in `Large` from the engine
//! call that ran out of a capacity, as the app's games do; the test reports those games with
//! the list that filled. `just fuzz` plays these at length; no check runs them.

use std::collections::{BTreeMap, HashMap};

use crate::common::widening::play_widening;
use crate::common::{games, seeds};
use beryl_cards::CARDS;
use beryl_cards::decks::{fuzz_deck_from, keystone_family};
use beryl_engine::{GameTable, Outcome, Profile, StallReason, State};

/// Plays a family's games: two fuzz decks per game from its cards, a quarter as many games as
/// the whole fuzzer plays. No game may panic, break an invariant or stall for a reason other
/// than a budget; a game that leaves `Small` is reported, with the list that filled.
fn family(name: &str) {
    let cards = keystone_family(name).expect("a keystone family");
    let n = games(8).div_ceil(4).max(2);
    let mut stalls: BTreeMap<String, u64> = BTreeMap::new();
    let mut widened: BTreeMap<&str, u64> = BTreeMap::new();
    let mut kinds: HashMap<String, u64> = HashMap::new();
    let mut decisions = 0u64;
    for seed in seeds(n) {
        let decks = [
            fuzz_deck_from(seed, &cards),
            fuzz_deck_from(seed.wrapping_add(1 << 32), &cards),
        ];
        let t = GameTable::new(&CARDS, [decks[0].as_slice(), decks[1].as_slice()]).unwrap();
        let profile = if seed.is_multiple_of(2) {
            Profile::EXACT
        } else {
            Profile::SMART
        };
        let played = play_widening(&t, State::new(&t, seed, profile).unwrap(), seed, &mut kinds);
        decisions += u64::from(played.decisions);
        if let Some(list) = played.filled {
            *widened.entry(list).or_default() += 1;
        }
        match played.outcome {
            Outcome::Stalled(StallReason::Capacity) => {
                panic!("{name}, seed {seed}: a capacity ran out in Large");
            }
            Outcome::Stalled(reason) => *stalls.entry(format!("{reason:?}")).or_default() += 1,
            Outcome::Win(_) | Outcome::Draw => {}
        }
    }
    println!(
        "{name}: {n} games, {decisions} decisions; stalls by reason {stalls:?}; \
         {} left Small, by the list that filled: {widened:?}",
        widened.values().sum::<u64>()
    );
}

#[test]
#[ignore = "random games of one family: `just fuzz` plays them"]
fn attachments() {
    family("attachments");
}

#[test]
#[ignore = "random games of one family: `just fuzz` plays them"]
fn layers() {
    family("layers");
}

#[test]
#[ignore = "random games of one family: `just fuzz` plays them"]
fn replacement() {
    family("replacement");
}

#[test]
#[ignore = "random games of one family: `just fuzz` plays them"]
fn zones_and_text() {
    family("zones-and-text");
}

#[test]
#[ignore = "random games of one family: `just fuzz` plays them"]
fn copies() {
    family("copies");
}

#[test]
#[ignore = "random games of one family: `just fuzz` plays them"]
fn costs() {
    family("costs");
}

#[test]
#[ignore = "random games of one family: `just fuzz` plays them"]
fn identity() {
    family("identity");
}

#[test]
#[ignore = "random games of one family: `just fuzz` plays them"]
fn decisions() {
    family("decisions");
}

/// Worldgorger Dragon's enters ability exiles 33 Mountains, linking each to the Dragon with a
/// memo, one more than the 32 memos `Small` holds. The game goes on in `Large`, naming the
/// memos as the list that filled, and plays on without running out of a capacity there.
#[test]
fn a_dragon_exiling_more_than_small_holds_plays_on_in_large() {
    use beryl_cards::names::{MOUNTAIN, PLAINS, WORLDGORGER_DRAGON};
    use beryl_engine::{Seat, TurnStep, Zone};
    let t = GameTable::new(
        &CARDS,
        [&[(33, MOUNTAIN), (7, WORLDGORGER_DRAGON)], &[(20, PLAINS)]],
    )
    .unwrap();
    let mut s: State = State::new(&t, 0, Profile::EXACT).unwrap();
    s.board().begin_at(3, Seat::P0, TurnStep::PrecombatMain);
    for slot in s.library(Seat::P0).to_vec() {
        let zone = if s.definition_name(&t, slot) == MOUNTAIN {
            Zone::Battlefield
        } else {
            Zone::Hand
        };
        s.board().place(slot, zone).unwrap();
    }
    let played = play_widening(&t, s, 0, &mut HashMap::new());
    assert_ne!(played.outcome, Outcome::Stalled(StallReason::Capacity));
    assert_eq!(played.filled, Some("the memos"), "{:?}", played.outcome);
}
