//! The Trackers store's tests: which games keep a tracker, what ends one, what it tells
//! apart and what it keeps across layouts.

use beryl_card::{Ability, CardDef, Condition, Face, Objects, Types};

use super::*;
use crate::event::Sink;
use crate::layout::Large;
use crate::object::Derived;
use crate::profile::Profile;
use crate::rule_changes::fixtures::LOYALTY;
use crate::table::CardTable;
use crate::zone::Place;
use crate::{LargeSlot, Seat, Slot};

static BEAR: CardDef = CardDef {
    name: "Test Bear",
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(2),
        toughness: Some(2),
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// A planeswalker with a loyalty ability, and a static ability that reads whether it entered
/// this turn.
static WALKER: CardDef = CardDef {
    name: "Test Walker",
    faces: &[Face {
        types: Types::PLANESWALKER,
        loyalty: Some(3),
        abilities: &[
            Ability::rule_change(&LOYALTY).only_if(&Condition::EnteredThisTurn(Objects::This)),
            Ability::loyalty(1),
        ],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static CARDS: CardTable = CardTable::new(&[&BEAR, &WALKER]);

/// A game of bears with `walkers` of the second player's cards a planeswalker, and two of
/// the first player's bears on the battlefield.
fn game(walkers: u8) -> (GameTable, State, [Slot; 2]) {
    let bears: &[(u8, &str)] = &[(40, "Test Bear")];
    let mixed = [(40 - walkers, "Test Bear"), (walkers, "Test Walker")];
    let t = GameTable::new(&CARDS, [bears, if walkers == 0 { bears } else { &mixed }]).unwrap();
    let mut s = State::new(&t, 7, Profile::EXACT).unwrap();
    let slots = [s.library(Seat::P0)[0], s.library(Seat::P0)[1]];
    for slot in slots {
        s.board().place(slot, Zone::Battlefield).unwrap();
    }
    (t, s, slots)
}

/// Stores layer results with no continuous effect, as the rules' recompute would.
fn derive<L: Layout>(t: &GameTable, s: &mut State<L>) {
    let Some(mut cache) = s.rederive() else {
        return;
    };
    for i in 0..cache.battlefield().len() {
        let what = cache.copiable(cache.battlefield()[i]);
        cache.results()[i] = Derived {
            power: what.power(t),
            toughness: what.toughness(t),
            types: what.face(t).types,
            ..Derived::default()
        };
    }
    cache.finish(0, Default::default());
}

#[test]
fn a_tracker_is_kept_only_in_a_game_with_a_card_that_reads_it() {
    // No card of the game reads either tracker: the rules' write is one AND, and nothing
    // is kept.
    let (t, mut s, [a, _]) = game(0);
    for tracker in Tracker::ALL {
        assert!(!t.can(tracker.capability()));
        s.track(&t, tracker, a);
        assert!(!s.tracked(tracker, a));
    }
    assert_eq!(s.trackers, Trackers::empty());
    assert_eq!(s.check_stores(&t), Ok(()));
    // One arranged all the same breaks the store's invariant.
    s.board().set_tracked(a, Tracker::EnteredThisTurn, true);
    assert!(s.check_stores(&t).is_err());

    // One card that reads them, in either deck, and both are kept for every permanent.
    let (t, mut s, [a, b]) = game(1);
    for tracker in Tracker::ALL {
        assert!(t.can(tracker.capability()));
        s.track(&t, tracker, a);
        assert!(s.tracked(tracker, a) && !s.tracked(tracker, b));
        assert!(Observation::new(&s, Seat::P1).tracked(tracker, a));
    }
    assert_eq!(s.check_stores(&t), Ok(()));
    // A tracker keys no slot: a tracked permanent leaves by the move's common path.
    assert!(s.keyed.is_empty());
}

#[test]
fn a_tracker_ends_with_the_turn_and_as_its_permanent_leaves() {
    let (t, mut s, [a, b]) = game(1);
    for slot in [a, b] {
        s.track(&t, Tracker::EnteredThisTurn, slot);
        s.track(&t, Tracker::LoyaltyActivated, slot);
    }
    // A checkpoint of the board would lose what the turn has noted.
    assert_eq!(
        s.stores_lingering(&t),
        Some("Test Bear entered this turn".to_owned())
    );

    // The object the card becomes is new (CR 400.7), back on the battlefield included:
    // nothing is tracked of it until the rules note it again. The other's stay.
    s.move_object(&mut Sink::off(), a, Zone::Graveyard, Place::Top)
        .unwrap();
    assert_eq!(s.trackers(a), Tracked::default());
    assert_eq!(s.check_stores(&t), Ok(()));
    s.move_object(&mut Sink::off(), a, Zone::Battlefield, Place::Top)
        .unwrap();
    assert_eq!(s.trackers(a), Tracked::default());
    s.track(&t, Tracker::EnteredThisTurn, a);
    assert!(s.trackers(a).has(Tracker::EnteredThisTurn));
    assert!(!s.trackers(a).has(Tracker::LoyaltyActivated));
    assert!(Tracker::ALL.into_iter().all(|k| s.trackers(b).has(k)));

    // The next turn begins with nothing noted of it.
    s.begin_turn();
    assert_eq!(s.trackers, Trackers::empty());
    assert_eq!(s.stores_lingering(&t), None);
}

#[test]
fn signature_tells_two_permanents_apart_by_a_tracker() {
    let (t, mut s, [a, b]) = game(1);
    let same = |t: &GameTable, s: &mut State| {
        derive(t, s);
        s.signature(a) == s.signature(b)
    };
    assert!(same(&t, &mut s));
    // Nothing else about a permanent says a loyalty ability of it was activated.
    s.track(&t, Tracker::LoyaltyActivated, a);
    assert_eq!(s.object(a).flags(), s.object(b).flags());
    assert!(!same(&t, &mut s));
    s.track(&t, Tracker::LoyaltyActivated, b);
    assert!(same(&t, &mut s));
    s.track(&t, Tracker::EnteredThisTurn, b);
    assert!(!same(&t, &mut s));
}

#[test]
fn trackers_survive_small_to_large_and_back() {
    let (t, mut small, [a, b]) = game(1);
    small.track(&t, Tracker::EnteredThisTurn, a);
    small.track(&t, Tracker::LoyaltyActivated, b);
    derive(&t, &mut small);

    let large: State<Large> = small.widen();
    let wide = |slot: Slot| slot.to::<LargeSlot>().unwrap();
    assert!(large.tracked(Tracker::EnteredThisTurn, wide(a)));
    assert!(!large.tracked(Tracker::LoyaltyActivated, wide(a)));
    assert!(large.tracked(Tracker::LoyaltyActivated, wide(b)));
    assert_eq!(large.check_stores(&t), Ok(()));
    assert_eq!(large.fits::<Small>(&t), Ok(()));
    let back: State<Small> = large.relayout();
    assert_eq!(back, small);
}
