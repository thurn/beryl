//! The Effects store's tests: what it keeps across layouts, when it is full, and what ends
//! its entries.

use super::*;
use crate::layout::Large;
use crate::profile::Profile;
use crate::{CardIx, LargeCardIx, LargeSlot, Slot};

const PUMP: Payload = Payload::Layer(LayerChange::ModifyPt(2, 2));
const EXILE: Payload = Payload::Move(MoveReplacement::ExileInsteadOfGraveyard);

/// The object in the `i`th slot of a state with no game in it: enough to key a store.
fn nth<L: Layout>(s: &State<L>, i: usize) -> ObjectRef<L> {
    s.object_ref(L::Slot::from_index(i))
}

#[test]
fn an_effect_survives_small_to_large_and_back() {
    let mut small: State<Small> = State::empty(Profile::EXACT);
    let seventh = nth(&small, 7);
    small
        .start_effect(
            CardIx::from_index(3),
            Affects::object(seventh),
            PUMP,
            Duration::UntilYourNextTurn(Seat::P1),
        )
        .unwrap();
    small
        .start_effect(
            CardIx::FIRST,
            Affects::object(nth(&small, 200)),
            EXILE,
            Duration::Indefinite,
        )
        .unwrap();

    let large: State<Large> = small.widen();
    // A slot keeps its place, not its number: the 200th slot is in the pool.
    let pooled = Slot::from_index(200).to::<LargeSlot>().unwrap();
    assert_eq!(
        large.effects(),
        [
            ContinuousEffect {
                source: LargeCardIx::from_index(3),
                timestamp: 0,
                duration: Duration::UntilYourNextTurn(Seat::P1),
                affected: Affects::object(nth(&large, 7)),
                payload: PUMP,
            },
            ContinuousEffect {
                source: LargeCardIx::FIRST,
                timestamp: 1,
                duration: Duration::Indefinite,
                affected: Affects::object(large.object_ref(pooled)),
                payload: EXILE,
            },
        ]
    );
    // The wide game reads by kind and ends an object's effects as the narrow one does.
    assert_eq!(large.layer_effects().count(), 1);
    assert_eq!(
        large
            .move_replacements(large.object_ref(pooled))
            .collect::<Vec<_>>(),
        [MoveReplacement::ExileInsteadOfGraveyard]
    );

    let back: State<Small> = large.relayout();
    assert_eq!(back, small);
    assert_eq!(back.effects.keys(), small.keyed);
}

#[test]
fn a_full_store_returns_full_and_a_wide_one_grows() {
    let mut small: State<Small> = State::empty(Profile::EXACT);
    let mut large: State<Large> = State::empty(Profile::EXACT);
    for i in 0..EFFECTS {
        let object = Affects::object(nth(&small, i));
        assert_eq!(
            small.start_effect(CardIx::FIRST, object, EXILE, Duration::EndOfTurn),
            Ok(())
        );
    }
    let one_more = Affects::object(nth(&small, EFFECTS));
    assert_eq!(
        small.start_effect(CardIx::FIRST, one_more, PUMP, Duration::EndOfTurn),
        Err(Full)
    );
    assert_eq!(small.effects().len(), EFFECTS);
    // What did not fit is in no index.
    assert_eq!(small.layer_effects().count(), 0);
    assert!(!small.keyed.contains(Slot::from_index(EFFECTS)));

    for i in 0..=EFFECTS {
        let object = Affects::object(nth(&large, i));
        assert_eq!(
            large.start_effect(LargeCardIx::FIRST, object, EXILE, Duration::EndOfTurn),
            Ok(())
        );
    }
    assert_eq!(large.effects().len(), EFFECTS + 1);
    assert_eq!(
        large.fits::<Small>(&GameTable::new(&NO_CARDS, [&[], &[]]).unwrap()),
        Err(Misfit::ListTooLong {
            list: "the continuous effects",
            len: EFFECTS + 1,
            capacity: EFFECTS,
        })
    );
}

static NO_CARDS: crate::table::CardTable = crate::table::CardTable::new(&[]);

