//! The Effects store's tests: what it keeps across layouts, when it is full, and what ends
//! its entries.

use beryl_card::Zone;

use super::*;
use crate::bitboard::Bitboard;
use crate::fit::Misfit;
use crate::fixed::Full;
use crate::ids::SlotId;
use crate::layout::Large;
use crate::profile::Profile;
use crate::relayout::Relayout;
use crate::rule_changes::fixtures::{self, FLASH, LIMIT, LOYALTY};
use crate::store::Store;
use crate::{CardIx, LargeCardIx, LargeSlot, Slot};

const PUMP: Payload = Payload::Layer(LayerChange::ModifyPt(2, 2));
const EXILE: Payload = Payload::Move(MoveReplacement::ExileInsteadOfGraveyard);

/// [`PUMP`] on `object`, to start.
fn pump<L: Layout>(object: ObjectRef<L>) -> Continuous<L> {
    Continuous::Layer(object, LayerChange::ModifyPt(2, 2))
}

/// [`EXILE`] on `object`, to start.
fn exile<L: Layout>(object: ObjectRef<L>) -> Continuous<L> {
    Continuous::Move(object, MoveReplacement::ExileInsteadOfGraveyard)
}

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
            pump(seventh),
            Duration::UntilYourNextTurn(Seat::P1),
        )
        .unwrap();
    small
        .start_effect(CardIx::FIRST, exile(nth(&small, 200)), Duration::Indefinite)
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
        let effect = exile(nth(&small, i));
        assert_eq!(
            small.start_effect(CardIx::FIRST, effect, Duration::EndOfTurn),
            Ok(())
        );
    }
    let one_more = pump(nth(&small, EFFECTS));
    assert_eq!(
        small.start_effect(CardIx::FIRST, one_more, Duration::EndOfTurn),
        Err(Full)
    );
    assert_eq!(small.effects().len(), EFFECTS);
    // What did not fit is in no index.
    assert_eq!(small.layer_effects().count(), 0);
    assert!(!small.keyed.contains(Slot::from_index(EFFECTS)));

    for i in 0..=EFFECTS {
        let effect = exile(nth(&large, i));
        assert_eq!(
            large.start_effect(LargeCardIx::FIRST, effect, Duration::EndOfTurn),
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
    let t = fixtures::table();
    let [rule, limit] = [&FLASH, &LIMIT].map(|rule| t.rule_id(rule).unwrap());
    let (you, other) = (Subject::Player(Seat::P1), Subject::Player(Seat::P0));
    let mut small: State<Small> = State::empty(Profile::EXACT);
    small.flags.derived_dirty = false;
    assert!(!small.rule_applies(&t, rule, you));
    small
        .start_effect(
            CardIx::FIRST,
            Continuous::Rule(you, rule),
            Duration::UntilYourNextTurn(Seat::P1),
        )
        .unwrap();
    // It applies from the moment it starts, to that player and that rule alone, and is
    // no input of the layers and about no object.
    assert!(small.rule_applies(&t, rule, you));
    assert!(!small.rule_applies(&t, rule, other));
    assert!(!small.rule_applies(&t, limit, you));
    assert!(!small.is_derived_stale());
    assert!(small.keyed.is_empty());
    assert_eq!(small.check_stores(&t), Ok(()));
    // A checkpoint would lose it, in the words saves hold.
    assert_eq!(
        small.stores_lingering(&t).as_deref(),
        Some("player 1 casting sorceries as though they had flash until their next turn")
    );

    let mut large: State<Large> = small.widen();
    assert_eq!(large.effects()[0].affected, Affects::Player(Seat::P1));
    assert!(large.rule_applies(&t, rule, Subject::Player(Seat::P1)));
    assert!(!large.rule_applies(&t, rule, Subject::Player(Seat::P0)));
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
    assert!(!large.rule_applies(&t, rule, Subject::Player(Seat::P1)));
    assert!(small.effects().is_empty());
    assert_eq!(small.stores_lingering(&t), None);
    assert!(!small.is_derived_stale(), "no layer result changed");
    assert_eq!(small.check_stores(&t), Ok(()));
}

#[test]
fn two_rule_changes_on_a_player_end_apart() {
    let t = fixtures::table();
    let [flash, limit] = [&FLASH, &LIMIT].map(|rule| t.rule_id(rule).unwrap());
    let to = Subject::Player(Seat::P0);
    let mut s: State<Small> = State::empty(Profile::EXACT);
    s.flags.derived_dirty = false;
    let rule = Continuous::Rule(to, flash);
    s.start_effect(CardIx::FIRST, rule, Duration::EndOfTurn)
        .unwrap();
    s.start_effect(CardIx::FIRST, rule, Duration::Indefinite)
        .unwrap();
    let limit_for = Continuous::Rule(to, limit);
    s.start_effect(CardIx::FIRST, limit_for, Duration::EndOfTurn)
        .unwrap();
    // The one that ends takes its own rule with it and leaves the rule another still
    // changes.
    s.expire(Duration::EndOfTurn);
    assert!(s.rule_applies(&t, flash, to));
    assert!(!s.rule_applies(&t, limit, to));
    assert_eq!(s.check_stores(&t), Ok(()));
}

#[test]
fn a_rule_change_on_an_object_ends_with_the_object() {
    let mut s: State<Small> = State::empty(Profile::EXACT);
    let (first, second) = (nth(&s, 1), nth(&s, 2));
    let rule = fixtures::table().rule_id(&LOYALTY).unwrap();
    s.start_effect(
        CardIx::FIRST,
        Continuous::Rule(Subject::Object(first), rule),
        Duration::Indefinite,
    )
    .unwrap();
    // No player is asked, so the layer results need not be current.
    assert!(s.effect_rules_on(first).contains(rule));
    assert!(s.effect_rules_on(second).is_empty());
    s.forget_stored(first.slot);
    assert!(s.effect_rules_on(first).is_empty());
    assert!(s.keyed.is_empty());
}

#[test]
fn effects_end_with_their_duration_and_with_their_object() {
    let mut s: State<Small> = State::empty(Profile::EXACT);
    let (first, second) = (nth(&s, 1), nth(&s, 2));
    let start = |s: &mut State<Small>, effect, duration| {
        s.start_effect(CardIx::FIRST, effect, duration).unwrap();
    };
    start(&mut s, pump(first), Duration::EndOfTurn);
    start(&mut s, exile(first), Duration::Indefinite);
    start(&mut s, pump(second), Duration::UntilYourNextTurn(Seat::P0));
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

/// A permission for player 1 to cast the card, for {2} more.
fn permission(replacement: Option<MoveReplacement>) -> Permission {
    Permission {
        player: Seat::P1,
        increase: 2,
        replacement,
    }
}

#[test]
fn a_permission_to_cast_ends_with_its_turn_or_with_its_card() {
    let t = GameTable::new(&ONE_CARD, [&[(1, "Test Card")], &[]]).unwrap();
    let mut s: State<Small> = State::empty(Profile::EXACT);
    let (this_turn, while_there) = (nth(&s, 1), nth(&s, 2));
    let flashback = permission(Some(MoveReplacement::Flashback));
    let plain = permission(None);
    s.start_effect(
        CardIx::FIRST,
        Continuous::Cast(this_turn, flashback),
        Duration::EndOfTurn,
    )
    .unwrap();
    s.start_effect(
        CardIx::FIRST,
        Continuous::Cast(while_there, plain),
        Duration::Indefinite,
    )
    .unwrap();
    let permitted = |s: &State<Small>| s.cast_permissions().collect::<Vec<_>>();
    assert_eq!(
        permitted(&s),
        [
            Permitted {
                card: this_turn,
                terms: flashback,
                source: CardIx::FIRST,
            },
            Permitted {
                card: while_there,
                terms: plain,
                source: CardIx::FIRST,
            },
        ]
    );
    assert!(flashback.is_flashback() && !plain.is_flashback());
    // A permission is no input of the layers, changes no cost and replaces no move: its
    // terms are the spell's, once there is one.
    assert!(!s.is_derived_stale());
    assert_eq!(s.cost_increase(this_turn), 0);
    assert_eq!(s.move_replacements(this_turn).count(), 0);
    assert_eq!(s.check_stores(&t), Ok(()));

    // Another player's next turn ends neither; the end of the turn ends the one that lasts
    // this turn; the other ends with its card's object and not before.
    s.expire(Duration::UntilYourNextTurn(Seat::P1));
    assert_eq!(permitted(&s).len(), 2);
    s.expire(Duration::EndOfTurn);
    assert_eq!(permitted(&s).len(), 1);
    assert_eq!(permitted(&s)[0].card, while_there);
    assert_eq!(s.keyed, Bitboard::single(while_there.slot));
    s.expire(Duration::EndOfTurn);
    assert_eq!(permitted(&s).len(), 1);
    s.forget_stored(while_there.slot);
    assert_eq!(permitted(&s), []);
    assert!(s.keyed.is_empty());
    assert_eq!(s.check_stores(&t), Ok(()));
}

#[test]
fn a_permission_and_a_spells_terms_survive_small_to_large_and_back() {
    let mut small: State<Small> = State::empty(Profile::EXACT);
    let (card, spell) = (nth(&small, 4), nth(&small, 9));
    let terms = permission(Some(MoveReplacement::ExileInsteadOfGraveyard));
    small
        .start_effect(
            CardIx::from_index(3),
            Continuous::Cast(card, terms),
            Duration::Indefinite,
        )
        .unwrap();
    // What a spell cast on such terms carries: its cost increases, added together, and
    // the replacement of where it goes.
    for terms in [
        Continuous::Cost(spell, CostChange::Increase(2)),
        Continuous::Cost(spell, CostChange::Increase(1)),
        Continuous::Move(spell, MoveReplacement::Flashback),
    ] {
        small
            .start_effect(CardIx::FIRST, terms, Duration::Indefinite)
            .unwrap();
    }
    assert_eq!(small.cost_increase(spell), 3);
    assert_eq!(small.cost_increase(card), 0);

    let large: State<Large> = small.widen();
    assert_eq!(
        large.cast_permissions().collect::<Vec<_>>(),
        [Permitted {
            card: nth(&large, 4),
            terms,
            source: LargeCardIx::from_index(3),
        }]
    );
    assert_eq!(large.cost_increase(nth(&large, 9)), 3);
    assert_eq!(
        large.move_replacements(nth(&large, 9)).collect::<Vec<_>>(),
        [MoveReplacement::Flashback]
    );
    let back: State<Small> = large.relayout();
    assert_eq!(back, small);
    assert_eq!(back.effects.keys(), small.keyed);
}

/// A game of one card, to name a permitted card in a save's words.
static ONE_CARD: crate::table::CardTable = crate::table::CardTable::new(&[&beryl_card::CardDef {
    name: "Test Card",
    faces: &[beryl_card::Face::EMPTY],
    tokens: &[],
}]);

#[test]
fn a_checkpoint_says_a_permission_in_the_words_saves_hold() {
    let t = GameTable::new(&ONE_CARD, [&[(1, "Test Card")], &[]]).unwrap();
    let mut s: State<Small> = State::empty(Profile::EXACT);
    s.start_effect(
        CardIx::FIRST,
        Continuous::Cast(nth(&s, 0), permission(None)),
        Duration::Indefinite,
    )
    .unwrap();
    assert_eq!(
        s.stores_lingering(&t).as_deref(),
        Some("player 1's permission to cast Test Card")
    );
    // After an effect on an object, where saves have always had permissions.
    s.start_effect(CardIx::FIRST, exile(nth(&s, 1)), Duration::Indefinite)
        .unwrap();
    assert_eq!(
        s.stores_lingering(&t).as_deref(),
        Some("an effect of Test Card on Test Card lasting indefinitely")
    );
}

/// A dealt game of sixty of one card a player, and the table it is of.
fn dealt() -> (GameTable, State<Small>) {
    let deck: &[(u8, &str)] = &[(60, "Test Card")];
    let t = GameTable::new(&ONE_CARD, [deck, deck]).unwrap();
    let s = State::new(&t, 7, Profile::EXACT).unwrap();
    (t, s)
}

/// Puts the cards in the slots at `places` onto the battlefield, and says which slots.
fn permanents<L: Layout>(s: &mut State<L>, places: &[usize]) -> Bitboard<L::Slot> {
    let slots = places.iter().map(|&i| L::Slot::from_index(i));
    slots
        .inspect(|&slot| s.board().place(slot, Zone::Battlefield).unwrap())
        .collect()
}

const PUMPED: LayerChange = LayerChange::ModifyPt(2, 2);

#[test]
fn one_effect_on_a_set_ends_with_its_duration_or_with_its_last_member() {
    let (t, mut s) = dealt();
    let [first, second] = [3, 70].map(Slot::from_index);
    let both = permanents(&mut s, &[3, 70]);
    let began = s.zones.next_timestamp;
    let start = |s: &mut State<Small>, members, duration| {
        s.start_effect_on_set(CardIx::FIRST, members, PUMPED, duration)
    };

    // One entry with one timestamp, whatever the number of members.
    assert_eq!(start(&mut s, both, Duration::Indefinite), Ok(()));
    assert_eq!(s.zones.next_timestamp, began + 1);
    let [effect] = *s.effects() else {
        panic!("one effect");
    };
    let Affects::Set(set) = effect.affected else {
        panic!("an effect on a set");
    };
    assert_eq!(effect.timestamp, began);
    assert_eq!((s.locked_in(set), s.keyed), (both, both));
    assert!(s.is_derived_stale());
    assert_eq!(s.check_stores(&t), Ok(()));

    // A member that leaves is no longer one, and is not again when its card comes back
    // (CR 400.7); the effect goes on for the other.
    s.board().place(first, Zone::Graveyard).unwrap();
    assert_eq!(s.locked_in(set), Bitboard::single(second));
    s.board().place(first, Zone::Battlefield).unwrap();
    assert_eq!(s.locked_in(set), Bitboard::single(second));
    assert_eq!(s.keyed, Bitboard::single(second));
    assert_eq!(s.layer_effects().count(), 1);
    assert_eq!(s.check_stores(&t), Ok(()));

    // No duration ends it. Its last member leaving does, and frees its place.
    s.expire(Duration::EndOfTurn);
    assert_eq!(s.effects().len(), 1);
    s.board().place(second, Zone::Hand).unwrap();
    assert_eq!(s.effects, Effects::empty());
    assert!(s.keyed.is_empty() && s.locked_in(set).is_empty());
    s.board().place(second, Zone::Battlefield).unwrap();

    // An effect that lasts until end of turn ends there, with every member still there.
    assert_eq!(start(&mut s, both, Duration::EndOfTurn), Ok(()));
    s.flags.derived_dirty = false;
    s.expire(Duration::EndOfTurn);
    assert_eq!(s.effects, Effects::empty());
    assert!(s.keyed.is_empty() && s.is_derived_stale());
    assert_eq!(s.check_stores(&t), Ok(()));
}

/// Stores the same layer results for every permanent, as the rules' recompute does for
/// twins whose effects come to the same today.
fn derive_alike(s: &mut State) {
    let Some(mut cache) = s.rederive() else {
        return;
    };
    for i in 0..cache.battlefield().len() {
        cache.results()[i] = crate::state::Derived::default();
    }
    cache.finish(
        crate::event::Listens::NONE,
        crate::event::Replaces::NONE,
        Default::default(),
        false,
    );
}

/// Two objects are interchangeable only if the same effects are on them (§7.2): their
/// layer results say what the effects make of them now, not for how long.
#[test]
fn signature_tells_two_objects_apart_by_the_effects_on_them() {
    let (t, mut s) = dealt();
    let [a, b, c, d] = [3, 4, 5, 6].map(Slot::from_index);
    permanents(&mut s, &[3, 4, 5]);
    let same = |s: &mut State, x, y| {
        derive_alike(s);
        s.signature(x) == s.signature(y)
    };
    assert!(same(&mut s, a, b));

    // A member of an effect's set is no twin of one that is no member; of another member,
    // it is.
    let members = [a, c].into_iter().collect();
    s.start_effect_on_set(CardIx::FIRST, members, PUMPED, Duration::EndOfTurn)
        .unwrap();
    assert!(!same(&mut s, a, b));
    assert!(same(&mut s, a, c));
    // The same change for longer is another effect, on a set or alone.
    let alone = pump(s.object_ref(b));
    s.start_effect(CardIx::FIRST, alone, Duration::Indefinite)
        .unwrap();
    assert!(!same(&mut s, a, b));
    // As the turn ends, the one that lasted is on one of them alone.
    s.expire(Duration::EndOfTurn);
    assert!(same(&mut s, a, c));
    assert!(!same(&mut s, a, b));
    s.board().place(b, Zone::Graveyard).unwrap();
    s.board().place(b, Zone::Battlefield).unwrap();
    assert!(same(&mut s, a, b));

    // A card a player may cast from where it is is no twin of one they may not, and is
    // one of a card they may cast on the same terms.
    let (in_hand, other) = (d, Slot::from_index(7));
    assert!(same(&mut s, in_hand, other));
    let permit = |s: &mut State, slot, player| {
        let terms = Permission {
            player,
            ..permission(None)
        };
        let permission = Continuous::Cast(s.object_ref(slot), terms);
        s.start_effect(CardIx::FIRST, permission, Duration::EndOfTurn)
            .unwrap();
    };
    permit(&mut s, in_hand, Seat::P0);
    assert!(!same(&mut s, in_hand, other));
    permit(&mut s, other, Seat::P1);
    assert!(!same(&mut s, in_hand, other));
    s.expire(Duration::EndOfTurn);
    permit(&mut s, in_hand, Seat::P0);
    permit(&mut s, other, Seat::P0);
    assert!(same(&mut s, in_hand, other));
    assert_eq!(s.check_stores(&t), Ok(()));
}

#[test]
fn an_effect_on_one_object_is_an_entry_on_it_and_one_on_none_never_begins() {
    let (t, mut s) = dealt();
    let one = permanents(&mut s, &[3]);
    let began = s.zones.next_timestamp;
    let nothing = Bitboard::EMPTY;
    assert_eq!(
        s.start_effect_on_set(CardIx::FIRST, nothing, PUMPED, Duration::EndOfTurn),
        Ok(())
    );
    assert!(s.effects().is_empty());
    assert_eq!(s.zones.next_timestamp, began);

    assert_eq!(
        s.start_effect_on_set(CardIx::FIRST, one, PUMPED, Duration::EndOfTurn),
        Ok(())
    );
    let third = s.object_ref(Slot::from_index(3));
    assert_eq!(s.effects()[0].affected, Affects::object(third));
    assert_eq!(s.effects()[0].timestamp, began);
    assert_eq!(s.effects.sets, Sets::empty());
    assert_eq!(s.check_stores(&t), Ok(()));
}

#[test]
fn a_set_survives_small_to_large_and_back() {
    let (t, mut small) = dealt();
    // The second player's cards keep their places, not their numbers.
    let members = permanents(&mut small, &[3, 70, 119]);
    let other = permanents(&mut small, &[4, 5]);
    for (members, duration) in [
        (members, Duration::Indefinite),
        (other, Duration::EndOfTurn),
    ] {
        small
            .start_effect_on_set(CardIx::FIRST, members, PUMPED, duration)
            .unwrap();
    }
    let sets = |effects: &[ContinuousEffect<Large>]| {
        effects
            .iter()
            .map(|e| match e.affected {
                Affects::Set(set) => set,
                other => panic!("{other:?} is no set"),
            })
            .collect::<Vec<_>>()
    };

    let mut large: State<Large> = small.widen();
    let [first, second] = sets(large.effects())[..] else {
        panic!("two effects");
    };
    assert_eq!(large.locked_in(first), Bitboard::<LargeSlot>::from(members));
    assert_eq!(large.locked_in(second), Bitboard::<LargeSlot>::from(other));
    assert_eq!(large.check_stores(&t), Ok(()));
    let back: State<Small> = large.try_relayout(&t).unwrap();
    assert_eq!(back, small);

    // The wide game ends a member and an effect as the narrow one does.
    let seventieth = Slot::from_index(70).to::<LargeSlot>().unwrap();
    large.board().place(seventieth, Zone::Graveyard).unwrap();
    large.expire(Duration::EndOfTurn);
    small
        .board()
        .place(Slot::from_index(70), Zone::Graveyard)
        .unwrap();
    small.expire(Duration::EndOfTurn);
    assert_eq!(large.effects().len(), 1);
    assert_eq!(large.locked_in(first).count(), 2);
    assert_eq!(large.try_relayout::<Small>(&t).unwrap(), small);
}

#[test]
fn a_full_table_of_sets_returns_full_and_a_wide_one_grows() {
    let (t, mut small) = dealt();
    let pairs: Vec<Bitboard> = (0..=SETS)
        .map(|i| permanents(&mut small, &[2 * i, 2 * i + 1]))
        .collect();
    let mut large: State<Large> = small.widen();
    for &pair in &pairs[..SETS] {
        assert_eq!(
            small.start_effect_on_set(CardIx::FIRST, pair, PUMPED, Duration::EndOfTurn),
            Ok(())
        );
    }
    let began = small.effects;
    assert_eq!(
        small.start_effect_on_set(CardIx::FIRST, pairs[SETS], PUMPED, Duration::EndOfTurn),
        Err(Full)
    );
    // What did not fit is in no index.
    assert_eq!(small.effects, began);
    assert!(!small.keyed.contains(Slot::from_index(2 * SETS)));
    assert_eq!(small.check_stores(&t), Ok(()));

    // A set whose effect ended leaves its place to the next.
    for member in [2, 3] {
        small
            .board()
            .place(Slot::from_index(member), Zone::Graveyard)
            .unwrap();
    }
    assert_eq!(small.effects().len(), SETS - 1);
    assert_eq!(
        small.start_effect_on_set(CardIx::FIRST, pairs[SETS], PUMPED, Duration::EndOfTurn),
        Ok(())
    );
    assert_eq!(small.check_stores(&t), Ok(()));

    for &pair in &pairs {
        let pair = Bitboard::<LargeSlot>::from(pair);
        assert_eq!(
            large.start_effect_on_set(LargeCardIx::FIRST, pair, PUMPED, Duration::EndOfTurn),
            Ok(())
        );
    }
    assert_eq!(large.effects().len(), SETS + 1);
    assert_eq!(
        large.fits::<Small>(&t),
        Err(Misfit::ListTooLong {
            list: "the locked-in sets",
            len: SETS + 1,
            capacity: SETS,
        })
    );
}