#[test]
fn a_rule_change_on_a_player_survives_a_relayout_and_ends_with_its_duration() {
    let t = GameTable::new(&NO_CARDS, [&[], &[]]).unwrap();
    let rule = Rule::SorceriesAsFlash;
    let (you, other) = (Affects::Player(Seat::P1), Affects::Player(Seat::P0));
    let mut small: State<Small> = State::empty(Profile::EXACT);
    small.flags.derived_dirty = false;
    assert!(!small.rule_applies(&t, rule, you));
    small
        .start_effect(
            CardIx::FIRST,
            you,
            Payload::Rule(rule),
            Duration::UntilYourNextTurn(Seat::P1),
        )
        .unwrap();
    // It applies from the moment it starts, to that player and that rule alone, and is
    // no input of the layers and about no object.
    assert!(small.rule_applies(&t, rule, you));
    assert!(!small.rule_applies(&t, rule, other));
    assert!(!small.rule_applies(&t, Rule::OpponentsCastAsSorceries, you));
    assert!(!small.is_derived_stale());
    assert!(small.keyed.is_empty());
    assert_eq!(small.check_stores(&t), Ok(()));
    // A checkpoint would lose it, in the words saves hold.
    assert_eq!(small.stores_lingering(&t), None);
    assert_eq!(
        small.rule_change_lingering().as_deref(),
        Some("player 1 casting sorceries as though they had flash until their next turn")
    );

    let mut large: State<Large> = small.widen();
    assert_eq!(large.effects()[0].affected, Affects::Player(Seat::P1));
    assert!(large.rule_applies(&t, rule, Affects::Player(Seat::P1)));
    assert!(!large.rule_applies(&t, rule, Affects::Player(Seat::P0)));
    let back: State<Small> = large.relayout();
    assert_eq!(back, small);

    // The end of this turn and the other player's next turn end nothing; the player's
    // own next turn does, in either layout.
    for ended in [Duration::EndOfTurn, Duration::UntilYourNextTurn(Seat::P0)] {
        small.expire(ended);
        assert!(small.rule_applies(&t, rule, you));
    }
    small.expire(Duration::UntilYourNextTurn(Seat::P1));
    large.expire(Duration::UntilYourNextTurn(Seat::P1));
    assert!(!small.rule_applies(&t, rule, you));
    assert!(!large.rule_applies(&t, rule, Affects::Player(Seat::P1)));
    assert!(small.effects().is_empty());
    assert_eq!(small.rule_change_lingering(), None);
    assert!(!small.is_derived_stale(), "no layer result changed");
    assert_eq!(small.check_stores(&t), Ok(()));
}

#[test]
fn two_rule_changes_on_a_player_end_apart() {
    let t = GameTable::new(&NO_CARDS, [&[], &[]]).unwrap();
    let to = Affects::Player(Seat::P0);
    let mut s: State<Small> = State::empty(Profile::EXACT);
    s.flags.derived_dirty = false;
    let rule = Payload::Rule(Rule::SorceriesAsFlash);
    s.start_effect(CardIx::FIRST, to, rule, Duration::EndOfTurn)
        .unwrap();
    s.start_effect(CardIx::FIRST, to, rule, Duration::Indefinite)
        .unwrap();
    let limit = Payload::Rule(Rule::OpponentsCastAsSorceries);
    s.start_effect(CardIx::FIRST, to, limit, Duration::EndOfTurn)
        .unwrap();
    // The one that ends takes its own rule with it and leaves the rule another still
    // changes.
    s.expire(Duration::EndOfTurn);
    assert!(s.rule_applies(&t, Rule::SorceriesAsFlash, to));
    assert!(!s.rule_applies(&t, Rule::OpponentsCastAsSorceries, to));
    assert_eq!(s.check_stores(&t), Ok(()));
}

#[test]
fn a_rule_change_on_an_object_ends_with_the_object() {
    let mut s: State<Small> = State::empty(Profile::EXACT);
    let (first, second) = (nth(&s, 1), nth(&s, 2));
    let rule = Rule::InstantLoyaltyTheTurnItEntered;
    s.start_effect(
        CardIx::FIRST,
        Affects::object(first),
        Payload::Rule(rule),
        Duration::Indefinite,
    )
    .unwrap();
    // No player is asked, so the layer results need not be current.
    assert!(s.effects.changes_rule(rule, Affects::object(first)));
    assert!(!s.effects.changes_rule(rule, Affects::object(second)));
    s.forget_stored(first.slot);
    assert!(!s.effects.changes_rule(rule, Affects::object(first)));
    assert!(s.keyed.is_empty());
}

#[test]
fn effects_end_with_their_duration_and_with_their_object() {
    let mut s: State<Small> = State::empty(Profile::EXACT);
    let (first, second) = (nth(&s, 1), nth(&s, 2));
    let start = |s: &mut State<Small>, object, payload, duration| {
        s.start_effect(CardIx::FIRST, Affects::object(object), payload, duration)
            .unwrap();
    };
    start(&mut s, first, PUMP, Duration::EndOfTurn);
    start(&mut s, first, EXILE, Duration::Indefinite);
    start(&mut s, second, PUMP, Duration::UntilYourNextTurn(Seat::P0));
    s.flags.derived_dirty = false;

    // Another player's turn beginning ends nothing.
    s.expire(Duration::UntilYourNextTurn(Seat::P1));
    assert_eq!(s.effects().len(), 3);
    assert!(!s.is_derived_stale());

    s.expire(Duration::EndOfTurn);
    assert_eq!(s.layer_effects().count(), 1);
    assert_eq!(s.move_replacements(first).count(), 1);
    assert!(s.is_derived_stale(), "a layer change ended");

    // The second object ends: its effect with it, and the first's stays.
    s.flags.derived_dirty = false;
    s.forget_stored(second.slot);
    assert_eq!(s.layer_effects().count(), 0);
    assert!(s.is_derived_stale());
    assert_eq!(s.effects().len(), 1);
    assert_eq!(s.keyed, Bitboard::single(first.slot));

    // Ending a replacement changes no layer result.
    s.flags.derived_dirty = false;
    s.forget_stored(first.slot);
    assert!(s.effects().is_empty());
    assert!(s.keyed.is_empty());
    assert!(!s.is_derived_stale());
    assert_eq!(s.move_replacements(first).count(), 0);
}
